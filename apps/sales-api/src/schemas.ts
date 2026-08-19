import { z } from "zod";

export const authTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
export const emailSchema = z.string().trim().email().max(320);
export const passwordSchema = z.string().min(8).max(200);
export const displayNameSchema = z.string().trim().min(1).max(80);
export const commissionBpsSchema = z.number().int().min(0).max(10_000);
// Retained referral marker, not pricing. The historical wire range remains 0..9500 bps.
export const referralDiscountBpsSchema = z.number().int().min(0).max(9_500);
export const promoMaxCountSchema = z.number().int().min(0).max(10_000);
export const promoMaxValueUsdSchema = z.number().int().min(0).max(100_000);
// ≤18 значащих цифр (< 1e18 нано = < $1e9) — заведомо в пределах pg bigint, без риска overflow.
export const nanoAmountSchema = z.string().regex(/^[1-9]\d{0,17}$/);

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
  // Legacy marker permission, retained only for expand-only request compatibility.
  referralDiscountEnabled: z.boolean().optional(),
  referralDiscountBps: referralDiscountBpsSchema.optional(),
});

// Ceiling on the discount a granted partner may give their own B2B customers. Same 0..9500 bps
// range the pricing policies accept; the grant is what makes a non-zero value meaningful.
export const b2bMaxDiscountBpsSchema = z.number().int().min(0).max(9_500);

export const adminCreateInviteSchema = z.object({
  telegramUsername: telegramUsernameSchema,
  commissionBps: commissionBpsSchema.optional(),
  subCommissionBps: commissionBpsSchema.optional(),
  // Onboarding-time B2B grant: the partner created from this invite already holds it.
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  // Legacy marker permission/value; current UI always creates invites with this disabled.
  referralDiscountEnabled: z.boolean().optional(),
  referralDiscountBps: referralDiscountBpsSchema.optional(),
  // Доступ к промокодам, задаваемый прямо на онбординге: сколько кодов и их макс. номинал в USD.
  promoMaxCount: promoMaxCountSchema.optional(),
  promoMaxValueUsd: promoMaxValueUsdSchema.optional(),
});

// Единственная сеть выплат — BSC (BEP-20): EVM-адрес.
export const walletSchema = z.object({
  address: z.string().trim().regex(/^0x[a-fA-F0-9]{40}$/),
});

export const updateSettingsSchema = z.object({
  displayName: displayNameSchema.optional(),
});

export const adminPatchPartnerSchema = z.object({
  commissionBps: commissionBpsSchema.optional(),
  subCommissionBps: commissionBpsSchema.optional(),
  referralDiscountBps: referralDiscountBpsSchema.optional(),
  referralDiscountEnabled: z.boolean().optional(),
  // Grant/revoke the B2B right and its ceiling on an existing partner.
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  status: z.enum(["active", "suspended", "pending"]).optional(),
}).refine((value) => Object.values(value).some((item) => item !== undefined), {
  message: "at least one field is required",
}).refine(
  // A ceiling without the grant is not a smaller permission — it is none at all, and storing it
  // would read like authority the partner does not have. Ask for both, or neither.
  (value) => !(value.b2bMaxDiscountBps !== undefined && value.b2bMaxDiscountBps > 0 && value.b2bEnabled === false),
  { message: "a B2B ceiling cannot be set while revoking the B2B grant" },
);

// Legacy marker writer schema, retained for expand-only compatibility.
export const partnerSetDiscountSchema = z.object({
  referralDiscountBps: referralDiscountBpsSchema,
});

// Legacy marker replacement. Zero clears the marker; no value changes pricing.
export const setReferralDiscountSchema = z.object({
  discountBps: referralDiscountBpsSchema,
});

// Маскированная ссылка на реферала: первые 8 hex его uuid (ровно то, что в userMask/userRef).
export const referralUserRefSchema = z.string().regex(/^[0-9a-f]{8}$/);

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

// Partner creates an integer-USD promo; discountBps is a retained, non-pricing marker.
export const createPromoSchema = z.object({
  valueUsd: z.coerce.number().int().positive().max(100_000),
  discountBps: referralDiscountBpsSchema.optional(),
});

// Legacy one-time attribution link; discountBps is audit metadata with no price effect.
export const createDiscountLinkSchema = z.object({
  discountBps: referralDiscountBpsSchema,
  note: z.string().trim().max(120).optional(),
});

// Админ включает промо партнёру и задаёт лимиты (номинал в USD, количество кодов).
export const adminPromoSchema = z.object({
  enabled: z.boolean(),
  maxValueUsd: z.coerce.number().int().min(0).max(100_000),
  maxCount: z.coerce.number().int().min(0).max(10_000),
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
