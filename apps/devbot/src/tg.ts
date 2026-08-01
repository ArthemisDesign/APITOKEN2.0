import { errorMessage, type Logger } from "./log.js";

/** Максимум символов в одном сообщении Bot API; коалесцинг держит запас. */
export const TG_MESSAGE_LIMIT = 4096;
const COALESCE_LIMIT = 3800;
const DEFAULT_MAX_ATTEMPTS = 5;
const DEFAULT_SEND_INTERVAL_MS = 1000;

export interface TgUser {
  id: number;
  username?: string;
}

export interface TgMessage {
  message_id: number;
  chat: { id: number };
  from?: TgUser;
  text?: string;
  message_thread_id?: number;
  date?: number;
}

export interface TgUpdate {
  update_id: number;
  message?: TgMessage;
  edited_message?: TgMessage;
}

export interface SendOptions {
  threadId?: number;
  replyTo?: number;
}

export interface TelegramBotOptions {
  token: string;
  apiRoot?: string;
  fetchFn?: typeof fetch;
  sleep?: (ms: number) => Promise<void>;
  logger?: Logger;
  maxAttempts?: number;
  /** Минимальный интервал между сообщениями в один чат (лимит Bot API для групп). */
  sendIntervalMs?: number;
  /** Счётчик неудачных отправок (метрика devbot_telegram_send_failures_total). */
  onSendFailure?: (method: string) => void;
}

/** Ошибка уровня Bot API (4xx/5xx с JSON-ответом) — перманентная, не ретраим. */
class TgApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly retryAfterSec?: number,
  ) {
    super(message);
  }
}

interface SendJob {
  text: string;
  threadId?: number;
  replyTo?: number;
  resolve: (messageId: number | null) => void;
}

const defaultSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Тонкий клиент Telegram Bot API по образцу crates/authbot/src/tg.rs:
 * без фреймворков, parse_mode HTML, redact токена из всех ошибок.
 * Сверху — исходящая очередь: 1 сообщение/с на чат, коалесцинг подряд идущих
 * сообщений в один топик, honour 429 retry_after, экспоненциальный backoff
 * на сетевых ошибках (максимум maxAttempts, дальше событие теряется с логом).
 */
export class TelegramBot {
  private readonly token: string;
  private readonly apiRoot: string;
  private readonly fetchFn: typeof fetch;
  private readonly sleep: (ms: number) => Promise<void>;
  private readonly maxAttempts: number;
  private readonly sendIntervalMs: number;
  private readonly logger?: Logger;
  private readonly onSendFailure?: (method: string) => void;

  private readonly queues = new Map<number, SendJob[]>();
  private readonly pumping = new Set<number>();

  constructor(options: TelegramBotOptions) {
    this.token = options.token;
    this.apiRoot = (options.apiRoot ?? "https://api.telegram.org").replace(/\/+$/, "");
    this.fetchFn = options.fetchFn ?? fetch;
    this.sleep = options.sleep ?? defaultSleep;
    this.maxAttempts = options.maxAttempts ?? DEFAULT_MAX_ATTEMPTS;
    this.sendIntervalMs = options.sendIntervalMs ?? DEFAULT_SEND_INTERVAL_MS;
    if (options.logger) this.logger = options.logger;
    if (options.onSendFailure) this.onSendFailure = options.onSendFailure;
  }

  /** Убрать bot-токен из строк ошибок (иначе утекает в journalctl через URL запроса). */
  redact(text: string): string {
    return text.split(this.token).join("***");
  }

  private url(method: string): string {
    return `${this.apiRoot}/bot${this.token}/${method}`;
  }

  /**
   * Вызов Bot API с ретраями: 429 → ждём retry_after; сетевые ошибки →
   * экспоненциальный backoff. Перманентные API-ошибки пробрасываются сразу.
   */
  private async call(method: string, body: Record<string, unknown>): Promise<unknown> {
    let lastError: Error | null = null;
    for (let attempt = 1; attempt <= this.maxAttempts; attempt += 1) {
      try {
        const response = await this.fetchFn(this.url(method), {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        });
        const data = await response.json().catch(() => null) as {
          ok?: boolean;
          description?: string;
          parameters?: { retry_after?: number };
          result?: unknown;
        } | null;
        if (response.ok && data?.ok === true) {
          return data.result;
        }
        const description = data?.description ?? `HTTP ${response.status}`;
        if (response.status === 429) {
          const retryAfterSec = data?.parameters?.retry_after ?? 1;
          if (attempt < this.maxAttempts) {
            this.logger?.warn(`telegram ${method}: 429, retry after ${retryAfterSec}s`);
            await this.sleep(retryAfterSec * 1000);
            continue;
          }
        }
        throw new TgApiError(this.redact(`${method}: ${description}`), response.status);
      } catch (error) {
        if (error instanceof TgApiError) throw error;
        lastError = error instanceof Error ? error : new Error(String(error));
        if (attempt < this.maxAttempts) {
          const backoffMs = 500 * 2 ** (attempt - 1);
          this.logger?.warn(
            this.redact(`telegram ${method}: network error (attempt ${attempt}/${this.maxAttempts}): ${errorMessage(error)}`),
          );
          await this.sleep(backoffMs);
        }
      }
    }
    throw new Error(this.redact(`${method}: failed after ${this.maxAttempts} attempts: ${lastError?.message ?? "unknown"}`));
  }

  private static canMerge(a: SendJob, b: SendJob): boolean {
    return a.threadId === b.threadId && a.replyTo === b.replyTo;
  }

  private async pump(chatId: number): Promise<void> {
    if (this.pumping.has(chatId)) return;
    this.pumping.add(chatId);
    try {
      for (;;) {
        const queue = this.queues.get(chatId);
        if (!queue || queue.length === 0) return;
        const first = queue.shift() as SendJob;
        const batch: SendJob[] = [first];
        let length = first.text.length;
        while (queue.length > 0) {
          const next = queue[0] as SendJob;
          if (!TelegramBot.canMerge(first, next)) break;
          if (length + 1 + next.text.length > COALESCE_LIMIT) break;
          batch.push(queue.shift() as SendJob);
          length += 1 + next.text.length;
        }
        const text = batch.map((job) => job.text).join("\n");
        let messageId: number | null = null;
        try {
          const body: Record<string, unknown> = {
            chat_id: chatId,
            text,
            parse_mode: "HTML",
            disable_web_page_preview: true,
          };
          if (first.threadId !== undefined) body.message_thread_id = first.threadId;
          if (first.replyTo !== undefined) body.reply_to_message_id = first.replyTo;
          const result = await this.call("sendMessage", body) as { message_id?: number } | null;
          messageId = typeof result?.message_id === "number" ? result.message_id : null;
        } catch (error) {
          this.onSendFailure?.("sendMessage");
          this.logger?.error(this.redact(`telegram sendMessage dropped: ${errorMessage(error)}`));
        }
        for (const job of batch) job.resolve(messageId);
        if ((this.queues.get(chatId)?.length ?? 0) > 0) {
          await this.sleep(this.sendIntervalMs);
        }
      }
    } finally {
      this.pumping.delete(chatId);
    }
  }

  /**
   * Отправка через очередь с троттлингом. Возвращает message_id или null,
   * если сообщение потеряно после всех ретраев (бот не падает из-за Telegram).
   */
  sendMessage(chatId: number, text: string, options: SendOptions = {}): Promise<number | null> {
    const job: SendJob = { text, resolve: () => undefined };
    if (options.threadId !== undefined) job.threadId = options.threadId;
    if (options.replyTo !== undefined) job.replyTo = options.replyTo;
    const promise = new Promise<number | null>((resolve) => {
      job.resolve = resolve;
    });
    const queue = this.queues.get(chatId) ?? [];
    queue.push(job);
    this.queues.set(chatId, queue);
    void this.pump(chatId);
    return promise;
  }

  /** Правка существующего сообщения. false — правка не удалась (событие не ретраится выше). */
  async editMessageText(chatId: number, messageId: number, text: string): Promise<boolean> {
    try {
      await this.call("editMessageText", {
        chat_id: chatId,
        message_id: messageId,
        text,
        parse_mode: "HTML",
        disable_web_page_preview: true,
      });
      return true;
    } catch (error) {
      // «message is not modified» — идемпотентный успех: фаза приходит из двух источников
      // (deploy/* statuses и production-* deployments), повторная правка рендерит тот же
      // текст. Это не сбой отправки — не тревожим ни метрику, ни error-лог.
      if (errorMessage(error).includes("message is not modified")) return true;
      this.onSendFailure?.("editMessageText");
      this.logger?.error(this.redact(`telegram editMessageText failed: ${errorMessage(error)}`));
      return false;
    }
  }

  /** Long polling getUpdates. Ошибка пробрасывается — цикл выше решает, когда повторить. */
  async getUpdates(offset: number | undefined, timeoutSec: number): Promise<TgUpdate[]> {
    const body: Record<string, unknown> = { timeout: timeoutSec };
    if (offset !== undefined) body.offset = offset;
    const result = await this.call("getUpdates", body);
    return Array.isArray(result) ? result as TgUpdate[] : [];
  }

  /** Webhook у бота явно удалён — команды работают только через long polling. */
  async deleteWebhook(): Promise<void> {
    try {
      await this.call("deleteWebhook", {});
    } catch (error) {
      this.logger?.warn(this.redact(`telegram deleteWebhook failed: ${errorMessage(error)}`));
    }
  }
}
