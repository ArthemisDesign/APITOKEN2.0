import { z } from "zod";

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  DATABASE_URL: z.string().url(),
  ENGINE_BASE_URL: z.string().url(),
  ENGINE_CONTROL_KEY: z.string().min(32),
  ENGINE_TIMEOUT_MS: z.coerce.number().int().positive().default(10_000),
  OPENKEYS_INTERNAL_BASE_URL: z.string().url().default("http://127.0.0.1:3410"),
  OPENKEYS_CONTROL_KEY: z.string().min(32).optional(),
  CREDIT_POLL_MS: z.coerce.number().int().min(100).default(1000),
  PUBLIC_API_BASE_URL: z.string().url().default("https://backend.apitoken.sale"),
  PLATEGA_MERCHANT_ID: z.string().min(1).optional(),
  PLATEGA_SECRET: z.string().min(1).optional(),
  PLATEGA_API_BASE_URL: z.string().url().default("https://app.platega.io"),
  PLATEGA_RECONCILE_MS: z.coerce.number().int().min(5_000).default(30_000),
  PLATEGA_RECONCILE_MIN_AGE_S: z.coerce.number().int().min(0).default(15),
  PRICING_POLL_MS: z.coerce.number().int().min(1000).default(60_000),
  // Job delivery runs on its own short tick, decoupled from the heavy per-user sweep above it.
  // A newly provisioned account's policy is what the customer's first dashboard load waits on, so
  // that latency must not be a function of how many accounts the fleet already has.
  PRICING_DISPATCH_MS: z.coerce.number().int().min(250).max(60_000).default(2_000),
  FUNDING_NORMALIZATION_POLL_MS: z.coerce.number().int().min(1000).default(5_000),
  FUNDING_NORMALIZATION_BATCH_SIZE: z.coerce.number().int().min(1).max(500).default(25),
  FUNDING_NORMALIZATION_INVENTORY_PAGE_SIZE: z.coerce.number().int().min(1).max(500).default(500),
  FUNDING_NORMALIZATION_LEASE_MS: z.coerce.number().int().min(30_000).max(3_600_000).default(300_000),
  FUNDING_NORMALIZATION_RETRY_MS: z.coerce.number().int().min(1000).max(3_600_000).default(15_000),
  STAGE8_CAPTURE_POLL_MS: z.coerce.number().int().min(1000).max(60_000).default(5_000),
  STAGE8_CAPTURE_LEASE_MS: z.coerce.number().int().min(30_000).max(3_600_000).default(300_000),
  STAGE8_CAPTURE_RETRY_MS: z.coerce.number().int().min(1000).max(3_600_000).default(15_000),
  STAGE8_CAPTURE_MAX_ATTEMPTS: z.coerce.number().int().min(1).max(100).default(10),
  PRICING_SHADOW_ROLLOUT_POLL_MS: z.coerce.number().int().min(1000).max(60_000).default(5_000),
  PRICING_SHADOW_ROLLOUT_LEASE_MS: z.coerce.number().int().min(30_000).max(3_600_000).default(300_000),
  PRICING_SHADOW_ROLLOUT_RETRY_MS: z.coerce.number().int().min(1000).max(3_600_000).default(15_000),
  PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS: z.coerce.number().int().min(1).max(100).default(10),
  PRICING_SHADOW_ROLLOUT_BATCH_SIZE: z.coerce.number().int().min(1).max(500).default(25),
  EMAIL_DELIVERY_MODE: z.enum(["disabled", "smtp"]).default("disabled"),
  EMAIL_POLL_MS: z.coerce.number().int().min(100).default(1000),
  AUTH_TOKEN_ENCRYPTION_KEY: z.string().regex(/^[A-Za-z0-9_-]{43}$/),
  PUBLIC_APP_BASE_URL: z.string().url().default("https://apitoken.sale"),
  EMAIL_FROM: z.string().email().optional(),
  SMTP_HOST: z.string().min(1).optional(),
  SMTP_PORT: z.coerce.number().int().min(1).max(65_535).optional(),
  SMTP_SECURE: z.enum(["true", "false"]).transform((value) => value === "true").default("true"),
  SMTP_USERNAME: z.string().min(1).optional(),
  SMTP_PASSWORD: z.string().min(1).optional(),
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
  if (value.NODE_ENV === "production") {
    const publicAppUrl = new URL(value.PUBLIC_APP_BASE_URL);
    if (
      publicAppUrl.origin !== "https://apitoken.sale" ||
      publicAppUrl.username !== "" ||
      publicAppUrl.password !== "" ||
      publicAppUrl.pathname !== "/" ||
      publicAppUrl.search !== "" ||
      publicAppUrl.hash !== ""
    ) {
      context.addIssue({
        code: "custom",
        message: "PUBLIC_APP_BASE_URL must be exactly the canonical production origin https://apitoken.sale",
      });
    }
  }
});

export type Environment = z.infer<typeof environmentSchema>;
export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
