import { spreadsheetExactInteger, spreadsheetSafeText } from "@/lib/csv";

export type OpenkeysPayingDays = 1 | 7 | 30;
export type OpenkeysPayingStatus = "all" | "active" | "disabled";
export type OpenkeysApiType = "anthropic" | "openai";

export interface OpenkeysUsageModel {
  model: string;
  provider?: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_5m_tokens: number;
  cache_write_1h_tokens: number;
  web_search_requests: number;
  official_nano: string;
  charged_nano: string;
}

export interface OpenkeysUsageBucket {
  tokens: number;
  official_nano: string;
}

export interface OpenkeysEngineUsage {
  account: string;
  window: string;
  since_ts: number;
  until_ts: number;
  requests: number;
  total_official_nano: string;
  total_charged_nano: string;
  buckets: {
    input: OpenkeysUsageBucket;
    output: OpenkeysUsageBucket;
    cache_read: OpenkeysUsageBucket;
    cache_write: OpenkeysUsageBucket;
    web_search: { requests: number; official_nano: string };
    unattributed_legacy: { official_nano: string };
  };
  models: OpenkeysUsageModel[];
  daily: Array<{
    day_ts: number;
    requests: number;
    official_nano: string;
    charged_nano: string;
  }>;
  daily_providers: Array<{
    day_ts: number;
    provider: string;
    requests: number;
    official_nano: string;
    charged_nano: string;
  }>;
  keys: Array<{
    key_masked: string | null;
    requests: number;
    official_nano: string;
    charged_nano: string;
  }>;
}

export type OpenkeysPayingUsage =
  | ({ status: "available" } & OpenkeysEngineUsage)
  | { status: "unavailable"; window: string };

export interface OpenkeysPayingRow {
  id: string;
  batchId: string;
  batchLabel: string | null;
  createdBy: string;
  keyMasked: string;
  engineAccountId: string;
  apiType: OpenkeysApiType;
  enabled: boolean;
  faceValueNano: string;
  pricingContract: "legacy" | "official_1_to_1";
  createdAt: string;
  deliveredAt: string;
  usage: OpenkeysPayingUsage;
}

export interface OpenkeysPayingResponse {
  days: OpenkeysPayingDays;
  total: number;
  limit: number;
  offset: number;
  rows: OpenkeysPayingRow[];
}

export interface OpenkeysPayingPageState {
  days: OpenkeysPayingDays;
  limit: number;
  offset: number;
  q: string;
  status: OpenkeysPayingStatus;
}

export const OPENKEYS_PAYING_MAX_OFFSET = 100_000;

export const INITIAL_OPENKEYS_PAYING_PAGE: OpenkeysPayingPageState = {
  days: 30,
  limit: 50,
  offset: 0,
  q: "",
  status: "all",
};

export function clampOpenkeysPayingOffset(offset: number): number {
  if (!Number.isFinite(offset) || offset <= 0) return 0;
  return Math.min(OPENKEYS_PAYING_MAX_OFFSET, Math.floor(offset));
}

export function openkeysPayingQuery(state: OpenkeysPayingPageState): string {
  const params = new URLSearchParams({
    days: String(state.days),
    limit: String(state.limit),
    offset: String(clampOpenkeysPayingOffset(state.offset)),
    status: state.status,
  });
  if (state.q) params.set("q", state.q);
  return params.toString();
}

function nanoBigInt(value: string | null | undefined): bigint {
  try {
    return BigInt(value ?? "0");
  } catch {
    return 0n;
  }
}

export function addNano(values: Array<string | null | undefined>): string {
  return values.reduce<bigint>((total, value) => total + nanoBigInt(value), 0n).toString();
}

export function openkeysChargedNano(usage: OpenkeysPayingUsage): string | null {
  return usage.status === "available" ? usage.total_charged_nano : null;
}

export function providerLabel(provider: string | null | undefined): string {
  return provider?.trim() || "не указан";
}

export const OPENKEYS_PAYING_CSV_HEADER = [
  "key_id",
  "key_masked",
  "engine_account_id",
  "batch_id",
  "batch_label",
  "seller",
  "status",
  "api_type",
  "nominal_nanoUSD_text",
  "pricing_contract",
  "delivered_at",
  "usage_status",
  "usage_window",
  "provider",
  "model",
  "requests",
  "input_tokens",
  "output_tokens",
  "cache_read_tokens",
  "cache_write_5m_tokens",
  "cache_write_1h_tokens",
  "web_search_requests",
  "official_nanoUSD_text",
  "charged_nanoUSD_text",
  "usage_total_official_nanoUSD_text",
  "usage_total_charged_nanoUSD_text",
];

export function buildOpenkeysPayingCsvRows(rows: OpenkeysPayingRow[]): unknown[][] {
  return rows.flatMap((row) => {
    const usage = row.usage;
    const models = usage.status === "available" && usage.models.length ? usage.models : [null];
    return models.map((model) => [
      spreadsheetSafeText(row.id),
      spreadsheetSafeText(row.keyMasked),
      spreadsheetSafeText(row.engineAccountId),
      spreadsheetSafeText(row.batchId),
      spreadsheetSafeText(row.batchLabel ?? ""),
      spreadsheetSafeText(row.createdBy),
      row.enabled ? "active" : "disabled",
      row.apiType,
      spreadsheetExactInteger(row.faceValueNano),
      spreadsheetSafeText(row.pricingContract),
      spreadsheetSafeText(row.deliveredAt),
      usage.status,
      spreadsheetSafeText(usage.window),
      spreadsheetSafeText(model?.provider ?? ""),
      spreadsheetSafeText(model?.model ?? ""),
      model?.requests ?? (usage.status === "available" ? usage.requests : ""),
      model?.input_tokens ?? "",
      model?.output_tokens ?? "",
      model?.cache_read_tokens ?? "",
      model?.cache_write_5m_tokens ?? "",
      model?.cache_write_1h_tokens ?? "",
      model?.web_search_requests ?? "",
      spreadsheetExactInteger(model?.official_nano ?? ""),
      spreadsheetExactInteger(model?.charged_nano ?? ""),
      spreadsheetExactInteger(usage.status === "available" ? usage.total_official_nano : ""),
      spreadsheetExactInteger(usage.status === "available" ? usage.total_charged_nano : ""),
    ]);
  });
}
