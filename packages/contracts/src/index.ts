import { z } from "zod";

export const decimalIntegerSchema = z.string().regex(/^-?\d+$/);
export const nonNegativeIntegerSchema = z.string().regex(/^\d+$/);

export const engineAccountSchema = z.object({
  account: z.string().startsWith("acct_"),
  balance_nano: decimalIntegerSchema,
  spent_nano: decimalIntegerSchema,
  reserved_nano: decimalIntegerSchema,
  balance: z.string(),
  mult_bp: z.number().int(),
  status: z.enum(["active", "disabled"]),
  handle: z.string().nullable(),
});

export type EngineAccount = z.infer<typeof engineAccountSchema>;

export const engineCreditResultSchema = z.object({
  account: z.string().startsWith("acct_"),
  balance_nano: decimalIntegerSchema,
  balance: z.string(),
});

export type EngineCreditResult = z.infer<typeof engineCreditResultSchema>;

export const createEngineAccountSchema = z.object({
  handle: z.string().trim().min(1).max(200).optional(),
  multBp: z.number().int().min(0).max(100_000).optional(),
});

export type CreateEngineAccount = z.infer<typeof createEngineAccountSchema>;

export const enqueueCreditSchema = z.object({
  paymentId: z.string().uuid(),
  engineAccountId: z.string().startsWith("acct_"),
  amountNano: z.coerce.bigint().positive(),
  idempotencyRef: z.string().trim().min(1).max(255),
});

export type EnqueueCredit = z.infer<typeof enqueueCreditSchema>;

export interface HealthStatus {
  ok: boolean;
  service: string;
  database: "up" | "down";
  engine: "up" | "down" | "unchecked";
}

/** User-entered whole USD. JSON numbers and decimal points are intentionally rejected. */
export const wholeUsdSchema = z.string().regex(/^[1-9]\d*$/, "amountUsd must contain positive whole USD digits only");

export const createCheckoutSchema = z.object({
  amountUsd: wholeUsdSchema,
  provider: z.literal("cryptomus").default("cryptomus"),
});

export type CreateCheckout = z.infer<typeof createCheckoutSchema>;

export const checkoutStatusSchema = z.enum(["creating", "pending", "paid", "canceled", "refunded", "failed"]);

export interface CheckoutView {
  id: string;
  provider: string;
  amountUsd: string;
  status: z.infer<typeof checkoutStatusSchema>;
  checkoutUrl: string | null;
  expiresAt: string | null;
}

export const authEmailSchema = z.string().trim().toLowerCase().email().max(254);
export const authPasswordSchema = z.string().min(12).max(128)
  .refine((value) => Buffer.byteLength(value, "utf8") <= 256, "password is too long");

export const registerSchema = z.object({
  email: authEmailSchema,
  password: authPasswordSchema,
}).strict();

export const loginSchema = registerSchema;

export interface AuthUserView {
  id: string;
  email: string;
  emailVerified: boolean;
  engineAccountStatus: "pending" | "active" | "error" | "disabled";
}
