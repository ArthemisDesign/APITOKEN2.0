import { z } from "zod";

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  PORT: z.coerce.number().int().min(1).max(65_535).default(3100),
  HOST: z.string().default("127.0.0.1"),
  API_DRAIN_DEADLINE_MS: z.coerce.number().int().min(1_000).max(29_000).default(25_000),
  SALES_DATABASE_URL: z.string().url(),
  SALES_TOKEN_ENCRYPTION_KEY: z.string().regex(/^[A-Za-z0-9_-]{43}$/),
  SALES_SESSION_TTL_SECONDS: z.coerce.number().int().min(900).max(31_536_000).default(2_592_000),
  // Telegram Login Widget: оба заданы — вход включён; иначе /v1/auth/* отвечает 503.
  TELEGRAM_BOT_TOKEN: z.string().min(30).optional(),
  TELEGRAM_BOT_USERNAME: z.string().regex(/^[A-Za-z0-9_]{5,32}$/).optional(),
  SALES_ADMIN_KEY: z.string().min(32),
  COMMERCE_BASE_URL: z.string().url().default("http://127.0.0.1:3000"),
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
  DEFAULT_COMMISSION_BPS: z.coerce.number().int().min(0).max(10_000).default(1000),
  DEFAULT_SUB_COMMISSION_BPS: z.coerce.number().int().min(0).max(10_000).default(1000),
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
});

export type Environment = z.infer<typeof environmentSchema>;

export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
