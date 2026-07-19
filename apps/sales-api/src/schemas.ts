import { z } from "zod";

export const authTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
export const emailSchema = z.string().trim().email().max(320);
export const passwordSchema = z.string().min(8).max(200);
export const displayNameSchema = z.string().trim().min(1).max(80);
export const commissionBpsSchema = z.number().int().min(0).max(10_000);
export const nanoAmountSchema = z.string().regex(/^[1-9]\d{0,26}$/);

export const inviteCodeSchema = z.string().trim().regex(/^[a-z0-9]{4,64}$/i);
// Telegram-юзернейм: 5–32 символа, буквы/цифры/подчёркивание; @ и регистр нормализуем сами.
export const telegramUsernameSchema = z.string().trim().regex(/^@?[A-Za-z0-9_]{5,32}$/);

// Payload Telegram Login Widget + опциональный инвайт для первой регистрации.
export const telegramAuthSchema = z.object({
  id: z.union([z.number().int().positive(), z.string().regex(/^\d{1,19}$/)]).transform(String),
  first_name: z.string().max(200).optional(),
  last_name: z.string().max(200).optional(),
  username: z.string().max(64).optional(),
  photo_url: z.string().max(1000).optional(),
  auth_date: z.coerce.number().int().positive(),
  hash: z.string().regex(/^[0-9a-f]{64}$/),
  inviteCode: inviteCodeSchema.optional(),
});

export const telegramApplySchema = telegramAuthSchema.omit({ inviteCode: true }).extend({
  note: z.string().trim().min(1).max(2000).optional(),
});

export const createInviteSchema = z.object({
  telegramUsername: telegramUsernameSchema,
  commissionBps: commissionBpsSchema.optional(),
});

export const adminCreateInviteSchema = z.object({
  telegramUsername: telegramUsernameSchema,
  commissionBps: commissionBpsSchema.optional(),
  subCommissionBps: commissionBpsSchema.optional(),
});

export const createPayoutSchema = z.object({
  amountNano: nanoAmountSchema,
  method: z.string().trim().min(1).max(100),
  // Фронт шлёт свободный текст (адрес кошелька/карта); объект тоже допустим — jsonb хранит оба.
  details: z.union([z.string().trim().min(1).max(2000), z.record(z.unknown())]).optional(),
});

export const updateSettingsSchema = z.object({
  displayName: displayNameSchema.optional(),
  payoutMethod: z.string().trim().min(1).max(100).optional(),
  payoutDetails: z.union([z.string().trim().max(2000), z.record(z.unknown())]).optional(),
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

export const adminApplicationsQuerySchema = z.object({
  status: z.enum(["pending", "approved", "rejected"]).optional(),
});

export const adminApplicationDecisionSchema = z.object({
  action: z.enum(["approve", "reject"]),
  commissionBps: commissionBpsSchema.optional(),
  subCommissionBps: commissionBpsSchema.optional(),
  note: z.string().trim().min(1).max(2000).optional(),
});
