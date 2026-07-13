import { z } from "zod";

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  PORT: z.coerce.number().int().min(1).max(65_535).default(3000),
  HOST: z.string().default("127.0.0.1"),
  DATABASE_URL: z.string().url(),
  ENGINE_BASE_URL: z.string().url(),
  ENGINE_CONTROL_KEY: z.string().min(32),
  ENGINE_TIMEOUT_MS: z.coerce.number().int().positive().default(10_000),
  PUBLIC_API_BASE_URL: z.string().url().default("https://api.apitoken.sale"),
  PUBLIC_APP_BASE_URL: z.string().url().default("https://apitoken.sale"),
  MIN_TOPUP_USD: z.string().regex(/^[1-9]\d*$/).transform(BigInt).default("1"),
  MAX_TOPUP_USD: z.string().regex(/^[1-9]\d*$/).transform(BigInt).default("10000"),
  SESSION_TTL_SECONDS: z.coerce.number().int().min(900).max(2_592_000).default(604_800),
  REQUIRE_VERIFIED_EMAIL: z.enum(["true", "false"]).transform((value) => value === "true").default("false"),
  DIGISELLER_SELLER_ID: z.coerce.number().int().positive().optional(),
  DIGISELLER_API_KEY: z.string().min(1).optional(),
  DIGISELLER_PRODUCT_ID: z.coerce.number().int().positive().optional(),
  DIGISELLER_CHECKOUT_TRACKING_SECRET: z.string().min(32).optional(),
  CRYPTOMUS_MERCHANT_ID: z.string().uuid().optional(),
  CRYPTOMUS_PAYMENT_API_KEY: z.string().min(1).optional(),
  GOOGLE_CLIENT_ID: z.string().min(1).optional(),
  GOOGLE_CLIENT_SECRET: z.string().min(1).optional(),
  GOOGLE_REDIRECT_URI: z.string().url().optional(),
}).superRefine((value, context) => {
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
  if (value.MIN_TOPUP_USD > value.MAX_TOPUP_USD) {
    context.addIssue({ code: "custom", message: "MIN_TOPUP_USD must not exceed MAX_TOPUP_USD" });
  }
  const googleConfigured = [value.GOOGLE_CLIENT_ID, value.GOOGLE_CLIENT_SECRET, value.GOOGLE_REDIRECT_URI]
    .filter((item) => item !== undefined).length;
  if (googleConfigured !== 0 && googleConfigured !== 3) {
    context.addIssue({ code: "custom", message: "all Google OIDC settings must be provided together" });
  }
  if (value.NODE_ENV === "production" && !value.REQUIRE_VERIFIED_EMAIL) {
    context.addIssue({ code: "custom", message: "REQUIRE_VERIFIED_EMAIL must be true in production" });
  }
});

export type Environment = z.infer<typeof environmentSchema>;

export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
