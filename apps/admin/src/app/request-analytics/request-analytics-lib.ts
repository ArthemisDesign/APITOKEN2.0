export type WindowHours = 24 | 168 | 720;

export interface AxisGroup { values?: Array<string | null>; count?: number }
export interface Axis { groups?: AxisGroup[]; truncated?: boolean }
export interface RequestFactRow {
  fact_id?: number; logical_request_id?: string; attempt?: number;
  client_kind?: string; client_source?: string; provider_plane?: string;
  route_class?: string; request_class?: string; requested_model?: string | null;
  executable_model?: string | null; stream?: boolean; admitted_at?: number;
  terminal_at?: number | null; http_status_code?: number | null;
  provider_terminal_class?: string | null; delivery_state?: string | null;
  billing_outcome?: string | null; internal_attempt_count?: number | null;
  admission_to_delivery_seconds?: number | null;
  admission_to_first_public_byte_seconds?: number | null;
  admission_to_terminal_seconds?: number | null;
}
export interface Coverage {
  persisted_facts?: number; terminal_facts?: number; nonterminal_facts?: number;
  required_evidence_unknown_facts?: number; coverage_percentage?: null; status?: "unknown";
}
export interface Runtime {
  continuity?: "process_local" | "unknown"; queue_depth?: number;
  persistence_health?: "unknown" | "healthy" | "failed";
  stuck_nonterminal_count?: number | null; process_started_at?: number | null;
}
export interface SummaryResponse {
  scope_version?: 1; from?: number; to?: number;
  summary?: { totals?: { persisted?: number; terminal?: number; nonterminal?: number; required_evidence_unknown?: number }; routes?: Axis; clients?: Axis };
  coverage?: Coverage; runtime?: Runtime;
}
export interface PageResponse {
  scope_version?: 1; from?: number; to?: number; rows?: RequestFactRow[];
  next_cursor?: string | null; coverage?: Coverage; runtime?: Runtime;
}
export interface LogicalResponse {
  scope_version?: 1; logical_request_id?: string; rows?: RequestFactRow[];
  truncated?: boolean; runtime?: Runtime;
}

export function requestAnalyticsWindow(hours: WindowHours, now = Date.now()): { from: number; to: number } {
  const to = Math.floor(now / 1000);
  return { from: to - hours * 3600, to };
}

export function requestAnalyticsUrls(hours: WindowHours, cursor?: string, now = Date.now()) {
  const { from, to } = requestAnalyticsWindow(hours, now);
  const common = `from=${from}&to=${to}`;
  return {
    summary: `/admin/request-analytics/summary?${common}`,
    page: `/admin/request-analytics?${common}&limit=100${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
  };
}

export function displayValue(value: string | null | undefined): string {
  return value && value !== "unknown" ? value : "—";
}

export function durationLabel(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  if (value < 1) return "< 1 с";
  return `${value} с`;
}

export function routeLabel(row: RequestFactRow): string {
  return [displayValue(row.provider_plane), displayValue(row.route_class), displayValue(row.request_class)].join(" · ");
}
