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
  /** Editable public/contract subscription price. Null means the contract price is not universal. */
  defaultMonthlyCostNano: bigint | null;
}

export const PRODUCT_CATALOG: readonly ProductDefinition[] = [
  { id: "claude-pro", provider: "claude", plan: "pro", label: "Claude Pro", compactLabel: "Pro", defaultMonthlyCostNano: 20n * NANO_PER_USD },
  { id: "claude-max5", provider: "claude", plan: "max5", label: "Claude Max 5×", compactLabel: "Max 5×", defaultMonthlyCostNano: 100n * NANO_PER_USD },
  { id: "claude-max20", provider: "claude", plan: "max20", label: "Claude Max 20×", compactLabel: "Max 20×", defaultMonthlyCostNano: 200n * NANO_PER_USD },
  { id: "chatgpt-plus", provider: "openai", plan: "chatgpt_plus", label: "ChatGPT Plus", compactLabel: "Plus", defaultMonthlyCostNano: 20n * NANO_PER_USD },
  { id: "chatgpt-pro", provider: "openai", plan: "chatgpt_pro", label: "ChatGPT Pro", compactLabel: "Pro", defaultMonthlyCostNano: 200n * NANO_PER_USD },
  { id: "chatgpt-business", provider: "openai", plan: "chatgpt_business", label: "ChatGPT Business", compactLabel: "Business", defaultMonthlyCostNano: null },
  { id: "google-ai-pro", provider: "gemini", plan: "google_ai_pro", label: "Google AI Pro", compactLabel: "AI Pro", defaultMonthlyCostNano: 20n * NANO_PER_USD },
  { id: "google-ai-ultra", provider: "gemini", plan: "google_ai_ultra", label: "Google AI Ultra", compactLabel: "AI Ultra", defaultMonthlyCostNano: null },
  { id: "code-assist-standard", provider: "gemini", plan: "code_assist_standard", label: "Code Assist Standard", compactLabel: "Standard", defaultMonthlyCostNano: null },
  { id: "code-assist-enterprise", provider: "gemini", plan: "code_assist_enterprise", label: "Code Assist Enterprise", compactLabel: "Enterprise", defaultMonthlyCostNano: null },
  { id: "workspace-ai-ultra", provider: "gemini", plan: "workspace_ai_ultra", label: "Workspace AI Ultra", compactLabel: "Workspace Ultra", defaultMonthlyCostNano: null },
] as const;

export interface WindowMetric {
  capacityNano: bigint | null;
  lowNano: bigint | null;
  highNano: bigint | null;
  measuredProfiles: number;
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
  return PRODUCT_CATALOG.map((product) => {
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
