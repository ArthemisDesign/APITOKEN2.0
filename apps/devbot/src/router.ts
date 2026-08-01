import { Dedup } from "./dedup.js";
import type { AlertInstance, DeployEvent, JournalEvent } from "./events.js";
import { errorMessage, type Logger } from "./log.js";
import type { DeployState, PhaseState, StateStore, TopicKey } from "./state.js";
import type { TelegramBot } from "./tg.js";

/** Денежные алерты дублируются заголовком-ответом в 💰 Commerce (см. DEVBOT.md §3). */
export const COMMERCE_ALERTS = new Set([
  "FailedWebhooksPresent",
  "StaleCheckoutSessions",
  "SalesPayoutBatchFailed",
  "SalesReferralReconciliationBacklog",
  "DurableQueueBacklog",
  "DurableQueueOldestItemStale",
  "DurableQueueDeadItems",
  "BalanceDivergenceDetected",
  "EngineSettlementBacklog",
  "EngineExpiredLeasePresent",
]);

const DEPLOY_PHASES = ["tests", "migration", "engine", "backend", "sales", "openkeys", "admin", "devbot"];
const DESCRIPTION_LIMIT = 1000;

export function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function truncate(text: string, limit: number): string {
  if (text.length <= limit) return text;
  return `${text.slice(0, limit - 1)}…`;
}

function fmtTime(ts: number, timeZone: string): string {
  return new Date(ts).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", timeZone });
}

function fmtDuration(ms: number): string {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function phaseIcon(state: PhaseState | undefined): string {
  if (state === "success") return "✅";
  if (state === "failure") return "❌";
  if (state === "pending") return "🔄";
  return "⏳";
}

export interface RouterDeps {
  tg: TelegramBot;
  chatId: number;
  topics: Record<TopicKey, number>;
  state: StateStore;
  dedup: Dedup;
  logger?: Logger;
  /** owner/repo — для ссылок на коммиты и runbook. */
  repoSlug: string;
  /** IANA time zone for all operator-facing timestamps. */
  timeZone: string;
  now?: () => number;
  /** Метрика devbot_events_total{topic,kind}. */
  onEvent?: (topic: TopicKey, kind: string) => void;
}

/**
 * Маршрутизация событие → топик + форматирование (DEVBOT.md §3–4).
 * Никогда не бросает наружу: событие теряется с логом, бот живёт дальше.
 */
export class Router {
  private readonly now: () => number;

  constructor(private readonly deps: RouterDeps) {
    this.now = deps.now ?? Date.now;
  }

  private runbookUrl(alertname: string): string {
    return `https://github.com/${this.deps.repoSlug}/blob/master/docs/ops/MONITORING.md#${alertname.toLowerCase()}`;
  }

  private commitUrl(sha: string): string {
    return `https://github.com/${this.deps.repoSlug}/commit/${sha}`;
  }

  private countEvent(topic: TopicKey, kind: string): void {
    this.deps.onEvent?.(topic, kind);
  }

  private formatFiring(alert: AlertInstance, count: number, now: number): string {
    const lines = [
      `🔴 <b>${escapeHtml(alert.alertname)}</b> [${escapeHtml(alert.severity)}${alert.component ? ` · ${escapeHtml(alert.component)}` : ""}]`,
    ];
    if (alert.summary) lines.push(`<i>${escapeHtml(alert.summary)}</i>`);
    if (alert.description) lines.push(escapeHtml(truncate(alert.description, DESCRIPTION_LIMIT)));
    lines.push(`Runbook: <a href="${this.runbookUrl(alert.alertname)}">MONITORING.md#${alert.alertname.toLowerCase()}</a>`);
    let footer = `Started: ${fmtTime(Date.parse(alert.startsAt) || now, this.deps.timeZone)} · fp: ${escapeHtml(alert.fingerprint.slice(0, 8))}`;
    if (count > 1) footer += ` · ×${count}, last: ${fmtTime(now, this.deps.timeZone)}`;
    lines.push(footer);
    return lines.join("\n");
  }

  private commerceHeader(alert: AlertInstance): string {
    return `💰 <b>${escapeHtml(alert.alertname)}</b> [${escapeHtml(alert.severity)}] — см. сообщение выше (reply)`;
  }

  async handleAlert(alert: AlertInstance): Promise<void> {
    try {
      if (alert.status === "resolved") {
        await this.handleResolved(alert);
      } else {
        await this.handleFiring(alert);
      }
    } catch (error) {
      this.deps.logger?.error(`router: alert ${alert.alertname} failed: ${errorMessage(error)}`);
    }
  }

  private async handleFiring(alert: AlertInstance): Promise<void> {
    const { state, dedup, tg, chatId, topics } = this.deps;
    const now = this.now();
    const severity = alert.severity === "critical" ? "critical" : "warning";
    const topic: TopicKey = severity === "critical" ? "critical" : "warnings";
    state.recordEvent({ ts: now, kind: "alert", name: alert.alertname, severity }, now);

    if (severity === "critical") {
      const storm = dedup.trackCritical(alert.alertname, now);
      if (storm.suppressed) {
        await this.handleStorm(storm.names, storm.total, storm.started, now);
        await state.save();
        return;
      }
    }

    const existing = dedup.lookup(alert.fingerprint, now);
    if (existing && !existing.resolved) {
      dedup.markRepeat(existing, now);
      if (severity === "warning" && !dedup.warningEditAllowed(existing, now)) {
        await state.save();
        return;
      }
      const ok = await tg.editMessageText(chatId, existing.messageId, this.formatFiring(alert, existing.count, now));
      dedup.markEdited(existing, now);
      if (!ok) {
        const messageId = await tg.sendMessage(chatId, this.formatFiring(alert, existing.count, now), {
          threadId: topics[existing.topic],
        });
        if (messageId !== null) existing.messageId = messageId;
      }
      this.countEvent(existing.topic, "alert-repeat");
      await state.save();
      return;
    }

    const text = this.formatFiring(alert, 1, now);
    const messageId = await tg.sendMessage(chatId, text, { threadId: topics[topic] });
    if (messageId !== null) {
      dedup.register(alert.fingerprint, { messageId, topic, now });
    }
    this.countEvent(topic, "alert");
    if (COMMERCE_ALERTS.has(alert.alertname)) {
      await tg.sendMessage(chatId, this.commerceHeader(alert), {
        threadId: topics.commerce,
        ...(messageId !== null ? { replyTo: messageId } : {}),
      });
      this.countEvent("commerce", "alert-dup");
    }
    await state.save();
  }

  private async handleStorm(names: string[], total: number, started: boolean, now: number): Promise<void> {
    const { tg, chatId, topics } = this.deps;
    const list = names.map((name) => `• ${escapeHtml(name)}`).join("\n");
    const text = `🔥 <b>${total} active critical alerts</b> (storm mode, сворачиваю до 10 мин тишины)\n${list}`;
    const previous = this.stormMessageId;
    if (!started && previous !== undefined) {
      const ok = await tg.editMessageText(chatId, previous, text);
      if (ok) {
        this.countEvent("critical", "storm");
        return;
      }
    }
    const messageId = await tg.sendMessage(chatId, text, { threadId: topics.critical });
    if (messageId !== null) this.stormMessageId = messageId;
    this.countEvent("critical", started ? "storm-start" : "storm");
  }

  private stormMessageId: number | undefined;

  private async handleResolved(alert: AlertInstance): Promise<void> {
    const { state, dedup, tg, chatId, topics } = this.deps;
    const now = this.now();
    const entry = dedup.lookup(alert.fingerprint, now);
    const startedMs = Date.parse(alert.startsAt);
    const duration = Number.isFinite(startedMs) ? fmtDuration(Math.max(0, now - startedMs)) : "?";
    const header = `🟢 <b>RESOLVED</b> <b>${escapeHtml(alert.alertname)}</b> [${escapeHtml(alert.severity)}] · duration ${duration}`;

    if (alert.severity === "critical") {
      // Для 🚨 Critical resolved — всегда отдельное сообщение-закрытие в ленте.
      await tg.sendMessage(chatId, header, {
        threadId: topics.critical,
        ...(entry ? { replyTo: entry.messageId } : {}),
      });
      if (entry) dedup.markResolved(entry, now);
      this.countEvent("critical", "alert-resolved");
      await state.save();
      return;
    }

    if (entry) {
      dedup.markResolved(entry, now);
      // Warning: правим исходное сообщение, если оно в пределах 48 ч (лимит Bot API на edit).
      const withinEditWindow = now - entry.firstAt < 48 * 3600 * 1000;
      if (withinEditWindow) {
        const ok = await tg.editMessageText(chatId, entry.messageId, header);
        if (ok) {
          this.countEvent("warnings", "alert-resolved");
          await state.save();
          return;
        }
      }
      await tg.sendMessage(chatId, header, { threadId: topics.warnings, replyTo: entry.messageId });
    } else {
      await tg.sendMessage(chatId, header, { threadId: topics.warnings });
    }
    this.countEvent("warnings", "alert-resolved");
    await state.save();
  }

  // ---------------------------------------------------------------- deploys

  private renderDeploy(deploy: DeployState, finished?: { ok: boolean; durationMs: number }): string {
    const short = deploy.sha.slice(0, 7);
    const title = escapeHtml(truncate(deploy.title, 120));
    // Финальное состояние читается с первого взгляда по заголовку, а не только по футеру.
    const header = finished
      ? `${finished.ok ? "✅ <b>Deployed</b>" : "❌ <b>Deploy failed</b>"} <code>${escapeHtml(short)}</code> — ${title}`
      : `🚀 <b>Deploy</b> <code>${escapeHtml(short)}</code> — ${title}`;
    const author = deploy.author ? `👤 <b>${escapeHtml(deploy.author)}</b>` : undefined;
    const commitLink = `<a href="${this.commitUrl(deploy.sha)}">commit</a>`;
    const text = (part: string | undefined): part is string => part !== undefined;

    // Успешный финал: промежуточные фазы не нужны — компактная сводка из двух строк.
    if (finished?.ok) {
      const meta = [author, `<i>done in ${fmtDuration(finished.durationMs)}</i>`, commitLink].filter(text);
      return [header, meta.join(" · ")].join("\n");
    }

    const meta = [
      author,
      finished ? `<i>failed in ${fmtDuration(finished.durationMs)}</i>` : `<i>Started ${fmtTime(deploy.startedAt, this.deps.timeZone)}</i>`,
      commitLink,
    ].filter(text);
    // Чеклист — две фиксированные строки 4+4, чтобы перенос не рвал фазы посередине.
    const icons = DEPLOY_PHASES.map((phase) => `${phaseIcon(deploy.phases[phase])} ${phase}`);
    const lines = [header, meta.join(" · "), icons.slice(0, 4).join(" · "), icons.slice(4).join(" · ")];
    if (deploy.failedPhase) lines.push(`❌ failed phase: <b>${escapeHtml(deploy.failedPhase)}</b>`);
    return lines.join("\n");
  }

  async handleDeployEvent(event: DeployEvent): Promise<void> {
    try {
      switch (event.kind) {
        case "new-sha":
          await this.deployNewSha(event.sha, event.title, event.author);
          break;
        case "phase":
          await this.deployPhase(event.sha, event.phase, event.state);
          break;
        case "green":
          await this.deployGreen(event.sha);
          break;
        case "quarantine":
          await this.deployQuarantine(event.sha, event.phase);
          break;
      }
    } catch (error) {
      this.deps.logger?.error(`router: deploy event ${event.kind} failed: ${errorMessage(error)}`);
    }
  }

  /**
   * Один GitHub snapshot может принести new-sha, несколько фаз и watchdog-финал.
   * Обрабатываем пакет строго последовательно: message_id нового Telegram-сообщения
   * должен появиться до первой фазовой или финальной правки.
   */
  async handleDeployEvents(events: DeployEvent[]): Promise<void> {
    for (const event of events) {
      await this.handleDeployEvent(event);
    }
  }

  private async deployNewSha(sha: string, title: string, author?: string): Promise<void> {
    const { state, tg, chatId, topics } = this.deps;
    const now = this.now();
    // Текущий деплой уходит в previousDeploy: его финал ещё может прийти от tail-опроса
    // поллера (HEAD ушёл вперёд раньше, чем мы увидели deploy/watchdog=success) — и тогда
    // править нужно уже ЕГО сообщение, а не новое.
    if (state.data.deploy && !state.data.deploy.done) {
      state.data.previousDeploy = state.data.deploy;
    }
    const deploy: DeployState = {
      sha,
      title,
      ...(author ? { author } : {}),
      messageId: null,
      startedAt: now,
      phases: {},
      done: false,
    };
    state.data.deploy = deploy;
    state.data.lastProcessedSha = sha;
    const messageId = await tg.sendMessage(chatId, this.renderDeploy(deploy), { threadId: topics.deploys });
    deploy.messageId = messageId;
    this.countEvent("deploys", "new-sha");
    await state.save();
  }

  /** Деплой по SHA: актуальный или предыдущий (события tail'а адресуются старому). */
  private findDeploy(sha: string): DeployState | undefined {
    const { deploy, previousDeploy } = this.deps.state.data;
    if (deploy?.sha === sha) return deploy;
    if (previousDeploy?.sha === sha) return previousDeploy;
    return undefined;
  }

  private async editDeploy(deploy: DeployState, finished?: { ok: boolean; durationMs: number }): Promise<void> {
    if (deploy.messageId === null) return;
    await this.deps.tg.editMessageText(this.deps.chatId, deploy.messageId, this.renderDeploy(deploy, finished));
  }

  private async deployPhase(sha: string, phase: string, phaseState: PhaseState): Promise<void> {
    const { state } = this.deps;
    const deploy = this.findDeploy(sha);
    // Финальный рендер авторитетен: поздняя фаза не должна разворачивать сводку обратно.
    if (!deploy || deploy.done) return;
    deploy.phases[phase] = phaseState;
    if (phaseState === "failure") deploy.failedPhase = phase;
    await this.editDeploy(deploy);
    this.countEvent("deploys", "phase");
    await state.save();
  }

  private async deployGreen(sha: string): Promise<void> {
    const { state } = this.deps;
    const deploy = this.findDeploy(sha);
    if (!deploy || deploy.done) return;
    deploy.done = true;
    const now = this.now();
    // deploy/watchdog зелёный — значит, прошли ВСЕ лейны, включая те, чей последний
    // известный нам статус ещё pending/неизвестен (наблюдалось в проде: финальное
    // сообщение «done», а engine навсегда 🔄). Нормализуем всё, кроме явных failure.
    for (const phase of DEPLOY_PHASES) {
      if (deploy.phases[phase] !== "failure") deploy.phases[phase] = "success";
    }
    state.recordEvent({ ts: now, kind: "deploy", name: sha.slice(0, 7), severity: "success" }, now);
    await this.editDeploy(deploy, { ok: true, durationMs: now - deploy.startedAt });
    this.countEvent("deploys", "green");
    await state.save();
  }

  private async deployQuarantine(sha: string, phase?: string): Promise<void> {
    const { state, tg, chatId, topics } = this.deps;
    const deploy = this.findDeploy(sha);
    // Phase failure and watchdog failure can arrive in separate polls. The first
    // terminal event owns both the message edit and the Critical notification.
    if (deploy?.done) return;
    const now = this.now();
    if (deploy) {
      deploy.done = true;
      if (phase) deploy.failedPhase = phase;
      await this.editDeploy(deploy, { ok: false, durationMs: now - deploy.startedAt });
    }
    state.recordEvent({ ts: now, kind: "deploy", name: sha.slice(0, 7), severity: "quarantine" }, now);
    const text = `🚨 <b>Deploy quarantined</b> <code>${escapeHtml(sha.slice(0, 7))}</code>${phase ? ` — phase <b>${escapeHtml(phase)}</b> failed` : ""}\n<a href="${this.commitUrl(sha)}">commit</a>${deploy?.author ? ` · 👤 ${escapeHtml(deploy.author)}` : ""} · SHA в карантине, пайплайн остановлен`;
    await tg.sendMessage(chatId, text, { threadId: topics.critical });
    this.countEvent("critical", "quarantine");
    await state.save();
  }

  // ---------------------------------------------------------------- journald

  async handleJournalEvent(event: JournalEvent): Promise<void> {
    try {
      const { tg, chatId, topics } = this.deps;
      const icon = event.severity === "critical" ? "🚨" : event.severity === "warning" ? "⚠️" : "ℹ️";
      const text = `${icon} <b>[${escapeHtml(event.source)}]</b> ${escapeHtml(truncate(event.text, 800))}`;
      await tg.sendMessage(chatId, text, { threadId: topics.deploys });
      this.countEvent("deploys", `journal-${event.severity}`);
      if (event.severity === "critical") {
        await tg.sendMessage(chatId, text, { threadId: topics.critical });
        this.countEvent("critical", "journal");
      }
    } catch (error) {
      this.deps.logger?.error(`router: journal event failed: ${errorMessage(error)}`);
    }
  }
}
