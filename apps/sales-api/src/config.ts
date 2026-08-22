import { z } from "zod";

const BSC_MAINNET_CHAIN_ID = 56;
const BSC_USDT_CONTRACT = "0x55d398326f99059fF775485246999027B3197955";

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function gweiUnits(value: string): bigint | null {
  const match = /^(0|[1-9]\d*)(?:\.(\d{1,9}))?$/.exec(value);
  if (!match) return null;
  return BigInt(match[1]!) * 1_000_000_000n + BigInt((match[2] ?? "").padEnd(9, "0"));
}

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  PORT: z.coerce.number().int().min(1).max(65_535).default(3100),
  HOST: z.string().default("127.0.0.1"),
  API_DRAIN_DEADLINE_MS: z.coerce.number().int().min(1_000).max(29_000).default(25_000),
  SALES_DATABASE_URL: z.string().url(),
  SALES_TOKEN_ENCRYPTION_KEY: z.string().regex(/^[A-Za-z0-9_-]{43}$/),
  SALES_SESSION_TTL_SECONDS: z.coerce.number().int().min(900).max(31_536_000).default(2_592_000),
  // In-process TTL кэша разрешённых сессий (0 = выключен). Отзыв через logout инвалидируется
  // сразу; отзыв прав админа (suspend) распространяется в пределах этого TTL.
  SALES_SESSION_CACHE_TTL_SECONDS: z.coerce.number().int().min(0).max(300).default(30),
  // Telegram Login Widget: оба заданы — вход включён; иначе /v1/auth/* отвечает 503.
  // Пустая строка в env == «не задано» (удобно для заготовленных строк в env-файле).
  TELEGRAM_BOT_TOKEN: z.preprocess((v) => (v === "" ? undefined : v), z.string().min(30).optional()),
  TELEGRAM_BOT_USERNAME: z.preprocess(
    (v) => (v === "" ? undefined : v),
    z.string().regex(/^[A-Za-z0-9_]{5,32}$/).optional(),
  ),
  SALES_ADMIN_KEY: z.string().min(32),
  COMMERCE_BASE_URL: z.string().url().default("http://127.0.0.1:8791"),
  SALES_CONTROL_KEY: z.string().min(32),
  PUBLIC_SALES_BASE_URL: z.string().url().default("https://partners.apitoken.sale"),
  PUBLIC_MAIN_SITE_URL: z.string().url().default("https://apitoken.sale"),
  EMAIL_VERIFICATION_TTL_SECONDS: z.coerce.number().int().min(900).max(604_800).default(86_400),
  PASSWORD_RESET_TTL_SECONDS: z.coerce.number().int().min(300).max(86_400).default(3_600),
  EMAIL_DELIVERY_MODE: z.enum(["log", "smtp"]).default("log"),
  EMAIL_FROM: z.string().email().optional(),
  SMTP_HOST: z.string().min(1).optional(),
  SMTP_PORT: z.coerce.number().int().min(1).max(65_535).optional(),
  SMTP_SECURE: z.enum(["true", "false"]).transform((value) => value === "true").default("true"),
  SMTP_USERNAME: z.string().min(1).optional(),
  SMTP_PASSWORD: z.string().min(1).optional(),
  SYNC_INTERVAL_MS: z.coerce.number().int().min(1_000).default(60_000),
  EMAIL_POLL_INTERVAL_MS: z.coerce.number().int().min(100).default(5_000),
  PARTNER_EFFECT_POLL_INTERVAL_MS: z.coerce.number().int().min(100).max(60_000).default(1_000),
  PARTNER_EFFECT_LEASE_SECONDS: z.coerce.number().int().min(10).max(3_600).default(60),
  DEFAULT_COMMISSION_BPS: z.coerce.number().int().min(0).max(10_000).default(1000),
  DEFAULT_SUB_COMMISSION_BPS: z.coerce.number().int().min(0).max(10_000).default(1000),
  // Минимальный профит за окно выплат: 0 = платим любую сумму > 0 (по решению юзера).
  // Оставлен как knob на будущее; UI про него не упоминает.
  SALES_MIN_PAYOUT_USD: z.coerce.number().int().min(0).max(100_000).default(0),
  // --- On-chain выплаты (USDT BEP-20). Секреты только server-side; браузеру/логам не отдаём.
  // Пустая строка == не задано. Если ключ/RPC не заданы — движок выплат отключён (endpoints → 503).
  PAYOUT_HOT_WALLET_KEY: z.preprocess((v) => (v === "" ? undefined : v), z.string().regex(/^(0x)?[0-9a-fA-F]{64}$/).optional()),
  PAYOUT_SEND_RPC_URL: z.preprocess(
    (v) => (v === "" ? undefined : v),
    z.string().url().refine(isHttpUrl, "payout send RPC must use HTTP(S)").optional(),
  ),
  // Публичные BSC RPC для чтения (баланс/нонс/квитанции), через запятую, с фолбэком.
  PAYOUT_READ_RPC_URLS: z.string().refine(
    (value) => {
      const urls = value.split(",").map((item) => item.trim()).filter(Boolean);
      return urls.length > 0 && urls.every(isHttpUrl);
    },
    "payout read RPC list must contain at least one HTTP(S) URL",
  ).default(
    "https://bsc-dataseed.binance.org,https://bsc-dataseed1.defibit.io,https://bsc-dataseed1.ninicoin.io,https://bsc.publicnode.com,https://binance.llamarpc.com",
  ),
  PAYOUT_USDT_CONTRACT: z.string()
    .regex(/^0x[0-9a-fA-F]{40}$/)
    .refine((value) => value.toLowerCase() === BSC_USDT_CONTRACT.toLowerCase(), "only canonical BSC USDT is supported")
    .default(BSC_USDT_CONTRACT),
  PAYOUT_CHAIN_ID: z.coerce.number().int()
    .refine((value) => value === BSC_MAINNET_CHAIN_ID, "only BSC mainnet is supported")
    .default(BSC_MAINNET_CHAIN_ID),
  PAYOUT_GAS_PRICE_GWEI: z.string().refine(
    (value) => {
      const units = gweiUnits(value);
      return units !== null && units > 0n && units <= 100_000_000_000n;
    },
    "payout gas price must be a positive decimal no greater than 100 gwei",
  ).default("0.05"),
  PAYOUT_CONFIRMATIONS: z.coerce.number().int().min(1).max(50).default(3),
  // Жёсткий гейт окна выплат. true (по умолчанию) = отправка ФИЗИЧЕСКИ возможна только в 3-дневное
  // окно; вне окна send-endpoints отвечают 423. false — только для теста/эксплуатации вручную.
  PAYOUT_ENFORCE_WINDOW: z.enum(["true", "false"]).transform((v) => v === "true").default("true"),
}).superRefine((value, context) => {
  if (value.EMAIL_DELIVERY_MODE === "smtp" && (!value.EMAIL_FROM || !value.SMTP_HOST || !value.SMTP_PORT)) {
    context.addIssue({ code: "custom", message: "EMAIL_FROM, SMTP_HOST and SMTP_PORT are required for SMTP delivery" });
  }
  if (Boolean(value.SMTP_USERNAME) !== Boolean(value.SMTP_PASSWORD)) {
    context.addIssue({ code: "custom", message: "SMTP_USERNAME and SMTP_PASSWORD must be configured together" });
  }
  if (value.NODE_ENV === "production" && value.EMAIL_DELIVERY_MODE !== "smtp") {
    context.addIssue({ code: "custom", message: "SMTP email delivery is required in production" });
  }
  if (Boolean(value.PAYOUT_HOT_WALLET_KEY) !== Boolean(value.PAYOUT_SEND_RPC_URL)) {
    context.addIssue({ code: "custom", message: "PAYOUT_HOT_WALLET_KEY and PAYOUT_SEND_RPC_URL must be configured together" });
  }
});

export type Environment = z.infer<typeof environmentSchema>;

export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
