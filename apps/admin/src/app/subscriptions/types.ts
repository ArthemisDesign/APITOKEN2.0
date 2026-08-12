// Типы payload'ов флотов подписок: GET /subs, /capacity, /codex-subs, /gemini-subs, /kimi-subs, /glm-subs.
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

// Общая expand-only lifecycle-проекция operational payload'ов.
// Времена — Unix epoch seconds; remaining уже вычислен producer'ом и может быть отрицательным.
export interface SubscriptionLifecycle {
  acquired_at?: number | null;
  subscription_expires_at?: number | null;
  subscription_days_left?: number | null;
}

// GET /capacity — live ёмкость Claude-флота (ключуется по маскированному email)
export interface CapacitySub extends SubscriptionLifecycle {
  email?: string;
  plan?: string;
  cooling?: boolean;
  calibrated?: boolean;
  util5h?: number;
  reset5h_in?: number | null;
  util7d?: number;
  reset7d_in?: number | null;
  cap5h_nano?: string;
  cap7d_nano?: string;
  rem5h_nano?: string;
  rem7d_nano?: string;
  cap5h_usd?: number;
  cap7d_usd?: number;
  rem5h_usd?: number;
  rem7d_usd?: number;
  avail_1h_usd?: number;
  avail_5h_usd?: number;
  avail_1d_usd?: number;
  avail_7d_usd?: number;
  routable?: boolean;
  auth_state?: string;
  dead_reason?: string;
  dead_since?: number;
  windows?: ClaudeSubWindow[];
}

export interface ClaudeSubWindow {
  window_kind?: "5h" | "7d" | string;
  window_minutes?: number;
  resets_at?: number | null;
  observed_at?: number | null;
  data_age_seconds?: number | null;
  snapshot_fresh?: boolean;
  /** Почему точной текущей доли нет — независимо от денежного `missing_reason`.
   *  `awaiting_probe` | `last_known_before_reset` | `window_rolled_over`; отсутствует, когда снапшот свежий. */
  quota_state?: string | null;
  used_fraction_units?: number | null;
  measurement_resolution_fraction_units?: number | null;
  current_quota_source?: string | null;
  displayed_quota_source?: string | null;
  last_known_quota_source?: string | null;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
  last_known_remaining_nano?: string | null;
  low_nano?: string | null;
  high_nano?: string | null;
  remaining_low_nano?: string | null;
  remaining_high_nano?: string | null;
  confidence_bp?: number | null;
  cohort_samples?: string | null;
  cohort_observed_fraction_units?: string | null;
  cohort_observed_spend_nano?: string | null;
  account_samples?: number | null;
  account_observed_fraction_units?: number | null;
  account_observed_spend_nano?: string | null;
  unattributed_fraction_units?: number | null;
  source?: string;
  same_plan_capacity?: boolean;
  missing_reason?: string | null;
}

export interface ClaudeWindowTotal {
  window_kind?: "5h" | "7d" | string;
  window_minutes?: number;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
  low_nano?: string | null;
  high_nano?: string | null;
  remaining_low_nano?: string | null;
  remaining_high_nano?: string | null;
  routable_subs?: number;
  calibrated_subs?: number;
  snapshot_subs?: number;
  plans_total?: number;
  calibrated_plans?: number;
  confidence_bp?: number | null;
  samples?: string;
  observed_fraction_units?: string;
  observed_spend_nano?: string;
  unattributed_fraction_units?: string;
  source?: string;
  workload_dependent?: boolean;
  fail_closed?: boolean;
  missing_reason?: string | null;
}

export interface ClaudePlanCohort {
  plan?: string;
  window_kind?: string;
  window_minutes?: number;
  subs_total?: number;
  routable_subs?: number;
  snapshot_subs?: number;
  routable_snapshot_subs?: number;
  measured_subs?: number;
  observed_fraction_units?: string;
  observed_spend_nano?: string;
  samples?: string;
  unattributed_fraction_units?: string;
  measurement_resolution_fraction_units?: number;
  confidence_bp?: number | null;
  capacity_per_sub_nano?: string | null;
  low_per_sub_nano?: string | null;
  high_per_sub_nano?: string | null;
  fleet_capacity_nano?: string | null;
  fleet_remaining_nano?: string | null;
  source?: string;
  same_plan_capacity?: boolean;
  missing_reason?: string | null;
}

export interface ClaudeCalibrationEvidence {
  email?: string;
  model?: string;
  service_tier?: string;
  inference_geo?: string;
  tariff_schedule_id?: string;
  turns?: number;
  first_completed_at?: number;
  last_completed_at?: number;
  input_tokens?: string;
  cache_read_tokens?: string;
  cache_write_5m_tokens?: string;
  cache_write_1h_tokens?: string;
  output_tokens?: string;
  search_queries?: string;
  api_input_nanousd?: string;
  api_cache_read_nanousd?: string;
  api_cache_write_5m_nanousd?: string;
  api_cache_write_1h_nanousd?: string;
  api_output_nanousd?: string;
  api_search_nanousd?: string;
  api_total_nanousd?: string;
}

export interface ClaudeRateTier {
  id: "standard" | "fast";
  tariff_schedule_id?: string;
  input_nanousd_per_token: string;
  cache_read_nanousd_per_token: string;
  cache_write_5m_nanousd_per_token: string;
  cache_write_1h_nanousd_per_token: string;
  output_nanousd_per_token: string;
}

export interface ClaudeConversionModel {
  id: string;
  display_name?: string;
  alias_generation?: number;
  web_search_nanousd_per_request?: string;
  us_inference_basis_points?: number;
  tiers: ClaudeRateTier[];
}

export interface CapacityResponse {
  now?: number;
  calibrated?: boolean;
  calibration_authority_available?: boolean;
  calibration_delivery?: {
    pending_events?: number;
    dropped_events?: number;
    persistence_ok?: boolean;
    queue_limit?: number;
  } | null;
  per_sub?: CapacitySub[];
  available_usd?: {
    next_7d?: number;
    next_1h?: number;
    next_5h?: number;
    next_1d?: number;
  };
  available_nano?: {
    next_7d?: string;
    next_1h?: string;
    next_5h?: string;
    next_1d?: string;
  };
  window_totals?: ClaudeWindowTotal[];
  plan_cohorts?: ClaudePlanCohort[];
  calibration_evidence?: ClaudeCalibrationEvidence[];
  conversion_models?: ClaudeConversionModel[];
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

export interface CodexHome extends SubscriptionLifecycle {
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
  /** epoch-секунды «сейчас» по часам runtime. */
  now?: number;
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
  quota_model_ids?: string[];
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
  remaining_fraction_units?: number;
  used_fraction_units?: number;
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

export interface GeminiProfile extends SubscriptionLifecycle {
  id?: string;
  /** Маскированная подсказка аккаунта; полный Google email не покидает runtime. */
  email?: string;
  plan?: string;
  authenticated?: boolean;
  /** Оператор вывел профиль из ротации (`pool_member_disables`). Он остаётся в списке — иначе
   *  его нельзя было бы вернуть, — но не маршрутизируется и не пробится. */
  disabled?: boolean;
  /** Оператор убрал уже отключённый профиль из списка по умолчанию. Только отображение:
   *  на маршрутизацию не влияет, движок продолжает отдавать строку, чтобы её можно было вернуть. */
  hidden?: boolean;
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
  measured_profiles?: number;
  observed_profiles?: number;
}

export interface GeminiConversionModel {
  id: string;
  display_name?: string;
  input_token_limit?: string;
  output_token_limit?: string;
  quota_model_ids?: string[];
  rates: {
    input_nanousd_per_token: string;
    audio_input_nanousd_per_token: string;
    cached_input_nanousd_per_token: string;
    cached_audio_input_nanousd_per_token: string;
    output_nanousd_per_token: string;
    image_output_nanousd_per_token: string;
    long_context_threshold?: string;
    long_input_nanousd_per_token: string;
    long_audio_input_nanousd_per_token: string;
    long_cached_input_nanousd_per_token: string;
    long_cached_audio_input_nanousd_per_token: string;
    long_output_nanousd_per_token: string;
  };
  search?: {
    billing_unit?: "query" | "grounded_prompt" | string;
    nanousd_per_unit?: string;
  };
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

export interface GeminiCalibrationDelivery {
  pending_events?: number;
  dropped_events?: number;
  persistence_ok?: boolean;
  queue_limit?: number;
}

export interface GeminiSubsResponse {
  enabled?: boolean;
  available?: number;
  authenticated?: number;
  inflight?: number;
  usage_metadata_missing?: number;
  calibration_authority_available?: boolean;
  calibration_delivery?: GeminiCalibrationDelivery | null;
  /** epoch-секунды «сейчас» по часам runtime (сбросы считаются от него). */
  now?: number;
  models?: GeminiModel[];
  profiles?: GeminiProfile[];
  window_totals?: GeminiWindowTotal[];
  conversion_models?: GeminiConversionModel[];
  transport?: GeminiTransport;
  affinity?: GeminiAffinity;
  failures?: GeminiFailures;
}

// GET /kimi-subs — KIMI-профили (backend-only плоскость с отдельным stable origin :8803).
// Деньги — decimal nano strings (BigInt); неизвестное — null, никогда не 0.
// Идентичность окна — exact duration_secs (18000 = rolling 5ч, 604800 = weekly);
// длительности — данные, а не фиксированные 5ч/7д слоты. Email/subject не
// сериализуются никогда: идентичность — opaque roster id + bounded plan label.
export interface KimiQuotaWindow {
  duration_secs?: number;
  /** Provider authority: used/limit — сырые native-unit счётчики (JSON numbers, i64). */
  used_units?: number;
  limit_units?: number;
  /** Доля использования в единицах 1e-8 (100% = 100_000_000), как у остальных флотов. */
  used_fraction_units?: number;
  measurement_resolution_fraction_units?: number;
  resets_at?: number | null;
  observed_at?: number | null;
}

export interface KimiCalibrationWindow {
  duration_secs?: number;
  samples?: number;
  confidence_bp?: number;
  capacity?: {
    current_nano?: string | null;
    low_nano?: string | null;
    high_nano?: string | null;
  } | null;
  /** null целиком, когда неизвестны и native, и API-значение. */
  remaining?: {
    native_units?: number | null;
    api_nano?: string | null;
  } | null;
  observed_spend_nano?: string;
  unattributed_fraction_units?: number;
  last_measured_at?: number | null;
  estimator_version?: number;
}

export interface KimiProfile {
  /** Opaque roster id; subject/email/phone не покидают runtime. */
  id?: string;
  /** Reviewed bounded label тарифа либо "unreviewed". */
  plan?: string;
  live?: boolean;
  inflight?: number;
  /** Три оси cooling; epoch-секунды окончания либо null. */
  cooling?: {
    auth_until?: number | null;
    transport_until?: number | null;
    quota_until?: number | null;
  } | null;
  quota_observed_at?: number | null;
  quota?: KimiQuotaWindow[];
  calibration?: KimiCalibrationWindow[];
}

export interface KimiDelivery {
  pending_events?: number;
  dropped_events?: number;
  persistence_ok?: boolean;
}

export interface KimiFleet {
  profiles?: number;
  live_profiles?: number;
  available_profiles?: number;
  inflight_requests?: number;
  auth_quarantined_profiles?: number;
  transport_cooling_profiles?: number;
  quota_cooling_profiles?: number;
}

export interface KimiSubsResponse {
  /** epoch-секунды «сейчас» по часам runtime (сбросы считаются от него). */
  now?: number;
  enabled?: boolean;
  /** false — durable-хранилище калибровки не читается: профили приходят без
   * calibration-строк, и «ждём данные» означает аварию read-стороны, а не отсутствие замеров. */
  calibration_authority_available?: boolean;
  delivery?: KimiDelivery | null;
  fleet?: KimiFleet | null;
  profiles?: KimiProfile[];
}

// GET /glm-subs — GLM (Z.ai Coding Plan) профили (backend-only плоскость внутри Anthropic
// runtime). Деньги — decimal nano strings (BigInt); native counters — plain integers
// (microcredits у калибровки, сырые provider counters у quota); неизвестное — null, никогда
// не 0. Идентичность окна — exact duration_secs (18000 = rolling 5ч, 604800 = weekly).
// Subject (digest ключа), ключ, proxy, base_url и credential path не сериализуются никогда:
// идентичность — opaque roster id + bounded plan label.
export interface GlmQuotaWindow {
  duration_secs?: number;
  /** Provider authority: сырые counters — plain integers либо null, пока семантика единиц
   * не доказана (docs/engine/GLM_PROVIDER.md §6.3); никогда не zero-filled. */
  used_units?: number | null;
  limit_units?: number | null;
  remaining_units?: number | null;
  /** Доля использования в единицах 1e-8 (100% = 100_000_000), как у остальных флотов. */
  used_fraction_units?: number | null;
  measurement_resolution_fraction_units?: number | null;
  resets_at?: number | null;
  observed_at?: number | null;
}

export interface GlmCalibrationWindow {
  duration_secs?: number;
  samples?: number;
  confidence_bp?: number;
  capacity?: {
    current_nano?: string | null;
    low_nano?: string | null;
    high_nano?: string | null;
  } | null;
  /** null целиком, когда неизвестны и native, и API-значение. native_units — microcredits. */
  remaining?: {
    native_units?: number | null;
    api_nano?: string | null;
  } | null;
  observed_spend_nano?: string;
  observed_spend_native_units?: number;
  unattributed_fraction_units?: number;
  last_measured_at?: number | null;
  estimator_version?: number;
}

export interface GlmProfile {
  /** Opaque roster id; subject/key/proxy/base_url не покидают runtime. */
  id?: string;
  /** Bounded label тарифа: Lite/Pro/Max либо "unreviewed". */
  plan?: string;
  /** Ключ подтверждён quota-probe. Не ось ротации: допуск решают dead/suspect/cooling. */
  live?: boolean;
  /** Auth-оси GLM — durable флаги, а не timed quarantine: dead до замены ключа Auth Bot'ом,
   * suspect до свежего прошедшего quota-probe. `auth_until` не существует. */
  account_dead?: boolean;
  account_suspect?: boolean;
  /** Две timed оси cooling; epoch-секунды окончания либо null. */
  cooling?: {
    transport_until?: number | null;
    quota_until?: number | null;
  } | null;
  inflight?: number;
  quota_observed_at?: number | null;
  quota?: GlmQuotaWindow[];
  calibration?: GlmCalibrationWindow[];
}

export interface GlmDelivery {
  pending_events?: number;
  dropped_events?: number;
  persistence_ok?: boolean;
}

export interface GlmFleet {
  profiles?: number;
  live_profiles?: number;
  available_profiles?: number;
  inflight_requests?: number;
  account_dead_profiles?: number;
  account_suspect_profiles?: number;
  transport_cooling_profiles?: number;
  quota_cooling_profiles?: number;
}

export interface GlmWindowTotal {
  /** Проекция exact duration_secs (18_000s = 300 мин, 604_800s = 10_080 мин). */
  window_minutes?: number;
  duration_secs?: number;
  /** Fail-closed суммы по флоту: null, пока хотя бы один профиль не измерен — никогда не 0. */
  capacity_nano?: string | null;
  remaining_nano?: string | null;
}

export interface GlmSubsResponse {
  /** epoch-секунды «сейчас» по часам runtime (сбросы считаются от него). */
  now?: number;
  enabled?: boolean;
  delivery?: GlmDelivery | null;
  fleet?: GlmFleet | null;
  window_totals?: GlmWindowTotal[];
  profiles?: GlmProfile[];
}

// GET /tripo3d-subs — Tripo3D (VAST API platform) профили (dedicated dormant plane
// `ProviderMode::Tripo3d`). Окон нет: prepaid баланс никогда не сбрасывается, поэтому
// единственный трек — balance remaining/full, а cohort заменяет plan+window как ось
// (docs/engine/TRIPO3D_PROVIDER.md §5.3). Деньги — decimal nano strings (BigInt); native
// counters — exact integers (micro-units прованной единицы, пока единица не доказана — null);
// неизвестное — null, никогда не 0. Subject (digest ключа), ключ, proxy, base_url и
// credential path не сериализуются никогда: идентичность — opaque roster id + bounded cohort.
export interface Tripo3dBalanceEvidence {
  /** epoch-секунды последнего balance-probe; null — наблюдений ещё не было. */
  observed_at?: number | null;
  /** Verbatim текст провайдера; parsed halves — micro-units либо null, пока unit не доказан. */
  balance_raw?: string | null;
  frozen_raw?: string | null;
  balance_micro_units?: number | null;
  frozen_micro_units?: number | null;
}

export interface Tripo3dCalibration {
  cohort?: string;
  samples?: number;
  confidence_bp?: number;
  capacity?: {
    current_nano?: string | null;
    low_nano?: string | null;
    high_nano?: string | null;
  } | null;
  /** null целиком, пока неизвестны обе половины баланса; api_nano — remaining sellable API-$. */
  remaining?: {
    native_micro_units?: number | null;
    api_nano?: string | null;
  } | null;
  observed_spend_nano?: string;
  observed_spend_native_millicredits?: number;
  last_measured_at?: number | null;
  estimator_version?: number;
}

export interface Tripo3dProfile {
  /** Opaque roster id; subject/key/proxy/base_url не покидают runtime. */
  id?: string;
  /** Declared top-up cohort (bounded, lowercase-normalized). */
  cohort?: string;
  /** Баланс-probe прошёл на этом поколении runtime. Не ось допуска: допуск решают
   *  hard (rate-limit/balance wall/shortfall) и soft (auth/transport) оси. */
  live?: boolean;
  /** HARD balance verdict (403 + code 2010): resting, пока probe не покажет средства. */
  balance_walled?: boolean;
  /** Три cooling-оси; epoch-секунды окончания либо null. */
  cooling?: {
    rate_limit_until?: number | null;
    auth_until?: number | null;
    transport_until?: number | null;
  } | null;
  inflight?: number;
  balance?: Tripo3dBalanceEvidence | null;
  calibration?: Tripo3dCalibration | null;
}

export interface Tripo3dDelivery {
  pending_events?: number;
  dropped_events?: number;
  persistence_ok?: boolean;
}

export interface Tripo3dFleet {
  profiles?: number;
  live_profiles?: number;
  available_profiles?: number;
  inflight_requests?: number;
  inflight_drains?: number;
  tracked_tasks?: number;
  rate_limited_profiles?: number;
  balance_walled_profiles?: number;
  auth_cooling_profiles?: number;
  transport_cooling_profiles?: number;
  missing_consumed_credit?: number;
  tariff_anomaly?: number;
  undocumented_final?: number;
  artifact_failures?: number;
}

export interface Tripo3dSubsResponse {
  /** epoch-секунды «сейчас» по часам runtime (сбросы считаются от него). */
  now?: number;
  enabled?: boolean;
  delivery?: Tripo3dDelivery | null;
  calibration_authority_available?: boolean;
  fleet?: Tripo3dFleet | null;
  profiles?: Tripo3dProfile[];
}
