import { randomUUID } from "node:crypto";
import {
  B2C_PRICING_TIERS,
  paymentProviderSchema,
  type EngineLedgerEntry,
  type EngineSettlementFundingEvidence,
  type EngineSettlementFundingEvidenceV2,
  type PricingPolicyEditorRule,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import {
  copyBusinessInvitationPolicyToReplacement,
  createBusinessInvitationPolicy,
  enqueuePricingJob,
  getManagedPricingPolicy,
  PricingPolicyWriteError,
  provisionBusinessClientPolicy,
} from "./pricing-policy-write.js";

export class InvalidBusinessInvitationError extends Error {}
export class BusinessInvitationNotFoundError extends Error {}
export class BusinessInvitationConflictError extends Error {}
export class BusinessCustomerNotFoundError extends Error {}
export class CustomerProfileNotFoundError extends Error {}
export class PricingLedgerAttributionError extends Error {}

export type CustomerPricingUnavailableReason =
  | "policy_catalog_disabled"
  | "admission_catalog_disabled"
  | "policy_master_switch_disabled"
  | "policy_scoped_switch_disabled"
  | "admission_master_switch_disabled"
  | "admission_scoped_switch_disabled"
  | "missing_pricing_rule";

export interface CustomerPricingRuleView {
  ruleId: string;
  scope: "provider" | "model";
  pricingMode: "track" | "discount";
  ruleOrigin: "managed" | "legacy";
  discountBps: number | null;
  payableMultiplierBp: number;
  trackEligible: boolean;
  retentionEligible: boolean;
  commissionEligible: boolean;
}

export interface CustomerPricingModelView {
  modelId: string;
  available: boolean;
  unavailableReasons: CustomerPricingUnavailableReason[];
  rule: CustomerPricingRuleView | null;
}

export interface CustomerPricingProviderView {
  providerId: string;
  available: boolean;
  models: CustomerPricingModelView[];
}

export interface CustomerPricingVersionView {
  effectiveVersion: string;
  policyVersion: string;
  catalogGeneration: string;
  switchGeneration: string;
  providers: CustomerPricingProviderView[];
}

export interface CustomerPricingPolicyView {
  accountClass: "b2c" | "b2b";
  productId: string;
  policyEnforcement: "legacy_scalar" | "shadow" | "strict";
  fundingEnforcement: "legacy_single" | "shadow" | "strict";
  reconciliationState: "pending" | "verified" | "exception";
  syncState: "legacy" | "pending" | "confirmed" | "failed";
  inSync: boolean;
  lastAcknowledgedAt: string | null;
  desired: CustomerPricingVersionView | null;
  applied: CustomerPricingVersionView | null;
}

export interface PricingSyncTarget {
  userId: string;
  engineAccountId: string;
}

export interface ClaimedPricingJob {
  id: string;
  userId: string;
  engineAccountId: string;
  multiplierBp: number;
  attempts: number;
}

export function utcMonthStart(value = new Date()): Date {
  return new Date(Date.UTC(value.getUTCFullYear(), value.getUTCMonth(), 1));
}

// РЕАЛЬНЫМИ (комиссионируемыми) считаем ТОЛЬКО депозиты через платёжные провайдеры:
// engine ref = `${provider}:${providerPaymentId}` (см. payments.ts). Это whitelist, а не blacklist:
// welcome-бонус (`signup-bonus:`), промо (`promo:`), админ-кредит (`admin-credit:`), пустой или
// неизвестный ref — «бесплатное», комиссия по нему НЕ идёт. Так любой новый нереальный источник
// денег по умолчанию бесплатный, и мы никогда случайно не выплатим комиссию с подарка.
const REAL_MONEY_REF_PREFIXES: readonly string[] = paymentProviderSchema.options.map((p) => `${p}:`);

export function isFreeCreditRef(ref: string | null | undefined): boolean {
  if (typeof ref !== "string") return true;
  return !REAL_MONEY_REF_PREFIXES.some((prefix) => ref.startsWith(prefix));
}

/** Подарочные источники денег: welcome-бонус, промо и восстановление бонуса. */
const BONUS_REF_PREFIXES: readonly string[] = ["signup-bonus:", "promo:", "bonus-restore", "welcome"];

export type PricingTopupSource = "payment" | "bonus" | "manual";

/**
 * Классификация пополнения ДЛЯ ОТЧЁТНОСТИ (не для комиссии — та живёт в isFreeCreditRef и
 * остаётся whitelist-строгой). `payment` — депозит через платёжного провайдера, `bonus` —
 * известный подарок, `manual` — всё прочее: админ-кредит и ручные зачисления, то есть реальные
 * деньги, полученные мимо платёжной системы. Неизвестный ref безопаснее считать ручным
 * пополнением (он попадает в отчёт как «оплачено вручную» и виден оператору), а не подарком.
 */
export function classifyTopupRef(ref: string | null | undefined): PricingTopupSource {
  if (typeof ref !== "string" || ref.trim() === "") return "manual";
  if (REAL_MONEY_REF_PREFIXES.some((prefix) => ref.startsWith(prefix))) return "payment";
  if (BONUS_REF_PREFIXES.some((prefix) => ref.startsWith(prefix))) return "bonus";
  return "manual";
}

/** Тир по НАКОПЛЕННОЙ сумме пополнений (`spendThresholdNano` = порог пополнения). 0 = none (<$100). */
export function tierForTopups(cumulativeNano: bigint): number {
  let tier = 0;
  for (let index = 1; index < B2C_PRICING_TIERS.length; index += 1) {
    if (cumulativeNano >= B2C_PRICING_TIERS[index]!.spendThresholdNano) tier = index;
  }
  return tier;
}

/** Тридцатидневное окно удержания в миллисекундах. */
const HOLD_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;
const PRICING_LEDGER_PAGE_SIZE = 1000;
const POSTGRES_INTEGER_MAX = 2_147_483_647n;
const SUPPORTED_LEDGER_ATTRIBUTION_SCHEMA_VERSIONS: ReadonlySet<bigint> = new Set([1n, 2n]);
const PROVIDER_BACKFILL_WINDOW_DAYS = 30;
const UNATTRIBUTED_PROVIDER_ID = "unattributed";
const UNAVAILABLE_PROVIDER_ID = "unavailable";
const PROVIDER_RECOVERY_VERSION = 2;

type LedgerAttribution = NonNullable<EngineLedgerEntry["attribution"]>;
type LedgerFundingAllocation = NonNullable<EngineLedgerEntry["funding_allocations"]>[number];

interface ValidatedLedgerAttribution {
  attribution: LedgerAttribution;
  allocations: Array<{
    ordinal: number;
    engineBucketId: string;
    bucketVersion: string;
    sourceType: string;
    sourceRef: string;
    amountNano: string;
  }>;
  paidFundedNano: bigint | null;
  nonPaidFundedNano: bigint | null;
  retentionEligible: boolean;
  // Release-v2 rows carry a NULL engine eligibility and derive the commission authority locally
  // from the immutable account class plus exact paid funding; other kinds store engine evidence.
  commissionEligible: boolean;
}

interface ValidatedPolicyFunding {
  allocations: ValidatedLedgerAttribution["allocations"];
  paidFundedNano: bigint;
  bonusFundedNano: bigint;
  otherFundedNano: bigint;
}

function normalizedProviderId(value: string | null | undefined): string | null {
  if (
    typeof value !== "string"
    || value.length === 0
    || value.length > 200
    || value.trim() !== value
    || /[\u0000-\u001f\u007f]/.test(value)
  ) return null;
  return value;
}

function isProviderRecoverySentinel(value: string | null): boolean {
  return value === UNATTRIBUTED_PROVIDER_ID || value === UNAVAILABLE_PROVIDER_ID;
}

function ledgerProviderEvidence(entry: EngineLedgerEntry): string {
  const providerId = normalizedProviderId(entry.provider)
    ?? normalizedProviderId(entry.attribution?.provider_id);
  return providerId === null || isProviderRecoverySentinel(providerId)
    ? UNATTRIBUTED_PROVIDER_ID
    : providerId;
}

function requiredLedgerText(value: string | null | undefined, field: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new PricingLedgerAttributionError(`engine ledger attribution is missing ${field}`);
  }
  return value;
}

function requiredLedgerInteger(
  value: string | null | undefined,
  field: string,
  allowZero = false,
): bigint {
  if (value === null || value === undefined || !/^\d+$/.test(value)) {
    throw new PricingLedgerAttributionError(`engine ledger attribution is missing ${field}`);
  }
  const parsed = BigInt(value);
  if (allowZero ? parsed < 0n : parsed <= 0n) {
    throw new PricingLedgerAttributionError(`engine ledger attribution has invalid ${field}`);
  }
  return parsed;
}

function requiredLedgerBoolean(value: boolean | null | undefined, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new PricingLedgerAttributionError(`engine ledger attribution is missing ${field}`);
  }
  return value;
}

function epochSecondsDate(value: string, field: string): Date {
  const seconds = BigInt(value);
  // JavaScript Date is bounded to +/-8.64e15 milliseconds. Engine timestamps are non-negative.
  if (seconds < 0n || seconds > 8_640_000_000_000n) {
    throw new PricingLedgerAttributionError(`engine ledger ${field} is outside the Date range`);
  }
  const result = new Date(Number(seconds) * 1000);
  if (Number.isNaN(result.getTime())) {
    throw new PricingLedgerAttributionError(`engine ledger ${field} is invalid`);
  }
  return result;
}

function allocationOrdinal(value: string | null, index: number): number {
  if (value === null) {
    throw new PricingLedgerAttributionError(
      `policy funding allocation ${index} is missing its durable allocation order`,
    );
  }
  const ordinal = requiredLedgerInteger(value, `funding allocation ${index} order`, true);
  if (ordinal > POSTGRES_INTEGER_MAX) {
    throw new PricingLedgerAttributionError(`funding allocation ${index} order is too large`);
  }
  return Number(ordinal);
}

function validatePolicyFundingAllocations(
  attribution: LedgerAttribution,
  allocations: readonly LedgerFundingAllocation[],
  chargedNano: bigint,
): ValidatedPolicyFunding {
  if (!Array.isArray(attribution.funding_allocation_json)) {
    throw new PricingLedgerAttributionError("policy attribution is missing raw funding evidence");
  }
  // The legacy bucket path only accepts legacy bucket evidence; release-v2 lot evidence belongs to
  // the separate release-v2 validation path and must not leak into bucket reconciliation.
  const bucketEvidence: EngineSettlementFundingEvidence[] = attribution.funding_allocation_json.map(
    (allocation, index) => {
      if (!("bucket_id" in allocation)) {
        throw new PricingLedgerAttributionError(
          `raw funding allocation ${index} is not legacy bucket evidence`,
        );
      }
      return allocation;
    },
  );
  let previousOrder: bigint | null = null;
  const seenBuckets = new Set<string>();
  let paidFundedNano = 0n;
  let bonusFundedNano = 0n;
  let otherFundedNano = 0n;
  const chargedEvidence = bucketEvidence.filter((allocation, index) => {
    const bucketId = requiredLedgerText(allocation.bucket_id, `raw funding allocation ${index} bucket`);
    const sourceType = requiredLedgerText(
      allocation.source_type,
      `raw funding allocation ${index} source type`,
    );
    const bucketVersion = requiredLedgerInteger(
      allocation.bucket_version,
      `raw funding allocation ${index} bucket version`,
    );
    const reservedNano = requiredLedgerInteger(
      allocation.reserved_nano,
      `raw funding allocation ${index} reserved amount`,
    );
    const evidenceChargedNano = requiredLedgerInteger(
      allocation.charged_nano,
      `raw funding allocation ${index} charged amount`,
      true,
    );
    const releasedNano = requiredLedgerInteger(
      allocation.released_nano,
      `raw funding allocation ${index} released amount`,
      true,
    );
    const order = requiredLedgerInteger(
      allocation.allocation_order,
      `raw funding allocation ${index} order`,
      true,
    );
    if (reservedNano !== evidenceChargedNano + releasedNano) {
      throw new PricingLedgerAttributionError(
        `raw funding allocation ${index} does not reconcile reserved, charged, and released amounts`,
      );
    }
    if (previousOrder !== null && order <= previousOrder) {
      throw new PricingLedgerAttributionError("raw funding allocation order is not strictly increasing");
    }
    previousOrder = order;
    const bucketIdentity = `${bucketId}\u0000${sourceType}\u0000${bucketVersion}`;
    if (seenBuckets.has(bucketIdentity)) {
      throw new PricingLedgerAttributionError("raw funding evidence repeats one bucket version");
    }
    seenBuckets.add(bucketIdentity);
    if (sourceType === "paid") paidFundedNano += evidenceChargedNano;
    else if (sourceType === "welcome_track_bonus") bonusFundedNano += evidenceChargedNano;
    else otherFundedNano += evidenceChargedNano;
    return evidenceChargedNano > 0n;
  });
  if (chargedEvidence.length !== allocations.length) {
    throw new PricingLedgerAttributionError(
      "normalized funding allocations do not match raw charged allocations",
    );
  }

  let normalizedTotal = 0n;
  const normalized = allocations.map((allocation, index) => {
    const evidence = chargedEvidence[index]!;
    const amountNano = requiredLedgerInteger(
      allocation.amount_nano,
      `funding allocation ${index} amount`,
    );
    const bucketVersion = requiredLedgerInteger(
      allocation.bucket_version,
      `funding allocation ${index} bucket version`,
    );
    const ordinal = allocationOrdinal(allocation.allocation_order, index);
    if (
      allocation.direction !== "debit"
      || allocation.bucket_id !== evidence.bucket_id
      || allocation.source_type !== evidence.source_type
      || bucketVersion !== BigInt(evidence.bucket_version)
      || amountNano !== BigInt(evidence.charged_nano)
      || BigInt(ordinal) !== BigInt(evidence.allocation_order)
    ) {
      throw new PricingLedgerAttributionError(
        `normalized funding allocation ${index} differs from immutable settlement evidence`,
      );
    }
    normalizedTotal += amountNano;
    return {
      ordinal,
      engineBucketId: requiredLedgerText(allocation.bucket_id, `funding allocation ${index} bucket`),
      bucketVersion: bucketVersion.toString(),
      sourceType: requiredLedgerText(allocation.source_type, `funding allocation ${index} source type`),
      sourceRef: allocation.source_ref,
      amountNano: amountNano.toString(),
    };
  });
  if (normalizedTotal !== chargedNano) {
    throw new PricingLedgerAttributionError(
      "normalized funding allocations do not cover the charged amount",
    );
  }
  return {
    allocations: normalized,
    paidFundedNano,
    bonusFundedNano,
    otherFundedNano,
  };
}

/**
 * Validates the immutable release-v2 lot evidence against the declared funding categories. The
 * v2 path has no normalized bucket rows: raw lot identity stays only in funding_allocation_json,
 * so exact reconciliation happens entirely here.
 */
function validateReleaseV2FundingAllocations(
  attribution: LedgerAttribution,
  chargedNano: bigint,
): { paidFundedNano: bigint; bonusFundedNano: bigint; otherFundedNano: bigint } {
  if (!Array.isArray(attribution.funding_allocation_json)) {
    throw new PricingLedgerAttributionError("release-v2 attribution is missing raw funding evidence");
  }
  const lotEvidence: EngineSettlementFundingEvidenceV2[] = attribution.funding_allocation_json.map(
    (allocation, index) => {
      if (!("lot_id" in allocation)) {
        throw new PricingLedgerAttributionError(
          `raw funding allocation ${index} is not release-v2 lot evidence`,
        );
      }
      return allocation;
    },
  );
  let previousOrder: bigint | null = null;
  let paidFundedNano = 0n;
  let bonusFundedNano = 0n;
  let otherFundedNano = 0n;
  let totalFundedNano = 0n;
  lotEvidence.forEach((allocation, index) => {
    requiredLedgerText(allocation.lot_id, `raw funding allocation ${index} lot`);
    const sourceType = requiredLedgerText(
      allocation.lot_source_type,
      `raw funding allocation ${index} lot source type`,
    );
    requiredLedgerInteger(allocation.lot_version, `raw funding allocation ${index} lot version`);
    const amountNano = requiredLedgerInteger(
      allocation.amount_nano,
      `raw funding allocation ${index} amount`,
    );
    const order = requiredLedgerInteger(
      allocation.allocation_order,
      `raw funding allocation ${index} order`,
      true,
    );
    if (allocation.direction !== "debit") {
      throw new PricingLedgerAttributionError(
        `raw funding allocation ${index} is not a debit lot allocation`,
      );
    }
    if (previousOrder !== null && order <= previousOrder) {
      throw new PricingLedgerAttributionError("raw funding allocation order is not strictly increasing");
    }
    previousOrder = order;
    if (sourceType === "paid") paidFundedNano += amountNano;
    else if (sourceType === "welcome_bonus") bonusFundedNano += amountNano;
    else otherFundedNano += amountNano;
    totalFundedNano += amountNano;
  });
  if (totalFundedNano !== chargedNano) {
    throw new PricingLedgerAttributionError(
      "release-v2 funding allocations do not cover the charged amount",
    );
  }
  return { paidFundedNano, bonusFundedNano, otherFundedNano };
}

function validateReleaseV2LedgerAttribution(
  entry: EngineLedgerEntry,
  attribution: LedgerAttribution,
  attributionSchemaVersion: bigint,
  chargedNano: bigint,
): ValidatedLedgerAttribution {
  if (attributionSchemaVersion !== 2n) {
    throw new PricingLedgerAttributionError("release-v2 attribution requires attribution schema version 2");
  }
  requiredLedgerText(entry.request_id, "engine request id");
  const providerId = requiredLedgerText(attribution.provider_id, "provider id");
  const officialNano = requiredLedgerInteger(attribution.official_nano, "official amount", true);
  if (entry.provider !== providerId || entry.official_nano === null || entry.official_nano === undefined) {
    throw new PricingLedgerAttributionError("ledger and attribution provider/official identity differ");
  }
  if (BigInt(entry.official_nano) !== officialNano) {
    throw new PricingLedgerAttributionError("ledger and attribution monetary identity differ");
  }
  const accountClass = requiredLedgerText(attribution.account_class, "account class");
  const releaseSchemaVersion = requiredLedgerInteger(
    attribution.release_schema_version,
    "release schema version",
  );
  if (releaseSchemaVersion < 2n) {
    throw new PricingLedgerAttributionError("release-v2 attribution has an unsupported release schema version");
  }
  requiredLedgerInteger(attribution.release_generation, "release generation");
  requiredLedgerText(attribution.release_digest, "release digest");
  if (attribution.release_billing_mode === null) {
    throw new PricingLedgerAttributionError("engine ledger attribution is missing release billing mode");
  }
  if (attribution.release_billing_mode === "balance") {
    requiredLedgerInteger(attribution.release_funding_generation, "release funding generation");
  } else if (attribution.release_funding_generation !== null) {
    throw new PricingLedgerAttributionError("meter-only release attribution carries a funding generation");
  }
  requiredLedgerText(attribution.policy_id, "policy id");
  requiredLedgerInteger(attribution.policy_version, "policy version");
  requiredLedgerText(attribution.policy_digest, "policy digest");
  requiredLedgerText(attribution.tariff_schedule_id, "tariff schedule id");
  requiredLedgerInteger(attribution.tariff_priced_ts, "tariff priced timestamp", true);
  if (attribution.official_cost_json === null) {
    throw new PricingLedgerAttributionError("release-v2 attribution is missing official cost evidence");
  }
  // Progressive pricing is gone from the release-v2 contract: any non-NULL legacy eligibility or
  // pricing-mode field means upstream tampering rather than a compatible transitional row.
  if (
    attribution.pricing_mode !== null
    || attribution.rule_origin !== null
    || attribution.track_eligible !== null
    || attribution.retention_eligible !== null
    || attribution.commission_eligible !== null
  ) {
    throw new PricingLedgerAttributionError("release-v2 attribution carries progressive pricing fields");
  }
  // The optional rule is an all-or-nothing discount override; a partial rule cannot be audited.
  const ruleFields = [
    attribution.rule_id,
    attribution.rule_digest,
    attribution.rule_scope,
    attribution.payable_multiplier_bp,
    attribution.discount_bps,
  ];
  if (!ruleFields.every((field) => field === null)) {
    requiredLedgerText(attribution.rule_id, "rule id");
    requiredLedgerText(attribution.rule_digest, "rule digest");
    if (attribution.rule_scope === null) {
      throw new PricingLedgerAttributionError("engine ledger attribution is missing rule scope");
    }
    if (attribution.payable_multiplier_bp === null) {
      throw new PricingLedgerAttributionError("release-v2 rule is missing payable multiplier");
    }
    if (
      attribution.discount_bps !== null
      && attribution.payable_multiplier_bp !== 10_000 - attribution.discount_bps
    ) {
      throw new PricingLedgerAttributionError(
        "release-v2 rule discount does not complement the payable multiplier",
      );
    }
  }

  const paidFundedNano = requiredLedgerInteger(attribution.paid_funded_nano, "paid funding", true);
  const bonusFundedNano = requiredLedgerInteger(attribution.bonus_funded_nano, "bonus funding", true);
  const otherFundedNano = requiredLedgerInteger(attribution.other_funded_nano, "other funding", true);
  if (paidFundedNano + bonusFundedNano + otherFundedNano !== chargedNano) {
    throw new PricingLedgerAttributionError("funding categories do not cover the charged amount");
  }
  const funding = validateReleaseV2FundingAllocations(attribution, chargedNano);
  if (
    funding.paidFundedNano !== paidFundedNano
    || funding.bonusFundedNano !== bonusFundedNano
    || funding.otherFundedNano !== otherFundedNano
  ) {
    throw new PricingLedgerAttributionError(
      "funding categories do not match immutable lot allocations",
    );
  }
  return {
    attribution,
    allocations: [],
    paidFundedNano,
    nonPaidFundedNano: bonusFundedNano + otherFundedNano,
    retentionEligible: false,
    // Release-v2 commission authority derives from the immutable account class and exact paid
    // funding only — deliberately independent of any pricing-mode or engine eligibility flag.
    commissionEligible: accountClass === "b2c" && paidFundedNano > 0n,
  };
}

function validateLedgerAttribution(
  entry: EngineLedgerEntry,
  chargedNano: bigint,
): ValidatedLedgerAttribution | null {
  const attribution = entry.attribution ?? null;
  const allocations = entry.funding_allocations ?? [];
  if (!attribution) {
    if (allocations.length > 0) {
      throw new PricingLedgerAttributionError(
        "engine ledger returned funding allocations without attribution",
      );
    }
    return null;
  }

  const attributionSchemaVersion = requiredLedgerInteger(
    attribution.attribution_schema_version,
    "attribution schema version",
  );
  if (!SUPPORTED_LEDGER_ATTRIBUTION_SCHEMA_VERSIONS.has(attributionSchemaVersion)) {
    throw new PricingLedgerAttributionError("engine ledger attribution schema version is unsupported");
  }
  requiredLedgerText(attribution.snapshot_digest, "snapshot digest");

  if (attribution.snapshot_kind === "release_v2") {
    return validateReleaseV2LedgerAttribution(
      entry,
      attribution,
      attributionSchemaVersion,
      chargedNano,
    );
  }
  if (attributionSchemaVersion !== 1n) {
    throw new PricingLedgerAttributionError("engine ledger attribution schema version is unsupported");
  }
  const retentionEligible = requiredLedgerBoolean(
    attribution.retention_eligible,
    "retention eligibility",
  );
  const trackEligible = requiredLedgerBoolean(attribution.track_eligible, "track eligibility");
  const commissionEligible = requiredLedgerBoolean(
    attribution.commission_eligible,
    "commission eligibility",
  );
  if (commissionEligible && !trackEligible) {
    throw new PricingLedgerAttributionError("commission eligibility requires track eligibility");
  }

  if (attribution.snapshot_kind === "legacy_scalar") {
    if (
      attribution.pricing_mode !== "legacy_scalar"
      || attribution.rule_origin !== "legacy"
      || attribution.payable_multiplier_bp === null
      || trackEligible
      || retentionEligible
      || commissionEligible
      || allocations.length > 0
    ) {
      throw new PricingLedgerAttributionError("legacy scalar attribution has policy-only fields");
    }
    return {
      attribution,
      allocations: [],
      paidFundedNano: null,
      nonPaidFundedNano: null,
      retentionEligible: false,
      commissionEligible: false,
    };
  }
  if (attribution.snapshot_kind !== "policy_v1") {
    throw new PricingLedgerAttributionError("engine ledger attribution has an unknown snapshot kind");
  }

  requiredLedgerText(entry.request_id, "engine request id");
  const providerId = requiredLedgerText(attribution.provider_id, "provider id");
  const officialNano = requiredLedgerInteger(attribution.official_nano, "official amount", true);
  if (entry.provider !== providerId || entry.official_nano === null || entry.official_nano === undefined) {
    throw new PricingLedgerAttributionError("ledger and attribution provider/official identity differ");
  }
  if (BigInt(entry.official_nano) !== officialNano) {
    throw new PricingLedgerAttributionError("ledger and attribution monetary identity differ");
  }

  requiredLedgerText(attribution.product_id, "product id");
  requiredLedgerText(attribution.account_class, "account class");
  requiredLedgerText(attribution.requested_model_id, "requested model id");
  requiredLedgerText(attribution.canonical_model_id, "canonical model id");
  requiredLedgerInteger(attribution.alias_generation, "alias generation");
  requiredLedgerText(attribution.rule_id, "rule id");
  requiredLedgerText(attribution.rule_digest, "rule digest");
  requiredLedgerText(attribution.rule_scope, "rule scope");
  requiredLedgerText(attribution.pricing_mode, "pricing mode");
  requiredLedgerText(attribution.rule_origin, "rule origin");
  if (attribution.payable_multiplier_bp === null) {
    throw new PricingLedgerAttributionError("policy attribution is missing payable multiplier");
  }
  requiredLedgerText(attribution.policy_id, "policy id");
  requiredLedgerInteger(attribution.policy_version, "policy version");
  requiredLedgerInteger(attribution.effective_policy_version, "effective policy version");
  requiredLedgerText(attribution.policy_digest, "effective policy digest");
  requiredLedgerText(attribution.source_policy_digest, "source policy digest");
  requiredLedgerInteger(attribution.catalog_generation, "catalog generation");
  requiredLedgerInteger(attribution.switch_generation, "switch generation");
  requiredLedgerInteger(attribution.admission_catalog_generation, "admission catalog generation");
  requiredLedgerText(attribution.admission_catalog_digest, "admission catalog digest");
  requiredLedgerInteger(attribution.admission_switch_generation, "admission switch generation");
  requiredLedgerText(attribution.admission_switch_digest, "admission switch digest");
  requiredLedgerInteger(attribution.runtime_manifest_generation, "runtime manifest generation");
  requiredLedgerText(attribution.runtime_manifest_digest, "runtime manifest digest");
  requiredLedgerText(attribution.tariff_schedule_id, "tariff schedule id");
  requiredLedgerInteger(attribution.tariff_priced_ts, "tariff priced timestamp", true);
  if (attribution.official_cost_json === null) {
    throw new PricingLedgerAttributionError("policy attribution is missing official cost evidence");
  }

  const paidFundedNano = requiredLedgerInteger(attribution.paid_funded_nano, "paid funding", true);
  const bonusFundedNano = requiredLedgerInteger(attribution.bonus_funded_nano, "bonus funding", true);
  const otherFundedNano = requiredLedgerInteger(attribution.other_funded_nano, "other funding", true);
  if (paidFundedNano + bonusFundedNano + otherFundedNano !== chargedNano) {
    throw new PricingLedgerAttributionError("funding categories do not cover the charged amount");
  }
  const funding = validatePolicyFundingAllocations(attribution, allocations, chargedNano);
  if (
    funding.paidFundedNano !== paidFundedNano
    || funding.bonusFundedNano !== bonusFundedNano
    || funding.otherFundedNano !== otherFundedNano
  ) {
    throw new PricingLedgerAttributionError(
      "funding categories do not match immutable bucket allocations",
    );
  }
  return {
    attribution,
    allocations: funding.allocations,
    paidFundedNano,
    nonPaidFundedNano: bonusFundedNano + otherFundedNano,
    retentionEligible,
    commissionEligible,
  };
}

export interface BusinessInviteRecord {
  id: string;
  email: string | null;
  encryptedToken: string;
  multiplierBp: number;
  expiresAt: Date;
  idempotentReplay: boolean;
  deliveryStatus: string;
}

export async function createBusinessInvite(database: Database, input: {
  email?: string;
  tokenHash: string;
  encryptedToken: string;
  multiplierBp: number;
  expiresAt: Date;
  idempotencyKey: string;
  actorId: string;
  reason: string;
  policyRules?: readonly PricingPolicyEditorRule[];
}): Promise<BusinessInviteRecord> {
  if (input.policyRules && input.multiplierBp !== 10_000) {
    throw new PricingPolicyWriteError(
      "invalid_owner_rule",
      "policy-based invitation requires a neutral 10000 compatibility multiplier",
    );
  }
  const client = await database.pool.connect();
  const email = input.email?.toLowerCase() ?? null;
  try {
    await client.query("BEGIN");
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [input.idempotencyKey]);
    const existing = await client.query<{
      id: string; email: string | null; encrypted_token: string | null;
      multiplier_bp: number; expires_at: Date; delivery_status: string | null;
      invitation_policy_id: string | null;
    }>(`
      SELECT bi.id, bi.email, bi.encrypted_token, bi.multiplier_bp, bi.expires_at,
             eo.status AS delivery_status, policy.invitation_policy_id
      FROM business_invites bi
      LEFT JOIN business_invite_policy_bindings policy ON policy.invite_id = bi.id
      LEFT JOIN LATERAL (
        SELECT status::text AS status FROM email_outbox
        WHERE business_invite_id = bi.id ORDER BY created_at DESC LIMIT 1
      ) eo ON TRUE
      WHERE bi.idempotency_key = $1
      FOR UPDATE OF bi
    `, [input.idempotencyKey]);
    const prior = existing.rows[0];
    if (prior) {
      if (prior.email !== email || prior.multiplier_bp !== input.multiplierBp) {
        throw new BusinessInvitationConflictError("idempotency key was already used for another invitation");
      }
      if (!prior.encrypted_token) {
        throw new BusinessInvitationConflictError("the invitation token is no longer available");
      }
      if (Boolean(prior.invitation_policy_id) !== Boolean(input.policyRules)) {
        throw new BusinessInvitationConflictError("idempotency key was already used with another pricing policy mode");
      }
      if (input.policyRules) {
        await createBusinessInvitationPolicy(client, {
          inviteId: prior.id,
          rules: input.policyRules,
          actorId: input.actorId,
          reason: input.reason,
        });
      }
      await client.query("COMMIT");
      return {
        id: prior.id,
        email: prior.email,
        encryptedToken: prior.encrypted_token,
        multiplierBp: prior.multiplier_bp,
        expiresAt: prior.expires_at,
        idempotentReplay: true,
        deliveryStatus: prior.delivery_status ?? "copy_only",
      };
    }

    const id = randomUUID();
    await client.query(`
      INSERT INTO business_invites (
        id, email, token_hash, encrypted_token, multiplier_bp, expires_at,
        idempotency_key, created_by_actor
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    `, [
      id, email, input.tokenHash, input.encryptedToken, input.multiplierBp,
      input.expiresAt, input.idempotencyKey, input.actorId,
    ]);
    if (input.policyRules) {
      await createBusinessInvitationPolicy(client, {
        inviteId: id,
        rules: input.policyRules,
        actorId: input.actorId,
        reason: input.reason,
      });
    }
    if (email) {
      await queueBusinessInviteEmail(client, {
        inviteId: id,
        recipient: email,
        encryptedToken: input.encryptedToken,
        multiplierBp: input.multiplierBp,
        expiresAt: input.expiresAt,
        policyBased: Boolean(input.policyRules),
      });
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'business_invite.created', 'business_invite', $2, $3::jsonb)
    `, [input.actorId, id, JSON.stringify({
      email,
      multiplierBp: input.multiplierBp,
      policyBased: Boolean(input.policyRules),
      expiresAt: input.expiresAt.toISOString(),
      delivery: email ? "email" : "copy_only",
      reason: input.reason,
    })]);
    await client.query("COMMIT");
    return {
      id,
      email,
      encryptedToken: input.encryptedToken,
      multiplierBp: input.multiplierBp,
      expiresAt: input.expiresAt,
      idempotentReplay: false,
      deliveryStatus: email ? "pending" : "copy_only",
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function lockBusinessInvite(
  client: PoolClient,
  input: { email: string; tokenHash: string },
): Promise<{ id: string; multiplierBp: number }> {
  const result = await client.query<{ id: string; multiplier_bp: number }>(`
    SELECT id, multiplier_bp
    FROM business_invites
    WHERE token_hash = $1 AND (email IS NULL OR lower(email) = lower($2))
      AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()
    FOR UPDATE
  `, [input.tokenHash, input.email]);
  const invite = result.rows[0];
  if (!invite) throw new InvalidBusinessInvitationError("invalid, expired, or email-mismatched business invitation");
  return { id: invite.id, multiplierBp: invite.multiplier_bp };
}

export async function getBusinessInvitePreview(
  database: Database,
  tokenHash: string,
): Promise<{
  email: string | null;
  multiplierBp: number;
  expiresAt: Date;
  policy: { currentVersion: number; rules: PricingPolicyEditorRule[] } | null;
} | null> {
  const result = await database.pool.query<{
    id: string; email: string | null; multiplier_bp: number; expires_at: Date;
  }>(`
    SELECT id::text, email, multiplier_bp, expires_at
    FROM business_invites
    WHERE token_hash = $1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()
  `, [tokenHash]);
  const row = result.rows[0];
  if (!row) return null;
  const policy = await getManagedPricingPolicy(database, {
    ownerType: "b2b_invitation",
    ownerId: row.id,
  });
  return {
    email: row.email,
    multiplierBp: row.multiplier_bp,
    expiresAt: row.expires_at,
    policy: policy ? { currentVersion: policy.currentVersion, rules: policy.rules } : null,
  };
}

export async function getBusinessInviteToken(
  database: Database,
  inviteId: string,
): Promise<{ encryptedToken: string; email: string | null; expiresAt: Date }> {
  const result = await database.pool.query<{
    encrypted_token: string | null; email: string | null; expires_at: Date;
  }>(`
    SELECT encrypted_token, email, expires_at
    FROM business_invites
    WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()
  `, [inviteId]);
  const row = result.rows[0];
  if (!row?.encrypted_token) throw new BusinessInvitationNotFoundError("active invitation not found");
  return { encryptedToken: row.encrypted_token, email: row.email, expiresAt: row.expires_at };
}

export async function revokeBusinessInvite(database: Database, input: {
  inviteId: string;
  actorId: string;
  reason: string;
}): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query(`
      UPDATE business_invites
      SET revoked_at = now(), revoked_by_actor = $2, encrypted_token = NULL
      WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
      RETURNING id
    `, [input.inviteId, input.actorId]);
    if (!result.rows[0]) throw new BusinessInvitationNotFoundError("active invitation not found");
    await client.query(`
      UPDATE email_outbox
      SET status = 'canceled', locked_at = NULL, locked_by = NULL,
          last_error = 'business invitation revoked', updated_at = now()
      WHERE business_invite_id = $1 AND status IN ('pending', 'processing')
    `, [input.inviteId]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'business_invite.revoked', 'business_invite', $2, $3::jsonb)
    `, [input.actorId, input.inviteId, JSON.stringify({ reason: input.reason })]);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function rotateBusinessInvite(database: Database, input: {
  inviteId: string;
  tokenHash: string;
  encryptedToken: string;
  expiresAt: Date;
  idempotencyKey: string;
  actorId: string;
  reason: string;
}): Promise<BusinessInviteRecord> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [input.idempotencyKey]);
    const replayResult = await client.query<{
      id: string; email: string | null; encrypted_token: string | null;
      multiplier_bp: number; expires_at: Date; delivery_status: string | null;
    }>(`
      SELECT replacement.id, replacement.email, replacement.encrypted_token,
             replacement.multiplier_bp, replacement.expires_at,
             eo.status::text AS delivery_status
      FROM business_invites original
      JOIN business_invites replacement ON replacement.id = original.superseded_by_invite_id
      LEFT JOIN LATERAL (
        SELECT status FROM email_outbox
        WHERE business_invite_id = replacement.id ORDER BY created_at DESC LIMIT 1
      ) eo ON TRUE
      WHERE original.id = $1 AND replacement.idempotency_key = $2
      FOR UPDATE OF replacement
    `, [input.inviteId, input.idempotencyKey]);
    const replay = replayResult.rows[0];
    if (replay) {
      if (!replay.encrypted_token) {
        throw new BusinessInvitationConflictError("the replacement invitation token is no longer available");
      }
      await client.query("COMMIT");
      return {
        id: replay.id,
        email: replay.email,
        encryptedToken: replay.encrypted_token,
        multiplierBp: replay.multiplier_bp,
        expiresAt: replay.expires_at,
        idempotentReplay: true,
        deliveryStatus: replay.delivery_status ?? "pending",
      };
    }
    const keyInUse = await client.query(
      "SELECT 1 FROM business_invites WHERE idempotency_key = $1",
      [input.idempotencyKey],
    );
    if (keyInUse.rows[0]) {
      throw new BusinessInvitationConflictError("idempotency key was already used for another invitation");
    }
    const oldResult = await client.query<{
      email: string | null; multiplier_bp: number; policy_based: boolean;
    }>(`
      SELECT invitation.email, invitation.multiplier_bp,
             (policy.invitation_policy_id IS NOT NULL) AS policy_based
      FROM business_invites invitation
      LEFT JOIN business_invite_policy_bindings policy ON policy.invite_id = invitation.id
      WHERE invitation.id = $1 AND invitation.consumed_at IS NULL AND invitation.revoked_at IS NULL
      FOR UPDATE OF invitation
    `, [input.inviteId]);
    const old = oldResult.rows[0];
    if (!old) throw new BusinessInvitationNotFoundError("active invitation not found");
    if (!old.email) throw new BusinessInvitationConflictError("copy-only invitations cannot be emailed; copy the existing link");
    const id = randomUUID();
    const replacementMultiplierBp = old.policy_based ? 10_000 : old.multiplier_bp;
    await client.query(`
      INSERT INTO business_invites (
        id, email, token_hash, encrypted_token, multiplier_bp, expires_at,
        idempotency_key, created_by_actor
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    `, [
      id, old.email, input.tokenHash, input.encryptedToken, replacementMultiplierBp,
      input.expiresAt, input.idempotencyKey, input.actorId,
    ]);
    const replacementPolicy = await copyBusinessInvitationPolicyToReplacement(client, {
      sourceInviteId: input.inviteId,
      replacementInviteId: id,
      actorId: input.actorId,
      reason: input.reason,
    });
    if (Boolean(replacementPolicy) !== old.policy_based) {
      throw new PricingPolicyWriteError("policy_not_found", "replacement invitation lost its source policy");
    }
    await client.query(`
      UPDATE business_invites
      SET revoked_at = now(), revoked_by_actor = $2, encrypted_token = NULL,
          superseded_by_invite_id = $3
      WHERE id = $1
    `, [input.inviteId, input.actorId, id]);
    await client.query(`
      UPDATE email_outbox
      SET status = 'canceled', locked_at = NULL, locked_by = NULL,
          last_error = 'superseded by a new business invitation', updated_at = now()
      WHERE business_invite_id = $1 AND status IN ('pending', 'processing')
    `, [input.inviteId]);
    await queueBusinessInviteEmail(client, {
      inviteId: id,
      recipient: old.email,
      encryptedToken: input.encryptedToken,
      multiplierBp: replacementMultiplierBp,
      expiresAt: input.expiresAt,
      policyBased: replacementPolicy !== null,
    });
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'business_invite.resent', 'business_invite', $2, $3::jsonb)
    `, [input.actorId, id, JSON.stringify({
      supersedesInviteId: input.inviteId,
      reason: input.reason,
    })]);
    await client.query("COMMIT");
    return {
      id,
      email: old.email,
      encryptedToken: input.encryptedToken,
      multiplierBp: replacementMultiplierBp,
      expiresAt: input.expiresAt,
      idempotentReplay: false,
      deliveryStatus: "pending",
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function queueBusinessInviteEmail(client: PoolClient, input: {
  inviteId: string;
  recipient: string;
  encryptedToken: string;
  multiplierBp: number;
  expiresAt: Date;
  policyBased?: boolean;
}): Promise<void> {
  const pricingPayload = input.policyBased
    ? { pricingPolicy: "provider_model" }
    : { discountPercent: 100 - input.multiplierBp / 100 };
  await client.query(`
    INSERT INTO email_outbox (id, business_invite_id, recipient, template, payload)
    VALUES ($1, $2, $3, 'business_invite', $4::jsonb)
  `, [randomUUID(), input.inviteId, input.recipient, JSON.stringify({
    encryptedToken: input.encryptedToken,
    ...pricingPayload,
    expiresAt: input.expiresAt.toISOString(),
  })]);
}

export async function getPricingView(database: Database, userId: string): Promise<Record<string, unknown> | null> {
  const result = await database.pool.query<PricingViewRow>(`
    SELECT cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.pricing_month_start,
           cp.cumulative_topup_nano, cp.tier_window_start, cp.tier_window_spent_nano, cp.referral_floor_bps
    FROM customer_profiles cp
    WHERE cp.user_id = $1
  `, [userId]);
  const row = result.rows[0];
  if (!row) return null;
  const discountPercent = 100 - row.multiplier_bp / 100;
  if (row.customer_type === "b2b") {
    return {
      customerType: "b2b",
      pricingMode: "manual",
      discountPercent,
      multiplierBp: row.multiplier_bp,
    };
  }
  // Prepay-модель: поля формы сохранены, но переосмыслены —
  // spentNano = НАКОПЛЕНО пополнений; retentionSpendNano = сколько тратить за 30 дней (hold);
  // nextTier.remainingNano = сколько ещё ДОЛОЖИТЬ до следующего тира.
  const currentTier = row.current_tier ?? 0;
  const tier = B2C_PRICING_TIERS[currentTier]!;
  const nextTier = B2C_PRICING_TIERS[currentTier + 1];
  const cumulative = BigInt(row.cumulative_topup_nano);
  // Скидочный «пол» партнёра (реф-ссылка сейлза). Если задан — реальная цена = min(тир, 100−floor),
  // и клиент показывает фиксированную партнёрскую ставку вместо тир-лестницы.
  const floorBps = row.referral_floor_bps ?? 0;
  const effectiveBp = effectiveMultiplierBp(tier.multiplierBp, floorBps);
  return {
    customerType: "b2c",
    pricingMode: "progressive",
    monthStart: row.pricing_month_start.toISOString(),
    tier: tier.code,
    discountPercent: tier.discountPercent,
    multiplierBp: tier.multiplierBp,
    // Фиксированная партнёрская скидка (0 = нет): floor и итоговая эффективная ставка/скидка.
    referralFloorBps: floorBps,
    effectiveMultiplierBp: effectiveBp,
    effectiveDiscountPercent: 100 - effectiveBp / 100,
    spentNano: cumulative.toString(),
    retentionSpendNano: tier.holdNano.toString(),
    windowSpentNano: BigInt(row.tier_window_spent_nano).toString(),
    windowStart: row.tier_window_start ? row.tier_window_start.toISOString() : null,
    nextTier: nextTier ? {
      tier: nextTier.code,
      discountPercent: nextTier.discountPercent,
      spendThresholdNano: nextTier.spendThresholdNano.toString(),
      remainingNano: (nextTier.spendThresholdNano > cumulative
        ? nextTier.spendThresholdNano - cumulative
        : 0n).toString(),
      visibleOfficialUsageUsd: nextTier.visibleOfficialUsageUsd,
    } : null,
  };
}

interface CustomerPricingBindingRow {
  id: string;
  account_class: "b2c" | "b2b";
  product_id: string;
  desired_effective_version: string | null;
  desired_digest: string | null;
  applied_effective_version: string | null;
  applied_digest: string | null;
  policy_enforcement: CustomerPricingPolicyView["policyEnforcement"];
  funding_enforcement: CustomerPricingPolicyView["fundingEnforcement"];
  reconciliation_state: CustomerPricingPolicyView["reconciliationState"];
  sync_state: CustomerPricingPolicyView["syncState"];
  last_ack_at: Date | null;
  legacy_multiplier_bp: number;
}

interface CustomerPricingVersionRow {
  binding_id: string;
  effective_version: string;
  policy_version: string;
  product_id: string;
  account_class: "b2c" | "b2b";
  catalog_generation: string;
  switch_generation: string;
}

interface CustomerPricingRuleRow {
  binding_id: string;
  effective_version: string;
  rule_id: string;
  scope_type: "provider" | "model";
  provider_id: string;
  canonical_model_id: string | null;
  pricing_mode: "track" | "discount";
  rule_origin: "managed" | "legacy";
  discount_bps: number | null;
  payable_multiplier_bp: number;
  track_eligible: boolean;
  retention_eligible: boolean;
  commission_eligible: boolean;
}

interface CustomerCatalogEntryRow {
  product_id: string;
  generation: string;
  provider_id: string;
  canonical_model_id: string;
  enabled: boolean;
}

interface CustomerSwitchEntryRow {
  generation: string;
  provider_id: string;
  scope_type: "master" | "product" | "segment";
  product_id: string;
  segment: "" | "b2c" | "b2b";
  catalog_generation: string | null;
  enabled: boolean;
}

function pricingVersionKey(bindingId: string, effectiveVersion: string): string {
  return `${bindingId}\u0000${effectiveVersion}`;
}

function catalogEntryKey(productId: string, generation: string): string {
  return `${productId}\u0000${generation}`;
}

function customerRuleView(row: CustomerPricingRuleRow): CustomerPricingRuleView {
  return {
    ruleId: row.rule_id,
    scope: row.scope_type,
    pricingMode: row.pricing_mode,
    ruleOrigin: row.rule_origin,
    discountBps: row.discount_bps,
    payableMultiplierBp: row.payable_multiplier_bp,
    trackEligible: row.track_eligible,
    retentionEligible: row.retention_eligible,
    commissionEligible: row.commission_eligible,
  };
}

/**
 * The legacy scalar rule for a binding whose policy is not engine-enforced yet: a plain
 * provider discount mirroring the legacy scalar that billing actually applies. trackEligible and
 * the other capability flags stay false — the legacy lane has no track/retention/commission
 * semantics of its own. Track-mode materialized rules are presented through this rule directly;
 * discount-mode rules go through shadowPresentationRule, which uses it as the clamp ceiling.
 */
function legacyScalarRule(
  binding: CustomerPricingBindingRow,
  version: CustomerPricingVersionRow,
  providerId: string,
): CustomerPricingRuleRow {
  return {
    binding_id: binding.id,
    effective_version: version.effective_version,
    rule_id: `provider:${providerId}:legacy-scalar`,
    scope_type: "provider",
    provider_id: providerId,
    canonical_model_id: null,
    pricing_mode: "discount",
    rule_origin: "legacy",
    discount_bps: 10_000 - binding.legacy_multiplier_bp,
    payable_multiplier_bp: binding.legacy_multiplier_bp,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  };
}

/**
 * The customer-facing discount rule for a binding whose policy is not engine-enforced yet: the
 * materialized per-provider policy discount, clamped so it never exceeds the discount the
 * legacy scalar actually bills. A tighter negotiated provider rate (policy discount below the
 * scalar's) is shown as configured — billing can only over-deliver on it until the release
 * cutover; a looser one is clamped to the scalar so the dashboard never promises a discount
 * billing does not apply. The rule keeps the "legacy" origin and no capability flags: the
 * enforced price is still the scalar, not the engine-delivered policy.
 */
function shadowPresentationRule(
  binding: CustomerPricingBindingRow,
  version: CustomerPricingVersionRow,
  materialized: CustomerPricingRuleRow,
): CustomerPricingRuleRow {
  const legacy = legacyScalarRule(binding, version, materialized.provider_id);
  if (materialized.pricing_mode !== "discount" || materialized.discount_bps === null) return legacy;
  const discountBps = Math.min(materialized.discount_bps, legacy.discount_bps ?? 0);
  return {
    ...materialized,
    rule_origin: "legacy",
    discount_bps: discountBps,
    payable_multiplier_bp: 10_000 - discountBps,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  };
}

function customerPricingVersionView(input: {
  binding: CustomerPricingBindingRow;
  version: CustomerPricingVersionRow;
  rules: readonly CustomerPricingRuleRow[];
  catalogEntries: ReadonlyMap<string, readonly CustomerCatalogEntryRow[]>;
  admissionCatalogGeneration: string | null;
  switchEntries: ReadonlyMap<string, readonly CustomerSwitchEntryRow[]>;
  admissionSwitchGeneration: string | null;
}): CustomerPricingVersionView {
  const { binding, version } = input;
  const policyCatalog = input.catalogEntries.get(
    catalogEntryKey(version.product_id, version.catalog_generation),
  ) ?? [];
  const admissionCatalog = input.admissionCatalogGeneration === null
    ? []
    : input.catalogEntries.get(catalogEntryKey(version.product_id, input.admissionCatalogGeneration)) ?? [];
  const modelIdentities = new Map<string, { providerId: string; modelId: string }>();
  for (const entry of [...policyCatalog, ...admissionCatalog]) {
    modelIdentities.set(`${entry.provider_id}\u0000${entry.canonical_model_id}`, {
      providerId: entry.provider_id,
      modelId: entry.canonical_model_id,
    });
  }

  const policySwitches = input.switchEntries.get(version.switch_generation) ?? [];
  const admissionSwitches = input.admissionSwitchGeneration === null
    ? []
    : input.switchEntries.get(input.admissionSwitchGeneration) ?? [];
  const scopedType = binding.account_class === "b2c" || binding.account_class === "b2b"
    ? "segment"
    : "product";
  const scopedSegment = scopedType === "segment" ? binding.account_class : "";
  const findSwitch = (
    entries: readonly CustomerSwitchEntryRow[],
    providerId: string,
    scopeType: "master" | "product" | "segment",
  ) => entries.find((entry) => (
    entry.provider_id === providerId
    && entry.scope_type === scopeType
    && entry.product_id === (scopeType === "master" ? "" : version.product_id)
    && entry.segment === (scopeType === "segment" ? scopedSegment : "")
  ));

  const modelsByProvider = new Map<string, CustomerPricingModelView[]>();
  const sortedModels = [...modelIdentities.values()].sort((left, right) => (
    left.providerId.localeCompare(right.providerId, "en")
    || left.modelId.localeCompare(right.modelId, "en")
  ));
  for (const identity of sortedModels) {
    const policyModel = policyCatalog.find((entry) => (
      entry.provider_id === identity.providerId && entry.canonical_model_id === identity.modelId
    ));
    const admissionModel = admissionCatalog.find((entry) => (
      entry.provider_id === identity.providerId && entry.canonical_model_id === identity.modelId
    ));
    const policyMaster = findSwitch(policySwitches, identity.providerId, "master");
    const policyScoped = findSwitch(policySwitches, identity.providerId, scopedType);
    const admissionMaster = findSwitch(admissionSwitches, identity.providerId, "master");
    const admissionScoped = findSwitch(admissionSwitches, identity.providerId, scopedType);
    const exactRule = input.rules.find((rule) => (
      rule.provider_id === identity.providerId
      && rule.scope_type === "model"
      && rule.canonical_model_id === identity.modelId
    ));
    const providerRule = input.rules.find((rule) => (
      rule.provider_id === identity.providerId && rule.scope_type === "provider"
    ));
    // While the binding's policy is not engine-enforced (legacy_scalar or shadow), billing still
    // applies the legacy scalar on the engine account to every provider. The customer-facing
    // rule therefore surfaces the materialized per-provider policy discount, clamped to never
    // advertise a discount beyond the scalar billing actually charges: a tighter negotiated
    // provider rate shows as configured (billing can only over-deliver on it until the release
    // cutover), a looser one is clamped to the scalar. Providers the policy does not cover stay
    // unavailable exactly as the materialized rules say, and non-discount (track) rules keep the
    // plain scalar presentation. Policy and scalar converge at the release cutover.
    const materialized = exactRule ?? providerRule ?? null;
    const legacyScalarActive = binding.account_class === "b2b"
      && binding.policy_enforcement !== "strict";
    const rule = legacyScalarActive && materialized !== null
      ? shadowPresentationRule(binding, version, materialized)
      : materialized;
    const reasons: CustomerPricingUnavailableReason[] = [];
    if (policyModel?.enabled !== true) reasons.push("policy_catalog_disabled");
    if (admissionModel?.enabled !== true) reasons.push("admission_catalog_disabled");
    if (policyMaster?.enabled !== true) reasons.push("policy_master_switch_disabled");
    if (
      policyScoped?.enabled !== true
      || policyScoped.catalog_generation !== version.catalog_generation
    ) reasons.push("policy_scoped_switch_disabled");
    if (admissionMaster?.enabled !== true) reasons.push("admission_master_switch_disabled");
    if (
      admissionScoped?.enabled !== true
      || (
        admissionScoped.catalog_generation !== input.admissionCatalogGeneration
        && admissionScoped.catalog_generation !== version.catalog_generation
      )
    ) reasons.push("admission_scoped_switch_disabled");
    if (!rule) reasons.push("missing_pricing_rule");
    const model: CustomerPricingModelView = {
      modelId: identity.modelId,
      available: reasons.length === 0,
      unavailableReasons: reasons,
      rule: rule ? customerRuleView(rule) : null,
    };
    const providerModels = modelsByProvider.get(identity.providerId) ?? [];
    providerModels.push(model);
    modelsByProvider.set(identity.providerId, providerModels);
  }

  return {
    effectiveVersion: version.effective_version,
    policyVersion: version.policy_version,
    catalogGeneration: version.catalog_generation,
    switchGeneration: version.switch_generation,
    providers: [...modelsByProvider.entries()].map(([providerId, models]) => ({
      providerId,
      available: models.some((model) => model.available),
      models,
    })),
  };
}

/**
 * Returns the complete customer-facing policy projection from one coherent PostgreSQL snapshot.
 * Applied and desired versions remain distinct: callers must never present a desired version as
 * engine-active while a durable ACK is still pending.
 */
export async function getCustomerPricingPolicyView(
  database: Database,
  userId: string,
): Promise<CustomerPricingPolicyView[]> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const bindingResult = await client.query<CustomerPricingBindingRow>(`
      SELECT binding.id, binding.account_class, binding.product_id,
             binding.desired_effective_version::text,
             binding.desired_digest,
             binding.applied_effective_version::text,
             binding.applied_digest,
             binding.policy_enforcement,
             binding.funding_enforcement,
             binding.reconciliation_state,
             binding.sync_state,
             binding.last_ack_at,
             account.mult_bp AS legacy_multiplier_bp
      FROM account_policy_bindings binding
      JOIN engine_accounts account
        ON account.id = binding.engine_account_record_id
       AND account.user_id = binding.user_id
       AND account.engine_account_id = binding.engine_account_id
      WHERE binding.user_id = $1
        AND binding.account_class IN ('b2c', 'b2b')
      ORDER BY binding.product_id, binding.created_at, binding.id
    `, [userId]);
    if (bindingResult.rows.length === 0) {
      await client.query("COMMIT");
      return [];
    }
    const bindingIds = bindingResult.rows.map((binding) => binding.id);
    const versionResult = await client.query<CustomerPricingVersionRow>(`
      SELECT version.binding_id, version.effective_version::text,
             version.policy_version::text, version.product_id, version.account_class,
             version.catalog_generation::text, version.switch_generation::text
      FROM account_policy_versions version
      JOIN account_policy_bindings binding ON binding.id = version.binding_id
      WHERE version.binding_id = ANY($1::uuid[])
        AND version.effective_version IN (
          binding.desired_effective_version,
          binding.applied_effective_version
        )
      ORDER BY version.binding_id, version.effective_version
    `, [bindingIds]);
    const ruleResult = await client.query<CustomerPricingRuleRow>(`
      SELECT rule.binding_id, rule.effective_version::text, rule.rule_id,
             rule.scope_type, rule.provider_id, rule.canonical_model_id,
             rule.pricing_mode, rule.rule_origin, rule.discount_bps,
             rule.payable_multiplier_bp, rule.track_eligible,
             rule.retention_eligible, rule.commission_eligible
      FROM account_policy_rules rule
      JOIN account_policy_bindings binding ON binding.id = rule.binding_id
      WHERE rule.binding_id = ANY($1::uuid[])
        AND rule.effective_version IN (
          binding.desired_effective_version,
          binding.applied_effective_version
        )
      ORDER BY rule.binding_id, rule.effective_version, rule.provider_id,
               rule.scope_type, rule.canonical_model_id, rule.rule_id
    `, [bindingIds]);
    const catalogResult = await client.query<CustomerCatalogEntryRow>(`
      SELECT entry.product_id, entry.generation::text, entry.provider_id,
             entry.canonical_model_id, entry.enabled
      FROM product_catalog_entries entry
      WHERE EXISTS (
        SELECT 1
        FROM account_policy_versions version
        JOIN account_policy_bindings binding ON binding.id = version.binding_id
        WHERE version.binding_id = ANY($1::uuid[])
          AND version.effective_version IN (
            binding.desired_effective_version,
            binding.applied_effective_version
          )
          AND version.product_id = entry.product_id
          AND version.catalog_generation = entry.generation
      ) OR EXISTS (
        SELECT 1
        FROM account_policy_bindings binding
        JOIN product_catalog_heads head ON head.product_id = binding.product_id
        WHERE binding.id = ANY($1::uuid[])
          AND head.product_id = entry.product_id
          AND head.active_generation = entry.generation
      )
      ORDER BY entry.product_id, entry.generation, entry.provider_id,
               entry.canonical_model_id
    `, [bindingIds]);
    const catalogHeadResult = await client.query<{ product_id: string; active_generation: string }>(`
      SELECT head.product_id, head.active_generation::text
      FROM product_catalog_heads head
      WHERE head.product_id IN (
        SELECT product_id FROM account_policy_bindings WHERE id = ANY($1::uuid[])
      )
      ORDER BY head.product_id
    `, [bindingIds]);
    const switchResult = await client.query<CustomerSwitchEntryRow>(`
      SELECT entry.generation::text, entry.provider_id, entry.scope_type,
             entry.product_id, entry.segment, entry.catalog_generation::text,
             entry.enabled
      FROM provider_switch_entries entry
      WHERE entry.generation IN (
        SELECT version.switch_generation
        FROM account_policy_versions version
        JOIN account_policy_bindings binding ON binding.id = version.binding_id
        WHERE version.binding_id = ANY($1::uuid[])
          AND version.effective_version IN (
            binding.desired_effective_version,
            binding.applied_effective_version
          )
        UNION
        SELECT head.active_generation FROM provider_switch_head head WHERE head.singleton = 1
      )
      ORDER BY entry.generation, entry.provider_id, entry.scope_type,
               entry.product_id, entry.segment
    `, [bindingIds]);
    const switchHeadResult = await client.query<{ active_generation: string }>(`
      SELECT active_generation::text FROM provider_switch_head WHERE singleton = 1
    `);

    const versions = new Map(versionResult.rows.map((version) => [
      pricingVersionKey(version.binding_id, version.effective_version),
      version,
    ]));
    const rules = new Map<string, CustomerPricingRuleRow[]>();
    for (const rule of ruleResult.rows) {
      const key = pricingVersionKey(rule.binding_id, rule.effective_version);
      const rows = rules.get(key) ?? [];
      rows.push(rule);
      rules.set(key, rows);
    }
    const catalogEntries = new Map<string, CustomerCatalogEntryRow[]>();
    for (const entry of catalogResult.rows) {
      const key = catalogEntryKey(entry.product_id, entry.generation);
      const rows = catalogEntries.get(key) ?? [];
      rows.push(entry);
      catalogEntries.set(key, rows);
    }
    const catalogHeads = new Map(catalogHeadResult.rows.map((head) => [
      head.product_id,
      head.active_generation,
    ]));
    const switchEntries = new Map<string, CustomerSwitchEntryRow[]>();
    for (const entry of switchResult.rows) {
      const rows = switchEntries.get(entry.generation) ?? [];
      rows.push(entry);
      switchEntries.set(entry.generation, rows);
    }
    const admissionSwitchGeneration = switchHeadResult.rows[0]?.active_generation ?? null;
    const view = bindingResult.rows.map((binding): CustomerPricingPolicyView => {
      const mapVersion = (effectiveVersion: string | null): CustomerPricingVersionView | null => {
        if (effectiveVersion === null) return null;
        const key = pricingVersionKey(binding.id, effectiveVersion);
        const version = versions.get(key);
        if (!version) return null;
        return customerPricingVersionView({
          binding,
          version,
          rules: rules.get(key) ?? [],
          catalogEntries,
          admissionCatalogGeneration: catalogHeads.get(binding.product_id) ?? null,
          switchEntries,
          admissionSwitchGeneration,
        });
      };
      return {
        accountClass: binding.account_class,
        productId: binding.product_id,
        policyEnforcement: binding.policy_enforcement,
        fundingEnforcement: binding.funding_enforcement,
        reconciliationState: binding.reconciliation_state,
        syncState: binding.sync_state,
        inSync: binding.sync_state === "confirmed"
          && binding.desired_effective_version !== null
          && binding.desired_effective_version === binding.applied_effective_version
          && binding.desired_digest === binding.applied_digest,
        lastAcknowledgedAt: binding.last_ack_at?.toISOString() ?? null,
        desired: mapVersion(binding.desired_effective_version),
        applied: mapVersion(binding.applied_effective_version),
      };
    });
    await client.query("COMMIT");
    return view;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function setBusinessPricing(database: Database, input: {
  userId: string;
  multiplierBp: number;
  actorId: string;
  reason: string;
}): Promise<{ engineAccountId: string; jobId: string }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ engine_account_id: string }>(`
      SELECT ea.engine_account_id
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1 AND cp.customer_type = 'b2b'
        AND ea.engine_account_id IS NOT NULL
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    if (!row) throw new BusinessCustomerNotFoundError("business customer not found");
    await client.query(`
      UPDATE customer_profiles SET multiplier_bp = $2, updated_at = now() WHERE user_id = $1;
    `, [input.userId, input.multiplierBp]);
    await client.query(`
      UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
    `, [input.userId, input.multiplierBp]);
    const jobId = await enqueuePricingJob(client, {
      userId: input.userId,
      engineAccountId: row.engine_account_id,
      multiplierBp: input.multiplierBp,
      reason: "b2b_manual",
    });
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing.b2b_changed', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({
      multiplierBp: input.multiplierBp,
      reason: input.reason,
      jobId,
    })]);
    await client.query("COMMIT");
    return { engineAccountId: row.engine_account_id, jobId };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Converts an existing B2C customer to a supplied negotiated B2B multiplier atomically. B2C
 * progress remains historical data, while the live tier/window/floor controls are cleared so no
 * later B2C reconciliation can change the negotiated rate. The conversion also provisions the
 * managed b2b_client policy (single Anthropic discount rule mirroring the multiplier), its
 * account binding, and a staged engine delivery job — the same end state invitation redemption
 * reaches; without it the admin policy editor has nothing to manage. Re-running the conversion
 * on an already-B2B customer repairs a missing policy (customers converted before this
 * provisioning existed) and is otherwise an unchanged no-op.
 */
export async function convertCustomerToBusiness(database: Database, input: {
  userId: string;
  actorId: string;
  reason: string;
  multiplierBp: number;
}): Promise<{ converted: boolean; multiplierBp: number; engineAccountId: string; jobId: string | null }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      customer_type: "b2c" | "b2b";
      current_tier: number | null;
      multiplier_bp: number;
      referral_floor_bps: number;
      engine_account_record_id: string;
      engine_account_id: string | null;
    }>(`
      SELECT cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.referral_floor_bps,
             ea.id::text AS engine_account_record_id, ea.engine_account_id
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    if (!row) throw new CustomerProfileNotFoundError("customer profile or engine account not found");
    if (!row.engine_account_id) throw new BusinessCustomerNotFoundError("customer engine account is not provisioned");
    if (row.customer_type === "b2b") {
      // Customers converted before managed-policy provisioning existed have no b2b_client
      // policy, so the admin editor cannot manage them. Re-running the conversion repairs
      // exactly that gap against the multiplier already in effect and stages the delivery;
      // a fully provisioned customer stays an unchanged no-op.
      const repaired = await provisionBusinessClientPolicy(client, {
        userId: input.userId,
        engineAccountRecordId: row.engine_account_record_id,
        engineAccountId: row.engine_account_id,
        multiplierBp: row.multiplier_bp,
        actorId: input.actorId,
        reason: input.reason,
      });
      if (!repaired.provisioned) {
        await client.query("ROLLBACK");
        return {
          converted: false,
          multiplierBp: row.multiplier_bp,
          engineAccountId: row.engine_account_id,
          jobId: null,
        };
      }
      await client.query(`
        INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
        VALUES ('admin', $1, 'pricing.b2b_policy_provisioned', 'user', $2, $3::jsonb)
      `, [input.actorId, input.userId, JSON.stringify({
        reason: input.reason,
        multiplierBp: row.multiplier_bp,
        policyId: repaired.policyId,
        policyVersion: repaired.policyVersion,
        policyDigest: repaired.policyDigest,
        policyJobId: repaired.jobId,
      })]);
      await client.query("COMMIT");
      return {
        converted: false,
        multiplierBp: row.multiplier_bp,
        engineAccountId: row.engine_account_id,
        jobId: repaired.jobId,
      };
    }

    await client.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL,
          tier_window_start = NULL, tier_window_spent_nano = 0,
          referral_floor_bps = 0, multiplier_bp = $2, updated_at = now()
      WHERE user_id = $1
    `, [input.userId, input.multiplierBp]);
    await client.query(`
      UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
    `, [input.userId, input.multiplierBp]);
    const policy = await provisionBusinessClientPolicy(client, {
      userId: input.userId,
      engineAccountRecordId: row.engine_account_record_id,
      engineAccountId: row.engine_account_id,
      multiplierBp: input.multiplierBp,
      actorId: input.actorId,
      reason: input.reason,
    });
    const jobId = await enqueuePricingJob(client, {
      userId: input.userId,
      engineAccountId: row.engine_account_id,
      multiplierBp: input.multiplierBp,
      reason: "b2b_conversion",
    });
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing.b2b_converted', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({
      reason: input.reason,
      previousMultiplierBp: row.multiplier_bp,
      negotiatedMultiplierBp: input.multiplierBp,
      previousTier: row.current_tier,
      previousReferralFloorBps: row.referral_floor_bps,
      managedPolicyId: policy.policyId,
      managedPolicyVersion: policy.policyVersion,
      managedPolicyDigest: policy.policyDigest,
      managedPolicyJobId: policy.jobId,
    })]);
    await client.query("COMMIT");
    return {
      converted: true,
      multiplierBp: input.multiplierBp,
      engineAccountId: row.engine_account_id,
      jobId,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Устанавливает «пол» скидки от сейлза для реферала ПАРТНЁРА. Клиент ОСТАЁТСЯ b2c и идёт по
 * обычным тир-правилам (тир, бонус, промо — как у всех); floor лишь гарантирует, что цена не хуже
 * скидки сейлза: эффективный mult = min(тир-mult, 10000 - floorBps). floorBps=0 снимает пол
 * (возврат к чистому тиру). Разграничение b2c/b2b сохранено: для business-b2b пол не применяется.
 * Мультипликатор доставляется в движок через durable engine_pricing_jobs. Идемпотентно.
 * Вызывается ТОЛЬКО для партнёрских реф-кодов (обычная сайтовая рефка сюда не идёт).
 */
export async function setReferralFloor(database: Database, input: {
  userId: string;
  floorBps: number; // 0..9500 (скидка ≤ 95%); 0 = снять пол
  actorId: string;
  // Явная АБСОЛЮТНАЯ установка пола (партнёр/админ меняет процент действующему рефералу):
  // обходит монотонный GREATEST и позволяет ПОНИЗИТЬ floor. Автоматические пути (signup,
  // sales-фид, промо) обязаны оставлять override=false — иначе replay затрёт лучшую скидку.
  override?: boolean;
}): Promise<{ applied: boolean; multiplierBp: number | null }> {
  if (!Number.isInteger(input.floorBps) || input.floorBps < 0 || input.floorBps > 9500) {
    throw new RangeError("referral floor must be an integer between 0 and 9500 bps (≤95%)");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      engine_account_id: string | null; customer_type: "b2c" | "b2b";
      current_tier: number | null; multiplier_bp: number; referral_floor_bps: number;
    }>(`
      SELECT ea.engine_account_id, cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.referral_floor_bps
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    // Пол применяется только к обычным b2c-аккаунтам; business-b2b не трогаем (своя прайс-логика).
    if (!row || row.customer_type !== "b2c" || row.current_tier === null) {
      await client.query("ROLLBACK");
      return { applied: false, multiplierBp: null };
    }
    const tierMult = B2C_PRICING_TIERS[row.current_tier]!.multiplierBp;
    // «Пол» скидки — МОНОТОНЕН: floorBps>0 только ПОДНИМАЕТ (GREATEST). Колонка одна, но пишут в неё
    // три независимых источника (промо, партнёрская ссылка, sales-фид) — при абсолютной записи они
    // затирали друг друга и клиент молча терял лучшую скидку. Теперь берём максимум (лучшее клиенту).
    // floorBps===0 — единственный путь ЯВНОГО сброса пола (напр. отзыв), обходит GREATEST.
    const effectiveFloor = input.override === true
      ? input.floorBps
      : input.floorBps === 0 ? 0 : Math.max(row.referral_floor_bps, input.floorBps);
    const multiplierBp = effectiveMultiplierBp(tierMult, effectiveFloor);
    if (row.referral_floor_bps === effectiveFloor && row.multiplier_bp === multiplierBp) {
      await client.query("ROLLBACK");
      return { applied: false, multiplierBp };
    }
    await client.query(`
      UPDATE customer_profiles
      SET referral_floor_bps = $2, multiplier_bp = $3, updated_at = now()
      WHERE user_id = $1
    `, [input.userId, effectiveFloor, multiplierBp]);
    await client.query(`
      UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
    `, [input.userId, multiplierBp]);
    if (row.engine_account_id) {
      await enqueuePricingJob(client, {
        userId: input.userId,
        engineAccountId: row.engine_account_id,
        multiplierBp,
        reason: "referral_floor",
      });
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('system', $1, 'pricing.referral_floor', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({ requestedFloorBps: input.floorBps, effectiveFloorBps: effectiveFloor, multiplierBp, override: input.override === true })]);
    await client.query("COMMIT");
    return { applied: true, multiplierBp };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function listPricingSyncTargets(database: Database): Promise<PricingSyncTarget[]> {
  // И b2c, и b2b: расход обязан попадать в immutable pricing_usage_events для обеих сегментов.
  // Прогрессивные эффекты (free-first, месяцы, тир-окна) применяются только к b2c внутри
  // applyPricingLedgerPage и в b2c-фильтрованных функциях тир-модели.
  const result = await database.pool.query<{ user_id: string; engine_account_id: string }>(`
    SELECT cp.user_id, ea.engine_account_id
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    JOIN users u ON u.id = cp.user_id
    WHERE cp.customer_type IN ('b2c', 'b2b') AND ea.status = 'active'
      AND ea.engine_account_id IS NOT NULL AND u.status = 'active'
    ORDER BY cp.user_id
  `);
  return result.rows.map((row) => ({ userId: row.user_id, engineAccountId: row.engine_account_id }));
}

export async function getPricingUsageCursor(
  database: Database,
  target: PricingSyncTarget,
): Promise<bigint> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(`
      DELETE FROM pricing_usage_cursors WHERE user_id = $1 AND engine_account_id <> $2
    `, [target.userId, target.engineAccountId]);
    // Invalidate the completion marker before network I/O. Only a terminal short page restores it;
    // a thrown/failed sync therefore cannot authorize window closure with a previous cycle's marker.
    const result = await client.query<{ last_ledger_id: string }>(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id, updated_at)
      VALUES ($1, $2, '-infinity')
      ON CONFLICT (engine_account_id) DO UPDATE SET updated_at = '-infinity'
      RETURNING last_ledger_id
    `, [target.engineAccountId, target.userId]);
    // Reconcile durable credit accrual markers on every pricing poll. This catches a missed
    // post-credit call and reverses markers whose payment has since been refunded/disputed.
    await reconcileTopupTier(client, target, "b2c_topup_reconcile");
    await client.query("COMMIT");
    return BigInt(result.rows[0]?.last_ledger_id ?? "0");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Marks an empty engine-ledger page as a completed scan. The cursor is invalidated before network
 * I/O by getPricingUsageCursor, so callers must invoke this only after the engine returns a genuine
 * terminal empty page.
 */
export async function completePricingUsageSync(
  database: Database,
  target: PricingSyncTarget,
): Promise<void> {
  await database.pool.query(`
    UPDATE pricing_usage_cursors
    SET updated_at = now()
    WHERE engine_account_id = $1 AND user_id = $2
  `, [target.engineAccountId, target.userId]);
}

async function resolveAttributionBinding(
  client: PoolClient,
  target: PricingSyncTarget,
  validated: ValidatedLedgerAttribution,
): Promise<string | null> {
  const attribution = validated.attribution;
  if (attribution.snapshot_kind !== "policy_v1") return null;
  const result = await client.query<{ id: string }>(`
    SELECT binding.id
    FROM account_policy_bindings binding
    JOIN account_policy_versions version
      ON version.binding_id = binding.id
     AND version.effective_version = $3
    JOIN account_policy_rules rule
      ON rule.binding_id = version.binding_id
     AND rule.effective_version = version.effective_version
     AND rule.rule_id = $12
    JOIN product_catalog_versions admission_catalog
      ON admission_catalog.product_id = version.product_id
     AND admission_catalog.generation = $24
     AND admission_catalog.content_digest = $25
    JOIN provider_switch_versions admission_switch
      ON admission_switch.generation = $26
     AND admission_switch.content_digest = $27
    WHERE binding.user_id = $1
      AND binding.engine_account_id = $2
      AND binding.policy_id = $4
      AND binding.product_id = $7
      AND binding.account_class = $8
      AND version.policy_id = $4
      AND version.policy_version = $5
      AND version.policy_digest = $6
      AND version.product_id = $7
      AND version.account_class = $8
      AND version.catalog_generation = $9
      AND version.switch_generation = $10
      AND version.content_digest = $11
      AND rule.rule_digest = $13
      AND rule.scope_type = $14
      AND rule.provider_id = $15
      AND rule.canonical_model_id IS NOT DISTINCT FROM $16::text
      AND rule.pricing_mode = $17
      AND rule.rule_origin = $18
      AND rule.discount_bps IS NOT DISTINCT FROM $19::integer
      AND rule.payable_multiplier_bp = $20
      AND rule.track_eligible = $21
      AND rule.retention_eligible = $22
      AND rule.commission_eligible = $23
  `, [
    target.userId,
    target.engineAccountId,
    attribution.effective_policy_version,
    attribution.policy_id,
    attribution.policy_version,
    attribution.source_policy_digest,
    attribution.product_id,
    attribution.account_class,
    attribution.catalog_generation,
    attribution.switch_generation,
    attribution.policy_digest,
    attribution.rule_id,
    attribution.rule_digest,
    attribution.rule_scope,
    attribution.provider_id,
    attribution.rule_scope === "model" ? attribution.canonical_model_id : null,
    attribution.pricing_mode,
    attribution.rule_origin,
    attribution.discount_bps,
    attribution.payable_multiplier_bp,
    attribution.track_eligible,
    attribution.retention_eligible,
    attribution.commission_eligible,
    attribution.admission_catalog_generation,
    attribution.admission_catalog_digest,
    attribution.admission_switch_generation,
    attribution.admission_switch_digest,
  ]);
  if (result.rows.length !== 1) {
    throw new PricingLedgerAttributionError(
      "engine policy attribution does not match one immutable commerce policy version",
    );
  }
  return result.rows[0]!.id;
}

async function insertPricingUsageAttribution(
  client: PoolClient,
  target: PricingSyncTarget,
  eventId: string,
  entry: EngineLedgerEntry,
  chargedNano: bigint,
  validated: ValidatedLedgerAttribution,
): Promise<void> {
  const attribution = validated.attribution;
  const bindingId = await resolveAttributionBinding(client, target, validated);
  const effectivePolicyDigest = attribution.snapshot_kind === "policy_v1"
    ? attribution.policy_digest
    : null;
  // policy_v1 stores the source policy digest in the legacy column; release-v2 has a single
  // policy identity, so its immutable policy_digest lands there directly (its CHECK requires
  // a non-empty value), while effective/binding columns stay NULL by contract.
  const storedPolicyDigest = attribution.snapshot_kind === "policy_v1"
    ? attribution.source_policy_digest
    : attribution.policy_digest;
  const tariffPricedAt = attribution.tariff_priced_ts === null
    ? null
    : epochSecondsDate(attribution.tariff_priced_ts, "tariff priced timestamp");
  await client.query(`
    INSERT INTO pricing_usage_attributions (
      pricing_usage_event_id, attribution_schema_version, snapshot_kind,
      engine_request_id, provider_id, product_id, account_class, binding_id,
      requested_model_id, canonical_model_id, served_model_id,
      served_canonical_model_id, billing_invariant_code, alias_generation,
      rule_id, rule_digest, rule_scope, pricing_mode, rule_origin, discount_bps,
      payable_multiplier_bp, policy_id, policy_version, effective_policy_version,
      effective_policy_digest, policy_digest, source_policy_digest,
      catalog_generation, switch_generation, admission_catalog_generation,
      admission_catalog_digest, admission_switch_generation, admission_switch_digest,
      runtime_manifest_generation, runtime_manifest_digest, tariff_schedule_id,
      tariff_priced_at, official_nano, charged_nano, official_cost_json,
      paid_funded_nano, bonus_funded_nano, other_funded_nano,
      funding_allocation_json, track_eligible, retention_eligible,
      commission_eligible, snapshot_digest,
      release_schema_version, release_generation, release_digest,
      release_billing_mode, release_funding_generation
    ) VALUES (
      $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,
      $17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,
      $33,$34,$35,$36,$37,$38,$39,$40::jsonb,$41,$42,$43,$44::jsonb,
      $45,$46,$47,$48,$49,$50,$51,$52,$53
    )
  `, [
    eventId,
    attribution.attribution_schema_version,
    attribution.snapshot_kind,
    entry.request_id ?? null,
    attribution.provider_id,
    attribution.product_id,
    attribution.account_class,
    bindingId,
    attribution.requested_model_id,
    attribution.canonical_model_id,
    attribution.served_model_id,
    attribution.served_canonical_model_id,
    attribution.billing_invariant_code,
    attribution.alias_generation,
    attribution.rule_id,
    attribution.rule_digest,
    attribution.rule_scope,
    attribution.pricing_mode,
    attribution.rule_origin,
    attribution.discount_bps,
    attribution.payable_multiplier_bp,
    attribution.policy_id,
    attribution.policy_version,
    attribution.effective_policy_version,
    effectivePolicyDigest,
    storedPolicyDigest,
    attribution.source_policy_digest,
    attribution.catalog_generation,
    attribution.switch_generation,
    attribution.admission_catalog_generation,
    attribution.admission_catalog_digest,
    attribution.admission_switch_generation,
    attribution.admission_switch_digest,
    attribution.runtime_manifest_generation,
    attribution.runtime_manifest_digest,
    attribution.tariff_schedule_id,
    tariffPricedAt,
    attribution.official_nano,
    chargedNano.toString(),
    attribution.official_cost_json === null ? null : JSON.stringify(attribution.official_cost_json),
    attribution.paid_funded_nano,
    attribution.bonus_funded_nano,
    attribution.other_funded_nano,
    attribution.funding_allocation_json === null
      ? null
      : JSON.stringify(attribution.funding_allocation_json),
    // Release-v2 rows carry NULL engine eligibility by contract; locally they are durable
    // non-tier rows, and commission authority is the computed class/paid-funding derivation.
    attribution.track_eligible ?? false,
    attribution.retention_eligible ?? false,
    validated.commissionEligible,
    attribution.snapshot_digest,
    attribution.release_schema_version,
    attribution.release_generation,
    attribution.release_digest,
    attribution.release_billing_mode,
    attribution.release_funding_generation,
  ]);
  for (const allocation of validated.allocations) {
    await client.query(`
      INSERT INTO pricing_usage_funding_allocations (
        pricing_usage_event_id, ordinal, engine_bucket_id, bucket_version,
        source_type, source_ref, amount_nano
      ) VALUES ($1,$2,$3,$4,$5,$6,$7)
    `, [
      eventId,
      allocation.ordinal,
      allocation.engineBucketId,
      allocation.bucketVersion,
      allocation.sourceType,
      allocation.sourceRef,
      allocation.amountNano,
    ]);
  }
}

/**
 * Returns the ledger cursor immediately before the oldest recent usage row whose provider has not
 * completed the current evidence algorithm and cannot already be proven by immutable pricing
 * attribution. NULL and both historical sentinels are eligible only below the current version, so
 * a stronger producer can retry old terminal rows once without creating an idle polling loop. The
 * engine retains charge detail for the same 30-day horizon used by the paying-users control room.
 */
export async function getPricingProviderBackfillCursor(
  database: Database,
  target: PricingSyncTarget,
  throughLedgerId: bigint,
): Promise<bigint | null> {
  if (throughLedgerId < 0n) throw new RangeError("provider backfill cursor must not be negative");
  const result = await database.pool.query<{ first_ledger_id: string | null }>(`
    SELECT min(event.ledger_entry_id)::text AS first_ledger_id
    FROM pricing_usage_events event
    LEFT JOIN pricing_usage_attributions attribution
      ON attribution.pricing_usage_event_id = event.id
    WHERE event.user_id = $1 AND event.engine_account_id = $2
      AND event.ledger_entry_id <= $3
      AND event.occurred_at >= now() - make_interval(days => $4)
      AND event.provider_recovery_version < $5
      AND (event.provider_id IS NULL OR event.provider_id IN ($6, $7))
      AND attribution.provider_id IS NULL
  `, [
    target.userId,
    target.engineAccountId,
    throughLedgerId.toString(),
    PROVIDER_BACKFILL_WINDOW_DAYS,
    PROVIDER_RECOVERY_VERSION,
    UNATTRIBUTED_PROVIDER_ID,
    UNAVAILABLE_PROVIDER_ID,
  ]);
  const firstLedgerId = result.rows[0]?.first_ledger_id;
  if (firstLedgerId === null || firstLedgerId === undefined) return null;
  const first = BigInt(firstLedgerId);
  return first > 0n ? first - 1n : 0n;
}

/**
 * Copies provider evidence from a retained engine ledger page onto matching immutable commerce
 * events. Amount and any existing attribution are locked and compared before the nullable field is
 * filled; conflicting evidence aborts the page instead of silently relabelling spend.
 */
export async function applyPricingProviderBackfillPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
): Promise<number> {
  const evidence = new Map<string, { amountNano: string; providerId: string }>();
  for (const entry of entries) {
    const amount = BigInt(entry.amount_nano);
    if (entry.kind !== "charge" || amount <= 0n) continue;
    const ledgerId = BigInt(entry.id).toString();
    const candidate = {
      amountNano: amount.toString(),
      providerId: ledgerProviderEvidence(entry),
    };
    const previous = evidence.get(ledgerId);
    if (
      previous !== undefined
      && (previous.amountNano !== candidate.amountNano || previous.providerId !== candidate.providerId)
    ) {
      throw new PricingLedgerAttributionError(
        `engine provider backfill repeated ledger ${ledgerId} with conflicting evidence`,
      );
    }
    evidence.set(ledgerId, candidate);
  }
  if (evidence.size === 0) return 0;

  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const ledgerIds = [...evidence.keys()];
    const existing = await client.query<{
      ledger_entry_id: string;
      amount_nano: string;
      provider_id: string | null;
      provider_recovery_version: number;
      attributed_provider_id: string | null;
    }>(`
      SELECT event.ledger_entry_id::text, event.amount_nano::text,
             event.provider_id, event.provider_recovery_version,
             attribution.provider_id AS attributed_provider_id
      FROM pricing_usage_events event
      LEFT JOIN pricing_usage_attributions attribution
        ON attribution.pricing_usage_event_id = event.id
      WHERE event.user_id = $1 AND event.engine_account_id = $2
        AND event.ledger_entry_id = ANY($3::bigint[])
      FOR UPDATE OF event
    `, [target.userId, target.engineAccountId, ledgerIds]);

    const updateLedgerIds: string[] = [];
    const updateProviderIds: string[] = [];
    for (const row of existing.rows) {
      const candidate = evidence.get(row.ledger_entry_id)!;
      if (row.amount_nano !== candidate.amountNano) {
        throw new PricingLedgerAttributionError(
          `engine provider backfill amount differs for ledger ${row.ledger_entry_id}`,
        );
      }
      const exactProviders = [row.provider_id, row.attributed_provider_id]
        .map(normalizedProviderId)
        .filter((value): value is string => value !== null && !isProviderRecoverySentinel(value));
      if (
        candidate.providerId !== UNATTRIBUTED_PROVIDER_ID
        && exactProviders.some((providerId) => providerId !== candidate.providerId)
      ) {
        throw new PricingLedgerAttributionError(
          `engine provider backfill identity differs for ledger ${row.ledger_entry_id}`,
        );
      }
      if (
        candidate.providerId === UNATTRIBUTED_PROVIDER_ID
        && exactProviders.length > 0
      ) continue;
      const currentProviderId = normalizedProviderId(row.provider_id);
      const needsCurrentRecovery = row.provider_recovery_version < PROVIDER_RECOVERY_VERSION
        && (currentProviderId === null || isProviderRecoverySentinel(currentProviderId))
        && normalizedProviderId(row.attributed_provider_id) === null;
      if (needsCurrentRecovery && candidate.providerId !== UNATTRIBUTED_PROVIDER_ID) {
        updateLedgerIds.push(row.ledger_entry_id);
        updateProviderIds.push(candidate.providerId);
      }
    }

    let updated = 0;
    if (updateLedgerIds.length > 0) {
      const result = await client.query(`
        UPDATE pricing_usage_events event
        SET provider_id = evidence.provider_id,
            provider_recovery_version = $5
        FROM unnest($3::bigint[], $4::text[]) AS evidence(ledger_entry_id, provider_id)
        WHERE event.user_id = $1 AND event.engine_account_id = $2
          AND event.ledger_entry_id = evidence.ledger_entry_id
      `, [
        target.userId,
        target.engineAccountId,
        updateLedgerIds,
        updateProviderIds,
        PROVIDER_RECOVERY_VERSION,
      ]);
      updated = result.rowCount ?? 0;
    }
    await client.query("COMMIT");
    return updated;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Terminalizes one attempted recovery range whose retained ledger evidence remains unavailable. */
export async function completePricingProviderBackfill(
  database: Database,
  target: PricingSyncTarget,
  throughLedgerId: bigint,
): Promise<number> {
  if (throughLedgerId < 0n) throw new RangeError("provider backfill cursor must not be negative");
  const result = await database.pool.query(`
    UPDATE pricing_usage_events event
    SET provider_id = $4, provider_recovery_version = $5
    WHERE event.user_id = $1 AND event.engine_account_id = $2
      AND event.ledger_entry_id <= $3
      AND event.occurred_at >= now() - make_interval(days => $6)
      AND event.provider_recovery_version < $5
      AND (event.provider_id IS NULL OR event.provider_id IN ($7, $8))
      AND NOT EXISTS (
        SELECT 1 FROM pricing_usage_attributions attribution
        WHERE attribution.pricing_usage_event_id = event.id
          AND attribution.provider_id IS NOT NULL
      )
  `, [
    target.userId,
    target.engineAccountId,
    throughLedgerId.toString(),
    UNAVAILABLE_PROVIDER_ID,
    PROVIDER_RECOVERY_VERSION,
    PROVIDER_BACKFILL_WINDOW_DAYS,
    UNATTRIBUTED_PROVIDER_ID,
    UNAVAILABLE_PROVIDER_ID,
  ]);
  return result.rowCount ?? 0;
}

/**
 * Пишет одно движковое пополнение в иммутабельную отчётную таблицу. Идемпотентна по
 * (engine_account_id, ledger_entry_id): повторная подача той же страницы ничего не двоит.
 */
async function recordPricingTopup(
  client: PoolClient,
  target: PricingSyncTarget,
  entry: EngineLedgerEntry,
  amount: bigint,
): Promise<void> {
  await client.query(`
    INSERT INTO pricing_usage_topups (
      id, user_id, engine_account_id, ledger_entry_id, ref, source, amount_nano, occurred_at
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    ON CONFLICT (engine_account_id, ledger_entry_id) DO NOTHING
  `, [
    randomUUID(),
    target.userId,
    target.engineAccountId,
    BigInt(entry.id).toString(),
    entry.ref,
    classifyTopupRef(entry.ref),
    amount.toString(),
    epochSecondsDate(entry.ts, "topup timestamp"),
  ]);
}

/**
 * Курсор догоняющего скана пополнений. Обычный курсор расхода уже стоит выше исторических
 * топапов, поэтому у отчётной таблицы свой маркер: пока он ниже основного курсора, воркер
 * ограниченными страницами перечитывает леджер с начала и заполняет историю ровно один раз.
 * NULL — история уже покрыта, скан не нужен.
 */
export async function getPricingTopupBackfillCursor(
  database: Database,
  target: PricingSyncTarget,
  throughLedgerId: bigint,
): Promise<bigint | null> {
  if (throughLedgerId < 0n) throw new RangeError("topup backfill cursor must not be negative");
  const result = await database.pool.query<{ scanned: string }>(`
    SELECT topups_scanned_through_ledger_id::text AS scanned
    FROM pricing_usage_cursors
    WHERE engine_account_id = $1 AND user_id = $2
  `, [target.engineAccountId, target.userId]);
  const scanned = result.rows[0]?.scanned;
  if (scanned === undefined) return null;
  const from = BigInt(scanned);
  return from >= throughLedgerId ? null : from;
}

/** Заполняет отчётные пополнения по одной странице леджера и двигает маркер скана. */
export async function applyPricingTopupBackfillPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
  scannedThroughLedgerId: bigint,
): Promise<number> {
  if (scannedThroughLedgerId < 0n) throw new RangeError("topup backfill cursor must not be negative");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    let inserted = 0;
    for (const entry of entries) {
      const amount = BigInt(entry.amount_nano);
      if (entry.kind !== "topup" || amount <= 0n) continue;
      const before = await client.query(`
        SELECT 1 FROM pricing_usage_topups
        WHERE engine_account_id = $1 AND ledger_entry_id = $2
      `, [target.engineAccountId, BigInt(entry.id).toString()]);
      if (before.rowCount) continue;
      await recordPricingTopup(client, target, entry, amount);
      inserted += 1;
    }
    await client.query(`
      UPDATE pricing_usage_cursors
      SET topups_scanned_through_ledger_id = GREATEST(topups_scanned_through_ledger_id, $3)
      WHERE engine_account_id = $1 AND user_id = $2
    `, [target.engineAccountId, target.userId, scannedThroughLedgerId.toString()]);
    await client.query("COMMIT");
    return inserted;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function applyPricingLedgerPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
): Promise<void> {
  if (entries.length === 0) return;
  // Legacy free-first требует ХРОНОЛОГИЧЕСКОГО порядка: топап должен фондировать последующие charge.
  // Леджер-id движка монотонны по времени создания, поэтому сортируем страницу по id по возрастанию —
  // иначе pre-attribution charge, пришедший раньше своего фондирующего топапа, завысил бы legacy
  // real_funded. Для policy rows деньги берутся из immutable funding evidence. Порядок также делает
  // продвижение курсора детерминированным.
  const ordered = [...entries].sort((a, b) => {
    const da = BigInt(a.id);
    const db = BigInt(b.id);
    return da < db ? -1 : da > db ? 1 : 0;
  });
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // B2B-профиль тоже лочится и синкается: у него нет тира и free-first проекции, но его
    // расход обязан попадать в immutable pricing_usage_events — иначе конвертация клиента в
    // B2B навсегда замораживает его курсор и админка недосчитывает реальные списания.
    const profileResult = await client.query<{
      customer_type: "b2c" | "b2b"; current_tier: number | null;
      pricing_month_start: Date | null; free_balance_nano: string;
    }>(`
      SELECT customer_type, current_tier, pricing_month_start,
             free_balance_nano::text AS free_balance_nano
      FROM customer_profiles
      WHERE user_id = $1 FOR UPDATE
    `, [target.userId]);
    const profile = profileResult.rows[0];
    if (!profile) {
      await client.query("ROLLBACK");
      return;
    }
    // Для b2b авторитет фондирования — только immutable evidence движка: локальная
    // free-first проекция, месячные окна и тир-счётчики к нему не применяются.
    const progressive = profile.customer_type === "b2c";
    // Курсор на старте: применяем эффекты (free projection, commission basis) ТОЛЬКО к записям выше него —
    // это делает применение страницы идемпотентным к повторной подаче тех же записей (free-топапы
    // не имеют ON CONFLICT, как у charge). customer_profiles залочена → обработка юзера сериализована.
    const cursorRow = await client.query<{ last_ledger_id: string }>(
      "SELECT last_ledger_id::text AS last_ledger_id FROM pricing_usage_cursors WHERE engine_account_id = $1 AND user_id = $2",
      [target.engineAccountId, target.userId],
    );
    const startCursor = BigInt(cursorRow.rows[0]?.last_ledger_id ?? "0");
    let freeBalance = BigInt(profile.free_balance_nano);
    let freeBalanceChanged = false;
    let lastLedgerId = 0n;
    let insertedCharge = false;
    for (const entry of ordered) {
      const ledgerId = BigInt(entry.id);
      if (ledgerId > lastLedgerId) lastLedgerId = ledgerId;
      if (ledgerId <= startCursor) continue; // уже обработано ранее — не двоим эффекты
      const amount = BigInt(entry.amount_nano);
      // Иммутабельная копия пополнения для отчётности: движковые топапы (админ-кредит, ручные)
      // не создают строки в payments, поэтому иначе реальные деньги клиента невидимы админке.
      // Деньгами и балансом эта таблица не управляет; вставка идемпотентна по (аккаунт, ledger id).
      if (entry.kind === "topup" && amount > 0n) {
        await recordPricingTopup(client, target, entry, amount);
      }
      // Бесплатные кредиты (welcome-бонус/промо) пополняют бесплатный баланс. У b2b локального
      // free-баланса нет — фондирование целиком остаётся авторитетом движка.
      if (entry.kind === "topup" && amount > 0n && isFreeCreditRef(entry.ref)) {
        if (progressive) {
          freeBalance += amount;
          freeBalanceChanged = true;
        }
        continue;
      }
      // Только положительные charge создают commission basis. Отрицательные `adjust`
      // (рефанд/чарджбэк/админ-клобэк) здесь НЕ откатывают уже начисленную комиссию — это
      // осознанный остаток (полный клобэк требует негативного real_funded по всей цепочке фида
      // и завязан на engine-компенсацию рефанда). Игнорирование безопасно по направлению: оно
      // может только НЕ доплатить, но не переплачивает.
      if (entry.kind !== "charge" || amount <= 0n) continue;
      const occurredAt = epochSecondsDate(entry.ts, "event timestamp");
      const validatedAttribution = validateLedgerAttribution(entry, amount);
      // New policy-aware rows trust only the engine's immutable settlement funding. The local
      // free-first projection remains solely for pre-attribution rows during rolling compatibility.
      const legacyFromFree = validatedAttribution === null && progressive
        ? amount < freeBalance ? amount : freeBalance
        : 0n;
      const attributedNonPaid = validatedAttribution?.nonPaidFundedNano ?? 0n;
      if (
        progressive
        && validatedAttribution?.attribution.snapshot_kind === "policy_v1"
        && attributedNonPaid > freeBalance
      ) {
        throw new PricingLedgerAttributionError(
          "engine policy funding exceeds the local immutable free-credit projection",
        );
      }
      // Release-v2 funding lives entirely in engine v2 lots; the local legacy free-credit
      // projection must neither validate nor absorb its bonus/other funding.
      const projectedFreeDebit = !progressive
        ? 0n
        : validatedAttribution === null
          ? legacyFromFree
          : validatedAttribution.attribution.snapshot_kind === "release_v2"
            ? 0n
            : attributedNonPaid;
      // Комиссионный базис — только доказанные деньги. У b2b legacy-проекции нет, поэтому
      // pre-attribution строка не создаёт базиса вовсе (недоплатить безопасно, переплатить — нет).
      const realFunded = validatedAttribution === null
        ? progressive ? amount - legacyFromFree : 0n
        : validatedAttribution.commissionEligible
          ? validatedAttribution.paidFundedNano ?? 0n
          : 0n;
      const retentionSpend = validatedAttribution === null || validatedAttribution.retentionEligible
        ? amount
        : 0n;
      const eventId = randomUUID();
      const providerId = ledgerProviderEvidence(entry);
      const inserted = await client.query<{ id: string }>(`
        INSERT INTO pricing_usage_events (
          id, user_id, engine_account_id, ledger_entry_id, provider_id,
          amount_nano, real_funded_nano, occurred_at, provider_recovery_version
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (engine_account_id, ledger_entry_id) DO NOTHING
        RETURNING id
      `, [
        eventId,
        target.userId,
        target.engineAccountId,
        ledgerId.toString(),
        providerId,
        entry.amount_nano,
        realFunded.toString(),
        occurredAt,
        providerId === UNATTRIBUTED_PROVIDER_ID ? 0 : PROVIDER_RECOVERY_VERSION,
      ]);
      if (!inserted.rows[0]) continue; // уже обработано — бесплатный баланс не трогаем повторно
      if (validatedAttribution !== null) {
        await insertPricingUsageAttribution(
          client,
          target,
          eventId,
          entry,
          amount,
          validatedAttribution,
        );
      }
      if (projectedFreeDebit > 0n) {
        freeBalance -= projectedFreeDebit;
        freeBalanceChanged = true;
      }
      insertedCharge = true;
      // Release-v2 usage is flat post-tier pricing: no progressive month/retention projection is
      // ever created for it (tier windows already exclude it via retention_eligible = false).
      // B2B не участвует в прогрессивной модели вообще.
      if (progressive && validatedAttribution?.attribution.snapshot_kind !== "release_v2") {
        const monthStart = utcMonthStart(occurredAt);
        await client.query(`
          INSERT INTO pricing_months (
            id, user_id, month_start, opening_tier, highest_tier, spent_nano
          ) VALUES ($1, $2, $3, $4, $4, $5)
          ON CONFLICT (user_id, month_start) DO UPDATE
          SET spent_nano = pricing_months.spent_nano + EXCLUDED.spent_nano, updated_at = now()
        `, [randomUUID(), target.userId, monthStart, profile.current_tier ?? 0, retentionSpend.toString()]);
      }
    }
    if (freeBalanceChanged) {
      await client.query(
        "UPDATE customer_profiles SET free_balance_nano = $2, updated_at = now() WHERE user_id = $1",
        [target.userId, freeBalance.toString()],
      );
    }
    const reachedStablePageEnd = entries.length < PRICING_LEDGER_PAGE_SIZE;
    await client.query(`
      UPDATE pricing_usage_cursors
      SET last_ledger_id = GREATEST(last_ledger_id, $3),
          updated_at = CASE WHEN $4 THEN now() ELSE updated_at END
      WHERE engine_account_id = $1 AND user_id = $2
    `, [target.engineAccountId, target.userId, lastLedgerId.toString(), reachedStablePageEnd]);

    // Prepay: расход НЕ поднимает тир (тир — за пополнения). The cached counter is rebuilt
    // from immutable events in the exact current [window_start, window_end) interval so late
    // ingestion cannot move a charge across retention windows.
    if (insertedCharge && progressive) await refreshTierWindowSpend(client, [target.userId], new Date());
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Prepay-тир: применяет ещё не учтённые confirmed-кредиты через durable accrual markers.
 * Повторный вызов идемпотентен, пропущенный вызов догоняется следующим reconcile, а marker
 * refunded/disputed-платежа удаляется с компенсирующим уменьшением cumulative.
 */
export async function applyTopupTier(database: Database, input: {
  engineAccountId: string;
  amountNano: bigint;
}): Promise<void> {
  if (input.amountNano <= 0n) throw new RangeError("top-up amount must be positive");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await reconcileTopupTier(client, {
      engineAccountId: input.engineAccountId,
    }, "b2c_topup");
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Закрытие 30-дневных окон удержания: у кого окно истекло — если за окно потрачено ≥ hold(tier),
 * окно продлевается; иначе откат на −1 тир, накопление сбрасывается к порогу нового тира, окно новое.
 */
export async function closeElapsedTierWindows(
  database: Database,
  now = new Date(),
  eligibleUserIds?: readonly string[],
): Promise<number> {
  if (eligibleUserIds?.length === 0) return 0;
  const windowDeadline = new Date(now.getTime() - HOLD_WINDOW_MS);
  let closed = 0;
  for (;;) {
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN");
      const result = await client.query<{
        user_id: string; current_tier: number; tier_window_start: Date; engine_account_id: string;
      }>(`
        SELECT cp.user_id, cp.current_tier, cp.tier_window_start, ea.engine_account_id
        FROM customer_profiles cp
        JOIN engine_accounts ea ON ea.user_id = cp.user_id
        JOIN pricing_usage_cursors puc
          ON puc.user_id = cp.user_id AND puc.engine_account_id = ea.engine_account_id
        WHERE cp.customer_type = 'b2c' AND cp.current_tier > 0
          AND ea.engine_account_id IS NOT NULL
          AND cp.tier_window_start IS NOT NULL AND cp.tier_window_start <= $1
          AND ($2::uuid[] IS NULL OR cp.user_id = ANY($2::uuid[]))
          -- A page shorter than the engine limit marks a completed ledger scan. If the current
          -- scan failed, updated_at is not advanced and this window is deferred rather than closed
          -- from incomplete usage.
          AND puc.updated_at >= cp.tier_window_start + interval '30 days'
        ORDER BY cp.tier_window_start, cp.user_id
        FOR UPDATE OF cp, ea SKIP LOCKED
        LIMIT 1
      `, [windowDeadline, eligibleUserIds ? [...eligibleUserIds] : null]);
      const row = result.rows[0];
      if (!row) {
        await client.query("COMMIT");
        return closed;
      }
      const windowEnd = new Date(row.tier_window_start.getTime() + HOLD_WINDOW_MS);
      const spentResult = await client.query<{ spent_nano: string }>(`
        SELECT COALESCE(SUM(event.amount_nano), 0)::text AS spent_nano
        FROM pricing_usage_events event
        LEFT JOIN pricing_usage_attributions attribution
          ON attribution.pricing_usage_event_id = event.id
        WHERE event.user_id = $1 AND event.engine_account_id = $2
          AND event.occurred_at >= $3 AND event.occurred_at < $4
          AND (
            attribution.pricing_usage_event_id IS NULL
            OR attribution.retention_eligible
          )
      `, [row.user_id, row.engine_account_id, row.tier_window_start, windowEnd]);
      const windowSpent = BigInt(spentResult.rows[0]?.spent_nano ?? "0");
      const held = windowSpent >= B2C_PRICING_TIERS[row.current_tier]!.holdNano;
      if (held) {
        await client.query(`
          UPDATE customer_profiles SET tier_window_start = $2, tier_window_spent_nano = 0, updated_at = now()
          WHERE user_id = $1
        `, [row.user_id, windowEnd]);
      } else {
        // Не удержал — откат на −1 тир; накопление к порогу нового тира; новое окно (или none → без окна).
        const nextTier = Math.max(0, row.current_tier - 1);
        const newCumulative = B2C_PRICING_TIERS[nextTier]!.spendThresholdNano;
        await applyTierChange(client, { userId: row.user_id, engineAccountId: row.engine_account_id }, nextTier, "b2c_window_downgrade");
        await client.query(`
          UPDATE customer_profiles
          SET cumulative_topup_nano = $2, tier_window_start = $3, tier_window_spent_nano = 0, updated_at = now()
          WHERE user_id = $1
        `, [row.user_id, newCumulative.toString(), nextTier > 0 ? windowEnd : null]);
      }
      // Carry already-ingested post-cutoff charges into the exact next window instead of losing them.
      await refreshTierWindowSpend(client, [row.user_id], now);
      // AUDIT-TODO(C19): persist an explicit engine cutoff watermark; cursor freshness is the
      // safest localized guard available until the Control API exposes a stable ledger watermark.
      await client.query("COMMIT");
      closed += 1;
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }
}

export async function claimNextPricingJob(
  database: Database,
  workerId: string,
): Promise<ClaimedPricingJob | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // Lease recovery is part of normal claiming, not a startup-only maintenance step. A failed
    // retryPricingJob write therefore delays a job by at most one lease interval instead of
    // stranding it in processing until process restart.
    await client.query(`
      UPDATE engine_pricing_jobs
      SET status = 'retry', locked_at = NULL, locked_by = NULL, next_attempt_at = now(),
          last_error = COALESCE(last_error, 'recovered expired pricing lease'), updated_at = now()
      WHERE status = 'processing'
        AND (locked_at IS NULL OR locked_at < now() - interval '5 minutes')
    `);
    // The unversioned scalar stream is retired only for accounts whose binding enforces the
    // versioned policy ('strict'). While a binding is 'shadow' (pre-cutover), the engine still
    // bills off the legacy accounts.mult_bp scalar — which only this stream writes — so scalar
    // jobs must be delivered, not drained. Strict-bound rows are retained as an audit record but
    // drained without another engine write: the monotonic policy jobs own that account.
    await client.query(`
      UPDATE engine_pricing_jobs legacy
      SET status = 'confirmed',
          reason = 'drained_to_versioned_policy:' || legacy.reason,
          confirmed_at = COALESCE(legacy.confirmed_at, now()),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      FROM account_policy_bindings binding
      WHERE binding.user_id = legacy.user_id
        AND binding.policy_enforcement = 'strict'
        AND binding.desired_effective_version IS NOT NULL
        AND binding.desired_digest IS NOT NULL
        AND legacy.status IN ('pending', 'retry')
    `);
    const result = await client.query<{
      id: string; user_id: string; engine_account_id: string; multiplier_bp: number; attempts: number;
    }>(`
      SELECT id, user_id, engine_account_id, multiplier_bp, attempts
      FROM engine_pricing_jobs
      WHERE status IN ('pending', 'retry') AND next_attempt_at <= now()
        AND NOT EXISTS (
          SELECT 1 FROM account_policy_bindings binding
          WHERE binding.user_id = engine_pricing_jobs.user_id
            AND binding.policy_enforcement = 'strict'
            AND binding.desired_effective_version IS NOT NULL
            AND binding.desired_digest IS NOT NULL
        )
      ORDER BY next_attempt_at, created_at
      FOR UPDATE SKIP LOCKED LIMIT 1
    `);
    const row = result.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }
    await client.query(`
      UPDATE engine_pricing_jobs SET status = 'processing', locked_at = now(), locked_by = $2,
        attempts = attempts + 1, updated_at = now() WHERE id = $1
    `, [row.id, workerId]);
    await client.query("COMMIT");
    return {
      id: row.id,
      userId: row.user_id,
      engineAccountId: row.engine_account_id,
      multiplierBp: row.multiplier_bp,
      attempts: row.attempts + 1,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function confirmPricingJob(database: Database, job: ClaimedPricingJob): Promise<void> {
  await database.pool.query(`
    UPDATE engine_pricing_jobs legacy
    SET status = 'confirmed',
        reason = 'drained_to_versioned_policy_after_processing:' || legacy.reason,
        confirmed_at = now(), locked_at = NULL, locked_by = NULL,
        last_error = NULL, updated_at = now()
    FROM account_policy_bindings binding
    WHERE legacy.id = $1 AND legacy.status = 'processing'
      AND binding.user_id = legacy.user_id
      AND binding.policy_enforcement = 'strict'
      AND binding.desired_effective_version IS NOT NULL
      AND binding.desired_digest IS NOT NULL
  `, [job.id]);
  await database.pool.query(`
    UPDATE engine_pricing_jobs job
    SET engine_account_id = COALESCE(ea.engine_account_id, job.engine_account_id),
        multiplier_bp = cp.multiplier_bp,
        reason = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.reason
          ELSE 'superseded_after_processing'
        END,
        status = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN 'confirmed'::pricing_job_status
          ELSE 'pending'::pricing_job_status
        END,
        attempts = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.attempts ELSE 0 END,
        next_attempt_at = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.next_attempt_at ELSE now() END,
        confirmed_at = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN now() ELSE NULL END,
        locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
    FROM customer_profiles cp
    LEFT JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE job.id = $1 AND job.status = 'processing' AND job.multiplier_bp = $2
      AND cp.user_id = job.user_id
  `, [job.id, job.multiplierBp]);
}

export async function retryPricingJob(database: Database, job: ClaimedPricingJob, error: string): Promise<void> {
  await database.pool.query(`
    UPDATE engine_pricing_jobs legacy
    SET status = 'confirmed',
        reason = 'drained_to_versioned_policy_after_processing:' || legacy.reason,
        confirmed_at = now(), locked_at = NULL, locked_by = NULL,
        last_error = NULL, updated_at = now()
    FROM account_policy_bindings binding
    WHERE legacy.id = $1 AND legacy.status = 'processing'
      AND binding.user_id = legacy.user_id
      AND binding.policy_enforcement = 'strict'
      AND binding.desired_effective_version IS NOT NULL
      AND binding.desired_digest IS NOT NULL
  `, [job.id]);
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(job.attempts, 10)));
  await database.pool.query(`
    UPDATE engine_pricing_jobs job
    SET engine_account_id = COALESCE(ea.engine_account_id, job.engine_account_id),
        multiplier_bp = cp.multiplier_bp,
        reason = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.reason
          ELSE 'superseded_after_processing'
        END,
        status = 'retry',
        attempts = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.attempts ELSE 0 END,
        next_attempt_at = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN now() + ($3 * interval '1 second')
          ELSE now()
        END,
        locked_at = NULL, locked_by = NULL,
        last_error = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN $2 ELSE NULL END,
        updated_at = now()
    FROM customer_profiles cp
    LEFT JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE job.id = $1 AND job.status = 'processing' AND job.multiplier_bp = $4
      AND cp.user_id = job.user_id
  `, [job.id, error.slice(0, 2000), delaySeconds, job.multiplierBp]);
}

export async function recoverStalePricingJobs(database: Database): Promise<number> {
  const result = await database.pool.query(`
    UPDATE engine_pricing_jobs SET status = 'retry', locked_at = NULL, locked_by = NULL,
      next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}

/** Эффективный mult = лучшее из тира и «пола» скидки сейлза (меньший mult = большая скидка). */
function effectiveMultiplierBp(tierMultiplierBp: number, referralFloorBps: number): number {
  return Math.min(tierMultiplierBp, 10_000 - referralFloorBps);
}

/**
 * Приводит multiplier_bp существующих b2c-профилей к АКТУАЛЬНОЙ лестнице B2C_PRICING_TIERS
 * (с учётом referral floor — персональные полы сейлза сохраняются). Обычные циклы пересчитывают
 * множитель только при СМЕНЕ тира, поэтому после изменения констант лестницы пользователи на том же
 * тире остались бы на старых bp навсегда. Идемпотентен; вызывается на старте pricing-воркера.
 * B2B-профили и их договорные скидки не трогает.
 */
export async function reconcileTierLadderMultipliers(database: Database): Promise<number> {
  const candidates = await database.pool.query<{
    user_id: string; current_tier: number | null; referral_floor_bps: number;
    multiplier_bp: number; engine_account_id: string;
  }>(`
    SELECT cp.user_id, cp.current_tier, cp.referral_floor_bps, cp.multiplier_bp, ea.engine_account_id
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE cp.customer_type = 'b2c' AND ea.engine_account_id IS NOT NULL
  `);
  let reconciled = 0;
  for (const candidate of candidates.rows) {
    const tier = candidate.current_tier ?? 0;
    const expected = effectiveMultiplierBp(B2C_PRICING_TIERS[tier]!.multiplierBp, candidate.referral_floor_bps);
    if (candidate.multiplier_bp === expected) continue;
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN");
      // Re-read under lock: a parallel topup/window transition may have already moved this user.
      const locked = await client.query<{ current_tier: number | null; multiplier_bp: number; referral_floor_bps: number }>(`
        SELECT current_tier, multiplier_bp, referral_floor_bps FROM customer_profiles
        WHERE user_id = $1 AND customer_type = 'b2c' FOR UPDATE
      `, [candidate.user_id]);
      const row = locked.rows[0];
      if (row) {
        const lockedTier = row.current_tier ?? 0;
        const lockedExpected = effectiveMultiplierBp(B2C_PRICING_TIERS[lockedTier]!.multiplierBp, row.referral_floor_bps);
        if (row.multiplier_bp !== lockedExpected) {
          await applyTierChange(client, {
            userId: candidate.user_id,
            engineAccountId: candidate.engine_account_id,
          }, lockedTier, "b2c_ladder_reconcile");
          reconciled += 1;
        }
      }
      await client.query("COMMIT");
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }
  return reconciled;
}

async function applyTierChange(
  client: PoolClient,
  target: PricingSyncTarget,
  tier: number,
  reason: string,
): Promise<void> {
  // Реф-скидка сейлза — «пол»: тир даёт свою скидку, но цена не хуже скидки сейлза. floor=0 → тир как есть.
  const floorResult = await client.query<{ referral_floor_bps: number }>(
    "SELECT referral_floor_bps FROM customer_profiles WHERE user_id = $1",
    [target.userId],
  );
  const floorBps = floorResult.rows[0]?.referral_floor_bps ?? 0;
  const multiplierBp = effectiveMultiplierBp(B2C_PRICING_TIERS[tier]!.multiplierBp, floorBps);
  await client.query(`
    UPDATE customer_profiles SET current_tier = $2, multiplier_bp = $3, updated_at = now()
    WHERE user_id = $1 AND customer_type = 'b2c'
  `, [target.userId, tier, multiplierBp]);
  await client.query(`UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1`, [
    target.userId, multiplierBp,
  ]);
  await enqueuePricingJob(client, {
    userId: target.userId,
    engineAccountId: target.engineAccountId,
    multiplierBp,
    reason,
  });
}

async function reconcileTopupTier(
  client: PoolClient,
  target: { engineAccountId: string; userId?: string },
  reason: string,
): Promise<void> {
  const profileResult = await client.query<{
    user_id: string; current_tier: number; cumulative_topup_nano: string;
  }>(`
    SELECT cp.user_id, cp.current_tier, cp.cumulative_topup_nano
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE ea.engine_account_id = $1 AND cp.customer_type = 'b2c'
      AND ($2::uuid IS NULL OR cp.user_id = $2::uuid)
    FOR UPDATE OF cp
  `, [target.engineAccountId, target.userId ?? null]);
  const profile = profileResult.rows[0];
  if (!profile) return;

  // The unique credit marker and the aggregate update stay in this transaction, so confirmed
  // top-ups and later refund/dispute reversals are applied exactly once across worker retries.
  const appliedResult = await client.query<{ amount_nano: string }>(`
    WITH eligible AS (
      SELECT ec.id AS credit_id
      FROM engine_credits ec
      JOIN payments p ON p.id = ec.payment_id
      LEFT JOIN pricing_credit_accruals pca ON pca.credit_id = ec.id
      WHERE ec.engine_account_id = $1 AND ec.status = 'confirmed'
        AND p.user_id = $2 AND p.status = 'paid' AND pca.credit_id IS NULL
    ), inserted AS (
      INSERT INTO pricing_credit_accruals (credit_id)
      SELECT credit_id FROM eligible
      ON CONFLICT (credit_id) DO NOTHING
      RETURNING credit_id
    )
    SELECT COALESCE(SUM(ec.amount_nano), 0)::text AS amount_nano
    FROM inserted i
    JOIN engine_credits ec ON ec.id = i.credit_id
  `, [target.engineAccountId, profile.user_id]);
  const reversedResult = await client.query<{ amount_nano: string }>(`
    WITH removed AS (
      DELETE FROM pricing_credit_accruals pca
      USING engine_credits ec, payments p
      WHERE pca.credit_id = ec.id AND ec.payment_id = p.id
        AND ec.engine_account_id = $1 AND p.user_id = $2
        AND p.status IN ('refunded', 'disputed')
      RETURNING ec.amount_nano
    )
    SELECT COALESCE(SUM(amount_nano), 0)::text AS amount_nano FROM removed
  `, [target.engineAccountId, profile.user_id]);

  const applied = BigInt(appliedResult.rows[0]?.amount_nano ?? "0");
  const reversed = BigInt(reversedResult.rows[0]?.amount_nano ?? "0");
  if (applied === 0n && reversed === 0n) return;
  const currentCumulative = BigInt(profile.cumulative_topup_nano);
  const cumulative = currentCumulative + applied > reversed
    ? currentCumulative + applied - reversed
    : 0n;
  const currentTier = profile.current_tier ?? 0;
  const newTier = tierForTopups(cumulative);
  await client.query(`
    UPDATE customer_profiles SET cumulative_topup_nano = $2, updated_at = now() WHERE user_id = $1
  `, [profile.user_id, cumulative.toString()]);
  if (newTier !== currentTier) {
    await applyTierChange(client, {
      userId: profile.user_id,
      engineAccountId: target.engineAccountId,
    }, newTier, reversed > 0n ? "b2c_refund_reversal" : reason);
    await client.query(`
      UPDATE customer_profiles
      SET tier_window_start = CASE WHEN $2 > 0 THEN now() ELSE NULL END,
          tier_window_spent_nano = 0, updated_at = now()
      WHERE user_id = $1
    `, [profile.user_id, newTier]);
    await refreshTierWindowSpend(client, [profile.user_id], new Date());
  }
}

/** Rebuilds the denormalized current-window spend from immutable, deduplicated charge events. */
export async function refreshTierWindowUsage(
  database: Database,
  userIds: readonly string[],
  asOf = new Date(),
): Promise<void> {
  if (userIds.length === 0) return;
  await refreshTierWindowSpend(database.pool, userIds, asOf);
}

async function refreshTierWindowSpend(
  queryable: Pick<PoolClient, "query">,
  userIds: readonly string[],
  asOf: Date,
): Promise<void> {
  await queryable.query(`
    UPDATE customer_profiles cp
    SET tier_window_spent_nano = CASE
          WHEN cp.tier_window_start IS NULL THEN 0
          ELSE COALESCE((
            SELECT SUM(pue.amount_nano)
            FROM pricing_usage_events pue
            LEFT JOIN pricing_usage_attributions attribution
              ON attribution.pricing_usage_event_id = pue.id
            WHERE pue.user_id = cp.user_id AND pue.engine_account_id = ea.engine_account_id
              AND pue.occurred_at >= cp.tier_window_start
              AND pue.occurred_at < LEAST(
                $2::timestamptz,
                cp.tier_window_start + interval '30 days'
              )
              AND (
                attribution.pricing_usage_event_id IS NULL
                OR attribution.retention_eligible
              )
          ), 0)
        END,
        updated_at = now()
    FROM engine_accounts ea
    WHERE cp.user_id = ANY($1::uuid[])
      AND cp.customer_type = 'b2c'
      AND ea.user_id = cp.user_id
  `, [userIds, asOf]);
}

interface PricingViewRow {
  customer_type: "b2c" | "b2b";
  current_tier: number | null;
  multiplier_bp: number;
  pricing_month_start: Date;
  cumulative_topup_nano: string;
  tier_window_start: Date | null;
  tier_window_spent_nano: string;
  referral_floor_bps: number | null;
}
