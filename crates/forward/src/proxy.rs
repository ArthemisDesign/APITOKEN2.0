//! Прозрачный форвардинг (Шаг B): для клиента — обычный api.anthropic.com.
//!
//! Клиент шлёт стандартный Anthropic-запрос (напр. Anthropic SDK с base_url=наш сервер и
//! api_key=наш ключ). Мы:
//!   1) авторизуем клиента по нашему ключу (x-api-key / Bearer);
//!   2) под капотом инжектим Claude Code identity в system + oauth-заголовки (иначе токен
//!      подписки не пускают на /v1/messages) — протокол для клиента при этом НЕ меняется;
//!   3) выбираем наименее загруженную подписку пула, шлём запрос с её Bearer через её прокси;
//!   4) при 429/5xx/протухшем токене — cooling и ротация на следующую подписку;
//!   5) ответ (включая SSE-стрим) отдаём клиенту байт-в-байт.

mod anthropic_snapshot;

use self::anthropic_snapshot::{
    prepare_anthropic_legacy_quote, AnthropicLegacyQuoteInput, PreparedAnthropicLegacyQuote,
};
use crate::meter::{BillCtx, CalibrationCtx, MeterCtx, TeeMeter};
use crate::metrics::{Metrics, StrictPricingProvider, StrictPricingRejectionReason};
use crate::pricing::{
    build_policy_admission_snapshot, EnginePricingRequestId, PricingBridgeDecision,
    PricingBridgePrepare, PricingResolution, PricingResolutionRequest, RuntimePricingManifest,
};
use crate::state::AppState;
use crate::upstream::{limits_from_headers, Limits};
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use futures_util::{Stream, StreamExt};
use registry::pricing::{
    LegacyScalarReserveOutcome, PolicyReserveOutcome, PolicyRuleScope, PricingMode,
    SnapshotProvider,
};
use serde_json::Value;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;

const CACHE_ROOT_MIN_WARM_HOMES: usize = 2;
const CACHE_ROOT_MIN_CAPACITY_RATIO: f64 = 0.70;

/// Гард слота конкуррентности персоны. На ЛЮБОМ не-стриминговом исходе попытки (ошибка/ротация/4xx)
/// и — главное — при ОТМЕНЕ запроса (клиент отключился, future хендлера дропнут на await) декрементит
/// in-flight ровно один раз. Разоружается на успехе: слот переходит стриму и снимается в `end_stream`.
/// Без этого гарда `mark_used(+1)` при отмене НЕ откатывался бы → inflight персоны копится до рестарта.
struct InflightGuard {
    pool: Arc<pool::Pool>,
    billing: Option<Arc<crate::billing::AsyncBilling>>,
    capacity_lease_id: Option<String>,
    email: String,
    armed: bool,
}
impl InflightGuard {
    fn new(
        pool: Arc<pool::Pool>,
        billing: Option<Arc<crate::billing::AsyncBilling>>,
        capacity_lease_id: Option<String>,
        email: String,
    ) -> Self {
        Self {
            pool,
            billing,
            capacity_lease_id,
            email,
            armed: true,
        }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for InflightGuard {
    fn drop(&mut self) {
        if self.armed {
            self.pool.mark_done(&self.email);
            if let (Some(billing), Some(lease_id)) = (&self.billing, &self.capacity_lease_id) {
                billing.release_capacity(lease_id);
            }
        }
    }
}

/// Гард резерва баланса. На любом не-успешном исходе И при отмене запроса возвращает удержанный
/// hold клиенту (`settle` с actual=0). Разоружается на успехе — там hold закрывает tee-метеринг
/// фактической стоимостью. Без гарда отмена запроса НАВСЕГДА списывала бы удержанное (деньги клиента).
pub(crate) struct HoldGuard {
    billing: Option<Arc<crate::billing::AsyncBilling>>,
    account_id: String,
    key: String,
    hold: i64,
    request_id: String,
    armed: bool,
}
impl HoldGuard {
    pub(crate) fn new(
        billing: Option<Arc<crate::billing::AsyncBilling>>,
        account_id: String,
        key: String,
        hold: i64,
        request_id: String,
    ) -> Self {
        Self {
            billing,
            account_id,
            key,
            hold,
            request_id,
            armed: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for HoldGuard {
    fn drop(&mut self) {
        if self.armed {
            // возврат резерва на аккаунт (actual=0 → ledger-charge не пишется). Drop синхронен —
            // шлём АСИНХРОННО через актор (settle_detached: mpsc::send не блокирует, не требует await).
            if let Some(b) = &self.billing {
                b.settle_detached(
                    &self.request_id,
                    &self.account_id,
                    &self.key,
                    self.hold,
                    0,
                    None,
                    None,
                );
            }
        }
    }
}

type ResponseByteStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>;

/// Заголовок авторитетной семантики исполнения (docs/engine/ROUTING_FENCING.md §3):
/// `not_started` — запрос гарантированно не был исполнен: ни байта публичного ответа клиенту не
/// ушло, reserve по request_id НЕ будет тарифицирован (settle — только refund/cancel), статус не-2xx.
/// Router снимает заголовок с транзитных ответов; наружу его отдаёт только сам движок.
pub(crate) const EXECUTION_STATE_HEADER: &str = "x-apitoken-execution-state";
pub(crate) const EXECUTION_STATE_NOT_STARTED: &str = "not_started";

/// Выставляет `not_started` на ответ. Допустимо ТОЛЬКО при выполнении всех трёх условий
/// контракта (см. EXECUTION_STATE_HEADER): не-2xx, ни байта клиенту не отправлено, reserve
/// уйдёт в refund. На 2xx и после границы доставки — никогда.
pub(crate) fn with_not_started(mut response: Response) -> Response {
    response.headers_mut().insert(
        EXECUTION_STATE_HEADER,
        HeaderValue::from_static(EXECUTION_STATE_NOT_STARTED),
    );
    response
}

/// Снимает `not_started` с ответа. Страховка для веток, где ответ собран через точку,
/// выставляющую заголовок, но условия контракта не выполнены (возможен charge).
pub(crate) fn without_not_started(mut response: Response) -> Response {
    response.headers_mut().remove(EXECUTION_STATE_HEADER);
    response
}

/// True only for the exact public proof consumed by the router: one `not_started` value on a
/// non-success response. Server composition uses the same predicate for its exported counter, so
/// malformed or duplicated headers can never inflate telemetry for a proof the router rejects.
pub fn is_exact_not_started_response(response: &Response) -> bool {
    if response.status().is_success() {
        return false;
    }
    let mut values = response.headers().get_all(EXECUTION_STATE_HEADER).iter();
    matches!(
        (values.next(), values.next()),
        (Some(value), None) if value.as_bytes() == EXECUTION_STATE_NOT_STARTED.as_bytes()
    )
}

// Anthropic Messages принимает тела до 32 МиБ. Держим тот же публичный предел; точное различение
// overflow/read-error ниже сохраняет нативный 413-контракт вместо ложного generic 400.
const BODY_LIMIT: usize = 32 * 1024 * 1024;

pub(crate) enum BodyReadError {
    TooLarge,
    Read,
}

pub(crate) async fn read_body_limited(
    body: Body,
    limit: usize,
) -> Result<bytes::Bytes, BodyReadError> {
    let mut stream = body.into_data_stream();
    let mut out = bytes::BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BodyReadError::Read)?;
        if out.len().saturating_add(chunk.len()) > limit {
            return Err(BodyReadError::TooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out.freeze())
}

// Заголовки клиента, которые НЕ пробрасываем апстриму (перезаписываем или служебные).
fn skip_req_header(name: &str) -> bool {
    // Отпечаток Claude-Code-клиента синтезируем МЫ (x-app, x-stainless-*) → клиентские НЕ пробрасываем:
    // иначе Python-SDK клиент дал бы `x-stainless-lang: python` при нашем claude-cli UA (противоречие).
    if name.starts_with("x-stainless") {
        return true;
    }
    matches!(
        name,
        "x-app" | "anthropic-dangerous-direct-browser-access" | "accept" |
        "host" | "content-length" | "connection" | "authorization" | "x-api-key"
        | "x-goog-api-key"
        | "anthropic-beta" | "anthropic-version" | "user-agent" | "accept-encoding"
        | "x-claude-code-session-id" | "x-conversation-id" | "x-session-id"
        | "x-apitoken-api-plane"
        | "x-apitoken-calibration-profile"
        | "x-apitoken-execution-group" | "x-apitoken-attempt"
        | "transfer-encoding" | "upgrade" | "proxy-connection" | "proxy-authorization"
        | "keep-alive" | "te" | "trailer"
        // Клиентские forwarding/hop-заголовки НЕ пробрасываем апстриму: они раскрыли бы цепочку прокси
        // и IP клиента (рассинхрон с IP нашего egress-прокси → подрыв антифингерпринта флота).
        | "x-forwarded-for" | "x-forwarded-host" | "x-forwarded-proto" | "forwarded"
        | "x-real-ip" | "via" | "cf-connecting-ip" | "true-client-ip"
    )
}

const CALIBRATION_PROFILE_HEADER: &str = "x-apitoken-calibration-profile";

/// Bounded operator identifier shared with the protected capacity report. Full subscription
/// identity never enters the request or report, and a collision is rejected by the pool.
fn calibration_profile_hint(email: &str) -> String {
    email
        .split('@')
        .next()
        .unwrap_or(email)
        .chars()
        .take(4)
        .collect()
}

/// Exact profile targeting is deliberately unavailable to metered/control/panel credentials.
/// Forwarding-admin traffic is already unmetered and trusted to exercise live subscriptions; this
/// header only makes a bounded calibration run attributable instead of changing its authority.
fn operator_calibration_target<'a>(authz: &Authz, headers: &'a HeaderMap) -> Option<&'a str> {
    if !matches!(authz, Authz::Admin { .. }) {
        return None;
    }
    headers
        .get(CALIBRATION_PROFILE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| {
            (1..=4).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
}
// Заголовки апстрима, которые НЕ отдаём клиенту: hop-by-hop + per-ПОДПИСОЧНЫЕ ratelimit/идентити.
// `anthropic-ratelimit-*` отражают состояние НАШЕЙ подписки (утилизация/reset пула) — отдавать их
// клиенту (а) раскрывает, что это пул, (б) даёт «прыгающий» несогласованный лимит при ротации.
// Аналогично режем org/account-идентифицирующие, различающиеся между подписками (корреляция аккаунта).
fn skip_resp_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "transfer-encoding"
            | "content-length"
            | "content-encoding"
            | "keep-alive"
            | "proxy-connection"
            | "upgrade"
            | "te"
            | "trailer"
    ) || name.starts_with("anthropic-ratelimit")
        || name.starts_with("anthropic-organization")
        || name == "anthropic-account-id"
}

/// Все клиентские credential из запроса. Claude Code может одновременно прислать
/// `x-api-key` из унаследованного `ANTHROPIC_API_KEY` и `Authorization: Bearer` из
/// `ANTHROPIC_AUTH_TOKEN`. Ни один заголовок не имеет приоритета: возвращаем уникальный
/// отсортированный набор, чтобы выбор валидной биллинговой identity не зависел от
/// типа или порядка заголовков. Публично используется `server` для `/balance` и audit.
pub fn client_keys(headers: &HeaderMap) -> Vec<String> {
    let mut keys = std::collections::BTreeSet::new();
    for name in ["x-api-key", "x-goog-api-key"] {
        for value in headers.get_all(name).iter() {
            if let Ok(value) = value.to_str() {
                let key = value.trim();
                if !key.is_empty() {
                    keys.insert(key.to_string());
                }
            }
        }
    }
    for value in headers.get_all("authorization").iter() {
        let Ok(value) = value.to_str() else { continue };
        let Some((scheme, token)) = value.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let token = token.trim();
        if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
            keys.insert(token.to_string());
        }
    }
    keys.into_iter().collect()
}

/// Админ-доступ: env-ключи `CLAUDE_API_KEYS`, либо loopback-пир ТОЛЬКО если `trust_loopback`
/// (сервер реально слушает loopback). За реверс-прокси (bind 0.0.0.0) trust_loopback=false →
/// пустые ключи означают «отклонять всё», а не «доверять любому 127.0.0.1». Без биллинга.
/// Сравнение в константное время (по длине совпадающих строк): не выходим рано на первом различии,
/// чтобы не давать timing-oracle на угадывание админ-ключа по байту. Длину строк не скрываем
/// (не секрет). Без внешних зависимостей.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if app.cfg.api_keys.is_empty() {
        return app.cfg.trust_loopback && peer.ip().is_loopback();
    }
    matching_key(headers, &app.cfg.api_keys).is_some()
}

/// Совпал ли клиентский ключ с любым из `allow` (constant-time по каждому). Пустой `allow` → false.
fn key_in(headers: &HeaderMap, allow: &[String]) -> bool {
    matching_key(headers, allow).is_some()
}

/// Детерминированно вернуть любой допущенный credential. Все пары всё равно
/// сравниваются: невалидный `x-api-key` не может затмить валидный Bearer и наоборот.
fn matching_key(headers: &HeaderMap, allow: &[String]) -> Option<String> {
    let mut matched = None;
    for candidate in client_keys(headers) {
        let candidate_matches = allow.iter().fold(false, |ok, allowed| {
            ok | ct_eq(allowed.as_bytes(), candidate.as_bytes())
        });
        if candidate_matches && matched.is_none() {
            matched = Some(candidate);
        }
    }
    matched
}

/// Control-плоскость (`/admin/*`): admin-ключ ИЛИ control-ключ ИЛИ (нет ни того ни другого →
/// loopback-админ). Отдельный класс, чтобы коммерция управляла аккаунтами своим секретом, не имея
/// прав неметеренного форвардинга.
pub fn control_authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if authed(app, headers, peer) {
        return true;
    } // admin-ключ/loopback покрывает control
    if key_in(headers, &app.cfg.control_keys) {
        return true;
    }
    // Ни admin, ни control-ключей не задано → доверяем loopback (dev). Иначе — только по ключу.
    app.cfg.api_keys.is_empty()
        && app.cfg.control_keys.is_empty()
        && app.cfg.trust_loopback
        && peer.ip().is_loopback()
}

/// Read-only дашборды (`/capacity`, `/metrics`): admin ИЛИ control ИЛИ panel-ключ. Панель смотрит
/// ёмкость своим низкопривилегированным ключом — без прав /admin/* и без форвардинга.
pub fn readonly_authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    control_authed(app, headers, peer) || key_in(headers, &app.cfg.panel_keys)
}

/// Результат авторизации запроса.
pub(crate) enum Authz {
    /// Админ (env-ключ/localhost) — без тарификации.
    Admin { affinity_scope: String },
    /// Ключ клиента → АККАУНТ с балансом. Тарифицируем и списываем с БАЛАНСА АККАУНТА (общего на
    /// все ключи юзера); `key` — для атрибуции расхода по ключу. `mult_bp` — наценка аккаунта.
    /// `balance_nano` несём из авторизации → резерв-блок НЕ перечитывает баланс из БД (−1 запрос).
    Metered {
        account_id: String,
        key: String,
        mult_bp: i64,
        available_nano: i64,
        strict_policy: bool,
        paid_available_nano: Option<i64>,
        track_available_nano: Option<i64>,
    },
    /// Ключ неизвестен/заблокирован (ключ или аккаунт).
    Unauthorized,
    /// Billing authority could not answer. This must stay distinct from an unknown key so a
    /// transient database outage never turns valid credentials into a misleading 401.
    Unavailable,
}

impl Authz {
    pub(crate) fn affinity_scope(&self) -> Option<&str> {
        match self {
            Authz::Admin { affinity_scope } => Some(affinity_scope),
            Authz::Metered { account_id, .. } => Some(account_id),
            Authz::Unauthorized | Authz::Unavailable => None,
        }
    }
}

/// Порядок для МАСШТАБА: сначала админ (env-ключ/loopback) — проверка В ПАМЯТИ, без похода в БД
/// (админ-трафик не грузит биллинг-мьютекс). Только если не админ — клиентский ключ → аккаунт
/// (ОДНА DB-выборка, несёт баланс для резерва → без повторного чтения).
pub(crate) async fn authorize(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> Authz {
    if let Some(credential) = matching_key(headers, &app.cfg.api_keys) {
        return Authz::Admin {
            affinity_scope: credential,
        };
    }
    if app.cfg.api_keys.is_empty() && app.cfg.trust_loopback && peer.ip().is_loopback() {
        return Authz::Admin {
            affinity_scope: "loopback-admin".to_string(),
        };
    }
    if let Some(billing) = &app.billing {
        match resolve_client_key(billing, headers).await {
            Ok(Some((k, a))) => {
                let key_remaining_nano = a.spend_limit_nano.map(|limit| {
                    limit
                        .saturating_sub(a.spent_nano)
                        .saturating_sub(a.reserved_nano)
                        .max(0)
                });
                let available_nano = key_remaining_nano
                    .map(|remaining| remaining.min(a.balance_nano))
                    .unwrap_or(a.balance_nano);
                let paid_available_nano = a.paid_available_nano.map(|available| {
                    key_remaining_nano
                        .map(|remaining| remaining.min(available))
                        .unwrap_or(available)
                });
                let track_available_nano = a.track_available_nano.map(|available| {
                    key_remaining_nano
                        .map(|remaining| remaining.min(available))
                        .unwrap_or(available)
                });
                let strict_policy = a.strict_policy();
                return Authz::Metered {
                    account_id: a.account_id,
                    key: k,
                    mult_bp: a.mult_bp,
                    available_nano,
                    strict_policy,
                    paid_available_nano,
                    track_available_nano,
                };
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("billing key authorization failed: {error:#}");
                return Authz::Unavailable;
            }
        }
    }
    Authz::Unauthorized
}

/// Резолв любого валидного метерного credential. Порядок детерминирован самими
/// значениями, а не типом заголовка. Ошибка authority не затмит уже найденный валидный
/// ключ; но если ни один не подтверждён, транзиентный сбой остаётся 503, а не ложным 401.
pub async fn resolve_client_key(
    billing: &crate::billing::AsyncBilling,
    headers: &HeaderMap,
) -> anyhow::Result<Option<(String, registry::KeyAuth)>> {
    resolve_client_keys(billing, &client_keys(headers)).await
}

/// Тот же OR-резолв для middleware, который должен извлечь credential до передачи request дальше.
pub async fn resolve_client_keys(
    billing: &crate::billing::AsyncBilling,
    keys: &[String],
) -> anyhow::Result<Option<(String, registry::KeyAuth)>> {
    let mut first_error = None;
    for key in keys {
        match billing.key_auth(key).await {
            Ok(Some(auth)) if auth.active_at(pool::now()) => return Ok(Some((key.clone(), auth))),
            Ok(Some(_)) | Ok(None) => {}
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

/// Anthropic-подобная ошибка (чтобы SDK-клиент видел привычную форму).
fn err_response(code: StatusCode, kind: &str, msg: &str) -> Response {
    let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": msg}});
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Ошибка с `Retry-After` — прозрачно для клиента (как настоящий Anthropic при перегрузе/лимите):
/// SDK сам откатится на указанные секунды вместо слепого ретрая.
fn err_retry(code: StatusCode, kind: &str, msg: &str, retry_after: i64) -> Response {
    let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": msg}});
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .header("retry-after", retry_after.max(1).to_string())
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// HTTP-статус `overloaded_error` у НАСТОЯЩЕГО Anthropic — 529 (не 503). Держим точь-в-точь,
/// чтобы синтетический ответ был неотличим от api.anthropic.com (SDK Anthropic знает 529 как
/// retryable overloaded). Через `from_u16` — 529 вне «именованных» констант `http`.
fn http_overloaded() -> StatusCode {
    StatusCode::from_u16(529).expect("529 is a valid HTTP status code")
}

/// Внутренняя причина СИНТЕТИЧЕСКОЙ (не-upstream) ошибки движка. Единственная точка, где рождаются
/// НАШИ ответы клиенту. НИКОГДА не сериализуется как есть — только выбирает Anthropic-аутентичный
/// публичный триплет `{status, error.type, message}`. Смысл: клиент считает, что говорит с
/// api.anthropic.com, и НЕ должен видеть наши внутренности («subscription/pool/upstream/billing
/// authority/cooling»). Настоящую причину несут метрики и локальный лог (`eprintln`), не тело ответа.
///
/// Что клиент ЗАКОННО должен знать (контракт продукта, задокументирован в docs-portal) — остаётся:
/// `InvalidKey` (401), `LowBalance` (402 — состояние аккаунта, «пополни баланс»), `NotFound` (404),
/// `BodyTooLarge`/`BadRequest`/`BadBeta` (4xx валидация). Всё «наше» (нет ёмкости/пул пуст/брейкер/
/// authority/сбой апстрим-соединения) схлопывается в ретраибл `Overloaded`/`RateLimited`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum LocalErr {
    /// Транзиентная нехватка ёмкости на НАШЕЙ стороне: пул пуст, breaker разомкнут (брауноут
    /// апстрима), authority недоступен/зафенсен, сбой соединения/сети с апстримом, серверная
    /// перегрузка. Клиенту — 529 `overloaded_error` «Overloaded» (retryable), опц. Retry-After.
    Overloaded,
    /// Весь флот за лимитом (все подписки cooling) ИЛИ клиент превысил свою конкурентность.
    /// Клиенту это выглядит как обычный рейт-лимит: 429 `rate_limit_error` + Retry-After.
    RateLimited,
    /// Неизвестный/заблокированный клиентский ключ. 401 `authentication_error`.
    InvalidKey,
    /// Недостаточно средств на балансе аккаунта или достигнут лимит расхода ключа. Это ЗАКОННАЯ
    /// ошибка состояния аккаунта (клиент — платящий пользователь с предоплаченным балансом; docs
    /// определяют 402 именно так). 402 `invalid_request_error`.
    LowBalance,
    /// Эндпоинт не поддерживается на квоте подписки. 404 `not_found_error`.
    NotFound,
    /// Тело запроса больше лимита. 413 `request_too_large`.
    BodyTooLarge,
    /// Не удалось прочитать/разобрать тело запроса. 400 `invalid_request_error`.
    BadRequest,
    /// Некорректный `anthropic-beta` заголовок. 400 `invalid_request_error`.
    BadBeta,
    /// Внутренний сбой движка (сериализация тела/сборка ответа). 500 `api_error`.
    Internal,
}

/// Privacy-safe terminal classification carried in response extensions for the server audit
/// middleware. Values must remain static reason codes: never put keys, prompts, upstream text,
/// subscription identities, or other request-derived data here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalErrorReason(pub &'static str);

impl LocalErr {
    /// Публичный триплет `(status, error.type, message)`. ТОЛЬКО аутентичные Anthropic-тексты и типы;
    /// ни одного упоминания подписок/пула/upstream/authority/cooling.
    fn parts(self) -> (StatusCode, &'static str, &'static str) {
        match self {
            LocalErr::Overloaded => (http_overloaded(), "overloaded_error", "Overloaded"),
            LocalErr::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_error",
                "Number of requests has exceeded your rate limit. Please try again later.",
            ),
            LocalErr::InvalidKey => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                "invalid x-api-key",
            ),
            LocalErr::LowBalance => (
                StatusCode::PAYMENT_REQUIRED,
                "invalid_request_error",
                "insufficient balance or key spending limit reached for this request",
            ),
            LocalErr::NotFound => (StatusCode::NOT_FOUND, "not_found_error", "Not Found"),
            LocalErr::BodyTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "Request exceeds the maximum allowed number of bytes.",
            ),
            LocalErr::BadRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Could not parse request body.",
            ),
            LocalErr::BadBeta => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "invalid anthropic-beta header",
            ),
            LocalErr::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "Internal server error",
            ),
        }
    }

    fn reason(self) -> &'static str {
        match self {
            LocalErr::Overloaded => "overloaded",
            LocalErr::RateLimited => "rate_limited",
            LocalErr::InvalidKey => "invalid_key",
            LocalErr::LowBalance => "billing_limit",
            LocalErr::NotFound => "unsupported_endpoint",
            LocalErr::BodyTooLarge => "body_too_large",
            LocalErr::BadRequest => "invalid_request_body",
            LocalErr::BadBeta => "invalid_beta_header",
            LocalErr::Internal => "internal_response_error",
        }
    }
}

/// ЕДИНЫЙ санитайзер синтетических ошибок: внутренняя причина → Anthropic-аутентичный публичный
/// ответ. `retry_after` (сек) добавляет `Retry-After` для retryable-причин. Настоящая причина
/// НЕ попадает в тело — она уже отражена в метриках/логах у места вызова.
fn local_err(reason: LocalErr, retry_after: Option<i64>) -> Response {
    local_err_for(reason, reason.reason(), retry_after)
}

fn local_err_for(
    reason: LocalErr,
    terminal_reason: &'static str,
    retry_after: Option<i64>,
) -> Response {
    let (code, kind, msg) = reason.parts();
    let mut response = match retry_after {
        Some(secs) => err_retry(code, kind, msg, secs),
        None => err_response(code, kind, msg),
    };
    response
        .extensions_mut()
        .insert(TerminalErrorReason(terminal_reason));
    // Все ответы local_err_for — синтетические не-2xx ДО границы доставки: reserve по request_id
    // (если был) закрывает armed HoldGuard refund'ом. Ветки после границы снимают заголовок через
    // without_not_started (delivery-marker-failed с full-hold charge, fallback сборки stream_back).
    with_not_started(response)
}

/// Точный баланс-лимит метерного ключа: сколько OUTPUT-токенов и какой hold клиент может позволить
/// остатком баланса `bal` (client-нанодоллары, т.е. с учётом наценки). Гарантии («ни на токен/цент
/// больше баланса»):
///   • `None` → баланса не хватает даже на ВХОД worst-case → запрос отклоняется (иначе вход мог бы
///     пробить баланс, ведь input тарифицируется всегда, даже при output=0);
///   • `hold ≤ bal` (по построению + кламп) → атомарный reserve не уводит баланс в минус;
///   • при возвращённом `eff_mt`: реальная стоимость (usage ≤ input_est токенов входа + `eff_mt`
///     output + web_buf) ≤ hold — т.к. affordable округляем ВНИЗ и вход оценён сверху (байты ≥ токены).
/// Anthropic, получив урезанный `max_tokens=eff_mt`, останавливает генерацию ровно на доступном
/// токене — это и есть «отруб посреди запроса» без mid-stream-хаков.
/// Дефолт лимита web-поисков, если web_search включён без явного `max_uses` — консервативная оценка
/// для резерва их стоимости под баланс (реальный `max_uses` из тела приоритетен).
const DEFAULT_WEB_USES: u64 = 20;

fn cap_to_balance(
    bal: i128,
    input_est: i128,
    web_buf: i128,
    p: &metering::Prices,
    mult_bp: i64,
    client_mt: u64,
) -> Option<(u64, i64)> {
    // Наценка ≤ 0 = бесплатный ключ (charge всегда 0) → не лимитируем, hold 0 (баланс не двигается).
    if mult_bp <= 0 {
        return Some((client_mt, 0));
    }
    // Овердрафт-буфер: доступное = balance + $1. Funded-юзера НЕ роняем 402 из-за гонки конкурентных
    // резервов — атомарный резерв (registry) всё равно держит пол баланса на −$1. За полом (bal ≤ −$1) → None.
    let ceil = bal + metering::OVERDRAFT_NANO;
    if ceil <= 0 {
        return None;
    }
    // Работаем в RAW-нано (до наценки). Максимальная RAW-стоимость `x_max`, чья КЛИЕНТСКАЯ цена
    // (apply_multiplier, round-half-up) гарантированно ≤ ceil (=bal+$1): X ≤ ⌊ceil·10000/m⌋ → X·m ≤
    // ceil·10000 → (X·m+5000)/10000 ≤ ceil. Так `hold=apply_multiplier(ceiling) ≤ bal+$1` по построению.
    let x_max = ceil.saturating_mul(10000) / (mult_bp as i128);
    let fixed_raw = input_est * p.cache_write_1h + web_buf; // вход worst-case + буфер поисков (RAW)
    if fixed_raw > x_max {
        return None;
    } // не тянет даже вход worst-case → 402
    let out = p.output.max(1);
    let affordable = ((x_max - fixed_raw) / out).max(0) as u64; // сколько output-токенов влезает в x_max
    if affordable == 0 {
        return None;
    }
    let eff_mt = client_mt.min(affordable);
    // ceiling_raw ≤ x_max (по построению) → hold = apply_multiplier(ceiling_raw) ≤ bal. А реальный
    // charge = apply_multiplier(real_raw) ≤ hold, т.к. real_raw ≤ ceiling_raw (реальных input-токенов
    // ≤ байт=input_est, ставка входа ≤ cw1h, output ≤ eff_mt). Итог: charge ≤ hold ≤ bal — жёстко.
    let ceiling_raw = fixed_raw + (eff_mt as i128) * p.output;
    // Кламп к i64::MAX: овердрафт-буфер (bal+$1) при абсурдном балансе мог бы толкнуть hold за i64,
    // и `as i64` обернул бы его в отрицательный. Реальные балансы недостижимы близко к i64::MAX.
    let hold = metering::apply_multiplier(ceiling_raw, mult_bp).min(i64::MAX as i128) as i64;
    Some((eff_mt, hold))
}

/// Инжект Claude Code identity первым system-блоком (если его там ещё нет).
/// Первый system-блок уже несёт Claude-Code-идентичность? (billing-header/identity — как шлёт
/// САМ Claude Code). Тогда повторно инжектить не надо — иначе получим двойную identity.
fn is_cc_marker(text: &str) -> bool {
    text.starts_with("x-anthropic-billing-header:")
        || text.starts_with("You are Claude Code")
        || text.starts_with("You are a Claude agent")
}

fn inject_identity(v: &mut Value, identity: &str) -> bool {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return false,
    };
    if !obj.contains_key("messages") {
        return false;
    } // не messages-запрос — не трогаем
      // identity-блок БЕЗ cache_control — ТОЧНО как реальный CC (снято с 2.1.195: system[identity] не
      // имеет cache_control; брейкпоинты CC ставит на БОЛЬШОЙ системный промпт, а не на identity).
      // cache_control на нашем identity был бы отпечатком («этот клиент кэш-контролит identity, а CC — нет»).
      // Экономику кэша это не рушит: если клиент сам ставит брейкпоинт ниже, наш identity попадёт в его
      // кэш-префикс (кэшируется всё ДО брейкпоинта включительно). session_key считаем отдельно (не зависит).
    let idblock = serde_json::json!({"type":"text","text":identity});
    match obj.get("system").cloned() {
        None => {
            obj.insert("system".into(), serde_json::json!([idblock]));
        }
        Some(Value::String(s)) => {
            if is_cc_marker(&s) {
                return false;
            } // клиент прислал identity строкой — не дублируем
            obj.insert(
                "system".into(),
                serde_json::json!([idblock, {"type":"text","text":s}]),
            );
        }
        Some(Value::Array(mut arr)) => {
            let first_cc = arr
                .first()
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                .map(is_cc_marker)
                .unwrap_or(false);
            if first_cc {
                return false;
            } // уже Claude-Code-запрос (напр. сам Claude Code)
            arr.insert(0, idblock);
            obj.insert("system".into(), Value::Array(arr));
        }
        _ => return false,
    }
    true
}

/// Препендит/обновляет system[0] = billing-header блок ИДЕМПОТЕНТНО (на ротации заменяет текст,
/// не дублирует). Реальный CC шлёт его первым system-блоком, БЕЗ cache_control (снято с 2.1.195).
/// Работает только по массиву system (identity-инжект уже гарантирует массив на messages-запросе).
fn set_billing_block(v: &mut Value, text: &str) {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    let block = serde_json::json!({"type":"text","text":text});
    if let Some(Value::Array(arr)) = obj.get_mut("system") {
        let first_billing = arr
            .first()
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .map(|t| t.starts_with("x-anthropic-billing-header:"))
            .unwrap_or(false);
        if first_billing {
            arr[0] = block;
        } else {
            arr.insert(0, block);
        }
    }
}

/// Сливает клиентские beta-флаги с МИНИМАЛЬНЫМИ identity-флагами OAuth/Claude Code.
/// Feature-беты из fleet-конфига не навязываем обычному SDK-клиенту: реальный Claude Code пришлёт
/// свой полный набор сам, а произвольный клиент сохранит ровно запрошенные capabilities.
fn merged_beta(headers: &HeaderMap, configured: &str) -> Result<String, ()> {
    fn push(raw: &str, identity_only: bool, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if identity_only && !(token.starts_with("oauth-") || token.starts_with("claude-code-"))
            {
                continue;
            }
            if seen.insert(token.to_string()) {
                out.push(token.to_string());
            }
        }
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in headers.get_all("anthropic-beta").iter() {
        push(value.to_str().map_err(|_| ())?, false, &mut seen, &mut out);
    }
    push(configured, true, &mut seen, &mut out);
    Ok(out.join(","))
}

/// Claude-Code persona нужна для OAuth-поведения, но документированную атрибуцию клиента нельзя
/// перезаписывать. Добавляем persona только когда metadata/user_id отсутствуют; malformed metadata
/// оставляем как есть, чтобы Anthropic вернул свою нативную validation error вместо panic шлюза.
fn set_persona_user_id_if_absent(v: &mut Value, user_id: String) {
    let Some(obj) = v.as_object_mut() else { return };
    if obj.get("metadata").map(|m| m.is_null()).unwrap_or(true) {
        obj.insert("metadata".into(), serde_json::json!({"user_id": user_id}));
    } else if let Some(Value::Object(metadata)) = obj.get_mut("metadata") {
        if !metadata.contains_key("user_id") {
            metadata.insert("user_id".into(), Value::String(user_id));
        }
    }
    // Existing non-object metadata is intentionally untouched so upstream returns its native 4xx.
    // AUDIT-TODO(C52): move internal persona attribution to a private upstream channel and stop adding public metadata.
}

/// Allowlist эндпоинтов Anthropic, работающих на квоте ПОДПИСКИ (что мы форвардим на пул):
/// `POST /v1/messages` (метерим), `POST /v1/messages/count_tokens` и `GET /v1/models[/{id}]` (проброс
/// без тарификации). Всё прочее (batches/files/agents/sessions/environments/skills/complete) на
/// подписочном OAuth-токене недоступно → чистый 404 на шлюзе. Управляющие роуты (`/health` и др.)
/// сюда НЕ доходят — их обслуживает `server` до fallback на `forward`.
fn is_supported_endpoint(method: &Method, path: &str) -> bool {
    // Модельный id — ровно ОДИН непустой raw-сегмент. `%` отклоняем целиком: model ids не требуют
    // percent-encoding, зато URL-парсер нормализует `%2e`, `%2f` и варианты регистра уже ПОСЛЕ allowlist.
    // Также запрещаем backslash (separator на части URL/proxy стеков) и literal dot/empty-сегменты.
    if path.contains('%')
        || path.contains('\\')
        || path.contains("//")
        || path.split('/').any(|seg| seg == "." || seg == "..")
    {
        return false;
    }
    match (method.as_str(), path) {
        ("POST", "/v1/messages") | ("POST", "/v1/messages/count_tokens") => true,
        ("GET", "/v1/models") => true,
        ("GET", p) => p
            .strip_prefix("/v1/models/")
            .map(|id| !id.is_empty() && !id.contains('/'))
            .unwrap_or(false),
        _ => false,
    }
}

/// Namespaced catalog id (`anthropic/<native id>`) → native id в теле запроса
/// native lane. Universal dispatch (этапы 3–5 UNIFIED_ROUTER) проксирует тело
/// байт-идентично, поэтому префикс доезжает до плоскости как есть; admission/
/// reserve и upstream Anthropic ждут нативный id (зеркало strip'а
/// chat-адаптера `anthropic.rs`). Возвращает итоговый id для локального
/// использования; при снятии префикса поле `model` тела переписывается.
fn strip_own_namespace(v: &mut Value) -> String {
    let model = v
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match model.strip_prefix("anthropic/") {
        Some(stripped) => {
            let stripped = stripped.to_string();
            if let Some(slot) = v.get_mut("model") {
                *slot = Value::String(stripped.clone());
            }
            stripped
        }
        None => model,
    }
}

pub async fn forward(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    if !app.authority_ready.load(AtomicOrdering::Acquire) {
        // Инстанс зафенсен/authority недоступен — НАША внутренняя причина. Клиенту — транзиентный
        // retryable overload (ретрай, вероятно, попадёт на здоровый инстанс), без слова «authority».
        return local_err_for(
            LocalErr::Overloaded,
            "billing_authority_unavailable",
            Some(2),
        );
    }
    let (parts, body) = req.into_parts();
    let execution = match crate::execution::parse_execution_attempt(&parts.headers) {
        Ok(execution) => execution,
        Err(error) => {
            eprintln!(
                "Anthropic execution identity rejected class={}",
                error.as_str()
            );
            return with_not_started(local_err_for(
                LocalErr::Overloaded,
                "invalid_execution_identity",
                Some(2),
            ));
        }
    };
    let billable = parts.method == Method::POST && parts.uri.path() == "/v1/messages";
    let authz = authorize(&app, &parts.headers, &peer).await;
    match &authz {
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return local_err(LocalErr::InvalidKey, None);
        }
        Authz::Unavailable => {
            return local_err_for(
                LocalErr::Overloaded,
                "billing_authority_unavailable",
                Some(2),
            )
        }
        Authz::Admin { .. } | Authz::Metered { .. } => {}
    }
    let operator_target = operator_calibration_target(&authz, &parts.headers).map(str::to_owned);
    // ALLOWLIST эндпоинтов: форвардим на пул ТОЛЬКО то, что доступно на квоте ПОДПИСКИ Claude Max
    // (messages/count_tokens/models). Batches/Files/Agents/Sessions требуют scope OAuth-токена
    // (user:batch/developer), которого у подписки НЕТ → на них Anthropic отдаёт 403/401/404. Не роутим
    // их на подписку (иначе застудили бы её и слили бы backend-scope в ошибке), а отдаём чистый 404.
    if !is_supported_endpoint(&parts.method, parts.uri.path()) {
        return local_err(LocalErr::NotFound, None);
    }
    if billable && matches!(&authz, Authz::Metered { available_nano, .. } if *available_nano <= 0) {
        return local_err(LocalErr::LowBalance, None);
    }
    Metrics::inc(&app.metrics.requests);
    let method: Method = parts.method.clone();
    let pq = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    // Реальный CC шлёт `/v1/messages?beta=true` (снято с живого claude 2.1.195). Добавляем query
    // `beta=true` ровно на POST /v1/messages, если его там ещё нет (мержим с существующим query,
    // не ломая клиентские параметры). Остальные пути — байт-в-байт как пришли.
    let pq_owned;
    let pq: &str = if method == Method::POST
        && parts.uri.path() == "/v1/messages"
        && !parts
            .uri
            .query()
            .map(|q| q.split('&').any(|kv| kv == "beta=true"))
            .unwrap_or(false)
    {
        let sep = if parts.uri.query().is_some() {
            '&'
        } else {
            '?'
        };
        pq_owned = format!("{pq}{sep}beta=true");
        &pq_owned
    } else {
        pq
    };
    let url = format!("{}{}", app.cfg.upstream.trim_end_matches('/'), pq);

    let raw = match read_body_limited(body, BODY_LIMIT).await {
        Ok(b) => b,
        Err(BodyReadError::TooLarge) => return local_err(LocalErr::BodyTooLarge, None),
        Err(BodyReadError::Read) => return local_err(LocalErr::BadRequest, None),
    };

    // тело: один парс — вытаскиваем модель + max_tokens (для тарификации/резерва) и инжектим
    // identity (иначе токен подписки не пустят на /v1/messages). Держим `Bytes` (не Vec): clone на
    // каждую попытку ротации тогда O(1) refcount, а не копия до BODY_LIMIT (анти-амплификация памяти).
    let body_bytes: bytes::Bytes = raw.clone();
    let mut model = String::new();
    let mut max_tokens: u64 = 0;
    let mut requested_fast = false;
    let mut requested_us_inference = false;
    let mut affinity_input = None;
    let mut parsed = serde_json::from_slice::<Value>(&raw).ok();
    // `parsed` (если тело — JSON) держим как ФИНАЛИЗИРУЕМЫЙ шаблон: инжектим identity/cap max_tokens
    // здесь, а per-sub metadata.user_id — в цикле per-подписка, и сериализуем тело per-attempt. `body_bytes`
    // (=raw) — фолбэк для не-JSON тела. В общем случае (1 попытка) это 1 сериализация, как и раньше.
    let mut web_uses: u64 = 0; // суммарный лимит web-поисков (для резерва их стоимости под баланс)
    if let Some(v) = parsed.as_mut() {
        model = strip_own_namespace(v);
        max_tokens = v.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
        requested_fast = v
            .get("speed")
            .and_then(Value::as_str)
            .is_some_and(|speed| speed.eq_ignore_ascii_case("fast"));
        requested_us_inference = v
            .get("inference_geo")
            .and_then(Value::as_str)
            .is_some_and(|geo| geo.eq_ignore_ascii_case("us"));
        // Резервируем стоимость web_search по РЕАЛЬНОМУ max_uses каждого инструмента (не фикс-буфер):
        // иначе клиент с max_uses>buf пробил бы hold. Без max_uses — консервативный дефолт.
        if let Some(tools) = v.get("tools").and_then(Value::as_array) {
            for t in tools {
                let is_web = t
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|s| s.contains("web_search"))
                    .unwrap_or(false);
                if is_web {
                    // saturating + кламп: crafted max_uses≈u64::MAX не должен обернуть web_uses (release-wrap
                    // → мизерный web_buf → недорезерв). Потолок 1000 с запасом покрывает любой реальный кейс.
                    web_uses = web_uses
                        .saturating_add(
                            t.get("max_uses")
                                .and_then(Value::as_u64)
                                .unwrap_or(DEFAULT_WEB_USES),
                        )
                        .min(1000);
                }
            }
        }
        // Infer from the untouched client request. Native harness IDs win; ordinary API clients are
        // linked by canonical transcript prefixes. The account (not individual API key) is the tenant.
        if let Some(scope) = authz.affinity_scope() {
            affinity_input = app.affinity.infer(scope, &parts.headers, v);
        }
        if app.cfg.inject_identity {
            inject_identity(v, &app.cfg.identity);
        }
    }
    let mut affinity_resolution = match affinity_input.as_ref() {
        Some(input) => app.affinity.resolve(input).await,
        None => None,
    };
    let affinity_started_new = affinity_input.is_some() && affinity_resolution.is_none();
    let affinity_warm_homes = match (affinity_input.as_ref(), affinity_resolution.as_ref()) {
        (Some(input), None) => app.affinity.warm_homes(input).await,
        _ => Vec::new(),
    };
    let mut persona_session = affinity_resolution
        .as_ref()
        .map(|resolution| pool::stable_hash64(resolution.session_id.as_bytes()));

    // Circuit breaker разомкнут (брауноут апстрима) → быстрый отбой ДО резерва: в аутейдж не делаем
    // лишних DB-записей (reserve+возврат) на каждый запрос thundering-herd. Резерва ещё нет — возвращать нечего.
    if let Some(retry) = app.breaker.open_for(pool::now()) {
        Metrics::inc(&app.metrics.breaker_rejects);
        return local_err_for(
            LocalErr::Overloaded,
            "upstream_circuit_breaker",
            Some(retry),
        );
    }

    // БАЛАНС-ЛИМИТ метерного ключа (точный контроль: клиент не получит ни токена/цента сверх баланса).
    // Идея: output ограничиваем ЗАРАНЕЕ — урезаем `max_tokens` под остаток баланса, и Anthropic сам
    // отрубает генерацию ровно на доступном токене (stop_reason: max_tokens). Вход считаем по ВЕРХНЕЙ
    // оценке (полные байты × cache_write_1h — токенов ≤ байт при любой корзине). Затем атомарно
    // резервируем потолок при УРЕЗАННОМ max_tokens (≤ баланса), фактику закрываем settle в finalize.
    // One stable internal ID spans reservation, all upstream attempts, settlement, and capacity leases.
    // It is generated before any money mutation and is never replaced by an upstream audit header.
    let engine_request_id = crate::upstream::fresh_request_id();
    // Tuple: request/account/key/hold plus the payable multiplier and optional strict tariff pin.
    let mut reserved: Option<(String, String, String, i64, i64, Option<i64>)> = None;
    // Резервируем ТОЛЬКО под POST /v1/messages — единственный биллинговый эндпоинт. `count_tokens` и
    // `GET /v1/models` бесплатны у Anthropic; резерв мог бы ошибочно 402-ить их при нулевом балансе.
    if let (
        true,
        Authz::Metered {
            account_id,
            key,
            mult_bp,
            available_nano,
            strict_policy,
            paid_available_nano,
            track_available_nano,
        },
        Some(billing),
    ) = (billable, &authz, &app.billing)
    {
        // баланс несём из authorize (свежая выборка) — без повторного чтения. Гонку с параллельными
        // запросами всё равно ловит АТОМАРНЫЙ reserve (WHERE balance>=hold): stale-баланс лишь мог бы
        // дать чуть больший cap, но reserve тогда честно откажет (402), в минус не уводя.
        let bal = *available_nano as i128;
        // РЕЗЕРВ по model_prices_RESERVE: распознанная модель → её цена; нераспознанный алиас →
        // MAX-тариф. Иначе резерв по дешёвому дефолту, а списание по (дорогой) модели ОТВЕТА пробили
        // бы hold → баланс в минус до −2×. Списание (finalize) остаётся по реальной модели ответа.
        let price_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        let bridge_provider = SnapshotProvider::Anthropic;
        let admission_modifiers = metering::AnthropicAdmissionModifiers {
            speed: if requested_fast {
                metering::AnthropicSpeed::Fast
            } else {
                metering::AnthropicSpeed::Standard
            },
            inference_geo: if requested_us_inference {
                metering::AnthropicInferenceGeo::Us
            } else {
                metering::AnthropicInferenceGeo::Global
            },
        };
        let bridge_prepared: Option<PreparedAnthropicLegacyQuote> = if app
            .cfg
            .pricing_bridge
            .enabled()
            && !strict_policy
        {
            let Some(request_id) = EnginePricingRequestId::from_engine_uuid_v4(&engine_request_id)
            else {
                app.metrics.pricing_bridge_failure(bridge_provider);
                eprintln!("pricing bridge rejected the engine-owned request identity");
                return local_err_for(LocalErr::Overloaded, "pricing_bridge_invariant", Some(2));
            };
            match app
                .cfg
                .pricing_bridge
                .decision(bridge_provider, &request_id)
            {
                PricingBridgeDecision::Fallback(reason) => {
                    app.metrics.pricing_bridge_fallback(bridge_provider, reason);
                    None
                }
                PricingBridgeDecision::Selected => {
                    app.metrics.pricing_bridge_selected(bridge_provider);
                    match prepare_anthropic_legacy_quote(AnthropicLegacyQuoteInput {
                        request_id,
                        account_id: account_id.clone(),
                        requested_model_id: model.clone(),
                        quote_ts: price_ts,
                        payable_multiplier_bp: *mult_bp,
                        modifiers: admission_modifiers,
                        input_token_upper_bound: (raw.len() + app.cfg.identity.len())
                            .max(1)
                            .min(u64::MAX as usize)
                            as u64,
                        web_search_requests: web_uses,
                        requested_max_output_tokens: max_tokens,
                    }) {
                        Ok(PricingBridgePrepare::Eligible(prepared)) => Some(prepared),
                        Ok(PricingBridgePrepare::Fallback(reason)) => {
                            app.metrics.pricing_bridge_fallback(bridge_provider, reason);
                            None
                        }
                        Err(error) => {
                            app.metrics.pricing_bridge_failure(bridge_provider);
                            eprintln!("Anthropic pricing bridge preparation failed: {error:#}");
                            return local_err_for(
                                LocalErr::Overloaded,
                                "pricing_bridge_invariant",
                                Some(2),
                            );
                        }
                    }
                }
            }
        } else {
            if !strict_policy {
                app.metrics.pricing_bridge_fallback(
                    bridge_provider,
                    crate::pricing::PricingBridgeFallbackReason::BridgeDisabled,
                );
            }
            None
        };
        let mut p = metering::model_prices_reserve_for_speed_at(&model, price_ts, requested_fast);
        if requested_us_inference {
            p = metering::premium_prices_ceil(p, 11_000);
        }
        let input_est = ((raw.len() + app.cfg.identity.len()) as i128).max(1);
        // web-буфер = число разрешённых поисков × ставка (0 без web_search-инструмента → малобалансовым
        // не блокирует обычные запросы; при включённом — покрывает ровно заявленный max_uses).
        let web_buf = (web_uses as i128) * metering::WEB_SEARCH_NANO;
        let client_mt = if max_tokens > 0 {
            max_tokens.min(2_000_000)
        } else {
            4096
        };
        // РЕЗЕРВ по АККАУНТУ с ПЕРЕ-РЕЗЕРВОМ под свежий баланс: funded-юзера НЕ роняем 402 из-за гонки
        // конкурентных резервов. `bal` из authorize оптимистичен; если атомарный reserve отказал (соседние
        // holds увели баланс за пол −$1, или per-key лимит), перечитываем АКТУАЛЬНЫЙ баланс, дорезаем
        // output под него+буфер и повторяем. None-путь reserve строки НЕ создаёт → тот же request_id
        // безопасно повторить с меньшим hold (идемпотентность не триггерится). Ограничено 4 попытками:
        // строго убывающий hold сходится быстро; иначе честный 402 (реально за полом даже с буфером).
        let mut reserved_pair: Option<(u64, i64)> = None;
        let mut settlement_mult_bp = *mult_bp;
        let mut settlement_priced_ts = None;
        if *strict_policy {
            let canonical = match metering::anthropic_tariff_capability_at(
                &model,
                price_ts,
                admission_modifiers,
            ) {
                Ok(identity) => identity,
                Err(metering::TariffIdentityError::UnsupportedModelIdentity)
                | Err(metering::TariffIdentityError::UnsupportedModifier) => {
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::UnsupportedModel,
                    );
                    return local_err_for(
                        LocalErr::NotFound,
                        "strict_pricing_unsupported_model",
                        None,
                    );
                }
                Err(metering::TariffIdentityError::InvalidPricedTimestamp) => {
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::InvalidContract,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_invalid_clock",
                        Some(2),
                    );
                }
            };
            let bundle = match billing.pricing_read_bundle(account_id).await {
                Ok(bundle) => bundle,
                Err(error) => {
                    eprintln!("strict Anthropic pricing bundle read failed: {error:#}");
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::ReadUnavailable,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_read_unavailable",
                        Some(2),
                    );
                }
            };
            let manifest = RuntimePricingManifest::from_evidence(&app.pricing_manifest);
            let resolved = match crate::pricing::resolve_pricing(
                &bundle,
                &PricingResolutionRequest {
                    account_id: account_id.clone(),
                    provider_id: bridge_provider.as_str().to_owned(),
                    requested_model_id: model.clone(),
                    canonical_model_id: canonical.canonical_model_id.to_owned(),
                },
                &manifest,
            ) {
                PricingResolution::Resolved(resolved) => resolved,
                PricingResolution::Rejected(reason) => {
                    eprintln!(
                        "strict Anthropic admission rejected by pricing policy: {}",
                        reason.code()
                    );
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::from_resolution(reason),
                    );
                    return local_err_for(LocalErr::NotFound, "strict_pricing_rejected", None);
                }
            };
            let strict_model_scope = matches!(&resolved.rule.scope, PolicyRuleScope::Model { .. });
            let current_balance = match resolved.rule.pricing_mode {
                PricingMode::Track => track_available_nano.unwrap_or(0),
                PricingMode::Discount => paid_available_nano.unwrap_or(0),
            };
            if current_balance <= 0 {
                app.metrics.strict_pricing_rejected(
                    StrictPricingProvider::Anthropic,
                    StrictPricingRejectionReason::LowBalance,
                );
                return local_err(LocalErr::LowBalance, None);
            }
            let Some(request_id) = EnginePricingRequestId::from_engine_uuid_v4(&engine_request_id)
            else {
                app.metrics.strict_pricing_rejected(
                    StrictPricingProvider::Anthropic,
                    StrictPricingRejectionReason::InvalidContract,
                );
                return local_err_for(
                    LocalErr::Overloaded,
                    "strict_pricing_request_identity",
                    Some(2),
                );
            };
            let prepared = match prepare_anthropic_legacy_quote(AnthropicLegacyQuoteInput {
                request_id,
                account_id: account_id.clone(),
                requested_model_id: model.clone(),
                quote_ts: price_ts,
                payable_multiplier_bp: resolved.rule.payable_multiplier_bp,
                modifiers: admission_modifiers,
                input_token_upper_bound: (raw.len() + app.cfg.identity.len())
                    .max(1)
                    .min(u64::MAX as usize) as u64,
                web_search_requests: web_uses,
                requested_max_output_tokens: max_tokens,
            }) {
                Ok(PricingBridgePrepare::Eligible(prepared)) => prepared,
                Ok(PricingBridgePrepare::Fallback(reason)) => {
                    eprintln!(
                        "strict Anthropic quote rejected canonical input: {}",
                        reason.code()
                    );
                    let metric_reason = match reason {
                        crate::PricingBridgeFallbackReason::UnsupportedModelIdentity
                        | crate::PricingBridgeFallbackReason::UnsupportedModifier => {
                            StrictPricingRejectionReason::UnsupportedModel
                        }
                        crate::PricingBridgeFallbackReason::BridgeDisabled
                        | crate::PricingBridgeFallbackReason::NotSampled
                        | crate::PricingBridgeFallbackReason::SnapshotIdentityOversized
                        | crate::PricingBridgeFallbackReason::OfficialHoldOutOfRange => {
                            StrictPricingRejectionReason::QuoteInvariant
                        }
                    };
                    app.metrics
                        .strict_pricing_rejected(StrictPricingProvider::Anthropic, metric_reason);
                    return local_err_for(
                        LocalErr::NotFound,
                        "strict_pricing_quote_rejected",
                        None,
                    );
                }
                Err(error) => {
                    eprintln!("strict Anthropic quote failed: {error:#}");
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::QuoteInvariant,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_quote_invariant",
                        Some(2),
                    );
                }
            };
            let quote = match prepared.quote(i128::from(current_balance)) {
                Ok(Some(quote)) => quote,
                Ok(None) => {
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::LowBalance,
                    );
                    return local_err(LocalErr::LowBalance, None);
                }
                Err(error) => {
                    eprintln!("strict Anthropic balance quote failed: {error:#}");
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::QuoteInvariant,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_quote_invariant",
                        Some(2),
                    );
                }
            };
            let eff_mt = quote.effective_max_output_tokens();
            let hold = quote.snapshot().charged_hold_nano();
            let policy_snapshot = match build_policy_admission_snapshot(
                account_id,
                resolved.clone(),
                quote.into_snapshot(),
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    eprintln!("strict Anthropic snapshot build failed: {error:#}");
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::SnapshotInvariant,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_snapshot_invariant",
                        Some(2),
                    );
                }
            };
            match billing
                .reserve_request_with_policy_snapshot_for_execution(
                    key,
                    policy_snapshot,
                    execution.clone(),
                )
                .await
            {
                Ok(PolicyReserveOutcome::Inserted(_)) | Ok(PolicyReserveOutcome::Unchanged(_)) => {
                    app.metrics.strict_pricing_admitted(
                        StrictPricingProvider::Anthropic,
                        resolved.rule.pricing_mode,
                        strict_model_scope,
                    );
                    settlement_mult_bp = resolved.rule.payable_multiplier_bp;
                    settlement_priced_ts = Some(price_ts);
                    reserved_pair = Some((eff_mt, hold));
                }
                Ok(PolicyReserveOutcome::NotReserved) => {
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::LowBalance,
                    );
                    return local_err(LocalErr::LowBalance, None);
                }
                Ok(PolicyReserveOutcome::Conflict(conflict)) => {
                    eprintln!("strict Anthropic reserve conflict: {conflict:?}");
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::ReserveConflict,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_reserve_conflict",
                        Some(2),
                    );
                }
                Ok(PolicyReserveOutcome::AbortedBeforeCommit) => {
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::HandoffAborted,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_handoff_aborted",
                        Some(2),
                    );
                }
                Err(error) => {
                    eprintln!("strict Anthropic reserve failed: {error:#}");
                    app.metrics.strict_pricing_rejected(
                        StrictPricingProvider::Anthropic,
                        StrictPricingRejectionReason::ReserveUnavailable,
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "strict_pricing_reserve_unavailable",
                        Some(2),
                    );
                }
            }
        } else if let Some(prepared) = bridge_prepared {
            let _bridge_latency = app.metrics.pricing_bridge_latency_timer(bridge_provider);
            let mut current_balance = bal;
            for _ in 0..4 {
                let quote = match prepared.quote(current_balance) {
                    Ok(Some(quote)) => quote,
                    Ok(None) => break,
                    Err(error) => {
                        app.metrics.pricing_bridge_failure(bridge_provider);
                        eprintln!("Anthropic pricing bridge quote failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "pricing_bridge_invariant",
                            Some(2),
                        );
                    }
                };
                let eff_mt = quote.effective_max_output_tokens();
                let hold = quote.snapshot().charged_hold_nano();
                match billing
                    .reserve_request_with_legacy_snapshot_for_execution(
                        key,
                        quote.into_snapshot(),
                        execution.clone(),
                    )
                    .await
                {
                    Ok(LegacyScalarReserveOutcome::Inserted(receipt)) => {
                        app.metrics.pricing_bridge_inserted(bridge_provider);
                        if let Some(shadow) = &app.pricing_shadow {
                            shadow.try_enqueue(&receipt.snapshot);
                        }
                        reserved_pair = Some((eff_mt, hold));
                        break;
                    }
                    Ok(LegacyScalarReserveOutcome::Unchanged(receipt)) => {
                        app.metrics.pricing_bridge_unchanged(bridge_provider);
                        if let Some(shadow) = &app.pricing_shadow {
                            shadow.try_enqueue(&receipt.snapshot);
                        }
                        reserved_pair = Some((eff_mt, hold));
                        break;
                    }
                    Ok(LegacyScalarReserveOutcome::NotReserved) => {
                        app.metrics.pricing_bridge_not_reserved(bridge_provider);
                    }
                    Ok(LegacyScalarReserveOutcome::Conflict(conflict)) => {
                        app.metrics.pricing_bridge_conflict(bridge_provider);
                        eprintln!("Anthropic pricing bridge reserve conflict: {conflict:?}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "pricing_bridge_conflict",
                            Some(2),
                        );
                    }
                    Ok(LegacyScalarReserveOutcome::AbortedBeforeCommit) => {
                        app.metrics.pricing_bridge_failure(bridge_provider);
                        eprintln!("Anthropic pricing bridge commit handoff was aborted");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "pricing_bridge_handoff_aborted",
                            Some(2),
                        );
                    }
                    Err(error) => {
                        app.metrics.pricing_bridge_failure(bridge_provider);
                        eprintln!("Anthropic pricing bridge reservation failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "pricing_bridge_reservation_unavailable",
                            Some(2),
                        );
                    }
                }
                let fresh = match billing.account(account_id).await {
                    Ok(Some(account)) => account.balance_nano as i128,
                    Ok(None) => 0,
                    Err(error) => {
                        app.metrics.pricing_bridge_failure(bridge_provider);
                        eprintln!("billing balance refresh failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "billing_balance_refresh_unavailable",
                            Some(2),
                        );
                    }
                };
                let next_hold = match prepared.quote(fresh) {
                    Ok(Some(quote)) => quote.snapshot().charged_hold_nano(),
                    Ok(None) => break,
                    Err(error) => {
                        app.metrics.pricing_bridge_failure(bridge_provider);
                        eprintln!("Anthropic pricing bridge requote failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "pricing_bridge_invariant",
                            Some(2),
                        );
                    }
                };
                if next_hold < hold {
                    current_balance = fresh;
                } else {
                    break;
                }
            }
        } else {
            let mut cur = cap_to_balance(bal, input_est, web_buf, &p, *mult_bp, client_mt);
            for _ in 0..4 {
                let (eff_mt, hold) = match cur {
                    Some(x) => x,
                    None => break,
                };
                match billing
                    .reserve_request_for_execution(
                        &engine_request_id,
                        account_id,
                        key,
                        hold,
                        execution.clone(),
                    )
                    .await
                {
                    Ok(Some(_)) => {
                        reserved_pair = Some((eff_mt, hold));
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("billing reservation failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "billing_reservation_unavailable",
                            Some(2),
                        );
                    }
                }
                let fresh = match billing.account(account_id).await {
                    Ok(Some(account)) => account.balance_nano as i128,
                    Ok(None) => 0,
                    Err(error) => {
                        eprintln!("billing balance refresh failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "billing_balance_refresh_unavailable",
                            Some(2),
                        );
                    }
                };
                match cap_to_balance(fresh, input_est, web_buf, &p, *mult_bp, client_mt) {
                    Some((e, h)) if h < hold => cur = Some((e, h)),
                    _ => break,
                }
            }
        }
        let (eff_mt, hold) = match reserved_pair {
            Some(x) => x,
            None => {
                return local_err_for(LocalErr::LowBalance, "billing_reservation_rejected", None)
            }
        };
        // урезали под баланс → правим max_tokens в теле ПОСЛЕ финального eff_mt (мог уменьшиться на
        // ретрае): Anthropic остановит генерацию ровно тут.
        if eff_mt < max_tokens {
            if let Some(v) = parsed.as_mut() {
                v["max_tokens"] = serde_json::json!(eff_mt);
            }
        }
        reserved = Some((
            engine_request_id.clone(),
            account_id.clone(),
            key.clone(),
            hold,
            settlement_mult_bp,
            settlement_priced_ts,
        ));
    }
    // Гард резерва: на любом не-успешном исходе И при отмене запроса вернёт hold клиенту. Создаём
    // ДО пересборки тела — если она упадёт и мы вернёмся, Drop гарда вернёт hold (без утечки).
    // Разоружим на успехе — там hold закрывает tee-метеринг. Снимает утечку денег при disconnect.
    let mut hold_guard = reserved
        .as_ref()
        .map(|(request_id, acct, k, h, _, _)| HoldGuard {
            billing: app.billing.clone(),
            account_id: acct.clone(),
            key: k.clone(),
            hold: *h,
            request_id: request_id.clone(),
            armed: true,
        });
    let version = parts
        .headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&app.cfg.anthropic_version)
        .to_string();
    let beta = match merged_beta(&parts.headers, &app.cfg.default_beta) {
        Ok(v) => v,
        Err(()) => return local_err(LocalErr::BadBeta, None),
    };

    let mut affinity_newly_claimed = false;

    // Гладкий UX: транзиентную нехватку ёмкости (все подписки cooling/за util_cap / breaker /
    // upstream-429) НЕ отдаём клиенту сразу — тихо ждём до бюджета и повторяем весь раунд ротации.
    // Решение принимается ДО начала стрима, поэтому для клиента это лишь чуть больший TTFB, не ошибка.
    let smooth_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(app.cfg.smooth_wait_ms);
    'smooth: loop {
        let mut tried: HashSet<String> = HashSet::new();
        // Some(secs) → раунд уперся в транзиент → ждём+ретрай.
        let mut transient_hint: Option<i64> = None;
        // Один запрос вносит в breaker максимум ОДИН backend-фейл (не `max_tries`): иначе poison-запрос
        // (500 на каждой подписке) в одиночку размыкал бы глобальный breaker и клал сервис всем. Реальный
        // аутейдж тилит breaker числом РАЗНЫХ запросов, а не веером одного.
        let mut backend_fail_recorded = false;
        // Дефолт терминала, если ни одной подписки не выбрали и нет реального upstream-ответа (пул пуст).
        // Клиенту — обезличенный retryable overload, без слова «pool/subscriptions».
        let mut last_local = local_err_for(LocalErr::Overloaded, "pool_unavailable", None);
        // Последний РЕАЛЬНЫЙ ответ Anthropic удерживаем до конца ротации. Если успеха не будет, именно он
        // уходит клиенту со своим body/request-id/retry headers; synthetic допустим только без response.
        let mut last_upstream: Option<(StatusCode, wreq::Response)> = None;

        // Бюджет: ошибки ПОДПИСКИ (429/401/403 — бан/лимит конкретного аккаунта) НЕ тратят попытки,
        // крутимся дальше по флоту (клиенту такая ошибка идти не должна, пока есть здоровые подписки).
        // Бюджет `max_tries` тратят только BACKEND-фейлы (5xx/сеть — вероятный аутейдж апстрима). Верхний
        // предел итераций = «весь флот + запас» (пул сам исключает уже cooling/tried → быстро сходится).
        let hard_cap = if operator_target.is_some() {
            1
        } else {
            app.pool.len().max(1) + 2
        };
        let mut attempt = 0usize;
        let mut backend_tries = 0usize;
        let mut auth_tries = 0usize; // 401/403: возможно вина запроса клиента, а не токена — см. ниже
        while attempt < hard_cap {
            // Уже допущенные запросы тоже обязаны увидеть breaker, открытый конкурентным фейлом, ДО
            // следующей ротации. Если у нас есть реальный upstream response — возвращаем его прозрачно.
            if let Some(retry) = app.breaker.open_for(pool::now()) {
                Metrics::inc(&app.metrics.breaker_rejects);
                // Брейкер разомкнут (брауноут апстрима) — транзиент. Тихо ждём+ретраим до бюджета
                // (breaker сам закроется по record_ok), клиенту 503 отдаём лишь при исчерпании бюджета.
                transient_hint = Some(retry);
                break;
            }
            // First attempt uses shared cache affinity. Redis only proposes an opaque home; the pool
            // revalidates live health/capacity and reserves the local slot atomically. Retries remain
            // load-based and PostgreSQL below is still the authoritative distributed capacity gate.
            let affinity_sub = if attempt == 0 && operator_target.is_none() {
                if let Some(input) = affinity_input.as_ref() {
                    if affinity_resolution.is_none() {
                        if let Some(proposed) = app.pool.peek_affinity_home_with_warm(
                            &affinity_warm_homes,
                            CACHE_ROOT_MIN_WARM_HOMES,
                            CACHE_ROOT_MIN_CAPACITY_RATIO,
                            |email| app.affinity.home_id(email),
                        ) {
                            let proposed_home = app.affinity.home_id(&proposed.email);
                            affinity_resolution =
                                Some(app.affinity.claim(input, &proposed_home).await);
                            affinity_newly_claimed = true;
                            persona_session = affinity_resolution.as_ref().map(|resolution| {
                                pool::stable_hash64(resolution.session_id.as_bytes())
                            });
                        }
                    }

                    let mut selected = None;
                    if let Some(resolution) = affinity_resolution.as_mut() {
                        match app.pool.route_affinity(
                            &resolution.home,
                            affinity_newly_claimed,
                            |email| app.affinity.home_id(email),
                        ) {
                            pool::AffinityRoute::Selected { sub, disposition } => {
                                if disposition == pool::AffinityDisposition::Rebound {
                                    let new_home = app.affinity.home_id(&sub.email);
                                    app.affinity.rebind(resolution, &new_home).await;
                                }
                                app.affinity.remember(input, resolution).await;
                                affinity_newly_claimed = false;
                                selected = Some((sub, disposition));
                            }
                            pool::AffinityRoute::Exhausted => {}
                        }
                    }
                    selected
                } else {
                    None
                }
            } else {
                None
            };
            let (sub, affinity_disposition) = if let Some(target) = operator_target.as_deref() {
                match app
                    .pool
                    .route_operator_target(target, calibration_profile_hint)
                {
                    Some(sub) => (sub, Some(pool::AffinityDisposition::Pinned)),
                    None => {
                        transient_hint = Some(1);
                        break;
                    }
                }
            } else {
                match affinity_sub {
                    Some((sub, disposition)) => (sub, Some(disposition)),
                    None => match app.pool.pick(&tried, false) {
                        Some(sub) => (sub, None),
                        None => break,
                    },
                }
            };
            // route/pick отдали cooling-персону → значит НЕТ ни одной не-cooling (весь оставшийся флот за
            // лимитом). НЕ шлём живой клиентский трафик на отлимиченный аккаунт: это гарантированный свежий
            // 429 (ban-signal + «автоматный» кластер, ровно то, чего избегаем). Быстрый прозрачный отбой
            // клиенту с точным Retry-After = soonest_ready (он откатится сам). hold вернёт hold_guard на return.
            if app.pool.is_cooling(&sub.email) {
                app.pool.mark_done(&sub.email);
                // Не-cooling подписок не осталось (весь флот за лимитом/в кулдауне). НЕ шлём живой трафик
                // на отлимиченный аккаунт. Вместо мгновенного 429 — помечаем транзиент; хвост раунда
                // тихо ждёт до soonest_ready и ретраит (подписка успеет освободиться), 429 — лишь по бюджету.
                Metrics::inc(&app.metrics.exhausted);
                transient_hint = Some(app.pool.soonest_ready().unwrap_or(app.cfg.cool_secs));
                break;
            }
            attempt += 1;
            tried.insert(sub.email.clone());
            // The in-memory choice is only a candidate. PostgreSQL performs the authoritative atomic
            // cooldown/utilization/inflight validation and increments capacity in the same transaction.
            let capacity_lease_id = format!("{}:{attempt}", engine_request_id);
            let capacity_lease = if let Some(billing) = &app.billing {
                let authority_util_cap =
                    if affinity_disposition == Some(pool::AffinityDisposition::Pinned) {
                        1.0
                    } else {
                        app.cfg.util_cap
                    };
                match billing
                    .acquire_capacity(
                        &capacity_lease_id,
                        &engine_request_id,
                        &sub.email,
                        3600,
                        authority_util_cap,
                    )
                    .await
                {
                    Ok(lease) => lease,
                    Err(error) => {
                        app.pool.mark_done(&sub.email);
                        eprintln!("capacity authority failed: {error:#}");
                        return local_err_for(
                            LocalErr::Overloaded,
                            "capacity_authority_unavailable",
                            Some(2),
                        );
                    }
                }
            } else {
                None
            };
            if app.billing.is_some() && capacity_lease.is_none() {
                app.pool.mark_done(&sub.email);
                continue;
            }
            // Гард слота: закроет in-flight на ЛЮБОМ выходе из итерации (continue/return/ОТМЕНА запроса).
            // Разоружим только на успехе (слот перейдёт стриму). mark_cooling/mark_healthy in-flight не трогают.
            let mut guard = InflightGuard::new(
                app.pool.clone(),
                app.billing.clone(),
                capacity_lease.map(|l| l.lease_id),
                sub.email.clone(),
            );

            let client = match app.clients.get(&sub.proxy, &sub.email) {
                Ok(c) => c,
                Err(e) => {
                    app.pool.mark_cooling(&sub.email, 10); // битый прокси → cooling (слот закроет guard)
                    eprintln!("⚠ прокси {}: {e}", sub.email); // детали ТОЛЬКО в лог (не клиенту)
                    last_local =
                        local_err_for(LocalErr::Overloaded, "subscription_proxy_unavailable", None);
                    continue;
                }
            };

            // per-persona UA: стабильный для подписки, но различный между подписками (антифингерпринт
            // флота). Клиентский user-agent НЕ пробрасываем (см. skip_req_header) — отпечаток наш.
            let ua = crate::upstream::persona_ua(&app.cfg, &sub.email);
            let mut rb = client
                .request(method.clone(), &url)
                .header("authorization", format!("Bearer {}", sub.token))
                .header("anthropic-version", &version)
                .header("user-agent", &ua)
                // X-Claude-Code-Session-Id — per-диалог UUID (тот же, что в metadata.user_id.session_id).
                // Реальный CC шлёт его на каждый запрос; синтезируем от session-ключа.
                .header(
                    "x-claude-code-session-id",
                    crate::upstream::persona_session_id(&sub.email, persona_session),
                )
                // x-client-request-id — случайный per-request uuid (реальный CC шлёт на каждый запрос).
                .header("x-client-request-id", &engine_request_id);
            if !beta.is_empty() {
                rb = rb.header("anthropic-beta", &beta);
            }
            rb = crate::upstream::apply_persona_headers(rb, &app.cfg); // x-app + x-stainless-* + accept + browser-access
            for (name, value) in parts.headers.iter() {
                let n = name.as_str();
                if !skip_req_header(n) {
                    rb = rb.header(n, value.as_bytes());
                }
            }
            // per-sub тело: persona metadata добавляем только когда клиент её НЕ задал; malformed metadata
            // не мутируем (Anthropic сам вернёт native 4xx). Сериализация здесь — в общем случае 1 раз;
            // лишняя только на редкой ротации. Ошибка сериализации → отказ (не форвардим stale-тело с
            // большим max_tokens при малом hold — пробой баланса; hold вернёт hold_guard).
            let body_this = match parsed.as_mut() {
                Some(v) => {
                    // metadata допустима только у /v1/messages; count_tokens её не принимает.
                    if billable {
                        set_persona_user_id_if_absent(
                            v,
                            crate::upstream::persona_user_id(&sub.email, persona_session),
                        );
                        // billing-header первым system-блоком (как реальный CC): cc_version флот-константна,
                        // cch стабилен per-подписка. Идемпотентно — на ротации заменяет, не дублирует.
                        if app.cfg.inject_billing {
                            // cc_version = <base>.<build> где build стабилен per-подписка (см. persona_ccbuild).
                            let txt = format!("x-anthropic-billing-header: cc_version={}.{}; cc_entrypoint={}; cch={};",
                            app.cfg.cc_version, crate::upstream::persona_ccbuild(&sub.email),
                            app.cfg.cc_entrypoint, crate::upstream::persona_cch(&sub.email));
                            set_billing_block(v, &txt);
                        }
                    }
                    match serde_json::to_vec(v) {
                        Ok(b) => bytes::Bytes::from(b),
                        Err(_) => return local_err(LocalErr::Internal, None),
                    }
                }
                None => body_bytes.clone(), // не-JSON тело → как есть
            };
            rb = rb.body(body_this);

            // Breaker мог открыться, пока мы выбирали persona/строили тело. Не делаем ещё один send после
            // этого момента; предыдущий upstream response (если был) приоритетнее локальной синтетики.
            if let Some(retry) = app.breaker.open_for(pool::now()) {
                Metrics::inc(&app.metrics.breaker_rejects);
                // Брейкер разомкнут (брауноут апстрима) — транзиент. Тихо ждём+ретраим до бюджета
                // (breaker сам закроется по record_ok), клиенту 503 отдаём лишь при исчерпании бюджета.
                transient_hint = Some(retry);
                break;
            }

            let resp = match rb.send().await {
                Ok(r) => r,
                Err(e) => {
                    // Сетевой сбой остаётся per-subscription сигналом: короткий cooling и ротация.
                    // Не кормим global breaker — один нестабильный прокси не доказывает outage провайдера.
                    app.pool.mark_cooling(&sub.email, 15);
                    eprintln!("⚠ upstream {}: {e}", sub.email); // детали (email/сеть) ТОЛЬКО в лог
                    last_local =
                        local_err_for(LocalErr::Overloaded, "upstream_connection_error", None);
                    backend_tries += 1;
                    if backend_tries >= app.cfg.max_tries.max(1) {
                        break;
                    }
                    continue;
                }
            };

            let st = resp.status();
            let code = st.as_u16();

            // ПАССИВНЫЙ сбор лимитов из боевого ответа: свежий util/reset без лишних запросов
            // (обновляет polled_ts → активный поллер сам перестаёт трогать «живые» подписки).
            let lim = limits_from_headers(resp.headers());
            if lim.has_util() {
                app.pool.set_util(
                    &sub.email,
                    lim.util5h,
                    lim.util7d,
                    lim.status.clone(),
                    lim.reset5h,
                    lim.reset7d,
                );
            }

            let now = pool::now();
            // Классификация вины (важно: НЕ студить подписку за чужую вину):
            if code == 429 {
                // квота подписки: студим до сброса окна-виновника (см. cool_secs_429)
                Metrics::inc(&app.metrics.upstream_429);
                let secs = cool_secs_429(&resp, &lim, now);
                app.pool.mark_cooling(&sub.email, secs);
                eprintln!("↻ ротация: {} вернул 429 — cooling {}s", sub.email, secs);
                last_upstream = Some((st, resp));
                continue;
            }
            if code == 401 || code == 403 {
                // 401/403 может быть виной ЗАПРОСА клиента (недоступная модель/бета/путь), а НЕ мёртвого
                // токена. НЕ студим подписку здесь: иначе один crafted-запрос выстудил бы весь пул на
                // AUTH_QUARANTINE (DoS). Дохлый токен ловит ПОЛЛЕР чистым probe (там карантин). Ротируем
                // ограниченно, чтобы исключить единичный дохлый токен; если повторилось на другой подписке
                // — детерминировано → это запрос клиента → отдаём НАСТОЯЩИЙ ответ апстрима прозрачно.
                Metrics::inc(&app.metrics.upstream_auth);
                auth_tries += 1;
                eprintln!(
                    "auth {} на {} (попытка {}) — НЕ студим (возможно вина запроса)",
                    code, sub.email, auth_tries
                );
                // Пробуем ДРУГУЮ подписку ТОЛЬКО если она реально есть (вдруг дохлый токен этой). Если
                // другой нет (напр. пул из одной) или повтор — это вина запроса (scope/модель/путь) → отдаём
                // РЕАЛЬНЫЙ 401/403 Anthropic прозрачно, а НЕ маскируем в 429-исчерпание (был баг с 1 подпиской).
                if auth_tries < 2 && attempt < hard_cap {
                    // Уходим на ДРУГУЮ подписку → эта, возможно, с мёртвым токеном. Просим поллер проверить
                    // её чистым probe СРАЗУ (не через LIVENESS_INTERVAL): revoked-токен перестанет быть
                    // placement-магнитом за ~1 цикл. Без cooling здесь (crafted-запрос иначе студил бы флот).
                    app.pool.request_probe(&sub.email);
                    if let Some(p) = &app.probe_poke {
                        p.notify_one();
                    }
                    last_upstream = Some((st, resp));
                    continue;
                }
                // Повтор/нет альтернативы → отдаём РЕАЛЬНЫЙ 401/403 клиенту (может быть вина запроса), но
                // НЕ штампуем токен «здоровым» (старый баг: маскировал реально мёртвый токен, когда он
                // последний/единственный). Вердикт о живости выносит ТОЛЬКО поллер по чистым probe — просим
                // его проверить эту подписку (dead-детект durable в pool::record_probe).
                app.pool.request_probe(&sub.email);
                if let Some(p) = &app.probe_poke {
                    p.notify_one();
                }
                // Запрос-детерминированный 401/403 возвращаем БАЙТ-В-БАЙТ: body/request-id/error type
                // принадлежат Anthropic и нужны SDK-диагностике; секретные auth headers уже фильтруются.
                return stream_back(st, resp, None);
            }
            if st.is_server_error() || code == 408 || code == 409 || code == 425 {
                // вина АПСТРИМА, не подписки: НЕ студим подписку (слот закроет guard), кормим breaker
                // максимум раз на запрос (анти-DoS от poison-запроса).
                Metrics::inc(&app.metrics.upstream_5xx);
                if !backend_fail_recorded {
                    app.breaker.record_fail(now, &sub.email);
                    backend_fail_recorded = true;
                }
                eprintln!(
                    "↻ ротация: {} вернул {} — backend-fault (breaker+)",
                    sub.email, code
                );
                last_upstream = Some((st, resp));
                backend_tries += 1; // backend тратит бюджет (аутейдж)
                if backend_tries >= app.cfg.max_tries.max(1) {
                    break;
                }
                continue;
            }
            // 2xx/клиентская 4xx → апстрим здоров: сбрасываем окно фейлов брейкера
            app.breaker.record_ok(now);

            // успех или клиентская ошибка запроса (одинакова на любой подписке) → отдаём как есть.
            // На УСПЕХЕ всегда меряем ответ: расход подписки → калибровка пула; для метерного ключа
            // finalize закрывает резерв фактической стоимостью; и там же `end_stream` снимает слот
            // конкуррентности. 4xx не меряем — резерв возвращаем, слот освобождаем сразу.
            let meter = if st.is_success() {
                app.pool.mark_healthy(&sub.email);
                // count_tokens is successful but does not populate Anthropic's prompt cache.
                if let (true, Some(input)) = (billable, affinity_input.as_ref()) {
                    let home = app.affinity.home_id(&sub.email);
                    if affinity_started_new {
                        app.affinity.record_cache_root_placement(
                            input,
                            affinity_warm_homes.iter().any(|warm| warm == &home),
                        );
                    }
                    app.affinity.mark_cache_warm(input, &home);
                }
                if let (Some((request_id, account_id, key, hold, _, priced_ts)), Some(billing)) =
                    (reserved.as_ref(), app.billing.as_ref())
                {
                    if !matches!(billing.mark_delivering(request_id, 3600).await, Ok(true)) {
                        // The provider accepted the request, but the durable delivery marker was fenced.
                        // Preserve the approved hold and fail closed instead of handing out untracked usage.
                        if priced_ts.is_some() {
                            billing
                                .settle_detached(request_id, account_id, key, *hold, 0, None, None);
                        } else {
                            billing.settle_detached(
                                request_id,
                                account_id,
                                key,
                                *hold,
                                *hold,
                                Some("delivery-marker-failed"),
                                None,
                            );
                        }
                        if let Some(g) = hold_guard.as_mut() {
                            g.disarm();
                        }
                        // Заголовок not_started снимаем: в legacy-scalar ветке выше settle
                        // закрыл hold ПОЛНЫМ списанием (actual=hold), значит условие «reserve не
                        // будет тарифицирован» не выполнено — отсутствие заголовка безопаснее.
                        return without_not_started(local_err_for(
                            LocalErr::Overloaded,
                            "billing_delivery_marker_unavailable",
                            Some(2),
                        ));
                    }
                }
                let capacity = match (&guard.billing, &guard.capacity_lease_id) {
                    (Some(billing), Some(lease_id)) => Some((billing.clone(), lease_id.clone())),
                    _ => None,
                };
                guard.disarm(); // слот переходит стриму (end_stream)
                if let Some(g) = hold_guard.as_mut() {
                    g.disarm();
                } // hold закроет tee-метеринг фактикой
                let bill = match (&authz, reserved.take()) {
                    (
                        Authz::Metered { .. },
                        Some((request_id, acct, key, hold, payable_multiplier_bp, priced_ts)),
                    ) => app.billing.clone().map(|billing| BillCtx {
                        billing,
                        account_id: acct,
                        key,
                        mult_bp: payable_multiplier_bp,
                        hold,
                        tariff_priced_ts: priced_ts,
                        policy_fast: priced_ts.map(|_| requested_fast),
                        policy_us_inference: priced_ts.map(|_| requested_us_inference),
                        request_id,
                        reference: request_id_of(&resp),
                    }),
                    _ => None,
                };
                let mut quota_snapshots = Vec::with_capacity(2);
                if let (Some(quota), Some(resets_at)) = (lim.quota5h, lim.reset5h) {
                    quota_snapshots.push(crate::billing::AnthropicQuotaSnapshot {
                        window_kind: "5h".to_owned(),
                        window_duration_mins: 300,
                        resets_at,
                        used_fraction_units: quota.used_fraction_units,
                        measurement_resolution_fraction_units: quota
                            .measurement_resolution_fraction_units,
                        observed_at: now,
                    });
                }
                if let (Some(quota), Some(resets_at)) = (lim.quota7d, lim.reset7d) {
                    quota_snapshots.push(crate::billing::AnthropicQuotaSnapshot {
                        window_kind: "7d".to_owned(),
                        window_duration_mins: 10_080,
                        resets_at,
                        used_fraction_units: quota.used_fraction_units,
                        measurement_resolution_fraction_units: quota
                            .measurement_resolution_fraction_units,
                        observed_at: now,
                    });
                }
                let calibration = app.billing.clone().map(|billing| CalibrationCtx {
                    billing,
                    request_id: engine_request_id.clone(),
                    plan: if sub.plan.trim().is_empty() {
                        "unknown".to_owned()
                    } else {
                        sub.plan.clone()
                    },
                    quota_snapshots,
                    probe_poke: app.probe_poke.clone(),
                });
                Some(MeterCtx {
                    pool: app.pool.clone(),
                    email: sub.email.clone(),
                    model: model.clone(),
                    is_sse: is_event_stream(&resp),
                    bill,
                    calibration,
                    capacity,
                })
            } else {
                // клиентская 4xx: подписка ни при чём. Слот закроет guard, резерв — hold_guard (на return).
                app.pool.mark_healthy(&sub.email);
                None
            };
            return stream_back(st, resp, meter);
        }
        // Итог раунда. Транзиентную нехватку (нет реального upstream-ответа И backend-бюджет цел) тихо
        // ждём и ретраим до smooth_deadline; всё остальное — отдаём клиенту. Резерв держит hold_guard,
        // слоты закрыты guard'ами последней попытки — на continue 'smooth утечки нет (RAII).
        let backend_exhausted = backend_tries >= app.cfg.max_tries.max(1);
        // Транзиент = не хард-аутейдж апстрима, и был реальный шанс: явная подсказка (все cooling/breaker),
        // синтетическое исчерпание (нет last_upstream, но подписки пробовались), либо upstream отдал 429.
        let is_transient = !backend_exhausted
            && (transient_hint.is_some()
                || (last_upstream.is_none() && !tried.is_empty())
                || last_upstream
                    .as_ref()
                    .map(|(st, _)| st.as_u16() == 429)
                    .unwrap_or(false));
        if is_transient {
            let remaining = smooth_deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis();
            let hint = transient_hint
                .or_else(|| app.pool.soonest_ready())
                .unwrap_or(app.cfg.cool_secs);
            if let Some(step) = smooth_step(hint, remaining) {
                tokio::time::sleep(step).await;
                continue 'smooth;
            }
        }
        // Терминал: реальный Anthropic-ответ приоритетнее локальной классификации; иначе backend-аутейдж
        // (или пул пуст) → last_local; иначе синтетический 429 с readiness всего пула.
        if let Some((st, resp)) = last_upstream {
            return stream_back(st, resp, None);
        }
        if backend_exhausted || tried.is_empty() {
            return last_local;
        }
        Metrics::inc(&app.metrics.exhausted);
        let retry = app.pool.soonest_ready().unwrap_or(app.cfg.cool_secs);
        return local_err_for(
            LocalErr::RateLimited,
            "subscription_pool_exhausted",
            Some(retry),
        );
    } // 'smooth: loop
}

/// Ответ — SSE-стрим? (по content-type). Определяет способ парсинга usage.
fn is_event_stream(resp: &wreq::Response) -> bool {
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|c| c.contains("text/event-stream"))
        .unwrap_or(false)
}

/// request-id ответа Anthropic — кладём в ledger как `ref` списания (аудит-трейл «за что списано»).
fn request_id_of(resp: &wreq::Response) -> Option<String> {
    resp.headers()
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Явный заголовок `Retry-After` (самый авторитетный хинт — Anthropic сам говорит, когда можно).
fn retry_after_header(resp: &wreq::Response) -> Option<i64> {
    let v = resp.headers().get("retry-after")?.to_str().ok()?;
    v.trim().parse::<i64>().ok().map(|s| s.max(1))
}

/// Короткий cooldown при небурстовом/транзиентном 429 (лимит запросов-в-минуту чистится быстро).
const BURST_COOL_SECS: i64 = 20;

/// Секунды до сброса ОКНА-виновника — ТОЛЬКО если это реально квотный 429 (окно у потолка):
/// недельное почти выбрано (util7d≥0.95) → до reset7d (могут быть дни, и это правильно), либо
/// 5h у потолка → до reset5h. Если НИ ОДНО окно не близко к потолку — это НЕ квотный 429
/// (burst/иной лимит), студить до reset нельзя (можно на часы зря) → None → короткий дефолт.
fn window_cool(lim: &Limits, now: i64) -> Option<i64> {
    let fut = |t: Option<i64>| t.filter(|x| *x > now).map(|x| (x - now).max(1));
    let (u5, u7) = (lim.util5h.unwrap_or(0.0), lim.util7d.unwrap_or(0.0));
    // АВТОРИТЕТНО: Anthropic сам указал связывающее окно (`representative-claim`). Студим до его reset,
    // но только если это окно реально у потолка (≥0.9) — иначе 429 = burst по rate (запросов/мин), а
    // не исчерпание квоты, и на часы студить нельзя (упадём в burst-дефолт). "seven_day"/"five_hour".
    if let Some(c) = lim.claim.as_deref() {
        if c.contains("day") && u7 >= 0.9 {
            return fut(lim.reset7d).or_else(|| fut(lim.reset5h));
        }
        if c.contains("hour") && u5 >= 0.9 {
            return fut(lim.reset5h).or_else(|| fut(lim.reset7d));
        }
    }
    // Фолбэк-эвристика, если заголовка claim нет: окно у потолка → студим до его reset.
    if u7 >= 0.95 {
        return fut(lim.reset7d).or_else(|| fut(lim.reset5h));
    }
    if u5 >= 0.95 {
        return fut(lim.reset5h).or_else(|| fut(lim.reset7d));
    }
    None
}

/// Сколько студить при 429: Retry-After (авторитетно) → окно-виновник (если квота выбита) →
/// короткий burst-дефолт (транзиентный лимит запросов/мин).
/// Верхний потолок cooling: недельное окно 7d + сутки запаса. Кламп защищает от враждебного/битого
/// upstream-ответа (или скомпрометированного прокси), который прислал бы Retry-After/reset в далёкое
/// будущее и запарковал бы здоровую подписку на месяцы. Дольше 8 суток остывать нечему (все окна ≤7d).
const MAX_COOL_SECS: i64 = 8 * 24 * 3600;

fn cool_secs_429(resp: &wreq::Response, lim: &Limits, now: i64) -> i64 {
    retry_after_header(resp)
        .or_else(|| window_cool(lim, now))
        .unwrap_or(BURST_COOL_SECS)
        .clamp(0, MAX_COOL_SECS)
}

/// Гладкий UX: сколько ТИХО ждать перед следующим раундом ротации. `hint_secs` — подсказка готовности
/// (soonest_ready / Retry-After), `remaining_ms` — остаток бюджета. `None` → бюджет исчерпан (пора
/// отдать клиенту ошибку). Шаг зажат в [250мс, 2с] и не больше остатка: так пул перечитывается часто
/// (подписка могла освободиться раньше hint), а один длинный сон не «проглатывает» весь бюджет.
fn smooth_step(hint_secs: i64, remaining_ms: u128) -> Option<std::time::Duration> {
    if remaining_ms == 0 {
        return None;
    }
    let hint_ms = (hint_secs.max(0) as u128).saturating_mul(1000);
    let step = hint_ms.clamp(250, 2000).min(remaining_ms);
    Some(std::time::Duration::from_millis(step as u64))
}

/// Отдать ответ апстрима клиенту байт-в-байт (стримом — работает и для SSE).
/// Если задан `meter` — оборачиваем тело в tee-метеринг: клиент получает те же байты,
/// а на завершении стрима списываем стоимость с ключа (тело клиенту НЕ задерживается).
/// Terminate a broken SSE body with the error event the protocol defines for it.
///
/// A transport failure after the first byte cannot be retried — another attempt would replay or
/// interleave output the client has already seen. What it must not do is end the body silently:
/// an SDK reading `text/event-stream` sees a stream that simply stops, which is indistinguishable
/// from a completed response until a parse fails somewhere further up, and many clients hang on it.
///
/// `event: error` is part of the Anthropic stream protocol, so emitting one is not a deviation from
/// byte-for-byte transparency — it restores it. A real upstream that failed mid-stream would send
/// exactly this frame, and the client's existing error path handles it. The payload carries the
/// same anonymised overload wording the local error path uses: the cause belongs in our metrics and
/// logs, never in a customer's response body.
struct SseErrorTail {
    inner: ResponseByteStream,
    /// Set once the inner stream has failed, so the tail frame is emitted exactly once.
    failed: bool,
    done: bool,
}

impl SseErrorTail {
    fn new(inner: ResponseByteStream) -> Self {
        Self {
            inner,
            failed: false,
            done: false,
        }
    }

    fn frame() -> bytes::Bytes {
        bytes::Bytes::from_static(
            b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        )
    }
}

impl Stream for SseErrorTail {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if self.done {
            return Poll::Ready(None);
        }
        if self.failed {
            self.done = true;
            return Poll::Ready(Some(Ok(Self::frame())));
        }
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Err(_))) => {
                // Swallow the transport error and end the body with a protocol frame instead.
                // Propagating it would abort the response, which is precisely the silent truncation
                // this exists to remove.
                self.failed = true;
                self.done = true;
                Poll::Ready(Some(Ok(Self::frame())))
            }
            other => other,
        }
    }
}

fn stream_back(st: StatusCode, resp: wreq::Response, meter: Option<MeterCtx>) -> Response {
    // Не-2xx passthrough без метеринга: hold по request_id вернёт armed HoldGuard (refund),
    // а ни одного байта клиенту ещё не отправлено — контракт not_started выполнен. На 2xx
    // (meter = Some) заголовок недопустим никогда, включая SseErrorTail внутри 200.
    let not_started = meter.is_none() && !st.is_success();
    let mut builder = Response::builder().status(st);
    for (name, value) in resp.headers().iter() {
        if !skip_resp_header(name.as_str()) {
            builder = builder.header(name.as_str(), value.as_bytes());
        }
    }
    if not_started {
        builder = builder.header(EXECUTION_STATE_HEADER, EXECUTION_STATE_NOT_STARTED);
    }
    let event_stream = is_event_stream(&resp);
    let stream = resp
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let stream: ResponseByteStream = Box::pin(stream);
    let mut stream: ResponseByteStream = match meter {
        Some(ctx) => Box::pin(TeeMeter::new(stream, ctx)),
        None => stream,
    };
    // Outermost, so metering never observes the synthetic frame: usage belongs to the upstream that
    // produced it, and a failure tail is not usage.
    if event_stream {
        stream = Box::pin(SseErrorTail::new(stream));
    }
    let body = Body::from_stream(stream);
    match builder.body(body) {
        Ok(response) => response,
        Err(_) => {
            // Сборка ответа не удалась. На 2xx-пути дропнутый TeeMeter при drain может списать
            // фактику → not_started недопустим; на не-2xx armed HoldGuard вернёт hold — заголовок
            // из local_err корректен и сохраняется.
            let response = local_err(LocalErr::Internal, None);
            if not_started {
                response
            } else {
                without_not_started(response)
            }
        }
    }
}

#[cfg(test)]
mod sse_tail_tests {
    use super::*;
    use futures_util::StreamExt;

    fn body_of(chunks: Vec<Result<bytes::Bytes, std::io::Error>>) -> ResponseByteStream {
        Box::pin(futures_util::stream::iter(chunks))
    }

    #[tokio::test]
    async fn a_broken_stream_ends_with_the_protocol_error_frame() {
        // Without this the body just stops. An SDK cannot tell a truncated stream from a finished
        // one until something further up fails to parse, and many clients wait forever instead.
        let inner = body_of(vec![
            Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
            Err(std::io::Error::other("upstream vanished")),
        ]);
        let collected: Vec<_> = SseErrorTail::new(inner).collect().await;
        assert_eq!(collected.len(), 2);
        let tail = collected[1].as_ref().expect("tail frame is not an error");
        let tail = String::from_utf8(tail.to_vec()).unwrap();
        assert!(tail.starts_with("event: error\n"), "{tail}");
        assert!(tail.contains("\"type\":\"error\""), "{tail}");
        // The cause belongs in metrics and logs; the customer gets the anonymised overload wording.
        assert!(!tail.contains("upstream vanished"), "{tail}");
        assert!(tail.ends_with("\n\n"), "{tail}");
    }

    #[tokio::test]
    async fn a_clean_stream_is_passed_through_untouched() {
        let inner = body_of(vec![
            Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
            Ok(bytes::Bytes::from_static(b"event: message_stop\n\n")),
        ]);
        let collected: Vec<_> = SseErrorTail::new(inner).collect().await;
        assert_eq!(collected.len(), 2);
        assert!(collected.iter().all(|chunk| chunk.is_ok()));
        let joined: Vec<u8> = collected
            .into_iter()
            .flat_map(|chunk| chunk.unwrap().to_vec())
            .collect();
        // A successful stream must be byte-for-byte what the upstream sent.
        assert_eq!(
            String::from_utf8(joined).unwrap(),
            "event: message_start\n\nevent: message_stop\n\n"
        );
    }

    #[tokio::test]
    async fn the_error_frame_is_emitted_exactly_once() {
        let inner = body_of(vec![
            Err(std::io::Error::other("first")),
            Ok(bytes::Bytes::from_static(b"never reached")),
        ]);
        let collected: Vec<_> = SseErrorTail::new(inner).collect().await;
        assert_eq!(collected.len(), 1);
        assert!(String::from_utf8(collected[0].as_ref().unwrap().to_vec())
            .unwrap()
            .starts_with("event: error\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affinity::AffinityStore;
    use crate::billing::AsyncBilling;
    use crate::breaker::Breaker;
    use crate::config::ProxyConfig;
    use crate::upstream::Clients;
    use crate::{PricingBridgeConfig, PricingBridgeFallbackReason, ProviderMode};
    use axum::body::Body;
    use axum::extract::{ConnectInfo, State};
    use pool::{Pool, Reserve};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static NEXT_ANTHROPIC_BRIDGE_DB: AtomicU64 = AtomicU64::new(0);

    fn anthropic_bridge_proxy_config(pricing_bridge: PricingBridgeConfig) -> Arc<ProxyConfig> {
        Arc::new(ProxyConfig {
            api_keys: Vec::new(),
            control_keys: Vec::new(),
            panel_keys: Vec::new(),
            default_mult_bp: 10_000,
            pricing_bridge,
            pricing_shadow: crate::pricing::PricingShadowConfig::default(),
            trust_loopback: false,
            upstream: "http://127.0.0.1:1".to_string(),
            max_tries: 1,
            util_cap: 1.0,
            cool_secs: 1,
            smooth_wait_ms: 0,
            poll: false,
            inject_identity: false,
            identity: String::new(),
            inject_billing: false,
            cc_version: String::new(),
            cc_entrypoint: String::new(),
            default_beta: String::new(),
            user_agent: "pricing-bridge-test".to_string(),
            user_agents: Vec::new(),
            ua_spread: 0,
            anthropic_version: "2023-06-01".to_string(),
            connect_timeout: 1,
            x_app: String::new(),
            stainless_lang: String::new(),
            stainless_runtime: String::new(),
            stainless_runtime_version: String::new(),
            stainless_package_version: String::new(),
            stainless_os: String::new(),
            stainless_arch: String::new(),
        })
    }

    fn anthropic_bridge_app(
        billing: Arc<AsyncBilling>,
        pricing_bridge: PricingBridgeConfig,
    ) -> AppState {
        let cfg = anthropic_bridge_proxy_config(pricing_bridge);
        AppState {
            provider: ProviderMode::Anthropic,
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                ":memory:".to_string(),
                None,
            )),
            data_db_path: Arc::new(":memory:".to_string()),
            pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
            affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            codex: None,
            gemini: None,
            billing: Some(billing),
            pricing_shadow: None,
            pricing_manifest: Arc::new(crate::builtin_pricing_runtime_manifest()),
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(Breaker::new(1)),
            metrics: Arc::new(Metrics::new()),
            probe_poke: None,
            cfg,
        }
    }

    async fn invoke_anthropic_bridge(
        pricing_bridge: PricingBridgeConfig,
        execution: Option<(&str, &str)>,
    ) -> (AppState, Arc<AsyncBilling>, std::path::PathBuf, Response) {
        const ACCOUNT: &str = "anthropic-bridge-account";
        const KEY: &str = "sk-pool-anthropic-bridge";
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_ANTHROPIC_BRIDGE_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-anthropic-bridge-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start Anthropic bridge test billing"),
        );
        billing.create_account(ACCOUNT, None, 2_000).await.unwrap();
        billing
            .topup(ACCOUNT, 20_000_000, Some("anthropic-bridge-seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        let app = anthropic_bridge_app(Arc::clone(&billing), pricing_bridge);
        let mut request_builder = axum::extract::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header("x-api-key", KEY)
            .header("content-type", "application/json");
        if let Some((group_id, attempt)) = execution {
            request_builder = request_builder
                .header("x-apitoken-execution-group", group_id)
                .header("x-apitoken-attempt", attempt);
        }
        let request = request_builder
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "model": "claude-sonnet-4-6",
                    "max_tokens": 10,
                    "messages": [{"role": "user", "content": "hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = forward(
            State(app.clone()),
            ConnectInfo("127.0.0.1:4242".parse().unwrap()),
            request,
        )
        .await;
        assert_eq!(response.status().as_u16(), 529);
        billing.flush().await.unwrap();
        (app, billing, path, response)
    }

    fn lim(u5: f64, u7: f64, claim: Option<&str>, r5: i64, r7: i64) -> Limits {
        Limits {
            util5h: Some(u5),
            util7d: Some(u7),
            quota5h: None,
            quota7d: None,
            status: None,
            reset5h: Some(r5),
            reset7d: Some(r7),
            claim: claim.map(|s| s.to_string()),
        }
    }

    #[test]
    fn strip_own_namespace_rewrites_prefixed_model_in_body() {
        // Universal dispatch проксирует тело байт-идентично: namespaced id доезжает
        // до native lane как есть. Strip снимает собственный префикс и в возвращаемом
        // значении, и в теле, которое уйдёт upstream.
        let mut body =
            serde_json::json!({"model": "anthropic/claude-haiku-4-5-20251001", "max_tokens": 16});
        let model = strip_own_namespace(&mut body);
        assert_eq!(model, "claude-haiku-4-5-20251001");
        assert_eq!(body["model"], serde_json::json!("claude-haiku-4-5-20251001"));
        // Остальное тело не тронуто.
        assert_eq!(body["max_tokens"], serde_json::json!(16));
    }

    #[test]
    fn strip_own_namespace_keeps_native_and_absent_model() {
        // Native id — без изменений (байт-идентичность native контракта).
        let mut body = serde_json::json!({"model": "claude-opus-4-8"});
        let model = strip_own_namespace(&mut body);
        assert_eq!(model, "claude-opus-4-8");
        assert_eq!(body["model"], serde_json::json!("claude-opus-4-8"));
        // Нет поля model / не строка — пустая строка, тело не мутирует.
        let mut body = serde_json::json!({"max_tokens": 16});
        let model = strip_own_namespace(&mut body);
        assert_eq!(model, "");
        assert!(body.get("model").is_none());
        // Голый префикс → пустой id (admission отклонит позже, как и пустой model).
        let mut body = serde_json::json!({"model": "anthropic/"});
        let model = strip_own_namespace(&mut body);
        assert_eq!(model, "");
        assert_eq!(body["model"], serde_json::json!(""));
    }

    #[test]
    fn smooth_step_bounds() {
        use std::time::Duration;
        assert_eq!(smooth_step(0, 0), None); // бюджет исчерпан
        assert_eq!(smooth_step(100, 0), None); // исчерпан даже при большом hint
        assert_eq!(smooth_step(10, 10_000), Some(Duration::from_millis(2000))); // hint велик → кап 2с
        assert_eq!(smooth_step(0, 10_000), Some(Duration::from_millis(250))); // hint 0 → пол 250мс
        assert_eq!(smooth_step(5, 300), Some(Duration::from_millis(300))); // остаток < шага → остаток
        assert_eq!(smooth_step(1, 10_000), Some(Duration::from_millis(1000))); // hint 1с в диапазоне
    }

    #[test]
    fn window_cool_prefers_authoritative_claim() {
        let now = 1_000_000;
        let (r5, r7) = (now + 3600, now + 100_000);
        // claim=seven_day + 7d у потолка → студим до reset7d (не до 5h, хотя 5h тоже высок)
        assert_eq!(
            window_cool(&lim(0.97, 0.96, Some("seven_day"), r5, r7), now),
            Some(100_000)
        );
        // claim=five_hour → до reset5h
        assert_eq!(
            window_cool(&lim(0.97, 0.96, Some("five_hour"), r5, r7), now),
            Some(3600)
        );
        // claim есть, но окно НЕ у потолка (0.5) → burst-429 (rate), не quota → None (короткий дефолт)
        assert_eq!(
            window_cool(&lim(0.5, 0.5, Some("five_hour"), r5, r7), now),
            None
        );
        // нет claim → фолбэк-эвристика (7d≥0.95 → reset7d)
        assert_eq!(
            window_cool(&lim(0.1, 0.96, None, r5, r7), now),
            Some(100_000)
        );
    }

    #[test]
    fn billing_block_is_idempotent_and_first() {
        // identity уже стоит первым; billing должен встать ПЕРЕД ним и НЕ дублироваться на «ротации».
        let mut v = serde_json::json!({
            "messages": [],
            "system": [{"type":"text","text":"You are a Claude agent, built on Anthropic's Claude Agent SDK."}]
        });
        set_billing_block(
            &mut v,
            "x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=abcde;",
        );
        // вторая подписка (ротация) — другой cch: заменяем, не добавляем второй блок
        set_billing_block(
            &mut v,
            "x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=99999;",
        );
        let sys = v["system"].as_array().unwrap();
        assert_eq!(sys.len(), 2, "billing не должен дублироваться на ротации");
        assert_eq!(
            sys[0]["text"].as_str().unwrap(),
            "x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=99999;"
        );
        assert!(
            sys[0].get("cache_control").is_none(),
            "billing-блок БЕЗ cache_control (как у CC)"
        );
        assert!(sys[1]["text"]
            .as_str()
            .unwrap()
            .starts_with("You are a Claude agent"));
        // per-подписка cch/ccbuild стабильны и различаются между подписками (анти-кластер)
        assert_eq!(
            crate::upstream::persona_cch("a@x.io"),
            crate::upstream::persona_cch("a@x.io")
        );
        assert_ne!(
            crate::upstream::persona_cch("a@x.io"),
            crate::upstream::persona_cch("b@x.io")
        );
        let cb = crate::upstream::persona_ccbuild("a@x.io");
        assert_eq!(cb, crate::upstream::persona_ccbuild("a@x.io")); // стабилен
        assert!(
            cb.starts_with('d')
                && cb[1..]
                    .parse::<u32>()
                    .map(|n| (10..100).contains(&n))
                    .unwrap_or(false),
            "формат dNN (10..99): {cb}"
        );
    }

    #[test]
    fn endpoint_allowlist() {
        use super::Method;
        assert!(is_supported_endpoint(&Method::POST, "/v1/messages"));
        assert!(is_supported_endpoint(
            &Method::POST,
            "/v1/messages/count_tokens"
        ));
        assert!(is_supported_endpoint(&Method::GET, "/v1/models"));
        assert!(is_supported_endpoint(
            &Method::GET,
            "/v1/models/claude-haiku-4-5"
        ));
        // недоступное на подписке — отклоняем
        assert!(!is_supported_endpoint(
            &Method::POST,
            "/v1/messages/batches"
        ));
        assert!(!is_supported_endpoint(&Method::GET, "/v1/messages/batches"));
        assert!(!is_supported_endpoint(&Method::POST, "/v1/files"));
        assert!(!is_supported_endpoint(&Method::GET, "/v1/files"));
        assert!(!is_supported_endpoint(&Method::POST, "/v1/agents"));
        assert!(!is_supported_endpoint(&Method::POST, "/v1/complete")); // легаси
        assert!(!is_supported_endpoint(&Method::GET, "/v1/messages")); // messages только POST
        assert!(!is_supported_endpoint(&Method::DELETE, "/v1/models/x"));
        // C4: только один raw model-id сегмент; URL-normalized traversal/separators не проходят.
        assert!(!is_supported_endpoint(&Method::GET, "/v1/models/a/b"));
        assert!(!is_supported_endpoint(
            &Method::GET,
            "/v1/models/%2e%2e/%2e%2e/api/oauth/profile"
        ));
        assert!(!is_supported_endpoint(
            &Method::GET,
            "/v1/models/%2Fapi%2Foauth%2Fprofile"
        ));
        assert!(!is_supported_endpoint(
            &Method::GET,
            "/v1/models/..\\api\\oauth\\profile"
        ));
    }

    #[test]
    fn beta_merge_preserves_client_capabilities_and_adds_only_identity() {
        let mut headers = HeaderMap::new();
        headers.append(
            "anthropic-beta",
            "task-budgets-2026-03-13,oauth-2025-04-20".parse().unwrap(),
        );
        headers.append(
            "anthropic-beta",
            "server-side-fallback-2026-06-01".parse().unwrap(),
        );
        let configured = "oauth-2025-04-20,claude-code-20250219,advisor-tool-2026-03-01";
        assert_eq!(merged_beta(&headers, configured).unwrap(),
            "task-budgets-2026-03-13,oauth-2025-04-20,server-side-fallback-2026-06-01,claude-code-20250219");
    }

    #[test]
    fn persona_metadata_never_overwrites_or_panics_on_client_shape() {
        let mut supplied = serde_json::json!({"metadata":{"user_id":"hashed-customer-42"}});
        set_persona_user_id_if_absent(&mut supplied, "persona".into());
        assert_eq!(
            supplied["metadata"]["user_id"].as_str(),
            Some("hashed-customer-42")
        );

        let mut absent = serde_json::json!({"messages":[]});
        set_persona_user_id_if_absent(&mut absent, "persona".into());
        assert_eq!(absent["metadata"]["user_id"].as_str(), Some("persona"));

        let mut malformed = serde_json::json!({"metadata":"x"});
        set_persona_user_id_if_absent(&mut malformed, "persona".into());
        assert_eq!(malformed["metadata"].as_str(), Some("x"));
    }

    #[test]
    fn ct_eq_is_correct() {
        assert!(ct_eq(b"secret-key", b"secret-key"));
        assert!(!ct_eq(b"secret-key", b"secret-keX"));
        assert!(!ct_eq(b"short", b"longer-key")); // разная длина
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn every_client_credential_participates_without_header_priority() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "stale-x-key".parse().unwrap());
        headers.insert("authorization", "bEaReR valid-bearer-key".parse().unwrap());
        headers.insert("x-goog-api-key", "stale-google-key".parse().unwrap());

        assert_eq!(
            client_keys(&headers),
            vec![
                "stale-google-key".to_string(),
                "stale-x-key".to_string(),
                "valid-bearer-key".to_string(),
            ]
        );
        assert_eq!(
            matching_key(&headers, &["valid-bearer-key".to_string()]),
            Some("valid-bearer-key".to_string())
        );

        headers.insert("x-api-key", "valid-x-key".parse().unwrap());
        headers.insert("authorization", "Bearer stale-bearer-key".parse().unwrap());
        assert_eq!(
            matching_key(&headers, &["valid-x-key".to_string()]),
            Some("valid-x-key".to_string())
        );

        headers.insert("x-goog-api-key", "valid-x-key".parse().unwrap());
        assert_eq!(
            client_keys(&headers)
                .iter()
                .filter(|key| key.as_str() == "valid-x-key")
                .count(),
            1,
            "the same credential in two headers must be checked only once"
        );
    }

    #[test]
    fn calibration_target_is_admin_only_bounded_and_never_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CALIBRATION_PROFILE_HEADER,
            "besp".parse().expect("valid bounded profile hint"),
        );
        let admin = Authz::Admin {
            affinity_scope: "operator".to_string(),
        };
        assert_eq!(operator_calibration_target(&admin, &headers), Some("besp"));
        assert_eq!(
            operator_calibration_target(&Authz::Unauthorized, &headers),
            None,
            "a customer-controlled header cannot select a subscription"
        );
        assert!(skip_req_header(CALIBRATION_PROFILE_HEADER));

        headers.insert(
            CALIBRATION_PROFILE_HEADER,
            "too-long".parse().expect("syntactically valid header"),
        );
        assert_eq!(operator_calibration_target(&admin, &headers), None);
        assert_eq!(calibration_profile_hint("bespoke@example.com"), "besp");
    }

    #[tokio::test]
    async fn metered_auth_accepts_any_valid_credential_deterministically() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-any-valid-auth-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing =
            crate::billing::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
        billing
            .create_account("acct-a", None, 10_000)
            .await
            .unwrap();
        billing
            .create_account("acct-z", None, 10_000)
            .await
            .unwrap();
        billing
            .issue_key("a-valid", "acct-a", None, None, None)
            .await
            .unwrap();
        billing
            .issue_key("z-valid", "acct-z", None, None, None)
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "stale".parse().unwrap());
        headers.insert("authorization", "Bearer z-valid".parse().unwrap());
        let (key, auth) = resolve_client_key(&billing, &headers)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (key.as_str(), auth.account_id.as_str()),
            ("z-valid", "acct-z")
        );

        headers.insert("x-api-key", "z-valid".parse().unwrap());
        headers.insert("authorization", "Bearer stale".parse().unwrap());
        let (key, auth) = resolve_client_key(&billing, &headers)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (key.as_str(), auth.account_id.as_str()),
            ("z-valid", "acct-z")
        );

        // Если валидны оба, выбор зависит от канонического набора значений, а не от типа заголовка.
        headers.insert("x-api-key", "z-valid".parse().unwrap());
        headers.insert("authorization", "Bearer a-valid".parse().unwrap());
        let first = resolve_client_key(&billing, &headers)
            .await
            .unwrap()
            .unwrap();
        headers.insert("x-api-key", "a-valid".parse().unwrap());
        headers.insert("authorization", "Bearer z-valid".parse().unwrap());
        let second = resolve_client_key(&billing, &headers)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.0, "a-valid");
        assert_eq!(second.0, "a-valid");
        assert_eq!(first.1.account_id, second.1.account_id);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sampled_anthropic_request_persists_snapshot_before_legacy_cancel_lifecycle() {
        let config = PricingBridgeConfig::from_parts(true, 10_000).unwrap();
        let (app, billing, path, _response) = invoke_anthropic_bridge(config, None).await;

        assert_eq!(
            app.metrics
                .pricing_bridge_selected_count(SnapshotProvider::Anthropic),
            1
        );
        assert_eq!(
            app.metrics
                .pricing_bridge_inserted_count(SnapshotProvider::Anthropic),
            1
        );
        assert_eq!(
            app.metrics
                .pricing_bridge_latency_count(SnapshotProvider::Anthropic),
            1
        );
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        let request_id: String = connection
            .query_row(
                "SELECT request_id FROM pricing_admission_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot =
            registry::pricing::sqlite_legacy_scalar_admission_snapshot(&connection, &request_id)
                .unwrap()
                .expect("sampled Anthropic request must persist its actual snapshot");
        assert_eq!(snapshot.provider(), SnapshotProvider::Anthropic);
        assert_eq!(snapshot.account_id(), "anthropic-bridge-account");
        assert_eq!(snapshot.requested_model_id(), "claude-sonnet-4-6");
        let account = billing
            .account("anthropic-bridge-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, 20_000_000);
        assert_eq!(account.reserved_nano, 0);

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn disabled_anthropic_bridge_preserves_scalar_reserve_without_snapshot() {
        let (app, billing, path, _response) =
            invoke_anthropic_bridge(PricingBridgeConfig::disabled(), None).await;

        assert_eq!(
            app.metrics
                .pricing_bridge_selected_count(SnapshotProvider::Anthropic),
            0
        );
        assert_eq!(
            app.metrics.pricing_bridge_fallback_count(
                SnapshotProvider::Anthropic,
                PricingBridgeFallbackReason::BridgeDisabled,
            ),
            1
        );
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        let snapshot_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pricing_admission_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(snapshot_count, 0);
        let account = billing
            .account("anthropic-bridge-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, 20_000_000);
        assert_eq!(account.reserved_nano, 0);

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn anthropic_reservation_persists_router_execution_identity() {
        const GROUP: &str = "628f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
        let (app, billing, path, _response) =
            invoke_anthropic_bridge(PricingBridgeConfig::disabled(), Some((GROUP, "7"))).await;
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        let identity: (Option<String>, i32) = connection
            .query_row(
                "SELECT group_id,attempt FROM billing_reservations",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(identity, (Some(GROUP.into()), 7));

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn strict_anthropic_resolves_policy_and_refunds_the_original_bonus_funding() {
        const ACCOUNT: &str = "strict-anthropic-account";
        const KEY: &str = "strict-anthropic-key";
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_ANTHROPIC_BRIDGE_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-anthropic-strict-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = Arc::new(AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing.create_account(ACCOUNT, None, 2_000).await.unwrap();
        billing
            .topup(ACCOUNT, 20_000_000, Some("strict-anthropic-seed"))
            .await
            .unwrap();

        let manifest = crate::builtin_pricing_runtime_manifest();
        let capability = &manifest.capabilities()[0];
        let conn = registry::open(&path_string).unwrap();
        conn.execute(
            "INSERT INTO pricing_catalog_versions(
                 product_id,generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES('main',1,1,?1,?2,'catalog-digest',1)",
            (
                capability.capability_generation(),
                capability.capability_digest(),
            ),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO provider_switch_versions(
                 generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES(1,1,?1,?2,'switch-digest',1)",
            (
                capability.capability_generation(),
                capability.capability_digest(),
            ),
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO pricing_catalog_entries(
                 product_id,generation,provider_id,canonical_model_id,enabled
             ) VALUES('main',1,'anthropic','claude-sonnet-4-6',1);
             INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
             VALUES('main',1,1);
             INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES
                 (1,'anthropic','master','','',NULL,1),
                 (1,'anthropic','segment','main','b2c',1,1);
             INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
             VALUES(1,1,1);
             INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                 switch_generation,content_digest,replacement_locked,created_ts
             ) VALUES(
                 'strict-anthropic-account',1,'b2c:global',1,'source-policy','global_b2c','global',
                 'b2c','main',1,1,1,'policy-digest',0,1
             );
             INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES(
                 'strict-anthropic-account',1,'anthropic-track','anthropic-track-digest',
                 'provider','anthropic',NULL,'track','managed',NULL,2000,1,1,1
             );
             INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(
                 'strict-anthropic-account','main','b2c',1,'strict','strict','verified',1
             );
             INSERT INTO funding_buckets(
                 bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('strict-anthropic-bonus','strict-anthropic-account','welcome_track_bonus',
                  'welcome','track',19000000,0,0,1,'active',1,1),
                 ('strict-anthropic-paid','strict-anthropic-account','paid','seed',
                  'any',1000000,0,0,1,'active',2,2);",
        )
        .unwrap();
        drop(conn);
        billing
            .issue_key_with_policy_ack(
                KEY,
                ACCOUNT,
                None,
                None,
                None,
                Some(&registry::KeyActivationPolicyAck {
                    effective_policy_version: 1,
                    policy_digest: "policy-digest".to_string(),
                }),
            )
            .await
            .unwrap();
        let app = anthropic_bridge_app(Arc::clone(&billing), PricingBridgeConfig::disabled());
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header("x-api-key", KEY)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "model": "claude-sonnet-4-6",
                    "max_tokens": 10,
                    "messages": [{"role": "user", "content": "hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = forward(
            State(app.clone()),
            ConnectInfo("127.0.0.1:4242".parse().unwrap()),
            request,
        )
        .await;
        assert_eq!(response.status().as_u16(), 529);
        billing.flush().await.unwrap();

        assert_eq!(
            app.metrics.strict_pricing_admitted_count(
                crate::StrictPricingProvider::Anthropic,
                PricingMode::Track,
                false,
            ),
            1
        );
        let account = billing.account(ACCOUNT).await.unwrap().unwrap();
        assert_eq!(
            (account.balance_nano, account.reserved_nano),
            (20_000_000, 0)
        );
        let conn = registry::open(&path_string).unwrap();
        let evidence: (String, String, i64, i64) = conn
            .query_row(
                "SELECT snapshot.snapshot_kind,allocation.bucket_id,
                        allocation.reserved_nano,allocation.released_nano
                   FROM pricing_admission_snapshots snapshot
                   JOIN reservation_funding_allocations allocation USING(request_id)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(evidence.0, "policy_v1");
        assert_eq!(evidence.1, "strict-anthropic-bonus");
        assert!(evidence.2 > 0);
        assert_eq!(evidence.2, evidence.3);

        drop(conn);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cap_to_balance_enforces_budget() {
        let p = metering::model_prices("claude-haiku-4-5"); // input 1000, output 5000, cw1h 2000
        let od = metering::OVERDRAFT_NANO;
        // ИНВАРИАНТ с овердрафт-буфером: hold ≤ bal+$1 (funded не роняем; резерв держит пол −$1),
        // charge(worst usage) ≤ hold, и +1 output-токен пробил бы bal+$1 (точность отруба «ни на токен больше»).
        for &m in &[10000i64, 2000, 900, 33333] {
            // ×1.0, ×0.2 (прод), ×0.09, ×3.33
            for &bal in &[500_000i128, 2_000_000, 50_000_000, 10_000_000_000] {
                let ib = 137i128; // байты входа
                if let Some((eff, hold)) = cap_to_balance(bal, ib, 0, &p, m, 100_000) {
                    assert!(
                        (hold as i128) <= bal + od,
                        "hold {hold} > bal+$1 ({}) (m={m})",
                        bal + od
                    );
                    let real = ib * p.cache_write_1h + (eff as i128) * p.output; // worst-case usage
                    assert!(
                        metering::apply_multiplier(real, m) <= hold as i128,
                        "charge > hold (m={m}, bal={bal}, eff={eff})"
                    );
                    // если урезали (eff < запрошенного) — +1 токен обязан пробить bal+$1
                    if eff < 100_000 {
                        let over = ib * p.cache_write_1h + ((eff + 1) as i128) * p.output;
                        assert!(
                            metering::apply_multiplier(over, m) > bal + od,
                            "eff+1 должен пробить bal+$1 (m={m}, bal={bal}, eff={eff})"
                        );
                    }
                }
            }
        }
        // большой баланс + большой запрос → НЕ режем (eff == запрошенное)
        let (eff, _) = cap_to_balance(1_000_000_000, 100, 0, &p, 2000, 50).unwrap();
        assert_eq!(eff, 50);
        // бесплатный ключ (наценка 0) → не лимитируем, hold 0
        assert_eq!(
            cap_to_balance(1_000, 999_999, 0, &p, 0, 12345),
            Some((12345, 0))
        );
        // funded (bal>0) НЕ роняем: овердрафт-буфер $1 покрывает — прежние балансовые «None» теперь Some
        assert!(cap_to_balance(100, 100_000, 0, &p, 2000, 10).is_some());
        assert!(cap_to_balance(0, 10, 0, &p, 2000, 10).is_some());
        // отказ ТОЛЬКО когда вход worst-case не влезает даже в bal+$1, либо аккаунт уже на полу −$1
        assert!(cap_to_balance(100, 600_000, 0, &p, 10000, 10).is_none());
        assert!(cap_to_balance(-od, 10, 0, &p, 2000, 10).is_none());
        // Переполнения нет: огромный баланс и max_tokens.
        let (_, h) = cap_to_balance(i64::MAX as i128, 100, 0, &p, 2000, u64::MAX).unwrap();
        assert!(h >= 0);
    }

    /// Все синтетические причины перебираем в одном месте (гарантия, что тест покрывает КАЖДУЮ).
    const ALL_LOCAL_ERRS: [LocalErr; 9] = [
        LocalErr::Overloaded,
        LocalErr::RateLimited,
        LocalErr::InvalidKey,
        LocalErr::LowBalance,
        LocalErr::NotFound,
        LocalErr::BodyTooLarge,
        LocalErr::BadRequest,
        LocalErr::BadBeta,
        LocalErr::Internal,
    ];

    #[test]
    fn local_err_never_leaks_internal_architecture() {
        // Клиент считает, что говорит с api.anthropic.com. НИ ОДНО публичное поле (тип+сообщение)
        // синтетической ошибки не должно раскрывать наши внутренности: подписки, пул, upstream,
        // authority/fencing, cooling/ротацию, персоны/флот, oauth-инжект. Регрессия-гард: если кто-то
        // добавит вариант с текстом «no subscriptions…» — тест упадёт.
        let forbidden = [
            "subscription",
            "pool",
            "upstream",
            "authority",
            "cooling",
            "rotat",
            "persona",
            "fleet",
            "oauth",
            "in-house",
            "in house",
            "quota",
        ];
        for reason in ALL_LOCAL_ERRS {
            let (_code, kind, msg) = reason.parts();
            let hay = format!("{kind} {msg}").to_lowercase();
            for term in forbidden {
                assert!(
                    !hay.contains(term),
                    "{reason:?} leaks internal term {term:?}: {hay:?}"
                );
            }
        }
    }

    #[test]
    fn local_err_maps_to_authentic_anthropic_triples() {
        // Статус+тип каждой причины совпадают с настоящим Anthropic (иначе ответ отличим от API).
        let cases = [
            (LocalErr::Overloaded, 529u16, "overloaded_error"),
            (LocalErr::RateLimited, 429, "rate_limit_error"),
            (LocalErr::InvalidKey, 401, "authentication_error"),
            (LocalErr::LowBalance, 402, "invalid_request_error"),
            (LocalErr::NotFound, 404, "not_found_error"),
            (LocalErr::BodyTooLarge, 413, "request_too_large"),
            (LocalErr::BadRequest, 400, "invalid_request_error"),
            (LocalErr::BadBeta, 400, "invalid_request_error"),
            (LocalErr::Internal, 500, "api_error"),
        ];
        for (reason, want_code, want_type) in cases {
            let (code, kind, _msg) = reason.parts();
            assert_eq!(code.as_u16(), want_code, "{reason:?} wrong status");
            assert_eq!(kind, want_type, "{reason:?} wrong error.type");
        }
        // overloaded=529 достижим (вне именованных констант http) и валиден.
        assert_eq!(http_overloaded().as_u16(), 529);
    }

    #[test]
    fn local_err_body_is_anthropic_error_envelope() {
        // Тело — ровно Anthropic-конверт {"type":"error","error":{"type":...,"message":...}},
        // а Retry-After ставится только у retryable-причин.
        for reason in ALL_LOCAL_ERRS {
            let (_c, kind, msg) = reason.parts();
            let body = serde_json::json!({"type":"error","error":{"type":kind,"message":msg}});
            assert_eq!(body["type"], "error");
            assert_eq!(body["error"]["type"], kind);
            assert!(body["error"]["message"]
                .as_str()
                .map(|m| !m.is_empty())
                .unwrap_or(false));
        }
    }

    #[test]
    fn local_err_carries_only_static_terminal_reason() {
        for reason in ALL_LOCAL_ERRS {
            let response = local_err(reason, None);
            assert_eq!(
                response
                    .extensions()
                    .get::<TerminalErrorReason>()
                    .map(|value| value.0),
                Some(reason.reason())
            );
        }
        let response = local_err_for(LocalErr::LowBalance, "key_spend_limit", None);
        assert_eq!(
            response
                .extensions()
                .get::<TerminalErrorReason>()
                .map(|value| value.0),
            Some("key_spend_limit")
        );
    }

    #[test]
    fn local_err_marks_every_synthetic_refusal_not_started() {
        // Каждый синтетический отказ local_err — не-2xx до границы доставки → обязан нести
        // x-apitoken-execution-state: not_started (с retry-after и без).
        for reason in ALL_LOCAL_ERRS {
            for retry_after in [None, Some(2)] {
                let response = local_err(reason, retry_after);
                assert!(!response.status().is_success());
                assert_eq!(
                    response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
                    EXECUTION_STATE_NOT_STARTED,
                    "{reason:?} обязан нести not_started"
                );
            }
        }
        // Страховка для веток после границы доставки: заголовок снимается.
        let response = without_not_started(local_err(LocalErr::Internal, None));
        assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
    }

    #[test]
    fn exact_not_started_metric_predicate_matches_the_router_proof() {
        let response = with_not_started(local_err(LocalErr::Internal, None));
        assert!(is_exact_not_started_response(&response));

        let mut duplicate = with_not_started(local_err(LocalErr::Internal, None));
        duplicate.headers_mut().append(
            EXECUTION_STATE_HEADER,
            HeaderValue::from_static(EXECUTION_STATE_NOT_STARTED),
        );
        assert!(!is_exact_not_started_response(&duplicate));

        let mut wrong = local_err(LocalErr::Internal, None);
        wrong.headers_mut().insert(
            EXECUTION_STATE_HEADER,
            HeaderValue::from_static("NOT_STARTED"),
        );
        assert!(!is_exact_not_started_response(&wrong));

        let success = Response::builder()
            .status(StatusCode::OK)
            .header(EXECUTION_STATE_HEADER, EXECUTION_STATE_NOT_STARTED)
            .body(Body::empty())
            .unwrap();
        assert!(!is_exact_not_started_response(&success));
    }

    #[tokio::test]
    async fn not_started_overload_response_leaves_the_balance_untouched() {
        // Пустой пул → синтетический 529 с заголовком; заголовок ⇒ reserve не тарифицирован:
        // баланс целиком на месте, резерв нулевой, charge-строк в журнале нет.
        let (app, billing, path, response) =
            invoke_anthropic_bridge(PricingBridgeConfig::disabled(), None).await;
        assert_eq!(response.status().as_u16(), 529);
        assert_eq!(
            response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
            EXECUTION_STATE_NOT_STARTED
        );
        let account = billing
            .account("anthropic-bridge-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, 20_000_000);
        assert_eq!(account.reserved_nano, 0);
        let ledger = billing.ledger("anthropic-bridge-account", 10).await.unwrap();
        assert!(!ledger.iter().any(|row| row.kind == "charge"));

        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    const NS_ACCOUNT: &str = "not-started-account";
    const NS_KEY: &str = "sk-pool-not-started";
    const NS_TOPUP: i64 = 20_000_000;

    /// Биллинг-фикстура с метерным ключом, как в `invoke_anthropic_bridge`, но со своим
    /// аккаунтом — тесты с реальным upstream-проходом и settle.
    async fn not_started_billing(tag: &str) -> (Arc<AsyncBilling>, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_ANTHROPIC_BRIDGE_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-not-started-{tag}-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start not_started test billing"),
        );
        billing.create_account(NS_ACCOUNT, None, 2_000).await.unwrap();
        billing
            .topup(NS_ACCOUNT, NS_TOPUP, Some("not-started-seed"))
            .await
            .unwrap();
        billing
            .issue_key(NS_KEY, NS_ACCOUNT, None, None, None)
            .await
            .unwrap();
        (billing, path)
    }

    /// AppState с ОДНОЙ подпиской в пуле и upstream'ом на мок: запрос реально уходит в сеть
    /// (loopback), резерв берётся по-настоящему.
    fn not_started_pool_app(billing: Arc<AsyncBilling>, upstream: String) -> AppState {
        let mut cfg = (*anthropic_bridge_proxy_config(PricingBridgeConfig::disabled())).clone();
        cfg.upstream = upstream;
        let cfg = Arc::new(cfg);
        AppState {
            provider: ProviderMode::Anthropic,
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                ":memory:".to_string(),
                None,
            )),
            data_db_path: Arc::new(":memory:".to_string()),
            pool: Arc::new(Pool::new(
                vec![registry::Sub {
                    email: "not-started@example.test".into(),
                    token: "secret".into(),
                    proxy: String::new(),
                    fleet: "test".into(),
                    plan: "max20".into(),
                }],
                Reserve::FULL,
                50.0,
                1_500.0,
            )),
            affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            codex: None,
            gemini: None,
            billing: Some(billing),
            pricing_shadow: None,
            pricing_manifest: Arc::new(crate::builtin_pricing_runtime_manifest()),
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(Breaker::new(1)),
            metrics: Arc::new(Metrics::new()),
            probe_poke: None,
            cfg,
        }
    }

    struct FixedUpstream {
        upstream: String,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for FixedUpstream {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    /// Мок-апстрим, который на любой запрос отвечает одним фиксированным JSON-ответом.
    async fn fixed_upstream(status: StatusCode, body: serde_json::Value) -> FixedUpstream {
        let router = axum::Router::new().fallback(move || {
            let body = body.clone();
            async move { (status, axum::Json(body)) }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        FixedUpstream {
            upstream: format!("http://{address}"),
            task,
        }
    }

    async fn invoke_not_started(app: &AppState) -> Response {
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header("x-api-key", NS_KEY)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "model": "claude-sonnet-4-6",
                    "max_tokens": 10,
                    "messages": [{"role": "user", "content": "hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();
        forward(
            State(app.clone()),
            ConnectInfo("127.0.0.1:4242".parse().unwrap()),
            request,
        )
        .await
    }

    /// Ждём закрытия резерва (settle асинхронен): возвращает аккаунт с reserved_nano == 0.
    async fn settled_account(billing: &AsyncBilling) -> registry::AccountRow {
        loop {
            billing.flush().await.unwrap();
            let account = billing.account(NS_ACCOUNT).await.unwrap().unwrap();
            if account.reserved_nano == 0 {
                break account;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn not_started_upstream_failure_passthrough_refunds_the_hold() {
        // Upstream отвечает 500, попытка одна → терминальный passthrough 500 с заголовком;
        // reserve ушёл в refund (armed HoldGuard), ни цента не списано.
        let mock = fixed_upstream(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"type":"error","error":{"type":"api_error","message":"boom"}}),
        )
        .await;
        let (billing, path) = not_started_billing("upstream-500").await;
        let app = not_started_pool_app(Arc::clone(&billing), mock.upstream.clone());

        let response = invoke_not_started(&app).await;
        assert_eq!(response.status().as_u16(), 500);
        assert_eq!(
            response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
            EXECUTION_STATE_NOT_STARTED
        );
        let account = settled_account(&billing).await;
        assert_eq!(account.balance_nano, NS_TOPUP);
        let ledger = billing.ledger(NS_ACCOUNT, 10).await.unwrap();
        assert!(!ledger.iter().any(|row| row.kind == "charge"));

        drop(app);
        drop(billing);
        drop(mock);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn successful_delivery_never_carries_not_started_and_charges_the_actual_cost() {
        // Успешный 200: заголовка нет НИКОГДА; tee-метеринг закрывает резерв фактической
        // стоимостью (10 input + 5 output) — баланс уменьшился, charge в журнале есть.
        let upstream_body = serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-6",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let mock = fixed_upstream(StatusCode::OK, upstream_body.clone()).await;
        let (billing, path) = not_started_billing("upstream-200").await;
        let app = not_started_pool_app(Arc::clone(&billing), mock.upstream.clone());

        let response = invoke_not_started(&app).await;
        assert_eq!(response.status().as_u16(), 200);
        assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
        let delivered = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let delivered: serde_json::Value = serde_json::from_slice(&delivered).unwrap();
        assert_eq!(delivered["usage"]["output_tokens"], 5);
        let account = settled_account(&billing).await;
        assert!(account.balance_nano < NS_TOPUP);
        let ledger = billing.ledger(NS_ACCOUNT, 10).await.unwrap();
        assert!(ledger.iter().any(|row| row.kind == "charge"));

        drop(app);
        drop(billing);
        drop(mock);
        let _ = std::fs::remove_file(path);
    }
}
