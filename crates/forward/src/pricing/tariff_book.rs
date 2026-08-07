//! The process-wide hot tariff override book.
//!
//! `pricing_tariff_overrides` (registry authority, migrations 0036/0037) republishes a tariff
//! family's price vector as data: compiled `metering` constants are the implicit version 1 of a
//! family and every override row is version >= 2, append-only and immutable. This module is the
//! runtime consumer of that table. One process-global [`TariffBook`] holds the last good
//! digest-verified snapshot behind an `RwLock<Arc<..>>`; the composition layer (`server`) installs
//! the kill switch once at startup and spawns [`refresher_loop`], which re-reads the table through
//! the billing reader actor every few seconds and atomically swaps the snapshot.
//!
//! Resolution contract:
//!
//! - RESERVE resolves the compiled family via the `metering::*_matched_tariff_at` helpers, then
//!   asks the book for an override effective at the priced timestamp. An override replaces ONLY the
//!   base price vector — speed/geo/long-context premiums and multipliers stay code-applied on top —
//!   and the request pins `<family>/v<version>` so settlement replays exactly what admission held.
//!   No override (or an empty/unavailable book) pins nothing and behaves byte-identically to the
//!   compiled constants.
//! - CHARGE looks the pin up by EXACT version ([`TariffBookSnapshot::version_payload`]). A pinned
//!   version must exist: the table is append-only and the reserve that pinned it read the row from
//!   this process, so a miss is an integrity error, never a silent reprice at compiled. Async
//!   settlement paths get one bounded refresh retry ([`TariffBook::version_payload_refreshed`]);
//!   the synchronous Anthropic tee-meter cannot await and treats a miss as the same integrity
//!   error directly. A cross-family serve (requested X, upstream served Y) reprices by Y's family
//!   at the pinned priced timestamp, matching the compiled-code behaviour of repricing by the
//!   served model.
//! - The book never blocks a request and never fails one: an unreadable authority keeps the last
//!   good snapshot (startup: empty = compiled everywhere), and the kill switch
//!   (`CLAUDE_API_TARIFF_OVERRIDES`, default on) makes the book answer empty.
//!
//! Process-global state is the established pattern for fleet-wide money policy in this crate
//! (`settlement_policy`): one authority table, one snapshot, read on the hot path of every plane.
//! Metering stays pure: the `registry` mirror payload → `metering` price struct converters live
//! here and mirror `crates/server/src/tariff_admin.rs`, which converts in the opposite direction
//! (compiled constants → mirror payloads for seeding). The two directions are deliberate textual
//! duplicates — sharing them would couple the seeding surface to the settlement hot path.

use registry::pricing::{
    resolve_tariff_override, TariffOverride, TariffOverridePayload,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// How often the background refresher re-reads the override table. The table is tiny and
/// append-only, so a full re-read is the cheapest correct refresh.
pub const TARIFF_BOOK_REFRESH_INTERVAL_SECS: u64 = 5;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// One immutable, digest-verified snapshot of the override table with every payload pre-parsed
/// into its typed mirror. Building one fails closed: a row whose payload no longer parses (a
/// schema drift the registry digest check cannot catch) rejects the whole snapshot, so the
/// refresher keeps the last good one instead of silently dropping a family's override.
pub struct TariffBookSnapshot {
    rows: Vec<TariffOverride>,
    payloads: HashMap<(String, i64), TariffOverridePayload>,
    fetched_at: i64,
}

impl TariffBookSnapshot {
    /// The empty book: every resolution falls through to the compiled constants.
    pub fn empty() -> Arc<Self> {
        static EMPTY: OnceLock<Arc<TariffBookSnapshot>> = OnceLock::new();
        EMPTY
            .get_or_init(|| {
                Arc::new(TariffBookSnapshot {
                    rows: Vec::new(),
                    payloads: HashMap::new(),
                    fetched_at: 0,
                })
            })
            .clone()
    }

    /// Parse and index one full table read. Rows arrive digest-verified from
    /// `PgStore::list_tariff_overrides`; the typed re-parse keys every payload by its exact
    /// (family, version) so settlement never re-validates on the hot path.
    pub fn from_rows(rows: Vec<TariffOverride>) -> anyhow::Result<Arc<Self>> {
        let mut payloads = HashMap::with_capacity(rows.len());
        for row in &rows {
            let payload = registry::pricing::parse_tariff_override_payload(
                &row.tariff_family,
                &row.payload,
            )
            .map_err(|error| {
                anyhow::anyhow!(
                    "tariff override row {}/v{} failed typed parsing: {error:#}",
                    row.tariff_family,
                    row.version
                )
            })?;
            payloads.insert((row.tariff_family.clone(), row.version), payload);
        }
        Ok(Arc::new(TariffBookSnapshot {
            rows,
            payloads,
            fetched_at: now_unix(),
        }))
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// When this snapshot was read from the authority (0 for the empty startup book).
    pub fn fetched_at(&self) -> i64 {
        self.fetched_at
    }

    /// The override that prices `family` at `priced_ts` — the greatest version with
    /// `effective_from <= priced_ts` — with its parsed payload and pin identity.
    pub fn resolve(&self, family: &str, priced_ts: i64) -> Option<(PinnedTariff, TariffOverridePayload)> {
        let row = resolve_tariff_override(&self.rows, family, priced_ts)?;
        let payload = *self
            .payloads
            .get(&(row.tariff_family.clone(), row.version))?;
        Some((PinnedTariff::of(row), payload))
    }

    /// The exact pinned version's payload for settlement. `None` on a miss is an integrity
    /// signal for the caller, never a fallback to compiled prices: the table is append-only and
    /// the reserve that produced the pin read this row.
    pub fn version_payload(&self, family: &str, version: i64) -> Option<TariffOverridePayload> {
        self.payloads
            .get(&(family.to_owned(), version))
            .copied()
    }
}

/// The tariff version pinned at admission: which override priced the hold. Settlement replays
/// this exact version; the durable price snapshots carry `schedule_id` (`<family>/v<version>`,
/// extending the compiled `<…>/v1` convention) so the ledger row stays self-explanatory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedTariff {
    pub family: String,
    pub version: i64,
    pub schedule_id: String,
}

impl PinnedTariff {
    fn of(row: &TariffOverride) -> Self {
        PinnedTariff {
            family: row.tariff_family.clone(),
            version: row.version,
            schedule_id: format!("{}/v{}", row.tariff_family, row.version),
        }
    }
}

/// The process-global book. `enabled` is the kill switch, installed once by the composition
/// layer; a disabled book always answers with the empty snapshot.
pub struct TariffBook {
    enabled: AtomicBool,
    current: RwLock<Arc<TariffBookSnapshot>>,
}

impl TariffBook {
    pub fn new(enabled: bool) -> Self {
        TariffBook {
            enabled: AtomicBool::new(enabled),
            current: RwLock::new(TariffBookSnapshot::empty()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// The current snapshot. Never blocks beyond the lock, never fails: a disabled book and a
    /// never-refreshed book both answer empty (= compiled constants everywhere).
    pub fn snapshot(&self) -> Arc<TariffBookSnapshot> {
        if !self.is_enabled() {
            return TariffBookSnapshot::empty();
        }
        self.current
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| TariffBookSnapshot::empty())
    }

    /// Replace the snapshot with a fresh full-table read. A payload that fails typed parsing
    /// rejects the whole swap, keeping the last good snapshot.
    pub fn swap_rows(&self, rows: Vec<TariffOverride>) -> anyhow::Result<()> {
        let snapshot = TariffBookSnapshot::from_rows(rows)?;
        match self.current.write() {
            Ok(mut guard) => *guard = snapshot,
            Err(_) => anyhow::bail!("tariff book lock is poisoned"),
        }
        Ok(())
    }

    /// One bounded refresh straight from the billing reader actor.
    pub async fn refresh_once(&self, billing: &crate::billing::AsyncBilling) -> anyhow::Result<()> {
        let rows = billing.list_tariff_overrides().await?;
        self.swap_rows(rows)
    }

    /// Exact-version payload for settlement with one bounded refresh retry on a cache miss.
    /// `fetch` lists the current table (production: `AsyncBilling::list_tariff_overrides`);
    /// a refresh failure answers from the last good snapshot, and a still-missing pinned version
    /// returns `None` for the caller's integrity-error path — never a compiled fallback.
    pub async fn version_payload_refreshed<F, Fut>(
        &self,
        family: &str,
        version: i64,
        fetch: F,
    ) -> Option<TariffOverridePayload>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<Vec<TariffOverride>>>,
    {
        if let Some(payload) = self.snapshot().version_payload(family, version) {
            return Some(payload);
        }
        match fetch().await {
            Ok(rows) => {
                if let Err(error) = self.swap_rows(rows) {
                    elog::error(
                        "tariff-book",
                        format!("pinned tariff refresh was rejected: {error:#}"),
                    );
                    return None;
                }
                self.snapshot().version_payload(family, version)
            }
            Err(error) => {
                elog::warn(
                    "tariff-book",
                    format!("pinned tariff refresh failed, using the last good snapshot: {error:#}"),
                );
                None
            }
        }
    }
}

static TARIFF_BOOK: OnceLock<TariffBook> = OnceLock::new();

/// The process-global book. Uninstalled (tests, non-server binaries) it is enabled and empty,
/// which is exactly the compiled-constants behaviour.
fn book() -> &'static TariffBook {
    TARIFF_BOOK.get_or_init(|| TariffBook::new(true))
}

/// Install the kill switch once at startup; called by the composition layer from parsed config.
pub fn install(enabled: bool) {
    book().enabled.store(enabled, Ordering::Release);
}

/// The current process-wide snapshot for one request-edge resolution.
pub fn snapshot() -> Arc<TariffBookSnapshot> {
    book().snapshot()
}

/// Exact pinned-version payload for async settlement paths, with one bounded refresh retry
/// straight through the billing reader actor on a cache miss.
pub async fn version_payload_refreshed(
    billing: &crate::billing::AsyncBilling,
    family: &str,
    version: i64,
) -> Option<TariffOverridePayload> {
    if !book().is_enabled() {
        return None;
    }
    book()
        .version_payload_refreshed(family, version, || async {
            billing.list_tariff_overrides().await
        })
        .await
}

/// The background refresher: re-read the whole tiny table on a fixed cadence and swap the
/// snapshot atomically. A failure keeps the last good snapshot; the warning is emitted on the
/// transition into the failure (and recovery is acknowledged) instead of every cycle.
pub async fn refresher_loop(billing: Arc<crate::billing::AsyncBilling>) {
    let mut failing = false;
    loop {
        match book().refresh_once(&billing).await {
            Ok(()) => {
                if failing {
                    elog::info("tariff-book", "tariff override refresh recovered");
                }
                failing = false;
            }
            Err(error) => {
                if !failing {
                    elog::warn(
                        "tariff-book",
                        format!(
                            "tariff override refresh failed, keeping the last good snapshot: {error:#}"
                        ),
                    );
                }
                failing = true;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(TARIFF_BOOK_REFRESH_INTERVAL_SECS)).await;
    }
}

// ── registry mirror → metering converters ─────────────────────────────────────
// Mirror of `crates/server/src/tariff_admin.rs` in the opposite direction. Kept as a deliberate
// textual duplicate: the seeding surface and the settlement hot path must not share code.

pub fn anthropic_prices(prices: &registry::pricing::AnthropicTariffPrices) -> metering::Prices {
    metering::Prices {
        input: prices.input,
        output: prices.output,
        cache_read: prices.cache_read,
        cache_write_5m: prices.cache_write_5m,
        cache_write_1h: prices.cache_write_1h,
    }
}

pub fn codex_prices(prices: &registry::pricing::CodexTariffPrices) -> metering::CodexPrices {
    metering::CodexPrices {
        input: prices.input,
        cached_input: prices.cached_input,
        cache_write_input: prices.cache_write_input,
        output: prices.output,
        api_fast_multiplier_basis_points: prices.api_fast_multiplier_basis_points,
        long_context_threshold: prices.long_context_threshold,
        long_input_basis_points: prices.long_input_basis_points,
        long_output_basis_points: prices.long_output_basis_points,
    }
}

pub fn codex_credit_rates(
    rates: &registry::pricing::CodexTariffCreditRates,
) -> metering::CodexCreditRates {
    metering::CodexCreditRates {
        input: rates.input,
        cached_input: rates.cached_input,
        output: rates.output,
    }
}

pub fn gemini_prices(prices: &registry::pricing::GeminiTariffPrices) -> metering::GeminiPrices {
    metering::GeminiPrices {
        input: prices.input,
        audio_input: prices.audio_input,
        cached_input: prices.cached_input,
        cached_audio_input: prices.cached_audio_input,
        output: prices.output,
        image_output: prices.image_output,
        long_context_threshold: prices.long_context_threshold,
        long_input: prices.long_input,
        long_audio_input: prices.long_audio_input,
        long_cached_input: prices.long_cached_input,
        long_cached_audio_input: prices.long_cached_audio_input,
        long_output: prices.long_output,
        search: match prices.search {
            registry::pricing::GeminiTariffSearchBilling::PerQuery { nano } => {
                metering::GeminiSearchBilling::PerQuery { nano }
            }
            registry::pricing::GeminiTariffSearchBilling::PerGroundedPrompt { nano } => {
                metering::GeminiSearchBilling::PerGroundedPrompt { nano }
            }
        },
    }
}

pub fn glm_prices(prices: &registry::pricing::GlmTariffPrices) -> metering::GlmPrices {
    metering::GlmPrices {
        cached_input: prices.cached_input,
        input: prices.input,
        cache_write: prices.cache_write,
        output: prices.output,
    }
}

pub fn glm_credit_rates(
    rates: &registry::pricing::GlmTariffCreditRates,
) -> metering::GlmCreditRates {
    metering::GlmCreditRates {
        input_tenths: rates.input_tenths,
        cached_input_tenths: rates.cached_input_tenths,
        output_tenths: rates.output_tenths,
    }
}

pub fn kimi_prices(prices: &registry::pricing::KimiTariffPrices) -> metering::KimiPrices {
    metering::KimiPrices {
        cached_input: prices.cached_input,
        input: prices.input,
        cache_write: prices.cache_write,
        output: prices.output,
    }
}

pub fn openai_image_prices(
    prices: &registry::pricing::OpenAiImageTariffPrices,
) -> metering::OpenAiImagePrices {
    metering::OpenAiImagePrices {
        fresh_text_input: prices.fresh_text_input,
        cached_text_input: prices.cached_text_input,
        fresh_image_input: prices.fresh_image_input,
        cached_image_input: prices.cached_image_input,
        image_output: prices.image_output,
    }
}

// ── shared reserve/charge decisions ───────────────────────────────────────────

/// The base price vector admission must hold with, plus the pin settlement will replay. `prices`
/// is the override vector when the book resolves one for `family` at `priced_ts` and the compiled
/// vector otherwise; premiums and multipliers are applied by the caller on top, exactly as with
/// the compiled vector today.
pub struct ReserveBase<T> {
    pub prices: T,
    pub pin: Option<PinnedTariff>,
}

/// Resolve the override base for one reserve/preflight price resolution. `convert` extracts the
/// provider's `metering` price struct from the typed payload; a kind mismatch is impossible by
/// construction (the payload schema is keyed by family prefix) and degrades to compiled with an
/// error log rather than a wrong-price hold.
pub fn reserve_base<T: Copy>(
    book: &TariffBookSnapshot,
    family: &str,
    priced_ts: i64,
    compiled: T,
    convert: fn(&TariffOverridePayload) -> Option<T>,
) -> ReserveBase<T> {
    match book.resolve(family, priced_ts) {
        Some((pin, payload)) => match convert(&payload) {
            Some(prices) => ReserveBase {
                prices,
                pin: Some(pin),
            },
            None => {
                elog::error(
                    "tariff-book",
                    format!("tariff override payload kind mismatch for family {family}"),
                );
                ReserveBase {
                    prices: compiled,
                    pin: None,
                }
            }
        },
        None => ReserveBase {
            prices: compiled,
            pin: None,
        },
    }
}

/// What settlement may charge with.
#[derive(Debug)]
pub enum ChargeBase<T> {
    /// The compiled constants, exactly as today (no pin, or no override for the served family).
    Compiled(T),
    /// An override base vector — prices plus the `<family>/v<version>` schedule id that keeps the
    /// ledger row and calibration event self-explanatory: the exact pinned version for the pinned
    /// family, or the served family's override resolved at the pinned priced timestamp after a
    /// cross-family serve.
    Override(T, String),
    /// The pinned version is absent from the book (and the bounded refresh, where the caller
    /// could await one, did not bring it back). The table is append-only, so this is an
    /// integrity error: the caller must apply its local failure semantics and must NOT reprice
    /// at compiled.
    MissingPinned,
}

/// Decide the base price vector for one charge/settlement. `served_family` is the family of the
/// model the upstream actually served (`None` — an unrecognized model — keeps the caller's
/// existing conservative fallback untouched, since overrides exist only for known families).
pub fn charge_base<T: Copy>(
    book: &TariffBookSnapshot,
    pinned: Option<&PinnedTariff>,
    served_family: Option<&str>,
    priced_ts: i64,
    compiled: T,
    convert: fn(&TariffOverridePayload) -> Option<T>,
) -> ChargeBase<T> {
    let Some(family) = served_family else {
        return ChargeBase::Compiled(compiled);
    };
    if let Some(pin) = pinned.filter(|pin| pin.family == family) {
        return match book.version_payload(&pin.family, pin.version) {
            Some(payload) => match convert(&payload) {
                Some(prices) => ChargeBase::Override(prices, pin.schedule_id.clone()),
                None => {
                    elog::error(
                        "tariff-book",
                        format!(
                            "pinned tariff payload kind mismatch for {}/v{}",
                            pin.family, pin.version
                        ),
                    );
                    ChargeBase::MissingPinned
                }
            },
            None => ChargeBase::MissingPinned,
        };
    }
    match book.resolve(family, priced_ts) {
        Some((pin, payload)) => match convert(&payload) {
            Some(prices) => ChargeBase::Override(prices, pin.schedule_id),
            None => {
                elog::error(
                    "tariff-book",
                    format!("tariff override payload kind mismatch for family {family}"),
                );
                ChargeBase::Compiled(compiled)
            }
        },
        None => ChargeBase::Compiled(compiled),
    }
}

/// Typed payload extractors for [`reserve_base`]/[`charge_base`]. Each accepts only the variant
/// its family prefix guarantees.
pub fn as_anthropic(payload: &TariffOverridePayload) -> Option<metering::Prices> {
    match payload {
        TariffOverridePayload::Anthropic(prices) => Some(anthropic_prices(prices)),
        _ => None,
    }
}

pub fn as_codex(payload: &TariffOverridePayload) -> Option<metering::CodexPrices> {
    match payload {
        TariffOverridePayload::Codex(prices) => Some(codex_prices(prices)),
        _ => None,
    }
}

pub fn as_codex_credits(payload: &TariffOverridePayload) -> Option<metering::CodexCreditRates> {
    match payload {
        TariffOverridePayload::CodexCredits(rates) => Some(codex_credit_rates(rates)),
        _ => None,
    }
}

pub fn as_gemini(payload: &TariffOverridePayload) -> Option<metering::GeminiPrices> {
    match payload {
        TariffOverridePayload::Gemini(prices) => Some(gemini_prices(prices)),
        _ => None,
    }
}

pub fn as_glm(payload: &TariffOverridePayload) -> Option<metering::GlmPrices> {
    match payload {
        TariffOverridePayload::Glm(prices) => Some(glm_prices(prices)),
        _ => None,
    }
}

pub fn as_glm_credits(payload: &TariffOverridePayload) -> Option<metering::GlmCreditRates> {
    match payload {
        TariffOverridePayload::GlmCredits(rates) => Some(glm_credit_rates(rates)),
        _ => None,
    }
}

pub fn as_kimi(payload: &TariffOverridePayload) -> Option<metering::KimiPrices> {
    match payload {
        TariffOverridePayload::Kimi(prices) => Some(kimi_prices(prices)),
        _ => None,
    }
}

pub fn as_openai_image(payload: &TariffOverridePayload) -> Option<metering::OpenAiImagePrices> {
    match payload {
        TariffOverridePayload::OpenAiImage(prices) => Some(openai_image_prices(prices)),
        _ => None,
    }
}

// ── test support ──────────────────────────────────────────────────────────────
// Populated-book tests in sibling modules share the one process-global book, so they serialize on
// this lock and always restore the empty book afterwards. Rows installed through
// `install_global_rows_for_test` conventionally carry `effective_from = i64::MAX`: an exact pinned
// version lookup finds them, while no timestamped resolve — including any concurrently running
// test that never installs a pin — can ever observe them.
#[cfg(test)]
pub(crate) static GLOBAL_BOOK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn test_row(
    family: &str,
    version: i64,
    effective_from: i64,
    payload: serde_json::Value,
) -> TariffOverride {
    TariffOverride {
        tariff_family: family.to_owned(),
        version,
        effective_from,
        payload,
        payload_digest: format!("sha256:v2:{}", "0".repeat(64)),
        created_ts: 1_000,
        created_by: "operator".to_owned(),
        reason: "unit test".to_owned(),
    }
}

#[cfg(test)]
pub(crate) fn install_global_rows_for_test(rows: Vec<TariffOverride>) {
    book().swap_rows(rows).expect("test rows parse");
}

#[cfg(test)]
pub(crate) fn clear_global_book_for_test() {
    book().swap_rows(Vec::new()).expect("empty rows parse");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(family: &str, version: i64, effective_from: i64, payload: serde_json::Value) -> TariffOverride {
        TariffOverride {
            tariff_family: family.to_owned(),
            version,
            effective_from,
            payload,
            payload_digest: format!("sha256:v2:{}", "0".repeat(64)),
            created_ts: 1_000,
            created_by: "operator".to_owned(),
            reason: "unit test".to_owned(),
        }
    }

    fn anthropic_payload(input: i128, output: i128) -> serde_json::Value {
        json!({
            "input": input.to_string(),
            "output": output.to_string(),
            "cache_read": "500",
            "cache_write_5m": "6250",
            "cache_write_1h": "10000"
        })
    }

    fn book_with(rows: Vec<TariffOverride>) -> Arc<TariffBookSnapshot> {
        TariffBookSnapshot::from_rows(rows).expect("test rows parse")
    }

    fn compiled() -> metering::Prices {
        metering::Prices {
            input: 5_000,
            output: 25_000,
            cache_read: 500,
            cache_write_5m: 6_250,
            cache_write_1h: 10_000,
        }
    }

    fn override_prices(input: i128, output: i128) -> metering::Prices {
        metering::Prices {
            input,
            output,
            ..compiled()
        }
    }

    #[test]
    fn an_empty_book_resolves_nothing_and_pins_nothing() {
        let book = TariffBookSnapshot::empty();
        assert!(book.is_empty());
        assert!(book.resolve("anthropic/standard/opus-current", 100).is_none());
        assert!(book.version_payload("anthropic/standard/opus-current", 2).is_none());
        let base = reserve_base(&book, "anthropic/standard/opus-current", 100, compiled(), as_anthropic);
        assert_eq!(base.prices, compiled());
        assert!(base.pin.is_none());
    }

    #[test]
    fn resolve_picks_the_greatest_effective_version() {
        let book = book_with(vec![
            row("anthropic/standard/opus-current", 2, 100, anthropic_payload(5_000, 25_000)),
            row("anthropic/standard/opus-current", 3, 200, anthropic_payload(6_000, 30_000)),
            row("anthropic/standard/opus-current", 4, 1_000_000, anthropic_payload(1, 1)),
        ]);
        let (pin, payload) = book.resolve("anthropic/standard/opus-current", 250).unwrap();
        assert_eq!(pin.version, 3);
        assert_eq!(pin.schedule_id, "anthropic/standard/opus-current/v3");
        assert_eq!(as_anthropic(&payload).unwrap().input, 6_000);
        let _ = payload;
        // Before v2 takes effect the family is still compiled.
        assert!(book.resolve("anthropic/standard/opus-current", 99).is_none());
        // Other families are untouched.
        assert!(book.resolve("anthropic/standard/sonnet-current", 250).is_none());
    }

    #[test]
    fn a_charge_at_the_pinned_version_survives_a_newer_version() {
        let book = book_with(vec![
            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
            row("anthropic/standard/opus-current", 3, 0, anthropic_payload(6_000, 30_000)),
        ]);
        let reserve = reserve_base(&book, "anthropic/standard/opus-current", 50, compiled(), as_anthropic);
        let pin = reserve.pin.unwrap();
        assert_eq!(pin.version, 3);
        assert_eq!(reserve.prices, override_prices(6_000, 30_000));

        // A still newer version appears after admission; settlement replays the pinned one.
        let newer = book_with(vec![
            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
            row("anthropic/standard/opus-current", 3, 0, anthropic_payload(6_000, 30_000)),
            row("anthropic/standard/opus-current", 4, 0, anthropic_payload(9_000, 90_000)),
        ]);
        match charge_base(&newer, Some(&pin), Some("anthropic/standard/opus-current"), 60, compiled(), as_anthropic) {
            ChargeBase::Override(prices, schedule_id) => {
                assert_eq!(prices, override_prices(6_000, 30_000));
                assert_eq!(schedule_id, "anthropic/standard/opus-current/v3");
            }
            other => panic!("pinned charge must replay v3, got {other:?}"),
        }
        // Without a pin the newest effective version resolves.
        match charge_base(&newer, None, Some("anthropic/standard/opus-current"), 60, compiled(), as_anthropic) {
            ChargeBase::Override(prices, _) => assert_eq!(prices, override_prices(9_000, 90_000)),
            other => panic!("unpinned charge must resolve the newest version, got {other:?}"),
        }
    }

    #[test]
    fn a_cross_family_serve_reprices_by_the_served_family() {
        let book = book_with(vec![
            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
            row("anthropic/standard/sonnet-current", 2, 0, anthropic_payload(3_000, 15_000)),
        ]);
        let reserve = reserve_base(&book, "anthropic/standard/opus-current", 50, compiled(), as_anthropic);
        let pin = reserve.pin.unwrap();
        // The upstream answered from another family: the pin does not apply, the served
        // family's override resolves at the pinned priced timestamp.
        match charge_base(&book, Some(&pin), Some("anthropic/standard/sonnet-current"), 60, compiled(), as_anthropic) {
            ChargeBase::Override(prices, schedule_id) => {
                assert_eq!(prices, override_prices(3_000, 15_000));
                assert_eq!(schedule_id, "anthropic/standard/sonnet-current/v2");
            }
            other => panic!("cross-family charge must reprice by the served family, got {other:?}"),
        }
        // A served model no branch recognizes has no family and keeps the compiled fallback.
        match charge_base(&book, Some(&pin), None, 60, compiled(), as_anthropic) {
            ChargeBase::Compiled(prices) => assert_eq!(prices, compiled()),
            other => panic!("an unknown served model keeps the compiled fallback, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_pinned_version_is_an_integrity_error_never_a_compiled_reprice() {
        let book = book_with(vec![
            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
        ]);
        let pin = PinnedTariff {
            family: "anthropic/standard/opus-current".to_owned(),
            version: 3,
            schedule_id: "anthropic/standard/opus-current/v3".to_owned(),
        };
        match charge_base(&book, Some(&pin), Some("anthropic/standard/opus-current"), 60, compiled(), as_anthropic) {
            ChargeBase::MissingPinned => {}
            other => panic!("a missing pinned version must be the integrity error, got {other:?}"),
        }
    }

    #[test]
    fn refresh_on_miss_brings_in_the_pinned_version_exactly_once() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let book = TariffBook::new(true);
            book.swap_rows(vec![
                row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
            ])
            .unwrap();
            let fetches = std::sync::atomic::AtomicU64::new(0);
            let payload = book
                .version_payload_refreshed("anthropic/standard/opus-current", 3, || {
                    fetches.fetch_add(1, Ordering::Relaxed);
                    async {
                        Ok(vec![
                            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
                            row("anthropic/standard/opus-current", 3, 0, anthropic_payload(6_000, 30_000)),
                        ])
                    }
                })
                .await
                .expect("the refresh delivers the pinned version");
            assert_eq!(as_anthropic(&payload).unwrap().input, 6_000);
            assert_eq!(fetches.load(Ordering::Relaxed), 1);
            // A second lookup is a pure cache hit.
            let again = book
                .version_payload_refreshed("anthropic/standard/opus-current", 3, || {
                    fetches.fetch_add(1, Ordering::Relaxed);
                    async { Ok(Vec::new()) }
                })
                .await;
            assert!(again.is_some());
            assert_eq!(fetches.load(Ordering::Relaxed), 1);
            // A genuinely absent version stays None after exactly one refresh (the table image
            // still carries every older version — it is append-only).
            let missing = book
                .version_payload_refreshed("anthropic/standard/opus-current", 9, || {
                    fetches.fetch_add(1, Ordering::Relaxed);
                    async {
                        Ok(vec![
                            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
                            row("anthropic/standard/opus-current", 3, 0, anthropic_payload(6_000, 30_000)),
                        ])
                    }
                })
                .await;
            assert!(missing.is_none());
            assert_eq!(fetches.load(Ordering::Relaxed), 2);
            // A failed refresh answers None and keeps the last good snapshot.
            let failed = book
                .version_payload_refreshed("anthropic/standard/opus-current", 9, || async {
                    Err(anyhow::anyhow!("authority down"))
                })
                .await;
            assert!(failed.is_none());
            assert!(book.snapshot().version_payload("anthropic/standard/opus-current", 3).is_some());
        });
    }

    #[test]
    fn the_kill_switch_answers_empty_regardless_of_rows() {
        let book = TariffBook::new(false);
        book.swap_rows(vec![
            row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000)),
        ])
        .unwrap();
        assert!(book.snapshot().is_empty());
        assert!(book
            .snapshot()
            .resolve("anthropic/standard/opus-current", i64::MAX)
            .is_none());
    }

    /// The installed process-global switch: off ignores every row, on restores resolution.
    #[test]
    fn the_installed_global_switch_ignores_rows_when_off() {
        let _lock = GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        install_global_rows_for_test(vec![row(
            "anthropic/standard/opus-current",
            2,
            0,
            anthropic_payload(5_000, 25_000),
        )]);
        install(false);
        assert!(snapshot().is_empty());
        assert!(snapshot()
            .resolve("anthropic/standard/opus-current", i64::MAX)
            .is_none());
        install(true);
        assert!(snapshot()
            .resolve("anthropic/standard/opus-current", i64::MAX)
            .is_some());
        clear_global_book_for_test();
        assert!(snapshot().is_empty());
    }

    #[test]
    fn a_row_that_fails_typed_parsing_rejects_the_whole_snapshot() {
        let mut bad = row("anthropic/standard/opus-current", 2, 0, anthropic_payload(5_000, 25_000));
        bad.payload = json!({"input": "not-a-number"});
        let good = row("moonshot/kimi/kimi-k3", 2, 0, json!({
            "cached_input": "300", "input": "3000", "cache_write": "3000", "output": "15000"
        }));
        assert!(TariffBookSnapshot::from_rows(vec![good, bad]).is_err());
    }

    #[test]
    fn every_compiled_family_vector_round_trips_through_the_book() {
        // The converter direction here (mirror → metering) must reproduce the exact compiled
        // vectors `crates/server/src/tariff_admin.rs` encodes for seeding, or a seeded override
        // would silently drift from the constants it mirrors.
        let now = 1_788_220_800;
        for (family, prices) in metering::anthropic_compiled_tariffs_at(now) {
            let payload = json!({
                "input": prices.input.to_string(),
                "output": prices.output.to_string(),
                "cache_read": prices.cache_read.to_string(),
                "cache_write_5m": prices.cache_write_5m.to_string(),
                "cache_write_1h": prices.cache_write_1h.to_string(),
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_anthropic(&resolved).unwrap(), prices, "{family}");
        }
        for (family, prices) in metering::codex_compiled_tariffs_at(now) {
            let payload = json!({
                "input": prices.input.to_string(),
                "cached_input": prices.cached_input.to_string(),
                "cache_write_input": prices.cache_write_input.to_string(),
                "output": prices.output.to_string(),
                "api_fast_multiplier_basis_points": prices.api_fast_multiplier_basis_points,
                "long_context_threshold": prices.long_context_threshold,
                "long_input_basis_points": prices.long_input_basis_points,
                "long_output_basis_points": prices.long_output_basis_points,
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_codex(&resolved).unwrap(), prices, "{family}");
        }
        for (family, rates) in metering::codex_compiled_credit_rates() {
            let payload = json!({
                "input": rates.input.to_string(),
                "cached_input": rates.cached_input.to_string(),
                "output": rates.output.to_string(),
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_codex_credits(&resolved).unwrap(), rates, "{family}");
        }
        for (family, prices) in metering::gemini_compiled_tariffs_at(now) {
            let search = match prices.search {
                metering::GeminiSearchBilling::PerQuery { nano } => {
                    json!({"kind": "per_query", "nano": nano.to_string()})
                }
                metering::GeminiSearchBilling::PerGroundedPrompt { nano } => {
                    json!({"kind": "per_grounded_prompt", "nano": nano.to_string()})
                }
            };
            let payload = json!({
                "input": prices.input.to_string(),
                "audio_input": prices.audio_input.to_string(),
                "cached_input": prices.cached_input.to_string(),
                "cached_audio_input": prices.cached_audio_input.to_string(),
                "output": prices.output.to_string(),
                "image_output": prices.image_output.to_string(),
                "long_context_threshold": prices.long_context_threshold,
                "long_input": prices.long_input.to_string(),
                "long_audio_input": prices.long_audio_input.to_string(),
                "long_cached_input": prices.long_cached_input.to_string(),
                "long_cached_audio_input": prices.long_cached_audio_input.to_string(),
                "long_output": prices.long_output.to_string(),
                "search": search,
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_gemini(&resolved).unwrap(), prices, "{family}");
        }
        for (family, prices) in metering::glm_compiled_tariffs_at(now) {
            let payload = json!({
                "cached_input": prices.cached_input.to_string(),
                "input": prices.input.to_string(),
                "cache_write": prices.cache_write.to_string(),
                "output": prices.output.to_string(),
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_glm(&resolved).unwrap(), prices, "{family}");
        }
        for (family, rates) in metering::glm_compiled_credit_rates() {
            let payload = json!({
                "input_tenths": rates.input_tenths.to_string(),
                "cached_input_tenths": rates.cached_input_tenths.to_string(),
                "output_tenths": rates.output_tenths.to_string(),
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_glm_credits(&resolved).unwrap(), rates, "{family}");
        }
        for (family, prices) in metering::kimi_compiled_tariffs_at(now) {
            let payload = json!({
                "cached_input": prices.cached_input.to_string(),
                "input": prices.input.to_string(),
                "cache_write": prices.cache_write.to_string(),
                "output": prices.output.to_string(),
            });
            let book = book_with(vec![row(family, 2, 0, payload)]);
            let (_, resolved) = book.resolve(family, now).unwrap();
            assert_eq!(as_kimi(&resolved).unwrap(), prices, "{family}");
        }
        let (family, prices) = metering::openai_image_compiled_tariff();
        let payload = json!({
            "fresh_text_input": prices.fresh_text_input.to_string(),
            "cached_text_input": prices.cached_text_input.to_string(),
            "fresh_image_input": prices.fresh_image_input.to_string(),
            "cached_image_input": prices.cached_image_input.to_string(),
            "image_output": prices.image_output.to_string(),
        });
        let book = book_with(vec![row(family, 2, 0, payload)]);
        let (_, resolved) = book.resolve(family, now).unwrap();
        assert_eq!(as_openai_image(&resolved).unwrap(), prices, "{family}");
    }
}
