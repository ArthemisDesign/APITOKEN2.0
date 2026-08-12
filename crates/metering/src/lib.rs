//! # metering — точный подсчёт токенов → USD-эквивалент
//!
//! Задача: **ни один токен не проходит мимо счёта**. Поэтому:
//! - учитываем ВСЕ корзины токенов, которые Anthropic тарифицирует по-разному:
//!   вход, выход, чтение кэша, запись кэша (5м и 1ч отдельно), web-search;
//! - парсим usage И из обычного JSON-ответа, И из стрима (SSE) — в стриме input/cache
//!   лежат в `message_start`, а output накапливается в `message_delta` (берём ПОСЛЕДНИЙ);
//! - считаем в ЦЕЛЫХ **нанодолларах** (1 USD = 1e9 нано) — без float-дрейфа, каждый токен
//!   даёт точный вклад. Цена $/Mtoken × 1000 = нано/токен (для всех цен — целое число).
//!
//! Крейт чистый (только serde_json), без сети/БД — тестируется в изоляции.

use serde_json::Value;

pub mod codex;
pub mod gemini;
pub mod glm;
pub mod kimi;
pub mod openai_image;
pub mod tripo3d;
pub use codex::{
    codex_catalog_at, codex_compiled_credit_rates, codex_compiled_tariffs_at,
    codex_credit_cost_nano, codex_credit_cost_nano_with_rates, codex_credit_rates,
    codex_credit_rates_family,
    codex_matched_credit_rates_at, codex_matched_tariff_at, codex_prices_at,
    codex_subscription_fast_multiplier_basis_points, codex_tariff_capability_at,
    CodexAdmissionTariffIdentity, CodexContextTier, CodexCreditRates, CodexCreditUsage,
    CodexModelSpec, CodexPriceEpoch, CodexPrices, CodexServiceTier, CodexTariffModifiers,
    CODEX_ALIAS_GENERATION, CODEX_CREDIT_FAMILY, CODEX_CREDIT_SCHEDULE_ID,
};
pub use gemini::{
    gemini_catalog_at, gemini_compiled_tariffs_at, gemini_matched_tariff_at, gemini_prices_at,
    GeminiModelSpec, GeminiPriceEpoch, GeminiPrices, GeminiSearchBilling, GeminiUsage,
};
pub use glm::{
    glm_catalog_at, glm_compiled_credit_rates, glm_compiled_tariffs_at, glm_credit_cost_micro,
    glm_credit_rates_for_served_model, glm_credits_at, glm_is_peak_utc,
    glm_matched_credit_rates_at, glm_matched_tariff_at, glm_prices_at,
    glm_prices_for_served_model, glm_resolve_subscription_model, glm_subscription_models,
    GlmCreditRates, GlmModelSpec, GlmPriceEpoch, GlmPrices, GlmSubscriptionModel, GlmUsage,
    GlmUsageError, GLM_CREDIT_SCHEDULE_ID, GLM_TARIFF_SCHEDULE_ID,
};
pub use kimi::{
    kimi_catalog_at, kimi_compiled_tariffs_at, kimi_matched_tariff_at, kimi_prices_at,
    kimi_prices_for_served_model, kimi_resolve_subscription_model, kimi_subscription_models,
    KimiModelSpec, KimiPriceEpoch, KimiPrices, KimiSubscriptionModel, KimiUsage, KimiUsageError,
    KIMI_TARIFF_SCHEDULE_ID,
};
pub use openai_image::{
    openai_image_compiled_tariff, openai_image_cost_nanodollars, openai_image_tariff,
    openai_image_tariff_family, OpenAiImageMeteringError, OpenAiImagePrices,
    OpenAiImageTariffIdentity, OpenAiImageUsage, GPT_IMAGE_2_ALIAS, GPT_IMAGE_2_SNAPSHOT,
    OPENAI_IMAGE_ALIAS_GENERATION, OPENAI_IMAGE_TARIFF_FAMILY, OPENAI_IMAGE_TARIFF_SCHEDULE_ID,
};
pub use tripo3d::{
    tripo3d_animate_retarget_credits, tripo3d_convert_model_credits, tripo3d_cost_nanodollars,
    tripo3d_edit_multiview_image_credits, tripo3d_tariff_family, tripo3d_tariff_schedule_id,
    tripo3d_task_credits, Tripo3dConvertMode, Tripo3dGeometryQuality, Tripo3dMeteringError,
    Tripo3dOptions, Tripo3dTaskKind, Tripo3dTextureQuality, TRIPO3D_NANOUSD_PER_CREDIT,
    TRIPO3D_TARIFF_FAMILY, TRIPO3D_TARIFF_SCHEDULE_ID,
};

pub const NANO_PER_USD: i128 = 1_000_000_000;
pub const WEB_SEARCH_NANO: i128 = 10_000_000; // $0.01 за web_search-запрос

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TariffIdentityError {
    InvalidPricedTimestamp,
    UnsupportedModelIdentity,
    UnsupportedModifier,
}

/// Opaque, versioned identifier for one immutable effective tariff epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TariffScheduleId(&'static str);

impl TariffScheduleId {
    pub(crate) const fn from_static(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Токены запроса по корзинам (u64 — точные счётчики, ничего не теряем).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,     // cache_read_input_tokens
    pub cache_write_5m_tokens: u64, // cache_creation.ephemeral_5m_input_tokens
    pub cache_write_1h_tokens: u64, // cache_creation.ephemeral_1h_input_tokens
    pub web_search_requests: u64,   // server_tool_use.web_search_requests
}

impl Usage {
    /// Сумма всех токеновых корзин — для контроля, что ничего не потеряли.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_read_tokens
            + self.cache_write_5m_tokens
            + self.cache_write_1h_tokens
    }
    pub fn is_zero(&self) -> bool {
        self.total_tokens() == 0 && self.web_search_requests == 0
    }
    /// Сложить (напр. несколько ответов на один ключ).
    pub fn add(&mut self, o: &Usage) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_read_tokens += o.cache_read_tokens;
        self.cache_write_5m_tokens += o.cache_write_5m_tokens;
        self.cache_write_1h_tokens += o.cache_write_1h_tokens;
        self.web_search_requests += o.web_search_requests;
    }
}

/// Цены по корзинам в **нанодолларах за токен** (= $/Mtoken × 1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prices {
    pub input: i128,
    pub output: i128,
    pub cache_read: i128,
    pub cache_write_5m: i128,
    pub cache_write_1h: i128,
}

/// Current Opus tariff (4.5–5). Unknown response models deliberately use `MAX_PRICES` below: a newly
/// introduced expensive model must never be silently billed at a cheaper historical fallback.
/// ($/Mtoken × 1000 — уже целое нано/токен значение; константы записаны сразу в нано/токен.)
const OPUS_CURRENT_PRICES: Prices = Prices {
    input: 5000,
    cache_read: 500,
    cache_write_5m: 6250,
    cache_write_1h: 10000,
    output: 25000,
};
/// САМЫЙ ДОРОГОЙ тариф по ВСЕМ корзинам (Opus 3/4.0/4.1: $15/$75). Для РЕЗЕРВА нераспознанной модели:
/// апстрим мог резолвить алиас в дорогую модель, а списание идёт по модели ОТВЕТА — если резервировать
/// по дешёвому дефолту, charge пробил бы hold. Резерв по MAX ⟹ charge ≤ hold при любом резолве.
const MAX_PRICES: Prices = Prices {
    input: 15000,
    cache_read: 1500,
    cache_write_5m: 18750,
    cache_write_1h: 30000,
    output: 75000,
};
const FAST_OPUS_CURRENT_PRICES: Prices = Prices {
    input: 10000,
    cache_read: 1000,
    cache_write_5m: 12500,
    cache_write_1h: 20000,
    output: 50000,
};
const FAST_OPUS_47_PRICES: Prices = Prices {
    input: 30000,
    cache_read: 3000,
    cache_write_5m: 37500,
    cache_write_1h: 60000,
    output: 150000,
};
/// Fable 5 / Mythos 5 — самые дорогие текущие модели ($10/$50).
const FABLE_5_PRICES: Prices = Prices {
    input: 10000,
    cache_read: 1000,
    cache_write_5m: 12500,
    cache_write_1h: 20000,
    output: 50000,
};
/// Opus 3 / 4.0 / 4.1 — старший исторический тариф ($15/$75).
const OPUS_LEGACY_PRICES: Prices = Prices {
    input: 15000,
    cache_read: 1500,
    cache_write_5m: 18750,
    cache_write_1h: 30000,
    output: 75000,
};
const SONNET_STANDARD_PRICES: Prices = Prices {
    input: 3000,
    cache_read: 300,
    cache_write_5m: 3750,
    cache_write_1h: 6000,
    output: 15000,
};
const SONNET5_INTRO_PRICES: Prices = Prices {
    input: 2000,
    cache_read: 200,
    cache_write_5m: 2500,
    cache_write_1h: 4000,
    output: 10000,
};
// 2026-09-01T00:00:00Z — Sonnet 5 intro pricing ends 2026-08-31
const SONNET5_STD_START: i64 = 1788220800;
/// Haiku тарифицируется ПОКОЛЕННО (цены росли): 4.5 = $1, 3.5 = $0.80, 3 = $0.25.
const HAIKU_45_PRICES: Prices = Prices {
    input: 1000,
    cache_read: 100,
    cache_write_5m: 1250,
    cache_write_1h: 2000,
    output: 5000,
};
const HAIKU_35_PRICES: Prices = Prices {
    input: 800,
    cache_read: 80,
    cache_write_5m: 1000,
    cache_write_1h: 1600,
    output: 4000,
};
// Haiku 3 cache_write_5m: $0.3125/Mtoken rounds to the whole-nano 313.
const HAIKU_3_PRICES: Prices = Prices {
    input: 250,
    cache_read: 25,
    cache_write_5m: 313,
    cache_write_1h: 500,
    output: 1250,
};

/// Version of the exact audited Anthropic alias table used for durable snapshots.
///
/// Generation 2 admits the three older reviewed identities (`claude-opus-4-6`, `claude-opus-4-5`,
/// `claude-sonnet-4-5`) and replaces the single hand-written dated alias with the published
/// `-YYYYMMDD` snapshot rule, so every model carries a canonical identity instead of only Haiku 4.5.
pub const ANTHROPIC_ALIAS_GENERATION: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicSpeed {
    Standard,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicInferenceGeo {
    Global,
    Us,
}

/// Request-side modifiers applied to the effective reserve tariff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicAdmissionModifiers {
    pub speed: AnthropicSpeed,
    pub inference_geo: AnthropicInferenceGeo,
}

impl AnthropicAdmissionModifiers {
    pub const fn inference_geo_basis_points(self) -> i64 {
        match self.inference_geo {
            AnthropicInferenceGeo::Global => 10_000,
            AnthropicInferenceGeo::Us => 11_000,
        }
    }
}

/// Exact, versioned Anthropic tariff identity suitable for a durable admission snapshot.
///
/// This exact capability table is intentionally narrower than the legacy heuristic price matcher.
/// A historical, future or malformed id returns a typed fallback so shadow code cannot pretend it
/// has an audited canonical identity. Product access remains a separate catalog/policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicAdmissionTariffIdentity {
    pub canonical_model_id: &'static str,
    pub alias_generation: i64,
    pub tariff_schedule_id: TariffScheduleId,
    pub schedule_effective_from: i64,
    pub effective_reserve_prices: Prices,
    pub modifiers: AnthropicAdmissionModifiers,
}

/// Published dated snapshots of audited models, mapped to their rolling canonical id.
///
/// Anthropic publishes each model under both a rolling alias (`claude-sonnet-4-5`) and a dated
/// snapshot (`claude-sonnet-4-5-20250929`), and SDK defaults routinely send the dated form. This
/// table stays explicit on purpose: a generic "strip the trailing date" rule would also promote an
/// unpublished future snapshot to today's tariff, which is exactly the silent mispricing
/// `anthropic_tariff_identity_does_not_promote_legacy_fallbacks` exists to prevent. Every entry
/// here is a snapshot this repository already routes (`crates/router/routing-presets.json`).
const ANTHROPIC_DATED_SNAPSHOT_ALIASES: &[(&str, &str)] = &[
    ("claude-haiku-4-5-20251001", "claude-haiku-4-5"),
    ("claude-opus-4-5-20251101", "claude-opus-4-5"),
    ("claude-sonnet-4-5-20250929", "claude-sonnet-4-5"),
];

/// Resolve a published dated snapshot to its canonical id, leaving every other id untouched.
fn canonical_alias_for(model_id: &str) -> &str {
    ANTHROPIC_DATED_SNAPSHOT_ALIASES
        .iter()
        .find(|(alias, _)| *alias == model_id)
        .map(|(_, canonical)| *canonical)
        .unwrap_or(model_id)
}

/// Resolve one audited model capability to its canonical id, effective tariff epoch and typed
/// reserve-time modifiers. This function is pure and has no production caller in Stage 3B1c.1;
/// it does not grant product access.
pub fn anthropic_tariff_capability_at(
    requested_model_id: &str,
    priced_ts: i64,
    modifiers: AnthropicAdmissionModifiers,
) -> Result<AnthropicAdmissionTariffIdentity, TariffIdentityError> {
    if priced_ts <= 0 {
        return Err(TariffIdentityError::InvalidPricedTimestamp);
    }

    let requested_model_id = canonical_alias_for(requested_model_id);

    let canonical_model_id = [
        "claude-opus-4-8",
        "claude-opus-4-7",
        "claude-opus-4-6",
        "claude-opus-4-5",
        "claude-sonnet-5",
        "claude-sonnet-4-6",
        "claude-sonnet-4-5",
        "claude-haiku-4-5",
        "claude-opus-5",
        "claude-fable-5",
    ]
    .into_iter()
    .find(|canonical| requested_model_id == *canonical)
    .ok_or(TariffIdentityError::UnsupportedModelIdentity)?;

    let fast = modifiers.speed == AnthropicSpeed::Fast;
    let (tariff_schedule_id, schedule_effective_from) = match (canonical_model_id, fast) {
        ("claude-opus-4-8" | "claude-opus-5", true) => ("anthropic/fast/opus-current/v1", 0),
        (_, true) => ("anthropic/fast/opus-4-7-conservative/v1", 0),
        // Opus 4.5 through 5 share one published standard tariff, so they share one schedule id.
        (
            "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" | "claude-opus-4-5"
            | "claude-opus-5",
            false,
        ) => ("anthropic/standard/opus-current/v1", 0),
        ("claude-fable-5", false) => ("anthropic/standard/fable-5/v1", 0),
        ("claude-sonnet-5", false) if priced_ts < SONNET5_STD_START => {
            ("anthropic/standard/sonnet-5-intro/v1", 0)
        }
        ("claude-sonnet-5", false) => ("anthropic/standard/sonnet-current/v1", SONNET5_STD_START),
        ("claude-sonnet-4-6" | "claude-sonnet-4-5", false) => {
            ("anthropic/standard/sonnet-current/v1", 0)
        }
        ("claude-haiku-4-5", false) => ("anthropic/standard/haiku-4-5/v1", 0),
        _ => return Err(TariffIdentityError::UnsupportedModelIdentity),
    };

    let base_prices = model_prices_reserve_for_speed_at(canonical_model_id, priced_ts, fast);
    let effective_reserve_prices = if modifiers.inference_geo == AnthropicInferenceGeo::Us {
        premium_prices_ceil(base_prices, modifiers.inference_geo_basis_points())
    } else {
        base_prices
    };

    Ok(AnthropicAdmissionTariffIdentity {
        canonical_model_id,
        alias_generation: ANTHROPIC_ALIAS_GENERATION,
        tariff_schedule_id: TariffScheduleId::from_static(tariff_schedule_id),
        schedule_effective_from,
        effective_reserve_prices,
        modifiers,
    })
}

/// Цены модели для СПИСАНИЯ ($/Mtoken → нано/токен). Совпадают с официальными ставками Anthropic.
/// Нераспознанная строка → консервативный MAX. (Для РЕЗЕРВА см. `model_prices_reserve`.)
/// Чистая таблица без зависимости от часов: Sonnet 5 возвращается по стандартной ставке.
pub fn model_prices(model: &str) -> Prices {
    model_prices_at(model, SONNET5_STD_START)
}

/// Effective-dated цены для списания. Только Sonnet 5 меняет ставку по времени; остальные модели
/// полностью совпадают с `model_prices`.
pub fn model_prices_at(model: &str, now_unix: i64) -> Prices {
    model_prices_matched_at(model, now_unix).unwrap_or(MAX_PRICES)
}

/// Цены для РЕЗЕРВА (hold). Распознанная модель → её цена (апстрим отдаст ТОТ ЖЕ тариф → charge ≤ hold,
/// точность сохранена). Нераспознанная → MAX_PRICES (страховка от алиаса, резолвящегося в дорогую
/// модель: иначе charge по ответу пробил бы hold и увёл баланс в минус до −2×). Разница с `model_prices`
/// Unknown models use MAX for both paths; recognized models remain exact.
pub fn model_prices_reserve(model: &str) -> Prices {
    model_prices_reserve_at(model, SONNET5_STD_START)
}

/// Effective-dated цены для резерва, с тем же тарифом Sonnet 5, что и итоговое списание.
pub fn model_prices_reserve_at(model: &str, now_unix: i64) -> Prices {
    model_prices_matched_at(model, now_unix).unwrap_or(MAX_PRICES)
}

/// Effective token prices after the response-reported speed modifier. Unknown fast models use the
/// highest published Fast Mode tariff, so a future model cannot silently fall through cheaply.
pub fn model_prices_for_speed_at(model: &str, now_unix: i64, fast: bool) -> Prices {
    if !fast {
        return model_prices_at(model, now_unix);
    }
    let model = model.to_ascii_lowercase();
    if model == "claude-opus-5" || model.contains("opus-4-8") {
        FAST_OPUS_CURRENT_PRICES
    } else {
        // Opus 4.7 has the highest published fast tariff. It is also the conservative fallback for
        // an unknown model that nevertheless reports speed=fast.
        FAST_OPUS_47_PRICES
    }
}

pub fn model_prices_reserve_for_speed_at(model: &str, now_unix: i64, fast: bool) -> Prices {
    if fast {
        model_prices_for_speed_at(model, now_unix, true)
    } else {
        model_prices_reserve_at(model, now_unix)
    }
}

/// Round each per-token price upward for reservation. This keeps a geo premium an upper bound even
/// for price points that do not divide cleanly at nanodollar precision.
pub fn premium_prices_ceil(prices: Prices, basis_points: i64) -> Prices {
    let apply = |value: i128| (value * basis_points.max(0) as i128 + 9_999) / 10_000;
    Prices {
        input: apply(prices.input),
        output: apply(prices.output),
        cache_read: apply(prices.cache_read),
        cache_write_5m: apply(prices.cache_write_5m),
        cache_write_1h: apply(prices.cache_write_1h),
    }
}

/// Точный матч тарифа по имени; `None` — не распознано (не свалилось в дефолт).
fn model_prices_matched_at(model: &str, now_unix: i64) -> Option<Prices> {
    anthropic_tariff_branch_at(model, now_unix).map(|(_, prices)| prices)
}

/// One ordered heuristic branch of the legacy Anthropic matcher: the tariff family key (the
/// compiled schedule id minus its `/vN` suffix) together with the branch prices.
///
/// This is the single branch table behind both `model_prices_matched_at` and the public
/// `anthropic_matched_tariff_at`, so the family key a hot tariff override targets can never
/// diverge from the price the compiled matcher actually returns.
fn anthropic_tariff_branch_at(model: &str, now_unix: i64) -> Option<(&'static str, Prices)> {
    let m = model.to_lowercase();
    // Fable 5 / Mythos 5 — самые дорогие текущие модели ($10/$50). ДОЛЖНЫ идти до дефолта,
    // иначе считались бы как Opus 4.8 ($5/$25) — недосписание вдвое (теряем деньги).
    if m.contains("fable") || m.contains("mythos") {
        return Some(("anthropic/standard/fable-5", FABLE_5_PRICES)); // $10 / 1.0 / 12.5 / 20 / 50
    }
    // Opus: 4.5–5 = $5/$25; Opus 3 / 4.0 / 4.1 = $15/$75 (старший тариф). Opus 4.0 приходит как
    // date-id `claude-opus-4-20250514` или голым алиасом — обе формы матчим ЯВНО: подстрока
    // "opus-4-0" их НЕ ловит (после `opus-4-` идёт `2`), падали бы в дефолт $5 = недосписание втрое.
    if m == "claude-opus-5"
        || m.contains("opus-4-8")
        || m.contains("opus-4-7")
        || m.contains("opus-4-6")
        || m.contains("opus-4-5")
    {
        return Some(("anthropic/standard/opus-current", OPUS_CURRENT_PRICES)); // $5 / 0.50 / 6.25 / 10 / 25
    }
    if m.contains("opus-4-1")
        || m == "claude-opus-4-20250514"
        || m == "claude-opus-4"
        || m.contains("opus-3")
        || m.contains("3-opus")
    {
        return Some(("anthropic/standard/opus-legacy", OPUS_LEGACY_PRICES)); // $15 / 1.50 / 18.75 / 30 / 75
    }
    // Sonnet 5: introductory $2/$10 through 2026-08-31, then the standard Sonnet tariff.
    // Match only "sonnet-5" so IDs such as sonnet-4-6 are never date-gated accidentally.
    if m.contains("sonnet-5") {
        return Some(if now_unix < SONNET5_STD_START {
            ("anthropic/standard/sonnet-5-intro", SONNET5_INTRO_PRICES)
        } else {
            ("anthropic/standard/sonnet-current", SONNET_STANDARD_PRICES)
        });
    }
    // Earlier Sonnet generations keep their historical standard $3/$15 tariff.
    if m.contains("sonnet") {
        return Some(("anthropic/standard/sonnet-current", SONNET_STANDARD_PRICES));
    }
    // Haiku тарифицируется ПОКОЛЕННО (цены росли): 4.5 = $1, 3.5 = $0.80, 3 = $0.25. Обе схемы
    // именования (haiku-3-5 и 3-5-haiku). "3-5" проверяем ДО общего Haiku 3 (подстроки пересекаются).
    if m.contains("haiku") {
        if m.contains("haiku-4-5") {
            return Some(("anthropic/standard/haiku-4-5", HAIKU_45_PRICES)); // $1 / 0.10 / 1.25 / 2 / 5
        }
        if m.contains("haiku-3-5") || m.contains("3-5-haiku") {
            return Some(("anthropic/standard/haiku-3-5", HAIKU_35_PRICES)); // $0.80 / 0.08 / 1.0 / 1.60 / 4
        }
        return Some(("anthropic/standard/haiku-3", HAIKU_3_PRICES)); // Haiku 3 = $0.25 / 0.025 / 0.3125→313 / 0.50 / 1.25
    }
    None // не распознано (дефолт решают вызывающие)
}

/// The tariff family key and prices of the heuristic branch that prices `model`, mirroring
/// `model_prices_for_speed_at` (standard path = `model_prices_matched_at`, fast path =
/// opus-current / opus-4-7-conservative).
///
/// Reserve/charge use this to know WHICH compiled tariff family a resolution used, so a hot
/// override row in `pricing_tariff_overrides` can target exactly that family. A model string no
/// branch recognizes returns `None` on the standard path — callers keep their existing
/// conservative `MAX_PRICES` fallback and simply have no family to override. The fast path always
/// matches, exactly like `model_prices_for_speed_at`, because its conservative fallback is itself
/// a published tariff family.
pub fn anthropic_matched_tariff_at(
    model: &str,
    now_unix: i64,
    fast: bool,
) -> Option<(&'static str, Prices)> {
    if !fast {
        return anthropic_tariff_branch_at(model, now_unix);
    }
    let m = model.to_ascii_lowercase();
    if m == "claude-opus-5" || m.contains("opus-4-8") {
        Some(("anthropic/fast/opus-current", FAST_OPUS_CURRENT_PRICES))
    } else {
        // Opus 4.7 has the highest published fast tariff. It is also the conservative fallback for
        // an unknown model that nevertheless reports speed=fast.
        Some(("anthropic/fast/opus-4-7-conservative", FAST_OPUS_47_PRICES))
    }
}

/// Every compiled Anthropic tariff family with its compiled price vector as of `now_unix`.
///
/// This is the seeding/diff inventory behind the hot tariff override surface: the entries are
/// the same named constants the heuristic branch matcher returns, so an enumerated price can
/// never diverge from the price the matcher bills (the regression matrix asserts exactly that).
///
/// `anthropic/standard/sonnet-5-intro` is time-bounded: it is included only while
/// `now_unix < SONNET5_STD_START`. After the compiled epoch flip the intro family is dead — the
/// matcher never emits it again — so seeding skips it instead of publishing a vector no
/// resolution can still produce. The flip itself is compiled; once hot overrides drive pricing,
/// the same epoch change is published as a normal new override version of
/// `anthropic/standard/sonnet-current`.
pub fn anthropic_compiled_tariffs_at(now_unix: i64) -> Vec<(&'static str, Prices)> {
    let mut tariffs = vec![
        ("anthropic/standard/fable-5", FABLE_5_PRICES),
        ("anthropic/standard/opus-current", OPUS_CURRENT_PRICES),
        ("anthropic/standard/opus-legacy", OPUS_LEGACY_PRICES),
        ("anthropic/standard/sonnet-current", SONNET_STANDARD_PRICES),
        ("anthropic/standard/haiku-4-5", HAIKU_45_PRICES),
        ("anthropic/standard/haiku-3-5", HAIKU_35_PRICES),
        ("anthropic/standard/haiku-3", HAIKU_3_PRICES),
        ("anthropic/fast/opus-current", FAST_OPUS_CURRENT_PRICES),
        ("anthropic/fast/opus-4-7-conservative", FAST_OPUS_47_PRICES),
    ];
    if now_unix < SONNET5_STD_START {
        tariffs.push(("anthropic/standard/sonnet-5-intro", SONNET5_INTRO_PRICES));
    }
    tariffs
}

/// Стоимость usage в **нанодолларах** (i128 — переполнения исключены). Каждая корзина
/// умножается на СВОЮ цену — ни один токен не выпадает.
pub fn cost_nanodollars(u: &Usage, p: &Prices) -> i128 {
    (u.input_tokens as i128) * p.input
        + (u.output_tokens as i128) * p.output
        + (u.cache_read_tokens as i128) * p.cache_read
        + (u.cache_write_5m_tokens as i128) * p.cache_write_5m
        + (u.cache_write_1h_tokens as i128) * p.cache_write_1h
        + (u.web_search_requests as i128) * WEB_SEARCH_NANO
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CostBreakdown {
    pub input: i128,
    pub output: i128,
    pub cache_read: i128,
    pub cache_write_5m: i128,
    pub cache_write_1h: i128,
    pub web_search: i128,
}

impl CostBreakdown {
    pub fn total(self) -> i128 {
        self.input
            + self.output
            + self.cache_read
            + self.cache_write_5m
            + self.cache_write_1h
            + self.web_search
    }

    pub fn apply_token_multiplier(self, basis_points: i64) -> Self {
        Self {
            input: apply_multiplier(self.input, basis_points),
            output: apply_multiplier(self.output, basis_points),
            cache_read: apply_multiplier(self.cache_read, basis_points),
            cache_write_5m: apply_multiplier(self.cache_write_5m, basis_points),
            cache_write_1h: apply_multiplier(self.cache_write_1h, basis_points),
            web_search: self.web_search,
        }
    }
}

pub fn cost_breakdown(u: &Usage, p: &Prices) -> CostBreakdown {
    CostBreakdown {
        input: (u.input_tokens as i128) * p.input,
        output: (u.output_tokens as i128) * p.output,
        cache_read: (u.cache_read_tokens as i128) * p.cache_read,
        cache_write_5m: (u.cache_write_5m_tokens as i128) * p.cache_write_5m,
        cache_write_1h: (u.cache_write_1h_tokens as i128) * p.cache_write_1h,
        web_search: (u.web_search_requests as i128) * WEB_SEARCH_NANO,
    }
}

/// Овердрафт-буфер: funded-запрос НИКОГДА не роняем 402 из-за гонки конкурентных резервов — баланс
/// аккаунта может на миг уйти в лёгкий минус до этого пола, settle сведёт фактикой (деньги на аккаунте
/// есть — тупой обрыв запроса хуже, чем −$1 просадка). Атомарный резерв (registry) держит пол баланса
/// на −$1 по построению; settle-кап (forward/meter) не даёт одному запросу списать больше hold+$1.
/// registry держит собственную i64-копию этого значения (крейт не зависит от metering) — СИНХРОНИТЬ.
pub const OVERDRAFT_NANO: i128 = 1_000_000_000; // $1

/// Применить множитель-наценку. `mult_bp` — множитель × 10000 (базисные пункты):
/// x1.0 = 10000, x2.78 = 27800. Округление до ближайшего нанодоллара (банковское — half-up).
pub fn apply_multiplier(nano: i128, mult_bp: i64) -> i128 {
    let m = mult_bp as i128;
    if nano >= 0 {
        (nano * m + 5000) / 10000
    } else {
        (nano * m - 5000) / 10000
    }
}

/// Release-v2 customer debit: точный контрактный floor — `floor(nano * mult_bp / 10000)`
/// (truncating i128-деление; release-v2 суммы неотрицательны по построению). Резерв
/// (`registry::pricing::release_v2`) уже floor'ит hold — финальный settlement обязан
/// совпасть с ним по формуле, иначе charged расходится с контрактом на ±1 нанодоллар.
/// Legacy scalar/strict пути НЕ используют этот хелпер: их half-up — immutable история,
/// которую registry повторно навязывает при legacy settlement.
pub fn apply_multiplier_floor(nano: i128, mult_bp: i64) -> i128 {
    nano * (mult_bp as i128) / 10000
}

/// Итоговая стоимость запроса с наценкой (нанодоллары).
pub fn cost_with_multiplier(u: &Usage, model: &str, mult_bp: i64) -> i128 {
    apply_multiplier(cost_nanodollars(u, &model_prices(model)), mult_bp)
}

/// Нанодоллары → строка "$X.XXXXXX" (6 знаков) для человека/лога.
pub fn nano_to_usd_string(nano: i128) -> String {
    let neg = nano < 0;
    let n = nano.unsigned_abs();
    let whole = n / NANO_PER_USD as u128;
    let micro = (n % NANO_PER_USD as u128) / 1000; // 6 знаков после точки
    format!("{}${}.{:06}", if neg { "-" } else { "" }, whole, micro)
}

// ── парсинг usage ────────────────────────────────────────────────────────────

/// Из блока `usage` (объект). Работает и для обычного ответа, и для message_start/-delta.
pub fn usage_from_value(u: &Value) -> Usage {
    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    // запись кэша: сначала точный сплит 5м/1ч; если его нет, а суммарный есть — всё в 5м.
    let split = u.get("cache_creation");
    let cw5_split = split
        .and_then(|c| c.get("ephemeral_5m_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cw1_split = split
        .and_then(|c| c.get("ephemeral_1h_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cw_total = g("cache_creation_input_tokens");
    let (cw5, cw1) = if cw5_split + cw1_split == 0 && cw_total > 0 {
        (cw_total, 0)
    } else {
        (cw5_split, cw1_split)
    };
    let web = u
        .get("server_tool_use")
        .and_then(|s| s.get("web_search_requests"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Usage {
        input_tokens: g("input_tokens"),
        output_tokens: g("output_tokens"),
        cache_read_tokens: g("cache_read_input_tokens"),
        cache_write_5m_tokens: cw5,
        cache_write_1h_tokens: cw1,
        web_search_requests: web,
    }
}

/// Был ли в SSE-потоке `error`-евент (Anthropic шлёт `event: error` / `{"type":"error"}` ПОСЛЕ 200,
/// напр. overloaded посреди генерации). HTTP-статус этого не отражает — детект по телу, чтобы такой
/// сбой не оставался «тихим» (логируется в forward, ротация уже невозможна — стрим пошёл).
pub fn sse_has_error(sse: &str) -> bool {
    for raw in sse.lines() {
        let line = raw.trim_start();
        let json = match line.strip_prefix("data:") {
            Some(r) => r.trim(),
            None => continue,
        };
        if json.is_empty() || json == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(json) {
            if v.get("type").and_then(Value::as_str) == Some("error") {
                return true;
            }
        }
    }
    false
}

/// Из полного JSON-ответа (не-стрим): берём объект `usage`.
pub fn usage_from_response_json(body: &[u8]) -> Usage {
    match serde_json::from_slice::<Value>(body) {
        Ok(v) => v.get("usage").map(usage_from_value).unwrap_or_default(),
        Err(_) => Usage::default(),
    }
}

pub fn speed_from_response_json(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()?
        .get("usage")?
        .get("speed")?
        .as_str()
        .map(str::to_string)
}

/// Модель из НЕ-стримового ответа (`model` верхнего уровня) — авторитетный сервёный id.
/// Тарификация по нему точнее, чем по запросу: клиент мог прислать алиас/`-latest`, апстрим
/// резолвит в конкретную датированную модель, и именно она возвращается в теле ответа.
pub fn model_from_response_json(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()?
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Модель из SSE-потока (`message_start.message.model`) — авторитетный сервёный id (см. выше).
pub fn model_from_sse(sse: &str) -> Option<String> {
    for raw in sse.lines() {
        let json = match raw.trim_start().strip_prefix("data:") {
            Some(r) => r.trim(),
            None => continue,
        };
        if json.is_empty() || json == "[DONE]" {
            continue;
        }
        let v: Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(m) = v
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(Value::as_str)
        {
            return Some(m.to_string());
        }
    }
    None
}

pub fn speed_from_sse(sse: &str) -> Option<String> {
    for raw in sse.lines() {
        let json = match raw.trim_start().strip_prefix("data:") {
            Some(raw) => raw.trim(),
            None => continue,
        };
        let value: Value = match serde_json::from_str(json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let usage = match value.get("type").and_then(Value::as_str) {
            Some("message_start") => value
                .get("message")
                .and_then(|message| message.get("usage")),
            Some("message_delta") => value.get("usage"),
            _ => None,
        };
        if let Some(speed) = usage
            .and_then(|usage| usage.get("speed"))
            .and_then(Value::as_str)
        {
            return Some(speed.to_string());
        }
    }
    None
}

/// Из накопленного SSE-потока. input/cache — из `message_start`; output — из ПОСЛЕДНЕГО
/// `message_delta` (там output_tokens кумулятивный). web-search — из delta, если есть.
pub fn usage_from_sse(sse: &str) -> Usage {
    let mut u = Usage::default();
    let mut have_output_usage = false;
    let mut delta_bytes: u64 = 0;
    let mut observed_web_search_requests: u64 = 0;
    for raw in sse.lines() {
        let line = raw.trim_start();
        let json = match line.strip_prefix("data:") {
            Some(r) => r.trim(),
            None => continue,
        };
        if json.is_empty() || json == "[DONE]" {
            continue;
        }
        let v: Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let block = v.get("content_block");
                if block.and_then(|b| b.get("type")).and_then(Value::as_str)
                    == Some("server_tool_use")
                    && block.and_then(|b| b.get("name")).and_then(Value::as_str)
                        == Some("web_search")
                {
                    observed_web_search_requests = observed_web_search_requests.saturating_add(1);
                }
            }
            Some("content_block_delta") => {
                // При обрыве до финального usage байты UTF-8 — консервативная верхняя граница числа
                // byte-level токенов. В отличие от chars/4 она не недосчитывает CJK/emoji.
                if let Some(d) = v.get("delta") {
                    for f in ["text", "thinking", "partial_json"] {
                        if let Some(t) = d.get(f).and_then(Value::as_str) {
                            delta_bytes = delta_bytes.saturating_add(t.len() as u64);
                        }
                    }
                }
            }
            Some("message_start") => {
                if let Some(mu) = v.get("message").and_then(|m| m.get("usage")) {
                    let s = usage_from_value(mu);
                    u.input_tokens = s.input_tokens;
                    u.cache_read_tokens = s.cache_read_tokens;
                    u.cache_write_5m_tokens = s.cache_write_5m_tokens;
                    u.cache_write_1h_tokens = s.cache_write_1h_tokens;
                    u.output_tokens = s.output_tokens; // обычно 1 — перезапишется delta
                    if s.web_search_requests > 0 {
                        u.web_search_requests = s.web_search_requests;
                    }
                }
            }
            Some("message_delta") => {
                if let Some(du) = v.get("usage") {
                    if let Some(o) = du.get("output_tokens").and_then(Value::as_u64) {
                        u.output_tokens = o; // кумулятивный — держим последний
                        have_output_usage = true;
                    }
                    if let Some(w) = du
                        .get("server_tool_use")
                        .and_then(|s| s.get("web_search_requests"))
                        .and_then(Value::as_u64)
                    {
                        u.web_search_requests = w;
                    }
                }
            }
            _ => {}
        }
    }
    u.web_search_requests = u.web_search_requests.max(observed_web_search_requests);
    if !have_output_usage {
        u.output_tokens = u.output_tokens.max(delta_bytes);
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── цены: 1M токенов каждой корзины = точная официальная ставка ────────────
    #[test]
    fn prices_exact_per_million() {
        let opus = model_prices("claude-opus-5");
        // 1M output Opus = ровно $25.00
        let u = Usage {
            output_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(cost_nanodollars(&u, &opus), 25 * NANO_PER_USD);
        // 1M input Opus = $5.00
        let u = Usage {
            input_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(cost_nanodollars(&u, &opus), 5 * NANO_PER_USD);
        // 1M cache_write_5m Opus = $6.25
        let u = Usage {
            cache_write_5m_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(cost_nanodollars(&u, &opus), 6_250_000_000);
        // 1M cache_read Opus = $0.50
        let u = Usage {
            cache_read_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(cost_nanodollars(&u, &opus), 500_000_000);
        // Haiku 4.5: 1M output = $5.00, 1M input = $1.00
        let hk = model_prices("claude-haiku-4-5-20251001");
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    output_tokens: 1_000_000,
                    ..Default::default()
                },
                &hk
            ),
            5 * NANO_PER_USD
        );
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    input_tokens: 1_000_000,
                    ..Default::default()
                },
                &hk
            ),
            NANO_PER_USD
        );
        // Haiku 3.5 cache_read $0.08/Mtoken
        let h35 = model_prices("claude-haiku-3-5");
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    cache_read_tokens: 1_000_000,
                    ..Default::default()
                },
                &h35
            ),
            80_000_000
        );
        // Sonnet 5: intro 1M output $10 before cutoff, standard $15 from cutoff onward.
        let sonnet5_intro = model_prices_at("claude-sonnet-5", SONNET5_STD_START - 1);
        let sonnet5_standard = model_prices_at("claude-sonnet-5", SONNET5_STD_START);
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    output_tokens: 1_000_000,
                    ..Default::default()
                },
                &sonnet5_intro
            ),
            10 * NANO_PER_USD
        );
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    output_tokens: 1_000_000,
                    ..Default::default()
                },
                &sonnet5_standard
            ),
            15 * NANO_PER_USD
        );
        // Fable 5 / Mythos 5: 1M output = $50, 1M input = $10 (НЕ дефолт Opus)
        let fable = model_prices("claude-fable-5");
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    output_tokens: 1_000_000,
                    ..Default::default()
                },
                &fable
            ),
            50 * NANO_PER_USD
        );
        assert_eq!(
            cost_nanodollars(
                &Usage {
                    input_tokens: 1_000_000,
                    ..Default::default()
                },
                &fable
            ),
            10 * NANO_PER_USD
        );
        assert_eq!(model_prices("claude-mythos-5"), fable);
        assert_ne!(fable, opus); // не свалилось в дефолт
                                 // неизвестная будущая модель → консервативный MAX, а не дешёвый исторический fallback
        assert_eq!(model_prices("что-то-непонятное"), MAX_PRICES);
    }

    #[test]
    fn sonnet5_intro_pricing_cutoff_boundary() {
        let intro = Prices {
            input: 2000,
            cache_read: 200,
            cache_write_5m: 2500,
            cache_write_1h: 4000,
            output: 10000,
        };
        let standard = Prices {
            input: 3000,
            cache_read: 300,
            cache_write_5m: 3750,
            cache_write_1h: 6000,
            output: 15000,
        };

        assert_eq!(
            model_prices_at("claude-sonnet-5", SONNET5_STD_START - 1),
            intro
        );
        assert_eq!(
            model_prices_at("claude-sonnet-5", SONNET5_STD_START),
            standard
        );
        assert_eq!(
            model_prices_reserve_at("claude-sonnet-5", SONNET5_STD_START - 1),
            intro
        );
        assert_eq!(
            model_prices_reserve_at("claude-sonnet-5", SONNET5_STD_START),
            standard
        );
        assert_eq!(model_prices("claude-sonnet-5"), standard);
        assert_eq!(model_prices_reserve("claude-sonnet-5"), standard);
        assert_eq!(
            model_prices_at("claude-sonnet-4-6", SONNET5_STD_START - 1),
            standard
        );
    }

    #[test]
    fn anthropic_tariff_capability_has_golden_identity_for_each_audited_model() {
        for (model, standard_schedule, standard_effective_from, fast_schedule) in [
            (
                "claude-opus-4-8",
                "anthropic/standard/opus-current/v1",
                0,
                "anthropic/fast/opus-current/v1",
            ),
            (
                "claude-opus-4-7",
                "anthropic/standard/opus-current/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-opus-4-6",
                "anthropic/standard/opus-current/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-opus-4-5",
                "anthropic/standard/opus-current/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-sonnet-4-5",
                "anthropic/standard/sonnet-current/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-sonnet-5",
                "anthropic/standard/sonnet-current/v1",
                SONNET5_STD_START,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-sonnet-4-6",
                "anthropic/standard/sonnet-current/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-haiku-4-5",
                "anthropic/standard/haiku-4-5/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
            (
                "claude-opus-5",
                "anthropic/standard/opus-current/v1",
                0,
                "anthropic/fast/opus-current/v1",
            ),
            (
                "claude-fable-5",
                "anthropic/standard/fable-5/v1",
                0,
                "anthropic/fast/opus-4-7-conservative/v1",
            ),
        ] {
            let standard = anthropic_tariff_capability_at(
                model,
                SONNET5_STD_START,
                AnthropicAdmissionModifiers {
                    speed: AnthropicSpeed::Standard,
                    inference_geo: AnthropicInferenceGeo::Global,
                },
            )
            .unwrap();
            assert_eq!(standard.canonical_model_id, model);
            assert_eq!(standard.alias_generation, ANTHROPIC_ALIAS_GENERATION);
            assert_eq!(standard.tariff_schedule_id.as_str(), standard_schedule);
            assert_eq!(standard.schedule_effective_from, standard_effective_from);

            let fast = anthropic_tariff_capability_at(
                model,
                SONNET5_STD_START,
                AnthropicAdmissionModifiers {
                    speed: AnthropicSpeed::Fast,
                    inference_geo: AnthropicInferenceGeo::Global,
                },
            )
            .unwrap();
            assert_eq!(fast.canonical_model_id, model);
            assert_eq!(fast.alias_generation, ANTHROPIC_ALIAS_GENERATION);
            assert_eq!(fast.tariff_schedule_id.as_str(), fast_schedule);
            assert_eq!(fast.schedule_effective_from, 0);
        }
    }

    #[test]
    fn anthropic_tariff_identity_pins_epoch_speed_and_geography() {
        let global_standard = AnthropicAdmissionModifiers {
            speed: AnthropicSpeed::Standard,
            inference_geo: AnthropicInferenceGeo::Global,
        };
        let intro = anthropic_tariff_capability_at(
            "claude-sonnet-5",
            SONNET5_STD_START - 1,
            global_standard,
        )
        .unwrap();
        let standard =
            anthropic_tariff_capability_at("claude-sonnet-5", SONNET5_STD_START, global_standard)
                .unwrap();
        assert_eq!(
            intro.tariff_schedule_id.as_str(),
            "anthropic/standard/sonnet-5-intro/v1"
        );
        assert_eq!(intro.schedule_effective_from, 0);
        assert_eq!(
            standard.tariff_schedule_id.as_str(),
            "anthropic/standard/sonnet-current/v1"
        );
        assert_eq!(standard.schedule_effective_from, SONNET5_STD_START);
        assert_ne!(
            intro.effective_reserve_prices,
            standard.effective_reserve_prices
        );

        let fast = anthropic_tariff_capability_at(
            "claude-opus-4-8",
            SONNET5_STD_START,
            AnthropicAdmissionModifiers {
                speed: AnthropicSpeed::Fast,
                inference_geo: AnthropicInferenceGeo::Global,
            },
        )
        .unwrap();
        assert_eq!(
            fast.tariff_schedule_id.as_str(),
            "anthropic/fast/opus-current/v1"
        );
        assert_eq!(fast.modifiers.speed, AnthropicSpeed::Fast);
        assert_eq!(
            fast.effective_reserve_prices,
            model_prices_reserve_for_speed_at("claude-opus-4-8", SONNET5_STD_START, true)
        );

        let us = anthropic_tariff_capability_at(
            "claude-opus-4-8",
            SONNET5_STD_START,
            AnthropicAdmissionModifiers {
                speed: AnthropicSpeed::Fast,
                inference_geo: AnthropicInferenceGeo::Us,
            },
        )
        .unwrap();
        assert_eq!(us.modifiers.inference_geo, AnthropicInferenceGeo::Us);
        assert_eq!(us.modifiers.inference_geo_basis_points(), 11_000);
        assert_eq!(
            us.effective_reserve_prices,
            premium_prices_ceil(fast.effective_reserve_prices, 11_000)
        );

        let web_only = Usage {
            web_search_requests: 1,
            ..Default::default()
        };
        assert_eq!(
            cost_nanodollars(&web_only, &fast.effective_reserve_prices),
            cost_nanodollars(&web_only, &us.effective_reserve_prices)
        );
    }

    #[test]
    fn anthropic_tariff_identity_does_not_promote_legacy_fallbacks() {
        for model in [
            "",
            "CLAUDE-OPUS-4-8",
            "claude-opus-4-8-latest",
            "claude-opus-4-8-20260701",
            "claude-opus-4-8-preview",
            "claude-opus-4-8-2026070x",
            "claude-opus-5-1",
            "claude-opus-5-latest",
            "claude-fable-5-1",
            "CLAUDE-FABLE-5",
            "claude-3-5-sonnet-20241022",
            "anything-sonnet-anything",
        ] {
            assert_eq!(
                anthropic_tariff_capability_at(
                    model,
                    SONNET5_STD_START,
                    AnthropicAdmissionModifiers {
                        speed: AnthropicSpeed::Standard,
                        inference_geo: AnthropicInferenceGeo::Global,
                    },
                ),
                Err(TariffIdentityError::UnsupportedModelIdentity),
                "{model}",
            );
        }

        for priced_ts in [i64::MIN, -1, 0] {
            assert_eq!(
                anthropic_tariff_capability_at(
                    "claude-sonnet-5",
                    priced_ts,
                    AnthropicAdmissionModifiers {
                        speed: AnthropicSpeed::Standard,
                        inference_geo: AnthropicInferenceGeo::Global,
                    },
                ),
                Err(TariffIdentityError::InvalidPricedTimestamp)
            );
        }
        // The dormant API does not alter the legacy conservative pricing behavior.
        assert_eq!(model_prices("что-то-непонятное"), MAX_PRICES);
        assert_eq!(
            model_prices("claude-3-5-sonnet-20241022"),
            SONNET_STANDARD_PRICES
        );
    }

    #[test]
    fn anthropic_tariff_identity_matches_live_reserve_chain_for_every_modifier() {
        for model in [
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "claude-opus-5",
            "claude-fable-5",
        ] {
            for fast in [false, true] {
                for us_inference in [false, true] {
                    let modifiers = AnthropicAdmissionModifiers {
                        speed: if fast {
                            AnthropicSpeed::Fast
                        } else {
                            AnthropicSpeed::Standard
                        },
                        inference_geo: if us_inference {
                            AnthropicInferenceGeo::Us
                        } else {
                            AnthropicInferenceGeo::Global
                        },
                    };
                    let identity =
                        anthropic_tariff_capability_at(model, SONNET5_STD_START, modifiers)
                            .unwrap();
                    let base = model_prices_reserve_for_speed_at(model, SONNET5_STD_START, fast);
                    let expected = if us_inference {
                        premium_prices_ceil(base, 11_000)
                    } else {
                        base
                    };
                    assert_eq!(identity.effective_reserve_prices, expected, "{model}");
                }
            }
        }
    }

    #[test]
    fn dated_haiku_alias_maps_to_the_bare_canonical_identity() {
        for fast in [false, true] {
            for us_inference in [false, true] {
                let modifiers = AnthropicAdmissionModifiers {
                    speed: if fast {
                        AnthropicSpeed::Fast
                    } else {
                        AnthropicSpeed::Standard
                    },
                    inference_geo: if us_inference {
                        AnthropicInferenceGeo::Us
                    } else {
                        AnthropicInferenceGeo::Global
                    },
                };
                let bare =
                    anthropic_tariff_capability_at("claude-haiku-4-5", SONNET5_STD_START, modifiers)
                        .unwrap();
                let dated = anthropic_tariff_capability_at(
                    "claude-haiku-4-5-20251001",
                    SONNET5_STD_START,
                    modifiers,
                )
                .unwrap();
                assert_eq!(bare, dated);
                assert_eq!(dated.canonical_model_id, "claude-haiku-4-5");
            }
        }
    }

    #[test]
    fn model_matcher_covers_real_ids() {
        let out1m = |m: &str| {
            cost_nanodollars(
                &Usage {
                    output_tokens: 1_000_000,
                    ..Default::default()
                },
                &model_prices(m),
            )
        };
        // Opus 4.0 date-id и голый алиас = $75/M (НЕ дефолт $25) — иначе недосписание втрое.
        assert_eq!(out1m("claude-opus-4-20250514"), 75 * NANO_PER_USD);
        assert_eq!(out1m("claude-opus-4"), 75 * NANO_PER_USD);
        assert_eq!(out1m("claude-opus-4-1-20250805"), 75 * NANO_PER_USD);
        assert_eq!(out1m("claude-3-opus-20240229"), 75 * NANO_PER_USD);
        // Opus 4.5–5 = $25/M.
        assert_eq!(out1m("claude-opus-5"), 25 * NANO_PER_USD);
        assert_eq!(out1m("claude-opus-4-8"), 25 * NANO_PER_USD);
        assert_eq!(out1m("claude-opus-4-5"), 25 * NANO_PER_USD);
        // Sonnet любого поколения = $15/M (обе схемы именования)
        for s in [
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-3-5-sonnet-20241022",
            "claude-3-7-sonnet-20250219",
            "claude-3-sonnet-20240229",
        ] {
            assert_eq!(out1m(s), 15 * NANO_PER_USD, "sonnet {s}");
        }
        // Haiku поколенно: 4.5=$5, 3.5=$4, 3=$1.25
        assert_eq!(out1m("claude-haiku-4-5-20251001"), 5 * NANO_PER_USD);
        assert_eq!(out1m("claude-3-5-haiku-20241022"), 4 * NANO_PER_USD); // альт-схема, НЕ дефолт
        assert_eq!(out1m("claude-3-haiku-20240307"), 1_250_000_000); // $1.25
    }

    #[test]
    fn reserve_prices_cover_any_served_model() {
        let out = |pr: &Prices| pr.output;
        // распознанная модель: цена РЕЗЕРВА == цене СПИСАНИЯ (апстрим отдаст тот же тариф → charge≤hold)
        for mdl in [
            "claude-haiku-4-5",
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-fable-5",
            "claude-opus-4-20250514",
            "claude-3-5-haiku-20241022",
        ] {
            assert_eq!(
                model_prices_reserve(mdl),
                model_prices(mdl),
                "recognized {mdl}"
            );
        }
        // НЕраспознанная строка (алиас/опечатка): резерв по MAX-тарифу (≥ ЛЮБОГО реального тарифа),
        // списание тоже использует MAX, пока ответная модель неизвестна. Сценарий Находки 1:
        let unknown = model_prices_reserve("claude-opus-4-0"); // request-matcher НЕ ловит
        assert_eq!(out(&unknown), 75000, "нераспознанное → MAX output $75");
        // MAX ≥ output ЛЮБОЙ известной модели (иначе резерв не покрыл бы charge)
        for mdl in [
            "claude-fable-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-20250514",
            "claude-sonnet-5",
            "claude-haiku-4-5",
        ] {
            assert!(out(&unknown) >= out(&model_prices(mdl)), "MAX ≥ {mdl}");
            assert!(unknown.cache_write_1h >= model_prices(mdl).cache_write_1h);
            assert!(unknown.input >= model_prices(mdl).input);
        }
        // СПИСАНИЕ неизвестного тоже fail-safe: не недобираем цену будущей дорогой модели.
        assert_eq!(model_prices("claude-opus-4-0").output, 75000);
        assert_eq!(model_prices("claude-opus-5-1").output, 75000);
    }

    #[test]
    fn opus5_fast_mode_uses_published_tariff() {
        let standard = model_prices_for_speed_at("claude-opus-5", SONNET5_STD_START, false);
        let fast = model_prices_for_speed_at("claude-opus-5", SONNET5_STD_START, true);

        assert_eq!(standard, OPUS_CURRENT_PRICES);
        assert_eq!(fast, FAST_OPUS_CURRENT_PRICES);
        assert_eq!(fast.input, 10_000);
        assert_eq!(fast.output, 50_000);
        assert_eq!(
            model_prices_reserve_for_speed_at("claude-opus-5", SONNET5_STD_START, true),
            fast
        );
    }

    #[test]
    fn model_extracted_from_response() {
        // не-стрим: top-level model
        let body = br#"{"model":"claude-opus-4-20250514","usage":{"output_tokens":1}}"#;
        assert_eq!(
            model_from_response_json(body).as_deref(),
            Some("claude-opus-4-20250514")
        );
        // SSE: message_start.message.model
        let sse = "event: message_start\n\
                   data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-haiku-4-5\",\"usage\":{\"input_tokens\":1}}}\n\n";
        assert_eq!(model_from_sse(sse).as_deref(), Some("claude-haiku-4-5"));
        // мусор → None (фолбэк на модель запроса в meter.rs)
        assert_eq!(model_from_response_json(b"not json"), None);
        assert_eq!(model_from_sse("event: ping\n\n"), None);
    }

    #[test]
    fn per_token_is_exact() {
        // 1 output-токен Opus = 25000 нано = $0.000025 — точно, без потерь
        let u = Usage {
            output_tokens: 1,
            ..Default::default()
        };
        assert_eq!(
            cost_nanodollars(&u, &model_prices("claude-opus-4-8")),
            25_000
        );
        // 3 токена → ровно 3×
        let u = Usage {
            output_tokens: 3,
            ..Default::default()
        };
        assert_eq!(
            cost_nanodollars(&u, &model_prices("claude-opus-4-8")),
            75_000
        );
    }

    #[test]
    fn full_usage_all_buckets_counted() {
        let u = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_5m_tokens: 100,
            cache_write_1h_tokens: 50,
            web_search_requests: 2,
        };
        let p = model_prices("claude-opus-4-8");
        // 1000*5000 + 500*25000 + 200*500 + 100*6250 + 50*10000 + 2*10_000_000
        let expect =
            1000 * 5000 + 500 * 25000 + 200 * 500 + 100 * 6250 + 50 * 10000 + 2 * 10_000_000;
        assert_eq!(cost_nanodollars(&u, &p), expect as i128);
        // сумма токенов = ровно все корзины
        assert_eq!(u.total_tokens(), 1850);
    }

    #[test]
    fn multiplier_exact() {
        assert_eq!(apply_multiplier(NANO_PER_USD, 10000), NANO_PER_USD);
        assert_eq!(apply_multiplier(NANO_PER_USD, 27800), 2_780_000_000);
        assert_eq!(apply_multiplier(NANO_PER_USD, 5000), 500_000_000);
        // Округление half-up.
        assert_eq!(apply_multiplier(3, 15000), 5); // 3*1.5=4.5→5
    }

    #[test]
    fn multiplier_floor_matches_the_release_v2_contract() {
        // Тождественность на ровных произведениях.
        assert_eq!(apply_multiplier_floor(NANO_PER_USD, 10000), NANO_PER_USD);
        assert_eq!(apply_multiplier_floor(NANO_PER_USD, 5000), 500_000_000);
        assert_eq!(apply_multiplier_floor(0, 5000), 0);
        // Контрактный floor: charged_nano = floor(official_nano * payable_multiplier_bp / 10000).
        assert_eq!(apply_multiplier_floor(3, 15000), 4); // 3*1.5=4.5→4 (half-up дал бы 5)
        // Граница half-up: остаток ровно 5000 не округляется вверх.
        assert_eq!(apply_multiplier_floor(5_000, 5_001), 2_500); // 25_005_000/10_000 = 2500.5→2500
        assert_eq!(apply_multiplier(5_000, 5_001), 2_501); // legacy half-up остаётся 2501
        // Максимальный payable-диапазон release-v2.
        assert_eq!(apply_multiplier_floor(9_999, 10_000), 9_999);
        assert_eq!(apply_multiplier_floor(9_999, 1), 0);
    }

    #[test]
    fn parse_json_with_cache_split() {
        let body = br#"{"id":"m","type":"message","usage":{
            "input_tokens":41,"output_tokens":10,"cache_read_input_tokens":7,
            "cache_creation_input_tokens":30,
            "cache_creation":{"ephemeral_5m_input_tokens":20,"ephemeral_1h_input_tokens":10}}}"#;
        let u = usage_from_response_json(body);
        assert_eq!(
            u,
            Usage {
                input_tokens: 41,
                output_tokens: 10,
                cache_read_tokens: 7,
                cache_write_5m_tokens: 20,
                cache_write_1h_tokens: 10,
                web_search_requests: 0
            }
        );
    }

    #[test]
    fn parse_json_cache_total_no_split_goes_to_5m() {
        let body =
            br#"{"usage":{"input_tokens":5,"output_tokens":3,"cache_creation_input_tokens":40}}"#;
        let u = usage_from_response_json(body);
        assert_eq!(u.cache_write_5m_tokens, 40);
        assert_eq!(u.cache_write_1h_tokens, 0);
    }

    #[test]
    fn parse_sse_input_from_start_output_from_last_delta() {
        let sse = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":50,\"cache_creation_input_tokens\":0,\"output_tokens\":1}}}\n\
\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{},\"usage\":{\"output_tokens\":37}}\n\
\n";
        let u = usage_from_sse(sse);
        // input/cache из message_start, output — ПОСЛЕДНИЙ (37), не потерян и не задвоен
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.cache_read_tokens, 50);
        // текст "hi" (2 симв) даёт floor 0 → honest ответ не переоценивается, остаётся 37
        assert_eq!(u.output_tokens, 37);
    }

    /// Анти-абьюз: клиент оборвал стрим ДО финального message_delta — output считаем по байтам
    /// отданного текста, а не по небезопасному chars/4 или «1 токену из message_start».
    #[test]
    fn sse_abort_before_message_delta_charges_by_utf8_bytes() {
        let text = "a".repeat(4000); // 4000 байт отданного текста, message_delta так и НЕ пришёл
        let sse = format!("\
event: message_start\n\
data: {{\"type\":\"message_start\",\"message\":{{\"usage\":{{\"input_tokens\":50,\"output_tokens\":1}}}}}}\n\
\n\
event: content_block_delta\n\
data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\
\n");
        let u = usage_from_sse(&sse);
        assert_eq!(u.input_tokens, 50);
        assert_eq!(
            u.output_tokens, 4000,
            "fallback charges the conservative UTF-8 byte bound"
        );
    }

    #[test]
    fn sse_abort_unicode_and_web_search_are_not_undercounted() {
        let text = "🙂".repeat(1000); // 4 UTF-8 bytes per scalar; chars/4 would charge only 250.
        let sse = format!("\
event: message_start\n\
data: {{\"type\":\"message_start\",\"message\":{{\"usage\":{{\"input_tokens\":10,\"output_tokens\":1}}}}}}\n\
\n\
event: content_block_start\n\
data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"server_tool_use\",\"id\":\"srvtoolu_1\",\"name\":\"web_search\"}}}}\n\
\n\
event: content_block_delta\n\
data: {{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\
\n");
        let u = usage_from_sse(&sse);
        assert_eq!(u.output_tokens, 4000);
        assert_eq!(u.web_search_requests, 1);
    }

    #[test]
    fn sse_error_event_detected() {
        let ok = "event: message_stop\ndata: {\"type\":\"message_stop\"}";
        assert!(!sse_has_error(ok));
        let err =
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\"}}";
        assert!(sse_has_error(err));
        assert!(!sse_has_error("")); // пусто — не ошибка
    }

    #[test]
    fn empty_or_malformed_is_zero_never_panics() {
        assert!(usage_from_response_json(b"not json").is_zero());
        assert!(usage_from_response_json(b"{}").is_zero());
        assert!(usage_from_sse("").is_zero());
        assert!(usage_from_sse("data: garbage\n\ndata: {\"type\":\"ping\"}").is_zero());
    }

    #[test]
    fn no_overflow_on_huge_counts() {
        // 10 млрд выходных токенов — i128 держит без переполнения
        let u = Usage {
            output_tokens: 10_000_000_000,
            ..Default::default()
        };
        let c = cost_nanodollars(&u, &model_prices("claude-opus-4-8"));
        assert_eq!(c, 10_000_000_000i128 * 25_000); // = $250 000
        assert_eq!(nano_to_usd_string(c), "$250000.000000");
    }

    #[test]
    fn client_pays_9_percent_of_real_api() {
        // Бизнес-правило владельца: клиент платит 9% от реального USD-эквивалента API (×0.09,
        // mult_bp=900), независимо от модели. Реальные $1000 API → клиент тратит ровно $90.
        let real = 1000i128 * NANO_PER_USD;
        assert_eq!(apply_multiplier(real, 900), 90 * NANO_PER_USD);
        // На реальном usage: 1M выходных токенов Opus = $25 API → клиент платит $2.25.
        let u = Usage {
            output_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(
            cost_with_multiplier(&u, "claude-opus-4-8", 900),
            2_250_000_000
        ); // $2.25
    }

    #[test]
    fn usd_string_format() {
        assert_eq!(nano_to_usd_string(25_000), "$0.000025");
        assert_eq!(nano_to_usd_string(2_780_000_000), "$2.780000");
        assert_eq!(nano_to_usd_string(0), "$0.000000");
    }

    // ── hot tariff family keys ───────────────────────────────────────────────

    #[test]
    fn matched_tariff_helper_mirrors_the_standard_matcher() {
        let cases: &[(&str, &str)] = &[
            ("claude-fable-5", "anthropic/standard/fable-5"),
            ("claude-mythos-5", "anthropic/standard/fable-5"),
            ("claude-opus-5", "anthropic/standard/opus-current"),
            ("claude-opus-4-8", "anthropic/standard/opus-current"),
            ("claude-opus-4-7", "anthropic/standard/opus-current"),
            ("claude-opus-4-6", "anthropic/standard/opus-current"),
            ("claude-opus-4-5", "anthropic/standard/opus-current"),
            ("claude-opus-4-1-20250805", "anthropic/standard/opus-legacy"),
            ("claude-opus-4-20250514", "anthropic/standard/opus-legacy"),
            ("claude-opus-4", "anthropic/standard/opus-legacy"),
            ("claude-3-opus-20240229", "anthropic/standard/opus-legacy"),
            ("claude-sonnet-4-6", "anthropic/standard/sonnet-current"),
            ("claude-sonnet-4-5", "anthropic/standard/sonnet-current"),
            ("claude-3-5-sonnet-20241022", "anthropic/standard/sonnet-current"),
            ("claude-haiku-4-5-20251001", "anthropic/standard/haiku-4-5"),
            ("claude-haiku-4-5", "anthropic/standard/haiku-4-5"),
            ("claude-haiku-3-5", "anthropic/standard/haiku-3-5"),
            ("claude-3-5-haiku-20241022", "anthropic/standard/haiku-3-5"),
            ("claude-3-haiku-20240307", "anthropic/standard/haiku-3"),
        ];
        for ts in [0, SONNET5_STD_START - 1, SONNET5_STD_START, i64::MAX] {
            for (model, family) in cases {
                let matched = model_prices_matched_at(model, ts).expect("recognized");
                let helper = anthropic_matched_tariff_at(model, ts, false).expect("recognized");
                assert_eq!(helper.0, *family, "{model} family");
                assert_eq!(helper.1, matched, "{model} prices");
                // The reserve/charge functions keep returning identical values.
                assert_eq!(model_prices_at(model, ts), matched, "{model} charge");
                assert_eq!(model_prices_reserve_at(model, ts), matched, "{model} reserve");
                assert_eq!(
                    model_prices_for_speed_at(model, ts, false),
                    matched,
                    "{model} standard speed path"
                );
            }
            // Sonnet 5 is the only time-gated family.
            let (family, prices) = anthropic_matched_tariff_at("claude-sonnet-5", ts, false).unwrap();
            assert_eq!(prices, model_prices_at("claude-sonnet-5", ts));
            assert_eq!(
                family,
                if ts < SONNET5_STD_START {
                    "anthropic/standard/sonnet-5-intro"
                } else {
                    "anthropic/standard/sonnet-current"
                },
                "sonnet-5 family at {ts}"
            );
        }
        // An unrecognized model has no branch and therefore no family; the legacy MAX fallback
        // of the price functions is unchanged.
        for model in ["", "claude-opus-4-0", "claude-opus-5-1", "непонятное"] {
            assert_eq!(
                anthropic_matched_tariff_at(model, SONNET5_STD_START, false),
                None,
                "{model}"
            );
            assert_eq!(
                model_prices_for_speed_at(model, SONNET5_STD_START, false),
                MAX_PRICES
            );
        }
    }

    #[test]
    fn matched_tariff_helper_mirrors_the_fast_path() {
        for ts in [0, SONNET5_STD_START, i64::MAX] {
            for model in [
                "claude-opus-5",
                "claude-opus-4-8",
                "claude-opus-4-7",
                "claude-sonnet-5",
                "claude-haiku-4-5",
                "claude-fable-5",
                "claude-opus-4-0", // unrecognized: fast still prices conservatively
                "непонятное",
            ] {
                let (family, prices) = anthropic_matched_tariff_at(model, ts, true)
                    .expect("the fast path always matches a published tariff family");
                assert_eq!(
                    prices,
                    model_prices_for_speed_at(model, ts, true),
                    "{model} fast prices"
                );
                let lowered = model.to_ascii_lowercase();
                let expected_family = if lowered == "claude-opus-5" || lowered.contains("opus-4-8")
                {
                    "anthropic/fast/opus-current"
                } else {
                    "anthropic/fast/opus-4-7-conservative"
                };
                assert_eq!(family, expected_family, "{model} fast family");
            }
        }
    }

    #[test]
    fn compiled_tariff_enumeration_covers_every_matcher_family_with_identical_prices() {
        let models = [
            "claude-fable-5",
            "claude-mythos-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-opus-4-5",
            "claude-opus-4-1-20250805",
            "claude-opus-4-20250514",
            "claude-3-opus-20240229",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-3-5-sonnet-20241022",
            "claude-haiku-4-5",
            "claude-haiku-3-5",
            "claude-3-5-haiku-20241022",
            "claude-3-haiku-20240307",
            "claude-opus-4-0", // unrecognized: only the fast path still prices it
        ];
        let mut emitted: std::collections::BTreeMap<&'static str, Prices> =
            std::collections::BTreeMap::new();
        for ts in [0, SONNET5_STD_START - 1, SONNET5_STD_START, i64::MAX] {
            let enumerated: std::collections::BTreeMap<&'static str, Prices> =
                anthropic_compiled_tariffs_at(ts).into_iter().collect();
            // The dead intro family is skipped after the compiled epoch flip, never before.
            assert_eq!(
                enumerated.contains_key("anthropic/standard/sonnet-5-intro"),
                ts < SONNET5_STD_START,
                "sonnet-5-intro presence at {ts}"
            );
            for model in models {
                for fast in [false, true] {
                    let Some((family, prices)) = anthropic_matched_tariff_at(model, ts, fast)
                    else {
                        continue;
                    };
                    assert_eq!(
                        enumerated.get(family),
                        Some(&prices),
                        "{model} fast={fast} family {family} at {ts} must enumerate identical prices"
                    );
                    emitted.insert(family, prices);
                }
            }
        }
        // Both directions: every family the enumeration can publish is reachable through the
        // matcher matrix above, so a seeded override never targets a family nothing resolves to.
        let full: std::collections::BTreeMap<&'static str, Prices> =
            anthropic_compiled_tariffs_at(0).into_iter().collect();
        assert_eq!(emitted, full, "enumeration and matcher families diverge");
    }
}
