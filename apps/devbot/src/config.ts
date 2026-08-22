import { z } from "zod";

/** Репозиторий, за которым следит поллер (origin remote этого репо). */
export const DEFAULT_GITHUB_REPO = "3xcalibur-tech/Claude_API";
export const DEFAULT_TIME_ZONE = "Asia/Tbilisi";

const timeZone = z.string().refine((value) => {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: value }).format();
    return true;
  } catch {
    return false;
  }
}, "DEVBOT_TIME_ZONE must be a valid IANA time zone");

const optionalSecret = (min: number) =>
  z.preprocess(
    (value) => (typeof value === "string" && value.trim() === "" ? undefined : value),
    z.string().min(min).optional(),
  );

const boolFromString = (defaultValue: "true" | "false") =>
  z.enum(["true", "false"])
    .transform((value) => value === "true")
    .default(defaultValue);

const threadId = (name: string) =>
  z.coerce.number({ invalid_type_error: `${name} must be a numeric Telegram thread id` })
    .int()
    .positive();

const configSchema = z.object({
  DEVBOT_TELEGRAM_TOKEN: z.string().min(10),
  DEVBOT_CHAT_ID: z.coerce.number().int(),
  DEVBOT_ADMIN_IDS: z.string()
    .regex(/^\s*\d+(\s*,\s*\d+)*\s*$/, "DEVBOT_ADMIN_IDS must be comma-separated numeric user ids")
    .transform((value) => value.split(",").map((part) => Number(part.trim()))),
  DEVBOT_TOPIC_CRITICAL: threadId("DEVBOT_TOPIC_CRITICAL"),
  DEVBOT_TOPIC_DEPLOYS: threadId("DEVBOT_TOPIC_DEPLOYS"),
  DEVBOT_TOPIC_WARNINGS: threadId("DEVBOT_TOPIC_WARNINGS"),
  DEVBOT_TOPIC_COMMERCE: threadId("DEVBOT_TOPIC_COMMERCE"),
  DEVBOT_TOPIC_DIGEST: threadId("DEVBOT_TOPIC_DIGEST"),
  DEVBOT_TOPIC_SUPPORT: z.preprocess(
    (value) => (typeof value === "string" && value.trim() === "" ? undefined : value),
    threadId("DEVBOT_TOPIC_SUPPORT").optional(),
  ),
  DEVBOT_PORT: z.coerce.number().int().min(1).max(65_535).default(3800),
  DEVBOT_AM_SECRET: z.string().min(16),
  DEVBOT_GITHUB_TOKEN: optionalSecret(10),
  DEVBOT_GITHUB_REPO: z.string().regex(/^[\w.-]+\/[\w.-]+$/).default(DEFAULT_GITHUB_REPO),
  DEVBOT_POLL_GITHUB_MS: z.coerce.number().int().min(5_000).default(45_000),
  DEVBOT_TIME_ZONE: timeZone.default(DEFAULT_TIME_ZONE),
  DEVBOT_ALERTMANAGER_URL: z.string().url().default("http://127.0.0.1:9093"),
  DEVBOT_ENGINE_READONLY_KEY: optionalSecret(16),
  DEVBOT_ENGINE_CONTROL_KEY: optionalSecret(16),
  DEVBOT_ENGINE_BASE_URL: z.string().url().default("http://127.0.0.1:8790"),
  DEVBOT_STATE_FILE: z.string().min(1).default("/var/lib/apitoken/devbot/state.json"),
  DEVBOT_HEARTBEAT_FILE: z.string().min(1)
    .default("/var/lib/apitoken/monitoring/textfile/devbot.prom"),
  DEVBOT_JOURNALD_ENABLED: boolFromString("false"),
  DEVBOT_LOG_LEVEL: z.enum(["debug", "info", "warn", "error"]).default("info"),
  DEVBOT_CHATWOOT_SECRET: optionalSecret(16),
  DEVBOT_CHATWOOT_HMAC_SECRET: optionalSecret(8),
  DEVBOT_CHATWOOT_BASE_URL: z.string().url().default("https://support.apitoken.sale"),
});

export type RawConfig = z.infer<typeof configSchema>;

export interface TopicMap {
  critical: number;
  deploys: number;
  warnings: number;
  commerce: number;
  digest: number;
  /** 0 = Chatwoot support topic not provisioned; intake stays disabled. */
  support: number;
}

export interface DevbotConfig {
  telegramToken: string;
  chatId: number;
  adminIds: ReadonlySet<number>;
  topics: TopicMap;
  port: number;
  amSecret: string;
  githubToken?: string;
  githubRepo: string;
  pollGithubMs: number;
  timeZone: string;
  alertmanagerUrl: string;
  engineReadonlyKey?: string;
  engineControlKey?: string;
  engineBaseUrl: string;
  stateFile: string;
  heartbeatFile: string;
  journaldEnabled: boolean;
  logLevel: "debug" | "info" | "warn" | "error";
  chatwootSecret?: string;
  chatwootHmacSecret?: string;
  chatwootBaseUrl: string;
}

/** Парсит произвольный env-like объект — безопасно вызывать в тестах без process.env. */
export function parseConfig(env: Record<string, unknown>): DevbotConfig {
  const raw = configSchema.parse(env);
  const config: DevbotConfig = {
    telegramToken: raw.DEVBOT_TELEGRAM_TOKEN,
    chatId: raw.DEVBOT_CHAT_ID,
    adminIds: new Set(raw.DEVBOT_ADMIN_IDS),
    topics: {
      critical: raw.DEVBOT_TOPIC_CRITICAL,
      deploys: raw.DEVBOT_TOPIC_DEPLOYS,
      warnings: raw.DEVBOT_TOPIC_WARNINGS,
      commerce: raw.DEVBOT_TOPIC_COMMERCE,
      digest: raw.DEVBOT_TOPIC_DIGEST,
      support: raw.DEVBOT_TOPIC_SUPPORT ?? 0,
    },
    port: raw.DEVBOT_PORT,
    amSecret: raw.DEVBOT_AM_SECRET,
    githubRepo: raw.DEVBOT_GITHUB_REPO,
    pollGithubMs: raw.DEVBOT_POLL_GITHUB_MS,
    timeZone: raw.DEVBOT_TIME_ZONE,
    alertmanagerUrl: raw.DEVBOT_ALERTMANAGER_URL.replace(/\/+$/, ""),
    engineBaseUrl: raw.DEVBOT_ENGINE_BASE_URL.replace(/\/+$/, ""),
    stateFile: raw.DEVBOT_STATE_FILE,
    heartbeatFile: raw.DEVBOT_HEARTBEAT_FILE,
    journaldEnabled: raw.DEVBOT_JOURNALD_ENABLED,
    logLevel: raw.DEVBOT_LOG_LEVEL,
    chatwootBaseUrl: raw.DEVBOT_CHATWOOT_BASE_URL.replace(/\/+$/, ""),
  };
  if (raw.DEVBOT_GITHUB_TOKEN !== undefined) config.githubToken = raw.DEVBOT_GITHUB_TOKEN;
  if (raw.DEVBOT_ENGINE_READONLY_KEY !== undefined) config.engineReadonlyKey = raw.DEVBOT_ENGINE_READONLY_KEY;
  if (raw.DEVBOT_ENGINE_CONTROL_KEY !== undefined) config.engineControlKey = raw.DEVBOT_ENGINE_CONTROL_KEY;
  if (raw.DEVBOT_CHATWOOT_SECRET !== undefined) config.chatwootSecret = raw.DEVBOT_CHATWOOT_SECRET;
  if (raw.DEVBOT_CHATWOOT_HMAC_SECRET !== undefined) config.chatwootHmacSecret = raw.DEVBOT_CHATWOOT_HMAC_SECRET;
  return config;
}

export function loadConfig(): DevbotConfig {
  return parseConfig(process.env);
}
