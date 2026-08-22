import { HttpException, Injectable, ServiceUnavailableException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import type { Environment } from "./config.js";

const SALES_TIMEOUT_MS = 6_000;
const uuid = z.string().uuid();
const isoDate = z.string().datetime({ offset: true });
const nano = z.string().regex(/^-?(0|[1-9]\d*)$/);
const bps = z.number().int().min(0).max(10_000);
const providerId = z.enum(["anthropic", "openai", "google", "kimi", "glm"]);

const membershipSchema = z.object({
  partnerId: uuid,
  commerceUserId: uuid,
  status: z.enum(["active", "suspended", "pending"]),
  programEnabled: z.boolean(),
  programStartedAt: isoDate.nullable(),
  referralCode: z.string().min(1).max(64),
  parentPartnerId: uuid.nullable(),
  commissionBps: bps,
  teamShareBps: z.number().int().min(0).max(2_000).nullable(),
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  b2bCanDelegate: z.boolean(),
  b2bGrantSourcePartnerId: uuid.nullable(),
  payoutMethod: z.string().nullable(),
  payoutDetails: z.unknown().nullable(),
  createdAt: isoDate,
}).strict();

const totalsSchema = z.object({
  earnedNano: nano,
  directNano: nano,
  overrideNano: nano,
  adjustmentNano: nano,
  directAdjustmentNano: nano,
  overrideAdjustmentNano: nano,
  netNano: nano,
  directNetNano: nano,
  overrideNetNano: nano,
  paidNano: nano,
  pendingPayoutNano: nano,
  debtNano: nano,
  availableNano: nano,
  last30dSpendNano: nano,
  last30dEarnedNano: nano,
  last30dAdjustmentNano: nano,
  last30dNetNano: nano,
}).strict();

const referralSchema = z.object({
  commerceUserId: uuid,
  userRef: z.string().length(8),
  attributedAt: isoDate,
  spendNano: nano,
  earnedNano: nano,
  adjustmentNano: nano,
  netNano: nano,
  topupNano: nano,
}).strict();

const teamMemberSchema = z.object({
  id: uuid,
  commerceUserId: uuid,
  programEnabled: z.boolean(),
  programStartedAt: isoDate.nullable(),
  status: z.enum(["active", "suspended", "pending"]),
  commissionBps: bps,
  overrideBps: z.number().int().min(0).max(2_000),
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  b2bCanDelegate: z.boolean(),
  b2bGrantSourcePartnerId: uuid.nullable(),
  referredUsers: z.number().int().nonnegative(),
  theirEarnedNano: nano,
  theirAdjustmentNano: nano,
  theirNetNano: nano,
  myOverrideNano: nano,
  myOverrideAdjustmentNano: nano,
  myOverrideNetNano: nano,
}).strict();

const providerEarningsSchema = z.object({
  providerId: z.string().min(1).max(100).nullable(),
  events: z.number().int().nonnegative(),
  spendNano: nano,
  earnedNano: nano,
}).strict();

const dailyEarningsSchema = z.object({
  date: z.string().date(),
  spendNano: nano,
  earnedNano: nano,
  adjustmentNano: nano,
  netNano: nano,
}).strict();

const requestProviderTermSchema = z.object({
  providerId,
  requestedDiscountBps: z.number().int().min(0).max(9_500).nullable(),
  approvedDiscountBps: z.number().int().min(0).max(9_500).nullable(),
  decided: z.boolean(),
}).strict();

const requestSchema = z.object({
  id: uuid,
  requestType: z.enum(["b2b_conversion", "b2b_pricing", "commission_change"]),
  status: z.enum(["pending", "approved", "rejected", "applied", "apply_failed"]),
  requesterPartnerId: uuid,
  requesterEmail: z.string().email().nullable(),
  requesterDisplayName: z.string().nullable(),
  subjectPartnerId: uuid.nullable(),
  customerCommerceUserId: uuid.nullable(),
  customerEmail: z.string().email().nullable(),
  reason: z.string(),
  stateSnapshot: z.record(z.unknown()),
  requestedCommissionBps: bps.nullable(),
  requestedDiscountBps: z.number().int().min(0).max(9_500).nullable(),
  approvedCommissionBps: bps.nullable(),
  approvedDiscountBps: z.number().int().min(0).max(9_500).nullable(),
  reviewerActor: z.string().nullable(),
  reviewerNote: z.string().nullable(),
  reviewedAt: isoDate.nullable(),
  appliedAt: isoDate.nullable(),
  applyAttempts: z.number().int().nonnegative(),
  lastApplyError: z.string().nullable(),
  version: z.number().int().positive(),
  providerTerms: z.array(requestProviderTermSchema),
  effect: z.object({
    id: uuid,
    status: z.enum(["pending", "processing", "applied", "failed"]),
    attempts: z.number().int().nonnegative(),
    nextAttemptAt: isoDate.nullable(),
    terminal: z.boolean(),
    appliedAt: isoDate.nullable(),
    lastError: z.string().nullable(),
  }).strict().nullable(),
  createdAt: isoDate,
  updatedAt: isoDate,
}).strict();

const payoutSchema = z.object({
  id: uuid,
  partnerId: uuid,
  amountNano: nano,
  status: z.enum(["requested", "approved", "paid", "rejected"]),
  method: z.string(),
  details: z.unknown(),
  requestedAt: isoDate,
  decidedAt: isoDate.nullable(),
  paidAt: isoDate.nullable(),
  adminNote: z.string().nullable(),
  txHash: z.string().nullable(),
  chainStatus: z.string().nullable(),
}).strict();

const invitationSchema = z.object({
  id: uuid,
  commerceUserId: uuid,
  overrideBps: z.number().int().min(0).max(2_000),
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  b2bCanDelegate: z.boolean(),
  expiresAt: isoDate,
  consumedAt: isoDate.nullable(),
  revokedAt: isoDate.nullable(),
  createdAt: isoDate,
}).strict();

const periodStateSchema = z.object({
  now: isoDate,
  current: z.object({
    key: z.string(), start: isoDate, end: isoDate, accruedNano: nano, adjustmentNano: nano, netNano: nano,
  }).strict(),
  locked: z.array(z.object({
    key: z.string(), endedAt: isoDate, unlocksAt: isoDate, earnedNano: nano, adjustmentNano: nano, netNano: nano,
  }).strict()),
  nextPayout: z.object({ date: isoDate, estimatedNano: nano }).strict(),
  lifetimeEarnedNano: nano,
  lifetimeAdjustmentNano: nano,
  lifetimeNetNano: nano,
  lifetimePaidNano: nano,
  debtNano: nano,
  payableNano: nano,
  unpaidNano: nano,
}).strict();

const periodHistorySchema = z.object({
  key: z.string(),
  index: z.union([z.literal(1), z.literal(2)]),
  start: isoDate,
  end: isoDate,
  phase: z.enum(["accruing", "locked", "payable", "closed"]),
  payoutDate: isoDate,
  earnedNano: nano,
  adjustmentNano: nano,
  netNano: nano,
}).strict();

export const referralSnapshotSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("unavailable"), membership: z.null() }).strict(),
  z.object({ state: z.literal("disabled"), membership: membershipSchema }).strict(),
  z.object({
    state: z.literal("active"),
    activated: z.boolean(),
    membership: membershipSchema,
    totals: totalsSchema,
    referrals: z.array(referralSchema),
    team: z.array(teamMemberSchema),
    earnings: z.object({
      days: z.number().int().min(1).max(365),
      daily: z.array(dailyEarningsSchema),
      providers: z.array(providerEarningsSchema),
      providerDaily: z.array(z.object({ date: z.string().date(), providers: z.array(providerEarningsSchema) }).strict()),
    }).strict(),
    invitations: z.array(invitationSchema),
    requests: z.array(requestSchema),
    payouts: z.array(payoutSchema),
    period: periodStateSchema,
    periodHistory: z.array(periodHistorySchema),
    payoutPolicy: z.object({
      minPayoutNano: nano,
      lockDays: z.literal(7),
      windowDays: z.literal(3),
    }).strict(),
  }).strict(),
]);

const adminPartnerSchema = z.object({
  partnerId: uuid,
  commerceUserId: uuid,
  programEnabled: z.boolean(),
  programStartedAt: isoDate.nullable(),
  status: z.enum(["active", "suspended", "pending"]),
  referralCode: z.string(),
  commissionBps: bps,
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamShareBps: z.number().int().min(0).max(2_000).nullable(),
  parentPartnerId: uuid.nullable(),
  referredUsers: z.number().int().nonnegative(),
  teamSize: z.number().int().nonnegative(),
  earnedNano: nano,
  adjustmentNano: nano,
  netNano: nano,
  debtNano: nano,
  payableNano: nano,
  paidNano: nano,
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  teamInvitesEnabled: z.boolean(),
  b2bCanDelegate: z.boolean(),
  createdAt: isoDate,
}).strict();

export const adminPartnersSchema = z.object({ items: z.array(adminPartnerSchema) }).strict();
export const adminRequestsSchema = z.object({ items: z.array(requestSchema), nextCursor: z.string().nullable() }).strict();
export const adminPayoutsSchema = z.object({ items: z.array(payoutSchema) }).strict();

export const teamInvitationMutationSchema = z.object({ invitation: z.object({
  id: uuid,
  inviterPartnerId: uuid,
  commerceUserId: uuid,
  overrideBps: z.number().int().min(0).max(2_000),
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  b2bCanDelegate: z.boolean(),
  expiresAt: isoDate,
  createdAt: isoDate,
  created: z.boolean(),
}).strict() }).strict();

export const teamInvitationRevocationSchema = z.object({ invitation: z.object({
  id: uuid,
  commerceUserId: uuid,
  revokedAt: isoDate,
  revoked: z.boolean(),
}).strict() }).strict();

export const teamMemberMutationSchema = z.object({ authority: z.object({
  memberId: uuid,
  overrideBps: z.number().int().min(0).max(2_000),
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  b2bCanDelegate: z.boolean(),
}).strict() }).strict();

export const requestMutationSchema = z.object({ request: requestSchema }).strict();
export const walletMutationSchema = z.object({ membership: membershipSchema }).strict();
export const onboardingMutationSchema = z.object({ created: z.boolean(), membership: membershipSchema }).strict();
export const partnerMutationSchema = z.object({ membership: membershipSchema }).strict();
export const payoutMutationSchema = z.object({ payout: payoutSchema }).strict();
export const businessPricingMutationSchema = z.object({
  operationRef: z.string().min(8).max(200),
  idempotentReplay: z.boolean(),
  userId: uuid,
  converted: z.boolean(),
  customerType: z.literal("b2b"),
  discountPercent: z.number().min(0).max(95),
  providers: z.record(z.string(), z.number().min(0).max(95)),
  ceilingPercent: z.number().int().min(0).max(95),
}).strict();

export type ReferralSalesSnapshot = z.infer<typeof referralSnapshotSchema>;
export type ReferralSalesRequest = z.infer<typeof requestSchema>;

export class ReferralSalesError extends HttpException {
  constructor(readonly salesStatus: number, message: string) {
    super(message, publicStatus(salesStatus));
    this.name = "ReferralSalesError";
  }
}

@Injectable()
export class ReferralSalesClient {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  async call<T>(
    path: string,
    schema: z.ZodType<T>,
    init: { method?: "GET" | "POST" | "PATCH" | "DELETE"; body?: unknown; idempotencyKey?: string; actor?: string } = {},
  ): Promise<T> {
    const base = this.config.get("SALES_API_URL", { infer: true });
    const key = this.config.get("SALES_CONTROL_KEY", { infer: true });
    if (!base || !key) throw new ServiceUnavailableException("partner program is not configured");
    const headers = new Headers({ "x-api-key": key });
    if (init.body !== undefined) headers.set("content-type", "application/json");
    if (init.idempotencyKey) headers.set("idempotency-key", init.idempotencyKey);
    if (init.actor) headers.set("x-admin-actor", init.actor);
    let response: Response;
    try {
      response = await fetch(new URL(`/v1/internal/referral/${path.replace(/^\//, "")}`, base), {
        method: init.method ?? "GET",
        headers,
        ...(init.body === undefined ? {} : { body: JSON.stringify(init.body) }),
        signal: AbortSignal.timeout(SALES_TIMEOUT_MS),
      });
    } catch {
      throw new ServiceUnavailableException("partner program is temporarily unavailable");
    }
    const payload = await response.json().catch(() => null) as unknown;
    if (!response.ok) {
      const message = publicStatus(response.status) === response.status
        ? errorMessage(payload) ?? "partner program request failed"
        : "partner program is temporarily unavailable";
      throw new ReferralSalesError(response.status, message);
    }
    const parsed = schema.safeParse(payload);
    if (!parsed.success) {
      throw new ServiceUnavailableException("partner program returned an invalid response");
    }
    return parsed.data;
  }
}

function errorMessage(payload: unknown): string | null {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) return null;
  const message = (payload as Record<string, unknown>).message;
  if (typeof message === "string" && message.length <= 500) return message;
  if (Array.isArray(message) && message.every((item) => typeof item === "string")) return message.join(". ").slice(0, 500);
  return null;
}

function publicStatus(salesStatus: number): number {
  return [400, 403, 404, 409, 422, 429].includes(salesStatus) ? salesStatus : 503;
}
