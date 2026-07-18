import { z } from "zod";

export const authTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
export const emailSchema = z.string().trim().email().max(320);
export const passwordSchema = z.string().min(8).max(200);
export const displayNameSchema = z.string().trim().min(1).max(80);
export const commissionBpsSchema = z.number().int().min(0).max(10_000);
export const nanoAmountSchema = z.string().regex(/^[1-9]\d{0,26}$/);

export const registerSchema = z.object({
  email: emailSchema,
  password: passwordSchema,
  displayName: displayNameSchema.optional(),
  inviteCode: z.string().trim().min(1).max(64).optional(),
});

export const loginSchema = z.object({
  email: emailSchema,
  password: passwordSchema,
});

export const verifyEmailSchema = z.object({ token: authTokenSchema });
export const emailOnlySchema = z.object({ email: emailSchema });
export const resetPasswordSchema = z.object({ token: authTokenSchema, password: passwordSchema });

export const createInviteSchema = z.object({
  commissionBps: commissionBpsSchema.optional(),
});

export const createPayoutSchema = z.object({
  amountNano: nanoAmountSchema,
  method: z.string().trim().min(1).max(100),
  details: z.record(z.unknown()).optional(),
});

export const updateSettingsSchema = z.object({
  displayName: displayNameSchema.optional(),
  payoutMethod: z.string().trim().min(1).max(100).optional(),
  payoutDetails: z.record(z.unknown()).optional(),
});

export const adminPatchPartnerSchema = z.object({
  commissionBps: commissionBpsSchema.optional(),
  subCommissionBps: commissionBpsSchema.optional(),
  status: z.enum(["active", "suspended", "pending"]).optional(),
}).refine((value) => Object.values(value).some((item) => item !== undefined), {
  message: "at least one field is required",
});

export const adminPayoutDecisionSchema = z.object({
  action: z.enum(["approve", "reject", "paid"]),
  note: z.string().trim().min(1).max(2000).optional(),
});

export const earningsQuerySchema = z.object({
  days: z.coerce.number().int().min(1).max(365).default(30),
});

export const adminPayoutsQuerySchema = z.object({
  status: z.enum(["requested", "approved", "paid", "rejected"]).optional(),
});
