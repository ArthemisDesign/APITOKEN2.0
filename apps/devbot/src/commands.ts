import { escapeHtml, truncate } from "./router.js";
import { errorMessage, type Logger } from "./log.js";
import type { StateStore } from "./state.js";
import type { TelegramBot, TgUpdate } from "./tg.js";

export interface ProbeTarget {
  name: string;
  url: string;
}

export interface EngineAccess {
  baseUrl: string;
  readonlyKey?: string;
  controlKey?: string;
}

export interface GithubAccess {
  token: string;
  repo: string;
}

export interface CommandsDeps {
  tg: TelegramBot;
  chatId: number;
  adminIds: ReadonlySet<number>;
  state: StateStore;
  alertmanagerUrl: string;
  probes: ProbeTarget[];
  /** Топик 📊 Digest — ответ на /digest и ежедневная сводка идут туда. */
  digestTopicId: number;
  logger?: Logger;
  fetchFn?: typeof fetch;
  engine?: EngineAccess;
  github?: GithubAccess;
  now?: () => number;
}

export const HELP_TEXT = [
  "<b>Команды devbot</b>",
  "/status — пайплайн, активные алерты, readiness плоскостей",
  "/alerts — активные алерты из Alertmanager",
  "/deploys [N] — последние N SHA master со статусами (дефолт 5)",
  "/pool — пул подписок движка (anthropic/codex/gemini)",
  "/settlement — settlement-health движка",
  "/silence &lt;alertname&gt; &lt;длительность&gt; — silence в Alertmanager (например <code>/silence HostDiskSpaceLow 2h</code>)",
  "/digest — сводка за 24 ч в топик 📊 Digest",
  "/help — этот список",
].join("\n");

const DURATION_RE = /^(\d+)\s*([smhd])$/i;
const DURATION_UNITS: Record<string, number> = { s: 1000, m: 60_000, h: 3_600_000, d: 86_400_000 };

/** «2h» / «30m» / «1d» → миллисекунды; null — не распарсилось. */
export function parseDuration(input: string): number | null {
  const match = DURATION_RE.exec(input.trim());
  if (!match) return null;
  const amount = Number(match[1]);
  const unit = DURATION_UNITS[(match[2] as string).toLowerCase()] as number;
  const ms = amount * unit;
  if (!Number.isFinite(ms) || ms <= 0 || ms > 7 * 86_400_000) return null;
  return ms;
}

/** Миллисекунды до ближайших hh:mm в заданной IANA time zone. */
export function msUntilNext(hour: number, minute: number, now: number, timeZone: string): number {
  const formatter = new Intl.DateTimeFormat("en-US", {
    timeZone,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });
  const firstMinute = Math.floor(now / 60_000) * 60_000 + 60_000;
  // 48h covers even a skipped wall-clock hour on a DST transition day.
  for (let offsetMinutes = 0; offsetMinutes < 48 * 60; offsetMinutes += 1) {
    const candidate = firstMinute + offsetMinutes * 60_000;
    const parts = formatter.formatToParts(candidate);
    const candidateHour = Number(parts.find((part) => part.type === "hour")?.value);
    const candidateMinute = Number(parts.find((part) => part.type === "minute")?.value);
    if (candidateHour === hour && candidateMinute === minute) return candidate - now;
  }
  throw new Error(`cannot find next ${hour}:${minute} in time zone ${timeZone}`);
}

function fmtAge(ts: number, now: number): string {
  const minutes = Math.max(0, Math.round((now - ts) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** Сводка /digest за 24 ч из журнала событий state-файла. */
export function buildDigestReport(events: StateStore["data"]["events"], now: number): string {
  const cutoff = now - 24 * 3600 * 1000;
  const recent = events.filter((event) => event.ts >= cutoff);
  const deploysOk = recent.filter((event) => event.kind === "deploy" && event.severity === "success").length;
  const deploysQuarantined = recent.filter((event) => event.kind === "deploy" && event.severity === "quarantine").length;
  const alerts = recent.filter((event) => event.kind === "alert");
  const supportCount = recent.filter((event) => event.kind === "support").length;
  const critical = alerts.filter((event) => event.severity === "critical").length;
  const warnings = alerts.filter((event) => event.severity === "warning");
  const topWarnings = new Map<string, number>();
  for (const event of warnings) {
    topWarnings.set(event.name, (topWarnings.get(event.name) ?? 0) + 1);
  }
  const top = [...topWarnings.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5);
  const lines = [
    "📊 <b>Digest за 24 ч</b>",
    `Деплои: ✅ ${deploysOk} · карантин 🚨 ${deploysQuarantined}`,
    `Алерты: 🔴 ${critical} critical · 🟡 ${warnings.length} warning`,
    `Support: 💬 ${supportCount} входящих`,
  ];
  if (top.length > 0) {
    lines.push("Топ warning:");
    for (const [name, count] of top) lines.push(`• ${escapeHtml(name)} ×${count}`);
  }
  if (recent.length === 0) lines.push("Событий не было — тишина.");
  return lines.join("\n");
}

interface AmApiAlert {
  labels?: Record<string, string>;
  status?: { state?: string };
  startsAt?: string;
}

/**
 * Команды из чата (DEVBOT.md §5). Гейт: только DEVBOT_CHAT_ID и только user id
 * из DEVBOT_ADMIN_IDS; чужие апдейты игнорируются молча. Обработчики никогда
 * не падают: сбойный backend показывается как «недоступно» в ответе.
 */
export class CommandHandler {
  private readonly fetchFn: typeof fetch;
  private readonly now: () => number;

  constructor(private readonly deps: CommandsDeps) {
    this.fetchFn = deps.fetchFn ?? fetch;
    this.now = deps.now ?? Date.now;
  }

  private async json<T>(url: string, init?: RequestInit): Promise<T> {
    const response = await this.fetchFn(url, init);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return await response.json() as T;
  }

  private reply(text: string, threadId?: number): Promise<number | null> {
    return this.deps.tg.sendMessage(this.deps.chatId, text, threadId !== undefined ? { threadId } : {});
  }

  async handleUpdate(update: TgUpdate): Promise<void> {
    const message = update.message;
    const text = message?.text;
    if (!message || !text) return;
    if (message.chat.id !== this.deps.chatId) return;
    const userId = message.from?.id;
    if (userId === undefined || !this.deps.adminIds.has(userId)) return;
    const match = /^\/(\w+)(?:@\w+)?(?:\s+(.*))?$/.exec(text.trim());
    if (!match) return;
    const command = (match[1] as string).toLowerCase();
    const args = (match[2] ?? "").trim();
    const threadId = message.message_thread_id;
    try {
      switch (command) {
        case "help":
          await this.reply(HELP_TEXT, threadId);
          break;
        case "status":
          await this.reply(await this.cmdStatus(), threadId);
          break;
        case "alerts":
          await this.reply(await this.cmdAlerts(), threadId);
          break;
        case "deploys":
          await this.reply(await this.cmdDeploys(args), threadId);
          break;
        case "pool":
          await this.reply(await this.cmdPool(), threadId);
          break;
        case "settlement":
          await this.reply(await this.cmdSettlement(), threadId);
          break;
        case "silence":
          await this.reply(await this.cmdSilence(args), threadId);
          break;
        case "digest":
          await this.cmdDigest();
          break;
        default:
          await this.reply(`Неизвестная команда. ${HELP_TEXT}`, threadId);
      }
    } catch (error) {
      this.deps.logger?.error(`commands: /${command} failed: ${errorMessage(error)}`);
      await this.reply(`⚠️ /${escapeHtml(command)} упала: ${escapeHtml(errorMessage(error))}`, threadId);
    }
  }

  private async activeAlerts(): Promise<AmApiAlert[]> {
    const alerts = await this.json<AmApiAlert[]>(`${this.deps.alertmanagerUrl}/api/v2/alerts`);
    return alerts.filter((alert) => alert.status?.state === "active");
  }

  private async pipelineSection(): Promise<string> {
    const { github, state } = this.deps;
    if (github) {
      try {
        const combined = await this.json<{
          state?: string;
          sha?: string;
          statuses?: { context?: string; state?: string }[];
        }>(`https://api.github.com/repos/${github.repo}/commits/master/status`, {
          headers: {
            accept: "application/vnd.github+json",
            authorization: `Bearer ${github.token}`,
            "user-agent": "apitoken-devbot",
          },
        });
        const sha = (combined.sha ?? "").slice(0, 7) || "?";
        const icon = combined.state === "success" ? "✅" : combined.state === "failure" ? "❌" : "🔄";
        const contexts = (combined.statuses ?? [])
          .filter((status) => status.context?.startsWith("deploy/"))
          .map((status) => `${status.context?.replace("deploy/", "")}: ${status.state}`)
          .join(" · ");
        return `Пайплайн: ${icon} <code>${escapeHtml(sha)}</code>${contexts ? ` (${escapeHtml(contexts)})` : ""}`;
      } catch (error) {
        return `Пайплайн: ⚠️ github недоступен (${escapeHtml(errorMessage(error))})`;
      }
    }
    const deploy = state.data.deploy;
    if (deploy) {
      return `Пайплайн: последний известный SHA <code>${escapeHtml(deploy.sha.slice(0, 7))}</code> (github не настроен)`;
    }
    return "Пайплайн: данных нет (github не настроен)";
  }

  private async alertsSection(): Promise<string> {
    try {
      const alerts = await this.activeAlerts();
      const critical = alerts.filter((alert) => alert.labels?.severity === "critical");
      const warning = alerts.filter((alert) => alert.labels?.severity !== "critical");
      if (alerts.length === 0) return "Алерты: активных нет ✅";
      const names = [...critical, ...warning]
        .slice(0, 8)
        .map((alert) => `${alert.labels?.severity === "critical" ? "🔴" : "🟡"} ${escapeHtml(alert.labels?.alertname ?? "?")}`)
        .join(", ");
      return `Алерты: 🔴 ${critical.length} · 🟡 ${warning.length}\n${names}`;
    } catch (error) {
      return `Алерты: ⚠️ alertmanager недоступен (${escapeHtml(errorMessage(error))})`;
    }
  }

  private async probesSection(): Promise<string> {
    const results = await Promise.all(this.deps.probes.map(async (probe) => {
      try {
        const response = await this.fetchFn(probe.url, { signal: AbortSignal.timeout(2000) });
        return `${response.ok ? "✅" : "❌"} ${probe.name}${response.ok ? "" : ` (${response.status})`}`;
      } catch {
        return `❌ ${probe.name}`;
      }
    }));
    return `Readiness: ${results.join(" · ")}`;
  }

  private async cmdStatus(): Promise<string> {
    const [pipeline, alerts, probes] = await Promise.all([
      this.pipelineSection(),
      this.alertsSection(),
      this.probesSection(),
    ]);
    return `<b>Status</b>\n${pipeline}\n${alerts}\n${probes}`;
  }

  private async cmdAlerts(): Promise<string> {
    const now = this.now();
    const alerts = await this.activeAlerts();
    if (alerts.length === 0) return "Активных алертов нет ✅";
    const lines = alerts.map((alert) => {
      const severity = alert.labels?.severity === "critical" ? "🔴" : "🟡";
      const name = escapeHtml(alert.labels?.alertname ?? "?");
      const component = alert.labels?.component ? ` [${escapeHtml(alert.labels.component)}]` : "";
      const started = Date.parse(alert.startsAt ?? "");
      const age = Number.isFinite(started) ? ` — ${fmtAge(started, now)}` : "";
      return `${severity} <b>${name}</b>${component}${age}`;
    });
    return `<b>Активные алерты (${alerts.length})</b>\n${lines.join("\n")}`;
  }

  private async cmdDeploys(args: string): Promise<string> {
    const { github, state } = this.deps;
    const n = Math.min(20, Math.max(1, Number.parseInt(args, 10) || 5));
    if (!github) {
      const deploy = state.data.deploy;
      if (!deploy) return "Github не настроен, истории деплоев нет.";
      return `Github не настроен. Текущий SHA: <code>${escapeHtml(deploy.sha.slice(0, 7))}</code> — ${escapeHtml(deploy.title)}`;
    }
    const commits = await this.json<{ sha: string; commit: { message: string } }[]>(
      `https://api.github.com/repos/${github.repo}/commits?sha=master&per_page=${n}`,
      {
        headers: {
          accept: "application/vnd.github+json",
          authorization: `Bearer ${github.token}`,
          "user-agent": "apitoken-devbot",
        },
      },
    );
    const lines: string[] = [];
    for (const commit of commits.slice(0, n)) {
      let icon = "⏳";
      try {
        const combined = await this.json<{ state?: string }>(
          `https://api.github.com/repos/${github.repo}/commits/${commit.sha}/status`,
          {
            headers: {
              accept: "application/vnd.github+json",
              authorization: `Bearer ${github.token}`,
              "user-agent": "apitoken-devbot",
            },
          },
        );
        icon = combined.state === "success" ? "✅" : combined.state === "failure" ? "❌" : "🔄";
      } catch {
        icon = "❔";
      }
      const title = escapeHtml(truncate(commit.commit.message.split("\n")[0] ?? "", 80));
      lines.push(`${icon} <code>${escapeHtml(commit.sha.slice(0, 7))}</code> ${title}`);
    }
    return `<b>Последние ${lines.length} SHA master</b>\n${lines.join("\n")}`;
  }

  private async engineGet(apiPath: string, key: string): Promise<unknown> {
    const engine = this.deps.engine;
    if (!engine) throw new Error("engine not configured");
    return await this.json(`${engine.baseUrl}${apiPath}`, {
      headers: { "x-api-key": key },
      signal: AbortSignal.timeout(5000),
    });
  }

  private async cmdPool(): Promise<string> {
    const engine = this.deps.engine;
    if (!engine?.readonlyKey) return "Engine не настроен (DEVBOT_ENGINE_READONLY_KEY отсутствует).";
    const sections: string[] = [];
    const endpoints: [string, string][] = [["/pool", "Anthropic"], ["/codex-subs", "Codex"], ["/gemini-subs", "Gemini"]];
    for (const [apiPath, label] of endpoints) {
      try {
        const data = await this.engineGet(apiPath, engine.readonlyKey);
        sections.push(`<b>${label}</b>: <code>${escapeHtml(truncate(JSON.stringify(data), 1200))}</code>`);
      } catch (error) {
        sections.push(`<b>${label}</b>: ⚠️ ${escapeHtml(errorMessage(error))}`);
      }
    }
    return `<b>Пул подписок</b>\n${sections.join("\n")}`;
  }

  private async cmdSettlement(): Promise<string> {
    const engine = this.deps.engine;
    if (!engine?.controlKey) return "Engine не настроен (DEVBOT_ENGINE_CONTROL_KEY отсутствует).";
    const data = await this.engineGet("/settlement-health", engine.controlKey);
    return `<b>Settlement health</b>\n<code>${escapeHtml(truncate(JSON.stringify(data, null, 2), 3000))}</code>`;
  }

  /** Единственная команда, которая пишет наружу: silence в Alertmanager (этап 3). */
  private async cmdSilence(args: string): Promise<string> {
    const [alertname, durationRaw] = args.split(/\s+/);
    if (!alertname || !durationRaw) {
      return "Формат: <code>/silence AlertName 2h</code>";
    }
    const durationMs = parseDuration(durationRaw);
    if (durationMs === null) {
      return `Не понял длительность «${escapeHtml(durationRaw)}». Примеры: 30m, 2h, 1d (максимум 7d).`;
    }
    const now = this.now();
    const body = {
      matchers: [{ name: "alertname", value: alertname, isRegex: false, isEqual: true }],
      startsAt: new Date(now).toISOString(),
      endsAt: new Date(now + durationMs).toISOString(),
      createdBy: "devbot",
      comment: "silenced via /silence",
    };
    const response = await this.fetchFn(`${this.deps.alertmanagerUrl}/api/v2/silences`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      throw new Error(`alertmanager silence: HTTP ${response.status}`);
    }
    const result = await response.json() as { silenceID?: string };
    return `🔇 <b>${escapeHtml(alertname)}</b> замьючен на ${escapeHtml(durationRaw)} (id <code>${escapeHtml(result.silenceID ?? "?")}</code>)`;
  }

  private async cmdDigest(): Promise<void> {
    const report = buildDigestReport(this.deps.state.data.events, this.now());
    await this.deps.tg.sendMessage(this.deps.chatId, report, { threadId: this.deps.digestTopicId });
  }
}

export interface PollingLoopDeps {
  bot: TelegramBot;
  handler: CommandHandler;
  logger?: Logger;
  sleep?: (ms: number) => Promise<void>;
  shouldStop?: () => boolean;
}

const defaultSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Long polling команд; ошибки Telegram не роняют цикл — пауза и повтор. */
export async function runPollingLoop(deps: PollingLoopDeps): Promise<void> {
  const sleep = deps.sleep ?? defaultSleep;
  const shouldStop = deps.shouldStop ?? (() => false);
  await deps.bot.deleteWebhook();
  let offset: number | undefined;
  while (!shouldStop()) {
    try {
      const updates = await deps.bot.getUpdates(offset, 30);
      for (const update of updates) {
        offset = update.update_id + 1;
        try {
          await deps.handler.handleUpdate(update);
        } catch (error) {
          deps.logger?.error(`polling: update ${update.update_id} failed: ${errorMessage(error)}`);
        }
      }
    } catch (error) {
      deps.logger?.warn(`polling: getUpdates failed: ${errorMessage(error)} — retry in 5s`);
      await sleep(5000);
    }
  }
}
