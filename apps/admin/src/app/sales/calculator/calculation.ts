import type {
  CapacityResponse,
  CodexHome,
  CodexHomeWindow,
  CodexSubsResponse,
  GeminiProfile,
  GeminiProfileWindow,
  GeminiSubsResponse,
} from "../../subscriptions/types";

export const NANO_PER_USD = 1_000_000_000n;
const BASIS_POINTS = 10_000n;

export type Provider = "claude" | "openai" | "gemini";

export interface ProductDefinition {
  id: string;
  provider: Provider;
  plan: string;
  label: string;
  compactLabel: string;
  quotaWeight: number;
  quotaLabel: string;
  /** Editable public/contract subscription price. Null means the contract price is not universal. */
  defaultMonthlyCostNano: bigint | null;
}

export const PRODUCT_CATALOG: readonly ProductDefinition[] = [
  { id: "claude-pro", provider: "claude", plan: "pro", label: "Claude Pro", compactLabel: "Pro", quotaWeight: 1, quotaLabel: "1× квоты Pro", defaultMonthlyCostNano: 20n * NANO_PER_USD },
  { id: "claude-max5", provider: "claude", plan: "max5", label: "Claude Max 5×", compactLabel: "Max 5×", quotaWeight: 5, quotaLabel: "5× квоты Pro", defaultMonthlyCostNano: 100n * NANO_PER_USD },
  { id: "claude-max20", provider: "claude", plan: "max20", label: "Claude Max 20×", compactLabel: "Max 20×", quotaWeight: 20, quotaLabel: "20× квоты Pro", defaultMonthlyCostNano: 200n * NANO_PER_USD },
  { id: "chatgpt-plus", provider: "openai", plan: "chatgpt_plus", label: "ChatGPT Plus", compactLabel: "Plus", quotaWeight: 1, quotaLabel: "1× квоты Plus", defaultMonthlyCostNano: 20n * NANO_PER_USD },
  { id: "chatgpt-pro-5x", provider: "openai", plan: "chatgpt_pro_5x", label: "ChatGPT Pro 5×", compactLabel: "Pro 5×", quotaWeight: 5, quotaLabel: "5× квоты Plus", defaultMonthlyCostNano: 100n * NANO_PER_USD },
  { id: "chatgpt-pro-20x", provider: "openai", plan: "chatgpt_pro", label: "ChatGPT Pro 20×", compactLabel: "Pro 20×", quotaWeight: 20, quotaLabel: "20× квоты Plus", defaultMonthlyCostNano: 200n * NANO_PER_USD },
  { id: "chatgpt-business", provider: "openai", plan: "chatgpt_business", label: "ChatGPT Business", compactLabel: "Business", quotaWeight: 1, quotaLabel: "1× квоты Plus", defaultMonthlyCostNano: null },
  { id: "google-ai-pro", provider: "gemini", plan: "google_ai_pro", label: "Google AI Pro", compactLabel: "AI Pro", quotaWeight: 1, quotaLabel: "1× квоты AI Pro", defaultMonthlyCostNano: 20n * NANO_PER_USD },
  { id: "google-ai-ultra", provider: "gemini", plan: "google_ai_ultra", label: "Google AI Ultra", compactLabel: "AI Ultra", quotaWeight: 20, quotaLabel: "до 20× квоты AI Pro", defaultMonthlyCostNano: null },
] as const;

export const PROVIDER_RATIO_BASIS: Record<Provider, { label: string; source: string; href: string }> = {
  claude: {
    label: "Anthropic 1:5:20",
    source: "Anthropic pricing",
    href: "https://www.anthropic.com/pricing",
  },
  openai: {
    label: "OpenAI 1:5:20",
    source: "OpenAI pricing",
    href: "https://learn.chatgpt.com/docs/pricing",
  },
  gemini: {
    label: "Google AI Pro:Ultra 1:20",
    source: "Google AI plans",
    href: "https://one.google.com/about/google-ai-plans/",
  },
};

export type WindowEvidence = "measured" | "estimated" | "unknown";

export interface EstimateSource {
  productId: string;
  label: string;
  ratioLabel: string;
}

export interface EstimateProvenance {
  basisLabel: string;
  sources: EstimateSource[];
}

export interface WindowMetric {
  capacityNano: bigint | null;
  lowNano: bigint | null;
  highNano: bigint | null;
  measuredProfiles: number;
  evidence: WindowEvidence;
  estimate: EstimateProvenance | null;
}

export interface ProductMetric {
  product: ProductDefinition;
  profiles: number;
  measuredProfiles: number;
  fiveHour: WindowMetric;
  sevenDay: WindowMetric;
  month: WindowMetric;
  confidenceBp: number | null;
  sourceOnline: boolean;
}

interface EvidenceWindow {
  capacityNano: bigint | null;
  lowNano: bigint | null;
  highNano: bigint | null;
  confidenceBp: number | null;
}

interface ProfileEvidence {
  provider: Provider;
  plan: string;
  fiveHour: EvidenceWindow;
  sevenDay: EvidenceWindow;
}

export interface CalibrationPayload {
  capacity: CapacityResponse | null;
  codex: CodexSubsResponse | null;
  gemini: GeminiSubsResponse | null;
}

const EMPTY_WINDOW: EvidenceWindow = {
  capacityNano: null,
  lowNano: null,
  highNano: null,
  confidenceBp: null,
};

export function decimalUsdToNano(value: string): bigint | null {
  const normalized = value.trim();
  if (!/^\d+(?:\.\d{0,9})?$/.test(normalized)) return null;
  const [whole, fraction = ""] = normalized.split(".");
  try {
    return BigInt(whole) * NANO_PER_USD + BigInt(fraction.padEnd(9, "0") || "0");
  } catch {
    return null;
  }
}

export function nanoToEditableUsd(value: bigint | null): string {
  if (value == null) return "";
  const whole = value / NANO_PER_USD;
  const fraction = (value % NANO_PER_USD).toString().padStart(9, "0").replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

function presentationUsdToNano(value: number | null | undefined): bigint | null {
  if (value == null || !Number.isFinite(value) || value < 0) return null;
  return decimalUsdToNano(String(value));
}

function canonicalNano(value: string | null | undefined, fallbackUsd?: number | null): bigint | null {
  if (value != null && /^\d+$/.test(value)) {
    try {
      return BigInt(value);
    } catch {
      return null;
    }
  }
  return presentationUsdToNano(fallbackUsd);
}

function evidenceWindow(
  capacityNano: bigint | null,
  lowNano: bigint | null = null,
  highNano: bigint | null = null,
  confidenceBp: number | null = null,
): EvidenceWindow {
  return { capacityNano, lowNano, highNano, confidenceBp };
}

function claudeEvidence(payload: CalibrationPayload): ProfileEvidence[] {
  return (payload.capacity?.per_sub ?? []).map((item) => ({
    provider: "claude",
    plan: item.plan ?? "",
    // Claude still publishes a calibrated EMA rather than an estimator envelope. Priors are
    // deliberately excluded: a sales value appears only after real utilisation movement.
    fiveHour: item.calibrated
      ? evidenceWindow(canonicalNano(item.cap5h_nano, item.cap5h_usd))
      : EMPTY_WINDOW,
    sevenDay: item.calibrated
      ? evidenceWindow(canonicalNano(item.cap7d_nano, item.cap7d_usd))
      : EMPTY_WINDOW,
  }));
}

function codexWindow(home: CodexHome, minutes: number): CodexHomeWindow | undefined {
  return (home.windows ?? []).find((window) => window.window_minutes === minutes);
}

function codexEvidenceWindow(window: CodexHomeWindow | undefined): EvidenceWindow {
  if (!window || window.source === "unknown") return EMPTY_WINDOW;
  return evidenceWindow(
    canonicalNano(window.capacity_nano, window.cap_usd),
    canonicalNano(window.low_nano, window.low_usd),
    canonicalNano(window.high_nano, window.high_usd),
    window.confidence == null ? null : Math.round(window.confidence * 10_000),
  );
}

function codexEvidence(payload: CalibrationPayload): ProfileEvidence[] {
  return (payload.codex?.homes ?? []).map((home) => ({
    provider: "openai",
    plan: home.plan ?? "",
    fiveHour: codexEvidenceWindow(codexWindow(home, 300)),
    sevenDay: codexEvidenceWindow(codexWindow(home, 10_080)),
  }));
}

function geminiWindow(profile: GeminiProfile, minutes: number): GeminiProfileWindow | undefined {
  return (profile.windows ?? []).find((window) => window.window_minutes === minutes);
}

function geminiEvidenceWindow(window: GeminiProfileWindow | undefined): EvidenceWindow {
  if (!window || window.source === "unknown") return EMPTY_WINDOW;
  return evidenceWindow(
    canonicalNano(window.capacity_nano, window.cap_usd),
    canonicalNano(window.low_nano, window.low_usd),
    canonicalNano(window.high_nano, window.high_usd),
    window.confidence == null ? null : Math.round(window.confidence * 10_000),
  );
}

function geminiEvidence(payload: CalibrationPayload): ProfileEvidence[] {
  return (payload.gemini?.profiles ?? []).map((profile) => ({
    provider: "gemini",
    plan: profile.plan ?? "",
    fiveHour: geminiEvidenceWindow(geminiWindow(profile, 300)),
    sevenDay: geminiEvidenceWindow(geminiWindow(profile, 10_080)),
  }));
}

function average(values: bigint[]): bigint | null {
  if (!values.length) return null;
  return values.reduce((sum, value) => sum + value, 0n) / BigInt(values.length);
}

function averageOptional(values: Array<bigint | null>, expected: number): bigint | null {
  const complete = values.filter((value): value is bigint => value != null);
  return complete.length === expected && expected > 0 ? average(complete) : null;
}

function monthlyEquivalent(fiveHour: bigint, sevenDay: bigint): bigint {
  const fromFiveHour = fiveHour * 144n;
  const fromSevenDay = (sevenDay * 30n) / 7n;
  return fromFiveHour < fromSevenDay ? fromFiveHour : fromSevenDay;
}

function aggregateWindow(profiles: ProfileEvidence[], key: "fiveHour" | "sevenDay"): WindowMetric {
  const measured = profiles.map((profile) => profile[key]).filter((window) => window.capacityNano != null);
  return {
    capacityNano: average(measured.map((window) => window.capacityNano as bigint)),
    lowNano: averageOptional(measured.map((window) => window.lowNano), measured.length),
    highNano: averageOptional(measured.map((window) => window.highNano), measured.length),
    measuredProfiles: measured.length,
    evidence: measured.length ? "measured" : "unknown",
    estimate: null,
  };
}

function aggregateMonth(profiles: ProfileEvidence[]): WindowMetric {
  const measured = profiles.filter(
    (profile) => profile.fiveHour.capacityNano != null && profile.sevenDay.capacityNano != null,
  );
  const capacities = measured.map((profile) =>
    monthlyEquivalent(profile.fiveHour.capacityNano as bigint, profile.sevenDay.capacityNano as bigint),
  );
  const lows = measured.map((profile) =>
    profile.fiveHour.lowNano != null && profile.sevenDay.lowNano != null
      ? monthlyEquivalent(profile.fiveHour.lowNano, profile.sevenDay.lowNano)
      : null,
  );
  const highs = measured.map((profile) =>
    profile.fiveHour.highNano != null && profile.sevenDay.highNano != null
      ? monthlyEquivalent(profile.fiveHour.highNano, profile.sevenDay.highNano)
      : null,
  );
  return {
    capacityNano: average(capacities),
    lowNano: averageOptional(lows, measured.length),
    highNano: averageOptional(highs, measured.length),
    measuredProfiles: measured.length,
    evidence: measured.length ? "measured" : "unknown",
    estimate: null,
  };
}

function aggregateConfidence(profiles: ProfileEvidence[]): number | null {
  const values = profiles
    .flatMap((profile) => [profile.fiveHour.confidenceBp, profile.sevenDay.confidenceBp])
    .filter((value): value is number => value != null);
  return values.length ? Math.round(values.reduce((sum, value) => sum + value, 0) / values.length) : null;
}

export function buildProductMetrics(payload: CalibrationPayload): ProductMetric[] {
  const evidence = [...claudeEvidence(payload), ...codexEvidence(payload), ...geminiEvidence(payload)];
  const online: Record<Provider, boolean> = {
    claude: payload.capacity !== null,
    openai: payload.codex !== null,
    gemini: payload.gemini !== null,
  };
  const directMetrics = PRODUCT_CATALOG.map((product) => {
    const profiles = evidence.filter(
      (profile) => profile.provider === product.provider && profile.plan === product.plan,
    );
    const measuredProfiles = profiles.filter(
      (profile) => profile.fiveHour.capacityNano != null || profile.sevenDay.capacityNano != null,
    ).length;
    return {
      product,
      profiles: profiles.length,
      measuredProfiles,
      fiveHour: aggregateWindow(profiles, "fiveHour"),
      sevenDay: aggregateWindow(profiles, "sevenDay"),
      month: aggregateMonth(profiles),
      confidenceBp: aggregateConfidence(profiles),
      sourceOnline: online[product.provider],
    };
  });

  return directMetrics.map((metric) => ({
    ...metric,
    fiveHour: estimateMissingWindow(metric, "fiveHour", directMetrics),
    sevenDay: estimateMissingWindow(metric, "sevenDay", directMetrics),
    month: estimateMissingWindow(metric, "month", directMetrics),
  }));
}

type WindowKey = "fiveHour" | "sevenDay" | "month";

function greatestCommonDivisor(left: number, right: number): number {
  let a = Math.abs(Math.trunc(left));
  let b = Math.abs(Math.trunc(right));
  while (b !== 0) [a, b] = [b, a % b];
  return a || 1;
}

function ratioLabel(targetWeight: number, sourceWeight: number): string {
  const divisor = greatestCommonDivisor(targetWeight, sourceWeight);
  const numerator = targetWeight / divisor;
  const denominator = sourceWeight / divisor;
  if (denominator === 1) return numerator === 1 ? "×1" : `×${numerator}`;
  if (numerator === 1) return `÷${denominator}`;
  return `×${numerator}/${denominator}`;
}

function scaleByQuota(value: bigint, targetWeight: number, sourceWeight: number): bigint {
  return (value * BigInt(targetWeight)) / BigInt(sourceWeight);
}

function estimateMissingWindow(
  target: ProductMetric,
  key: WindowKey,
  directMetrics: ProductMetric[],
): WindowMetric {
  const current = target[key];
  if (current.evidence === "measured") return current;

  const anchors = directMetrics.filter(
    (candidate) =>
      candidate.product.provider === target.product.provider &&
      candidate.product.id !== target.product.id &&
      candidate[key].evidence === "measured" &&
      candidate[key].capacityNano != null,
  );
  if (!anchors.length) return current;

  const capacities = anchors.map((anchor) =>
    scaleByQuota(
      anchor[key].capacityNano as bigint,
      target.product.quotaWeight,
      anchor.product.quotaWeight,
    ),
  );
  const lows = anchors.map((anchor) =>
    anchor[key].lowNano == null
      ? null
      : scaleByQuota(anchor[key].lowNano, target.product.quotaWeight, anchor.product.quotaWeight),
  );
  const highs = anchors.map((anchor) =>
    anchor[key].highNano == null
      ? null
      : scaleByQuota(anchor[key].highNano, target.product.quotaWeight, anchor.product.quotaWeight),
  );

  return {
    capacityNano: average(capacities),
    lowNano: averageOptional(lows, anchors.length),
    highNano: averageOptional(highs, anchors.length),
    measuredProfiles: anchors.reduce((sum, anchor) => sum + anchor[key].measuredProfiles, 0),
    evidence: "estimated",
    estimate: {
      basisLabel: PROVIDER_RATIO_BASIS[target.product.provider].label,
      sources: anchors.map((anchor) => ({
        productId: anchor.product.id,
        label: anchor.product.compactLabel,
        ratioLabel: ratioLabel(target.product.quotaWeight, anchor.product.quotaWeight),
      })),
    },
  };
}

export interface ScenarioInput {
  monthlyCapacityNano: bigint;
  quantity: number;
  utilizationBp: number;
  discountBp: number;
  subscriptionCostNano: bigint | null;
}

export interface ScenarioResult {
  fullCapacityNano: bigint;
  usedCapacityNano: bigint;
  offerNano: bigint;
  customerApiSavingsNano: bigint;
  unusedCapacityNano: bigint;
  missedRevenueNano: bigint;
  subscriptionSpendNano: bigint | null;
  idleSubscriptionSpendNano: bigint | null;
  grossMarginNano: bigint | null;
}

function clampInteger(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, Math.trunc(value)));
}

function multiplyBasisPoints(value: bigint, basisPoints: number): bigint {
  return (value * BigInt(clampInteger(basisPoints, 0, 10_000))) / BASIS_POINTS;
}

export function calculateScenario(input: ScenarioInput): ScenarioResult {
  const quantity = BigInt(clampInteger(input.quantity, 1, 10_000));
  const utilizationBp = clampInteger(input.utilizationBp, 0, 10_000);
  const discountBp = clampInteger(input.discountBp, 0, 10_000);
  const fullCapacityNano = input.monthlyCapacityNano * quantity;
  const usedCapacityNano = multiplyBasisPoints(fullCapacityNano, utilizationBp);
  const offerNano = multiplyBasisPoints(usedCapacityNano, 10_000 - discountBp);
  const unusedCapacityNano = fullCapacityNano - usedCapacityNano;
  const subscriptionSpendNano =
    input.subscriptionCostNano == null ? null : input.subscriptionCostNano * quantity;
  return {
    fullCapacityNano,
    usedCapacityNano,
    offerNano,
    customerApiSavingsNano: usedCapacityNano - offerNano,
    unusedCapacityNano,
    missedRevenueNano: multiplyBasisPoints(unusedCapacityNano, 10_000 - discountBp),
    subscriptionSpendNano,
    idleSubscriptionSpendNano:
      subscriptionSpendNano == null ? null : multiplyBasisPoints(subscriptionSpendNano, 10_000 - utilizationBp),
    grossMarginNano: subscriptionSpendNano == null ? null : offerNano - subscriptionSpendNano,
  };
}
