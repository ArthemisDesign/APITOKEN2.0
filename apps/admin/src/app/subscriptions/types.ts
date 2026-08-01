// Типы payload'ов флотов подписок: GET /subs, /capacity, /codex-subs, /gemini-subs.
// Все поля опциональны — панель деградирует молча, как admin-panel.js.
// Деньги на этой странице — легаси-доллары движка (*_usd), только отображение через money().

// GET /subs — Claude OAuth lifecycle
export interface ClaudeSub {
  email?: string;
  auth_state?: string;
  dead_reason?: string;
  /** epoch-секунды, когда подписка помечена dead. */
  dead_since_ts?: number;
  status?: string;
  has_token?: boolean;
  sub_days_left?: number;
  added?: string;
  peak_cap5h_usd?: number;
  peak_cap7d_usd?: number;
  proxy_host?: string;
  proxy_ok?: boolean;
  proxy_expire?: string;
}

export interface SubsResponse {
  subs?: ClaudeSub[];
  lifetime_days?: number | string;
}

// GET /capacity — live ёмкость Claude-флота (ключуется по маскированному email)
export interface CapacitySub {
  email?: string;
  cooling?: boolean;
  calibrated?: boolean;
  util5h?: number;
  reset5h_in?: number;
  util7d?: number;
  reset7d_in?: number;
  rem5h_usd?: number;
  rem7d_usd?: number;
  routable?: boolean;
}

export interface CapacityResponse {
  per_sub?: CapacitySub[];
  available_usd?: {
    next_7d?: number;
    next_1h?: number;
    next_5h?: number;
    next_1d?: number;
  };
}

// GET /codex-subs — GPT/Codex app-server homes (OpenAI-runtime)
export interface CodexHomeWindow {
  slot?: string;
  used_percent?: number;
  window_minutes?: number;
  source?: string;
  samples?: number;
  confidence?: number;
  remaining_usd?: number | null;
  cap_usd?: number | null;
  low_usd?: number | null;
  high_usd?: number | null;
}

export interface CodexRateLimit {
  resets_at?: number;
}

export interface CodexHome {
  id?: string;
  auth_ok?: boolean;
  process_live?: boolean;
  admitted?: boolean;
  reject_reason?: string;
  account_state?: string;
  snapshot_age_secs?: number | null;
  calibration_persistence_ok?: boolean;
  /** epoch-секунды, до которых home в cooling. */
  cooling_until?: number;
  inflight?: number;
  spend_usd_total?: number;
  windows?: CodexHomeWindow[];
  rate_limits?: { primary?: CodexRateLimit; secondary?: CodexRateLimit };
}

export interface CodexWindowTotal {
  cap_usd?: number | null;
  remaining_usd?: number | null;
  observed_homes?: number;
  measured_homes?: number;
}

export interface CodexSubsResponse {
  enabled?: boolean;
  available?: number;
  /** epoch-секунды ближайшего освобождения home. */
  soonest_ready?: number;
  homes?: CodexHome[];
  window_totals?: CodexWindowTotal[];
}

// GET /gemini-subs — Gemini Code Assist профили
export interface GeminiModel {
  id?: string;
  available?: number;
  healthy?: number;
  degraded?: number;
  unknown?: number;
  /** epoch-секунды ближайшей разморозки модели. */
  soonest_ready?: number;
}

export interface GeminiModelHealth {
  model_id?: string;
  cooling_until?: number;
  failure_streak?: number;
  last_success_at?: number;
  last_failure_at?: number;
  last_failure_class?: string | null;
}

export interface GeminiQuota {
  model_id?: string;
  token_type?: string;
  remaining_fraction?: number | null;
  remaining_amount?: string | null;
  reset_time?: string | null;
}

export interface GeminiProfileWindow {
  window_kind?: string;
  source?: string;
  remaining_fraction?: number | null;
  remaining_usd?: number | null;
  cap_usd?: number | null;
  low_usd?: number | null;
  high_usd?: number | null;
  observed_fraction_units?: number;
  observed_spend_usd?: number;
  samples?: number;
  confidence?: number;
  resets_at?: number;
}

export interface GeminiProfile {
  id?: string;
  authenticated?: boolean;
  inflight?: number;
  spend_usd_total?: number;
  cooling_until?: number;
  calibration_persistence_ok?: boolean;
  model_cooling?: GeminiModelHealth[];
  quotas?: GeminiQuota[];
  windows?: GeminiProfileWindow[];
  last_probe_at?: number;
  quota_updated_at?: number;
}

export interface GeminiWindowTotal {
  window_minutes?: number;
  cap_usd?: number | null;
  remaining_usd?: number | null;
  low_usd?: number | null;
  high_usd?: number | null;
  measured_profiles?: number;
}

export interface GeminiTransport {
  antigravity_version?: string;
  node_version?: string;
  http_version?: string;
  profile?: string;
  node_sha256?: string;
  expected_ja3?: string;
  expected_ja4?: string;
  userinfo_profile?: string;
  userinfo_http_version?: string;
  userinfo_expected_ja3?: string;
  userinfo_expected_ja4?: string;
}

export interface GeminiAffinity {
  local_hits?: number;
  redis_hits?: number;
  cache_root_hits?: number;
  misses?: number;
  redis_errors?: number;
  rebinds?: number;
}

export interface GeminiFailures {
  transport?: number;
  backend?: number;
  malformed?: number;
  stream_start?: number;
}

export interface GeminiSubsResponse {
  enabled?: boolean;
  available?: number;
  authenticated?: number;
  inflight?: number;
  usage_metadata_missing?: number;
  /** epoch-секунды «сейчас» по часам runtime (сбросы считаются от него). */
  now?: number;
  models?: GeminiModel[];
  profiles?: GeminiProfile[];
  window_totals?: GeminiWindowTotal[];
  transport?: GeminiTransport;
  affinity?: GeminiAffinity;
  failures?: GeminiFailures;
}
