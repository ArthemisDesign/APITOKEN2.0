import { z } from "zod";

function isLoopbackHostname(hostname: string): boolean {
  const normalized = hostname.toLowerCase().replace(/^\[|\]$/g, "");
  if (normalized === "localhost" || normalized === "::1") return true;

  const octets = normalized.split(".");
  return octets.length === 4
    && octets.every((octet) => /^\d{1,3}$/.test(octet) && Number(octet) <= 255)
    && Number(octets[0]) === 127;
}

const engineBaseUrlSchema = z.string().url().superRefine((value, context) => {
  const url = new URL(value);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    context.addIssue({ code: "custom", message: "ENGINE_BASE_URL must use HTTP or HTTPS" });
  }
  if (url.username || url.password) {
    context.addIssue({ code: "custom", message: "ENGINE_BASE_URL must not include credentials" });
  }
  if (url.pathname !== "/" || url.search || url.hash) {
    context.addIssue({ code: "custom", message: "ENGINE_BASE_URL must contain only an origin" });
  }
  if (url.protocol === "http:" && !isLoopbackHostname(url.hostname)) {
    context.addIssue({ code: "custom", message: "HTTP ENGINE_BASE_URL is allowed only for loopback hosts" });
  }
});

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  PORT: z.coerce.number().int().min(1).max(65_535).default(3000),
  HOST: z.string().default("127.0.0.1"),
  API_DRAIN_DEADLINE_MS: z.coerce.number().int().min(1_000).max(29_000).default(25_000),
  DATABASE_URL: z.string().url(),
  ENGINE_BASE_URL: engineBaseUrlSchema,
  ENGINE_CONTROL_KEY: z.string().min(32),
  ENGINE_TIMEOUT_MS: z.coerce.number().int().positive().default(10_000),
  PUBLIC_API_BASE_URL: z.string().url().default("https://backend.apitoken.sale"),
  PUBLIC_APP_BASE_URL: z.string().url().default("https://apitoken.sale"),
  MIN_TOPUP_USD: z.string().regex(/^[1-9]\d*$/).transform(BigInt).default("1"),
  MAX_TOPUP_USD: z.string().regex(/^[1-9]\d*$/).transform(BigInt).default("10000"),
  SESSION_TTL_SECONDS: z.coerce.number().int().min(900).max(2_592_000).default(604_800),
  AUTH_TOKEN_ENCRYPTION_KEY: z.string().regex(/^[A-Za-z0-9_-]{43}$/),
  EMAIL_VERIFICATION_REQUIRED: z.enum(["true", "false"])
    .transform((value) => value === "true")
    .default("false"),
  EMAIL_VERIFICATION_TTL_SECONDS: z.coerce.number().int().min(900).max(604_800).default(86_400),
  PASSWORD_RESET_TTL_SECONDS: z.coerce.number().int().min(300).max(86_400).default(3_600),
  COMMERCIAL_ADMIN_KEY: z.string().min(32).optional(),
  CONTENT_STUDIO_ENGINE_URL: z.string().url().default("https://api.apitoken.sale"),
  CONTENT_STUDIO_ENGINE_KEY: z.preprocess((value) => value === "" ? undefined : value, z.string().min(20).optional()),
  CONTENT_STUDIO_AI_MODEL: z.string().default("claude-sonnet-5"),
  CONTENT_STUDIO_AI_MAX_TOKENS: z.coerce.number().int().min(512).max(64_000).default(12_000),
  INDEXNOW_KEY: z.string().regex(/^[A-Za-z0-9_-]{8,128}$/).default("1308f989ba68a288dc5acf1423b445f8"),
  // Ключ internal-фида для sales bounded context (sales.apitoken.sale). Не задан — фид выключен.
  SALES_CONTROL_KEY: z.string().min(32).optional(),
  // База sales-api для обратного вызова (погашение промокодов). Не задано — промо выключено.
  SALES_API_URL: z.string().url().optional(),
  DIGISELLER_SELLER_ID: z.coerce.number().int().positive().optional(),
  DIGISELLER_API_KEY: z.string().min(1).optional(),
  DIGISELLER_PRODUCT_ID: z.coerce.number().int().positive().optional(),
  DIGISELLER_CHECKOUT_TRACKING_SECRET: z.string().min(32).optional(),
  CRYPTOMUS_MERCHANT_ID: z.string().uuid().optional(),
  CRYPTOMUS_PAYMENT_API_KEY: z.string().min(1).optional(),
  PLATEGA_MERCHANT_ID: z.string().min(1).optional(),
  PLATEGA_SECRET: z.string().min(1).optional(),
  PLATEGA_FX_MARGIN_BPS: z.coerce.number().int().min(0).max(5_000).default(0),
  PLATEGA_DEFAULT_PAYMENT_METHOD: z.coerce.number().int().positive().default(2),
  PLATEGA_RATE_URL: z.string().url().default("https://api.rapira.net/open/market/rates"),
  GOOGLE_CLIENT_ID: z.string().min(1).optional(),
  GOOGLE_CLIENT_SECRET: z.string().min(1).optional(),
  GOOGLE_REDIRECT_URI: z.string().url().optional(),
  GITHUB_CLIENT_ID: z.string().min(1).optional(),
  GITHUB_CLIENT_SECRET: z.string().min(1).optional(),
  GITHUB_REDIRECT_URI: z.string().url().optional(),
}).superRefine((value, context) => {
  if (value.NODE_ENV === "production" && !value.CONTENT_STUDIO_ENGINE_KEY) {
    context.addIssue({ code: "custom", message: "CONTENT_STUDIO_ENGINE_KEY is required in production" });
  }
  const configured = [
    value.DIGISELLER_SELLER_ID,
    value.DIGISELLER_API_KEY,
    value.DIGISELLER_PRODUCT_ID,
    value.DIGISELLER_CHECKOUT_TRACKING_SECRET,
  ].filter((item) => item !== undefined).length;
  if (configured !== 0 && configured !== 4) {
    context.addIssue({ code: "custom", message: "all DigiSeller settings must be provided together" });
  }
  const cryptomusConfigured = [value.CRYPTOMUS_MERCHANT_ID, value.CRYPTOMUS_PAYMENT_API_KEY]
    .filter((item) => item !== undefined).length;
  if (cryptomusConfigured !== 0 && cryptomusConfigured !== 2) {
    context.addIssue({ code: "custom", message: "all Cryptomus settings must be provided together" });
  }
  const plategaConfigured = [value.PLATEGA_MERCHANT_ID, value.PLATEGA_SECRET]
    .filter((item) => item !== undefined).length;
  if (plategaConfigured !== 0 && plategaConfigured !== 2) {
    context.addIssue({ code: "custom", message: "all Platega settings must be provided together" });
  }
  if (value.MIN_TOPUP_USD > value.MAX_TOPUP_USD) {
    context.addIssue({ code: "custom", message: "MIN_TOPUP_USD must not exceed MAX_TOPUP_USD" });
  }
  const googleConfigured = [value.GOOGLE_CLIENT_ID, value.GOOGLE_CLIENT_SECRET, value.GOOGLE_REDIRECT_URI]
    .filter((item) => item !== undefined).length;
  if (googleConfigured !== 0 && googleConfigured !== 3) {
    context.addIssue({ code: "custom", message: "all Google OIDC settings must be provided together" });
  }
  const githubConfigured = [value.GITHUB_CLIENT_ID, value.GITHUB_CLIENT_SECRET, value.GITHUB_REDIRECT_URI]
    .filter((item) => item !== undefined).length;
  if (githubConfigured !== 0 && githubConfigured !== 3) {
    context.addIssue({ code: "custom", message: "all GitHub OAuth settings must be provided together" });
  }
  if (
    value.NODE_ENV === "production" &&
    value.GITHUB_REDIRECT_URI !== undefined &&
    value.GITHUB_REDIRECT_URI !== "https://backend.apitoken.sale/v1/auth/github/callback"
  ) {
    context.addIssue({
      code: "custom",
      message: "GITHUB_REDIRECT_URI must use the canonical production callback",
    });
  }
});

export type Environment = z.infer<typeof environmentSchema>;

export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
