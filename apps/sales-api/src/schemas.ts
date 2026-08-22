import { z } from "zod";

export const authTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
export const emailSchema = z.string().trim().email().max(320);
export const passwordSchema = z.string().min(8).max(200);
export const displayNameSchema = z.string().trim().min(1).max(80);
export const commissionBpsSchema = z.number().int().min(0).max(10_000);
export const teamOverrideBpsSchema = z.number().int().min(0).max(2_000);
// Ceiling on the discount a granted partner may give their own B2B customers. Same 0..9500 bps
// range the pricing policies accept; the grant is what makes a non-zero value meaningful.
export const b2bMaxDiscountBpsSchema = z.number().int().min(0).max(9_500);
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
  // Retained as a tolerated legacy input, but ignored: a team member's platform rate is fixed by
  // Sales configuration (10% by default), never selected by the inviter.
  commissionBps: commissionBpsSchema.optional(),
  overrideBps: teamOverrideBpsSchema.optional(),
  teamOverrideMaxBps: teamOverrideBpsSchema.optional(),
  // Legacy marker permission, retained only for expand-only request compatibility.
  referralDiscountEnabled: z.boolean().optional(),
  referralDiscountBps: referralDiscountBpsSchema.optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  b2bCanDelegate: z.boolean().optional(),
}).refine(
  (value) => value.b2bEnabled !== false
    || ((value.b2bMaxDiscountBps ?? 0) === 0 && value.b2bCanDelegate !== true),
  { message: "a revoked B2B grant cannot retain a ceiling or delegation" },
);

export const adminCreateInviteSchema = z.object({
  telegramUsername: telegramUsernameSchema,
  commissionBps: commissionBpsSchema.optional(),
  subCommissionBps: commissionBpsSchema.optional(),
  teamOverrideMaxBps: teamOverrideBpsSchema.optional(),
  // Onboarding-time B2B grant: the partner created from this invite already holds it.
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bCanDelegate: z.boolean().optional(),
  // Legacy marker permission/value; current UI always creates invites with this disabled.
  referralDiscountEnabled: z.boolean().optional(),
  referralDiscountBps: referralDiscountBpsSchema.optional(),
  // Доступ к промокодам, задаваемый прямо на онбординге: сколько кодов и их макс. номинал в USD.
  promoMaxCount: promoMaxCountSchema.optional(),
  promoMaxValueUsd: promoMaxValueUsdSchema.optional(),
}).refine(
  (value) => value.b2bEnabled !== false
    || ((value.b2bMaxDiscountBps ?? 0) === 0 && value.b2bCanDelegate !== true),
  { message: "a revoked B2B grant cannot retain a ceiling or delegation" },
);

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
  teamOverrideMaxBps: teamOverrideBpsSchema.optional(),
  referralDiscountBps: referralDiscountBpsSchema.optional(),
  referralDiscountEnabled: z.boolean().optional(),
  // Grant/revoke the B2B right and its ceiling on an existing partner.
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bCanDelegate: z.boolean().optional(),
  status: z.enum(["active", "suspended", "pending"]).optional(),
}).refine((value) => Object.values(value).some((item) => item !== undefined), {
  message: "at least one field is required",
}).refine(
  // A ceiling without the grant is not a smaller permission — it is none at all, and storing it
  // would read like authority the partner does not have. Ask for both, or neither.
  (value) => !(value.b2bMaxDiscountBps !== undefined && value.b2bMaxDiscountBps > 0 && value.b2bEnabled === false),
  { message: "a B2B ceiling cannot be set while revoking the B2B grant" },
).refine(
  (value) => !(value.b2bCanDelegate === true && value.b2bEnabled === false),
  { message: "B2B delegation cannot be enabled while revoking the B2B grant" },
);

export const teamMemberControlsSchema = z.object({
  overrideBps: teamOverrideBpsSchema.optional(),
  teamOverrideMaxBps: teamOverrideBpsSchema.optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  b2bCanDelegate: z.boolean().optional(),
}).refine((value) => Object.values(value).some((item) => item !== undefined), {
  message: "at least one team control is required",
}).refine(
  (value) => value.b2bEnabled !== false
    || ((value.b2bMaxDiscountBps ?? 0) === 0 && value.b2bCanDelegate !== true),
  { message: "a revoked B2B grant cannot retain a ceiling or delegation" },
);

// Legacy marker writer schema, retained for expand-only compatibility.
export const partnerSetDiscountSchema = z.object({
  referralDiscountBps: referralDiscountBpsSchema,
});

// Legacy marker replacement. Zero clears the marker; no value changes pricing.
export const setReferralDiscountSchema = z.object({
  discountBps: referralDiscountBpsSchema,
});

// Partner-set B2B pricing for their own referral. Percents, matching the admin editor; the
// partner's granted ceiling narrows the range further and is checked on both sides.
const partnerB2bPercentSchema = z.number().int().min(0).max(95);

export const partnerBusinessPricingSchema = z.object({
  discountPercent: partnerB2bPercentSchema.optional(),
  // null removes a provider override so that provider falls back to the customer's default.
  providers: z.record(z.string(), partnerB2bPercentSchema.nullable()).optional(),
}).refine(
  (value) => value.discountPercent !== undefined
    || (value.providers !== undefined && Object.keys(value.providers).length > 0),
  { message: "nothing to change" },
);

// Маскированная ссылка на реферала: первые 8 hex его uuid (ровно то, что в userMask/userRef).
export const referralUserRefSchema = z.string().regex(/^[0-9a-f]{8}$/);

export const idempotencyKeySchema = z.string().trim().min(8).max(200);
export const partnerRequestReasonSchema = z.string().trim().min(1).max(4000);
export const partnerRequestTypeSchema = z.enum(["b2b_conversion", "b2b_pricing", "commission_change"]);
export const partnerRequestStatusSchema = z.enum(["pending", "approved", "rejected", "applied", "apply_failed"]);
export const partnerRequestProviderIdSchema = z.enum(["anthropic", "openai", "google", "kimi", "glm"]);

export const commissionChangeRequestSchema = z.object({
  requestedCommissionBps: commissionBpsSchema,
  reason: partnerRequestReasonSchema,
});

export const b2bPartnerRequestSchema = z.object({
  discountPercent: partnerB2bPercentSchema,
  providers: z.record(partnerRequestProviderIdSchema, partnerB2bPercentSchema.nullable()).optional(),
  reason: partnerRequestReasonSchema,
});

export const partnerRequestsQuerySchema = z.object({
  status: partnerRequestStatusSchema.optional(),
  requestType: partnerRequestTypeSchema.optional(),
  cursor: z.string().max(512).optional(),
  limit: z.coerce.number().int().min(1).max(100).default(25),
});

export const adminPartnerRequestDecisionSchema = z.discriminatedUnion("action", [
  z.object({
    action: z.literal("reject"),
    note: z.string().trim().min(1).max(4000),
  }),
  z.object({
    action: z.literal("approve"),
    note: z.string().trim().min(1).max(4000),
    commissionBps: commissionBpsSchema.optional(),
    discountPercent: partnerB2bPercentSchema.optional(),
    providers: z.record(partnerRequestProviderIdSchema, partnerB2bPercentSchema.nullable()).optional(),
  }),
]);

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
  teamOverrideMaxBps: teamOverrideBpsSchema.optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: b2bMaxDiscountBpsSchema.optional(),
  b2bCanDelegate: z.boolean().optional(),
  note: z.string().trim().min(1).max(2000).optional(),
}).refine(
  (value) => value.b2bEnabled === true
    || ((value.b2bMaxDiscountBps ?? 0) === 0 && value.b2bCanDelegate !== true),
  { message: "a revoked B2B grant cannot retain a ceiling or delegation" },
);
