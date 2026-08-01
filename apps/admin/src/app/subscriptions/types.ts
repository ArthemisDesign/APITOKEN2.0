// Типы payload'ов флотов подписок: GET /subs, /capacity, /codex-subs, /gemini-subs.
// Все поля опциональны — панель деградирует молча, как admin-panel.js.
// Для Codex canonical money — *_nano строки; legacy *_usd только отображаются. Остальные старые
// payload'ы страницы пока остаются presentation USD и не участвуют в money arithmetic.

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
  plan?: string;
  cooling?: boolean;
  calibrated?: boolean;
  util5h?: number;
  reset5h_in?: number;
  util7d?: number;
  reset7d_in?: number;
  cap5h_nano?: string;
  cap7d_nano?: string;
  cap5h_usd?: number;
  cap7d_usd?: number;
  rem5h_usd?: number;
  rem7d_usd?: number;
  avail_1h_usd?: number;
  avail_5h_usd?: number;
  avail_1d_usd?: number;
  avail_7d_usd?: number;
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

// GET /codex-subs — GPT/Codex native homes (OpenAI-runtime).
// *_nano strings are canonical money; *_usd numbers are presentation compatibility only.
export interface CodexHomeWindow {
  slot?: string;
  used_percent?: number;
  used_fraction_units?: number;
  used_fraction?: number;
  window_minutes?: number;
  resets_at?: number | null;
  source?: string;
  samples?: number;
  confidence?: number;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
  low_nano?: string | null;
  high_nano?: string | null;
  remaining_low_nano?: string | null;
  remaining_high_nano?: string | null;
  remaining_usd?: number | null;
  cap_usd?: number | null;
  low_usd?: number | null;
  high_usd?: number | null;
  remaining_low_usd?: number | null;
  remaining_high_usd?: number | null;
  /** Native ChatGPT subscription quota. All quantities are decimal nanocredit strings. */
  capacity_nanocredits?: string | null;
  remaining_nanocredits?: string | null;
  low_nanocredits?: string | null;
  high_nanocredits?: string | null;
  remaining_low_nanocredits?: string | null;
  remaining_high_nanocredits?: string | null;
  observed_spend_nanocredits?: string | null;
  credit_samples?: number | null;
  /** Provider movement repeated without either gateway ledger moving; not proof of external use. */
  unattributed_fraction_units?: number | null;
  observed_spend_nano?: string;
  observed_fraction_units?: number;
  workload_dependent?: boolean;
}

export interface CodexRateLimit {
  resets_at?: number;
  used_percent?: number;
  used_fraction_units?: number;
  used_fraction?: number;
}

export interface CodexHome {
  id?: string;
  plan?: string;
  /** Маскированная подсказка аккаунта из control API; полный ChatGPT email не покидает runtime. */
  email?: string;
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
  spend_nano_total?: string;
  spend_usd_total?: number;
  spend_nanocredits_total?: string | null;
  credit_tracking_started_ts?: number | null;
  calibration_pending_events?: number;
  calibration_dropped_events?: number;
  calibration_evidence?: CodexCalibrationEvidence[];
  windows?: CodexHomeWindow[];
  rate_limits?: { primary?: CodexRateLimit; secondary?: CodexRateLimit };
}

export interface CodexWindowTotal {
  window_minutes?: number;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
  low_nano?: string | null;
  high_nano?: string | null;
  remaining_low_nano?: string | null;
  remaining_high_nano?: string | null;
  cap_usd?: number | null;
  remaining_usd?: number | null;
  low_usd?: number | null;
  high_usd?: number | null;
  remaining_low_usd?: number | null;
  remaining_high_usd?: number | null;
  capacity_nanocredits?: string | null;
  remaining_nanocredits?: string | null;
  low_nanocredits?: string | null;
  high_nanocredits?: string | null;
  remaining_low_nanocredits?: string | null;
  remaining_high_nanocredits?: string | null;
  observed_spend_nanocredits?: string;
  credit_measured_homes?: number;
  credit_observed_homes?: number;
  unattributed_fraction_units?: string;
  observed_spend_nano?: string;
  observed_fraction_units?: string;
  source?: string;
  workload_dependent?: boolean;
  observed_homes?: number;
  measured_homes?: number;
}

/** Shared native-credit capacity for homes with the exact same paid plan and provider window. */
export interface CodexPlanCohort {
  plan?: string;
  window_minutes?: number;
  homes_total?: number;
  measured_homes?: number;
  measurement_resolution_fraction_units?: number;
  observed_fraction_units?: string;
  observed_spend_nanocredits?: string;
  capacity_per_home_nanocredits?: string | null;
  capacity_per_home_low_nanocredits?: string | null;
  capacity_per_home_high_nanocredits?: string | null;
  fleet_capacity_nanocredits?: string | null;
  fleet_capacity_low_nanocredits?: string | null;
  fleet_capacity_high_nanocredits?: string | null;
  fleet_remaining_nanocredits?: string | null;
  fleet_remaining_low_nanocredits?: string | null;
  fleet_remaining_high_nanocredits?: string | null;
  source?: string;
  same_plan_capacity?: boolean;
  workload_dependent?: boolean;
}

export interface CodexCalibrationEvidence {
  model?: string;
  service_tier?: string;
  provider_reported_tier?: string | null;
  api_tariff_schedule_id?: string;
  credit_schedule_id?: string;
  turns?: number;
  first_completed_at?: number;
  last_completed_at?: number;
  input_tokens?: string;
  cached_input_tokens?: string;
  cache_write_input_tokens?: string;
  output_tokens?: string;
  reasoning_output_tokens?: string;
  api_input_nanousd?: string;
  api_cached_input_nanousd?: string;
  api_cache_write_nanousd?: string;
  api_output_nanousd?: string;
  api_total_nanousd?: string;
  chatgpt_input_nanocredits?: string;
  chatgpt_cached_input_nanocredits?: string;
  chatgpt_output_nanocredits?: string;
  chatgpt_total_nanocredits?: string;
}

export interface CodexConversionModel {
  id: string;
  upstream?: string;
  api_tariff_schedule_id?: string;
  credit_schedule_id?: string;
  api: {
    input_nanousd_per_token: string;
    cached_input_nanousd_per_token: string;
    cache_write_nanousd_per_token: string;
    output_nanousd_per_token: string;
    fast_multiplier_basis_points?: number | null;
    long_context_threshold?: string;
    long_input_multiplier_basis_points?: number;
    long_output_multiplier_basis_points?: number;
  };
  chatgpt_credits: {
    input_nanocredits_per_token: string;
    cached_input_nanocredits_per_token: string;
    output_nanocredits_per_token: string;
    fast_multiplier_basis_points?: number | null;
  };
}

export interface CodexSubsResponse {
  enabled?: boolean;
  available?: number;
  /** epoch-секунды ближайшего освобождения home. */
  soonest_ready?: number;
  homes?: CodexHome[];
  window_totals?: CodexWindowTotal[];
  plan_cohorts?: CodexPlanCohort[];
  calibration_evidence_available?: boolean;
  credit_schedule_id?: string;
  conversion_models?: CodexConversionModel[];
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
  window_minutes?: number;
  source?: string;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
  low_nano?: string | null;
  high_nano?: string | null;
  remaining_low_nano?: string | null;
  remaining_high_nano?: string | null;
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
  plan?: string;
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
