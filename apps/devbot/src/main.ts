import { createAmServer, MetricsRegistry } from "./am-webhook.js";
import { CommandHandler, msUntilNext, runPollingLoop, buildDigestReport, type ProbeTarget } from "./commands.js";
import { loadConfig } from "./config.js";
import { Dedup } from "./dedup.js";
import { GithubPoller } from "./github-poller.js";
import { JournaldTail } from "./journald.js";
import { errorMessage, Logger } from "./log.js";
import { Router } from "./router.js";
import { StateStore } from "./state.js";
import { TelegramBot } from "./tg.js";

const SHUTDOWN_TIMEOUT_MS = 5000;

async function main(): Promise<void> {
  const config = loadConfig();
  const logger = new Logger(config.logLevel);
  logger.info(`starting devbot on 127.0.0.1:${config.port}`);

  const state = new StateStore(config.stateFile, logger);
  await state.load();
  const dedup = new Dedup(state.data.fingerprints);
  const metrics = new MetricsRegistry();

  const tg = new TelegramBot({
    token: config.telegramToken,
    logger,
    onSendFailure: () => metrics.incTelegramFailure(),
  });

  const router = new Router({
    tg,
    chatId: config.chatId,
    topics: config.topics,
    state,
    dedup,
    logger,
    repoSlug: config.githubRepo,
    timeZone: config.timeZone,
    onEvent: (topic, kind) => metrics.incEvent(topic, kind),
  });

  const am = createAmServer({
    port: config.port,
    secret: config.amSecret,
    metrics,
    logger,
    heartbeatFile: config.heartbeatFile,
    onAlerts: (alerts) => {
      for (const alert of alerts) void router.handleAlert(alert);
    },
  });
  await new Promise<void>((resolve, reject) => {
    am.server.once("error", reject);
    am.server.listen(config.port, "127.0.0.1", () => resolve());
  });

  let poller: GithubPoller | undefined;
  if (config.githubToken) {
    poller = new GithubPoller({
      token: config.githubToken,
      repo: config.githubRepo,
      state,
      logger,
      intervalMs: config.pollGithubMs,
      onEvents: (events) => router.handleDeployEvents(events),
    });
    poller.start();
  } else {
    logger.info("DEVBOT_GITHUB_TOKEN не задан — github-поллер выключен");
  }

  let journald: JournaldTail | undefined;
  if (config.journaldEnabled) {
    journald = new JournaldTail({
      logger,
      onEvent: (event) => void router.handleJournalEvent(event),
    });
    journald.start();
  }

  const probes: ProbeTarget[] = [
    { name: "engine-anthropic", url: "http://127.0.0.1:8790/ready" },
    { name: "engine-codex", url: "http://127.0.0.1:8792/ready" },
    { name: "engine-gemini", url: "http://127.0.0.1:8794/ready" },
    { name: "router", url: "http://127.0.0.1:8798/health" },
    { name: "commerce", url: "http://127.0.0.1:8791/v1/ready" },
  ];

  const commands = new CommandHandler({
    tg,
    chatId: config.chatId,
    adminIds: config.adminIds,
    state,
    alertmanagerUrl: config.alertmanagerUrl,
    probes,
    digestTopicId: config.topics.digest,
    logger,
    ...(config.engineReadonlyKey || config.engineControlKey
      ? {
        engine: {
          baseUrl: config.engineBaseUrl,
          ...(config.engineReadonlyKey ? { readonlyKey: config.engineReadonlyKey } : {}),
          ...(config.engineControlKey ? { controlKey: config.engineControlKey } : {}),
        },
      }
      : {}),
    ...(config.githubToken ? { github: { token: config.githubToken, repo: config.githubRepo } } : {}),
  });

  // Ежедневный дайджест в 10:00 локального времени в топик 📊 Digest.
  let digestTimer: NodeJS.Timeout | undefined;
  const scheduleDigest = () => {
    digestTimer = setTimeout(() => {
      void tg.sendMessage(config.chatId, buildDigestReport(state.data.events, Date.now()), {
        threadId: config.topics.digest,
      }).finally(() => {
        void state.save();
        scheduleDigest();
      });
    }, msUntilNext(10, 0, Date.now(), config.timeZone));
    digestTimer.unref();
  };
  scheduleDigest();

  let stopping = false;
  const shutdown = (signal: string) => {
    if (stopping) return;
    stopping = true;
    logger.info(`got ${signal}, shutting down`);
    poller?.stop();
    journald?.stop();
    if (digestTimer) clearTimeout(digestTimer);
    setTimeout(() => process.exit(0), SHUTDOWN_TIMEOUT_MS).unref();
    void state.save().then(() => am.close()).then(() => process.exit(0));
  };
  process.on("SIGTERM", () => shutdown("SIGTERM"));
  process.on("SIGINT", () => shutdown("SIGINT"));

  // Периодическая чистка TTL-хранилища и сохранение журнала событий.
  const pruneTimer = setInterval(() => {
    dedup.prune(Date.now());
    void state.save();
  }, 10 * 60 * 1000);
  pruneTimer.unref();

  await runPollingLoop({
    bot: tg,
    handler: commands,
    logger,
    shouldStop: () => stopping,
  });
}

main().catch((error) => {
  console.error(`[devbot] fatal: ${errorMessage(error)}`);
  process.exit(1);
});
