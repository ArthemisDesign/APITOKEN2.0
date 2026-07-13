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

use crate::meter::{BillCtx, MeterCtx, TeeMeter};
use crate::metrics::Metrics;
use crate::state::AppState;
use crate::upstream::{limits_from_headers, Limits};
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;

/// Гард слота конкуррентности персоны. На ЛЮБОМ не-стриминговом исходе попытки (ошибка/ротация/4xx)
/// и — главное — при ОТМЕНЕ запроса (клиент отключился, future хендлера дропнут на await) декрементит
/// in-flight ровно один раз. Разоружается на успехе: слот переходит стриму и снимается в `end_stream`.
/// Без этого гарда `mark_used(+1)` при отмене НЕ откатывался бы → inflight персоны копится до рестарта.
struct InflightGuard {
    pool: Arc<pool::Pool>,
    email: String,
    armed: bool,
}
impl InflightGuard {
    fn new(pool: Arc<pool::Pool>, email: String) -> Self { Self { pool, email, armed: true } }
    fn disarm(&mut self) { self.armed = false; }
}
impl Drop for InflightGuard {
    fn drop(&mut self) { if self.armed { self.pool.mark_done(&self.email); } }
}

/// Гард резерва баланса. На любом не-успешном исходе И при отмене запроса возвращает удержанный
/// hold клиенту (`settle` с actual=0). Разоружается на успехе — там hold закрывает tee-метеринг
/// фактической стоимостью. Без гарда отмена запроса НАВСЕГДА списывала бы удержанное (деньги клиента).
struct HoldGuard {
    billing: Option<Arc<crate::billing::AsyncBilling>>,
    account_id: String,
    key: String,
    hold: i64,
    armed: bool,
}
impl HoldGuard {
    fn disarm(&mut self) { self.armed = false; }
}
impl Drop for HoldGuard {
    fn drop(&mut self) {
        if self.armed {
            // возврат резерва на аккаунт (actual=0 → ledger-charge не пишется). Drop синхронен —
            // шлём АСИНХРОННО через актор (settle_detached: mpsc::send не блокирует, не требует await).
            if let Some(b) = &self.billing { b.settle_detached(&self.account_id, &self.key, self.hold, 0, None); }
        }
    }
}

/// Гард fair-share-слота ключа: освобождает счётчик одновременных запросов ключа на выходе из
/// `forward` (любой исход + отмена запроса). Живёт всю обработку запроса.
struct KeyGuard {
    limiter: Arc<crate::keylimiter::KeyLimiter>,
    key: String,
}
impl Drop for KeyGuard {
    fn drop(&mut self) { self.limiter.release(&self.key); }
}

// 16 МиБ: покрывает ЛЮБОЙ реальный messages-запрос (16MB ≈ 4M токенов — выше любого контекст-окна),
// но ограничивает DoS-амплификацию (тело читается и парсится в serde_json::Value ДО резерв-гейта;
// 64MB × конверт конкуррентности раздувало бы память в ГБ). Files/большие payload'ы мы не поддерживаем.
const BODY_LIMIT: usize = 16 * 1024 * 1024;

// Заголовки клиента, которые НЕ пробрасываем апстриму (перезаписываем или служебные).
fn skip_req_header(name: &str) -> bool {
    matches!(name,
        "host" | "content-length" | "connection" | "authorization" | "x-api-key"
        | "anthropic-beta" | "anthropic-version" | "user-agent" | "accept-encoding"
        | "transfer-encoding" | "upgrade" | "proxy-connection" | "proxy-authorization"
        | "keep-alive" | "te" | "trailer")
}
// Заголовки апстрима, которые НЕ отдаём клиенту: hop-by-hop + per-ПОДПИСОЧНЫЕ ratelimit/идентити.
// `anthropic-ratelimit-*` отражают состояние НАШЕЙ подписки (утилизация/reset пула) — отдавать их
// клиенту (а) раскрывает, что это пул, (б) даёт «прыгающий» несогласованный лимит при ротации.
// Аналогично режем org/account-идентифицирующие, различающиеся между подписками (корреляция аккаунта).
fn skip_resp_header(name: &str) -> bool {
    matches!(name,
        "connection" | "transfer-encoding" | "content-length" | "content-encoding"
        | "keep-alive" | "proxy-connection" | "upgrade" | "te" | "trailer")
        || name.starts_with("anthropic-ratelimit")
        || name.starts_with("anthropic-organization")
        || name == "anthropic-account-id"
}

/// Клиентский ключ из запроса (x-api-key либо Bearer). Публично — используется и в `server`
/// для эндпоинта `/balance`.
pub fn client_key(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    let a = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    a.strip_prefix("Bearer ").or_else(|| a.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
}

/// Идентификатор «сессии» диалога для cache-first роутинга (`pool::route`). Якорь = стабильный
/// **кэшируемый префикс**: клиентский ключ + `system` + ПЕРВОЕ сообщение (`messages[0]`). Именно этот
/// большой статический префикс живёт в prompt-cache и НЕ меняется от хода к ходу (диалог растёт в
/// хвост) — поэтому вся история консистентно садится на одну персону → cache-hit + паттерн одного
/// юзера. `None` → не messages-запрос: роутинг сессии не нужен, идём load-based [`pool::pick`].
/// Считаем ДО инжекта identity — по исходному контенту клиента (наш system-блок якорь не смещает).
fn session_key(headers: &HeaderMap, v: &Value) -> Option<u64> {
    let first = v.get("messages").and_then(Value::as_array).and_then(|m| m.first())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(k) = client_key(headers) { k.hash(&mut h); }
    if let Some(sys) = v.get("system") { sys.to_string().hash(&mut h); } // кэшируемый статический префикс
    first.to_string().hash(&mut h);
    Some(h.finish())
}

/// Админ-доступ: env-ключи `CLAUDE_API_KEYS`, либо loopback-пир ТОЛЬКО если `trust_loopback`
/// (сервер реально слушает loopback). За реверс-прокси (bind 0.0.0.0) trust_loopback=false →
/// пустые ключи означают «отклонять всё», а не «доверять любому 127.0.0.1». Без биллинга.
/// Сравнение в константное время (по длине совпадающих строк): не выходим рано на первом различии,
/// чтобы не давать timing-oracle на угадывание админ-ключа по байту. Длину строк не скрываем
/// (не секрет). Без внешних зависимостей.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

pub fn authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if app.cfg.api_keys.is_empty() {
        return app.cfg.trust_loopback && peer.ip().is_loopback();
    }
    match client_key(headers) {
        // fold (не any/short-circuit): проверяем все ключи в константное время каждый.
        Some(k) => app.cfg.api_keys.iter().fold(false, |ok, x| ok | ct_eq(x.as_bytes(), k.as_bytes())),
        None => false,
    }
}

/// Совпал ли клиентский ключ с любым из `allow` (constant-time по каждому). Пустой `allow` → false.
fn key_in(headers: &HeaderMap, allow: &[String]) -> bool {
    match client_key(headers) {
        Some(k) => allow.iter().fold(false, |ok, x| ok | ct_eq(x.as_bytes(), k.as_bytes())),
        None => false,
    }
}

/// Control-плоскость (`/admin/*`): admin-ключ ИЛИ control-ключ ИЛИ (нет ни того ни другого →
/// loopback-админ). Отдельный класс, чтобы коммерция управляла аккаунтами своим секретом, не имея
/// прав неметеренного форвардинга.
pub fn control_authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if authed(app, headers, peer) { return true; } // admin-ключ/loopback покрывает control
    if key_in(headers, &app.cfg.control_keys) { return true; }
    // Ни admin, ни control-ключей не задано → доверяем loopback (dev). Иначе — только по ключу.
    app.cfg.api_keys.is_empty() && app.cfg.control_keys.is_empty()
        && app.cfg.trust_loopback && peer.ip().is_loopback()
}

/// Read-only дашборды (`/capacity`, `/metrics`): admin ИЛИ control ИЛИ panel-ключ. Панель смотрит
/// ёмкость своим низкопривилегированным ключом — без прав /admin/* и без форвардинга.
pub fn readonly_authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    control_authed(app, headers, peer) || key_in(headers, &app.cfg.panel_keys)
}

/// Результат авторизации запроса.
enum Authz {
    /// Админ (env-ключ/localhost) — без тарификации.
    Admin,
    /// Ключ клиента → АККАУНТ с балансом. Тарифицируем и списываем с БАЛАНСА АККАУНТА (общего на
    /// все ключи юзера); `key` — для атрибуции расхода по ключу. `mult_bp` — наценка аккаунта.
    /// `balance_nano` несём из авторизации → резерв-блок НЕ перечитывает баланс из БД (−1 запрос).
    Metered { account_id: String, key: String, mult_bp: i64, balance_nano: i64 },
    /// Ключ/аккаунт есть, но баланс ≤ 0.
    PaymentRequired,
    /// Ключ неизвестен/заблокирован (ключ или аккаунт).
    Unauthorized,
}

/// Порядок для МАСШТАБА: сначала админ (env-ключ/loopback) — проверка В ПАМЯТИ, без похода в БД
/// (админ-трафик не грузит биллинг-мьютекс). Только если не админ — клиентский ключ → аккаунт
/// (ОДНА DB-выборка, несёт баланс для резерва → без повторного чтения).
async fn authorize(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> Authz {
    if authed(app, headers, peer) { return Authz::Admin; }
    if let (Some(billing), Some(k)) = (&app.billing, client_key(headers)) {
        if let Some(a) = billing.key_auth(&k).await {
            if !a.active {
                return Authz::Unauthorized; // ключ или аккаунт неактивен
            }
            if a.balance_nano <= 0 {
                return Authz::PaymentRequired;
            }
            return Authz::Metered {
                account_id: a.account_id, key: k, mult_bp: a.mult_bp, balance_nano: a.balance_nano,
            };
        }
    }
    Authz::Unauthorized
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

fn cap_to_balance(bal: i128, input_est: i128, web_buf: i128, p: &metering::Prices,
                  mult_bp: i64, client_mt: u64) -> Option<(u64, i64)> {
    if bal <= 0 { return None; }
    // Наценка ≤ 0 = бесплатный ключ (charge всегда 0) → не лимитируем, hold 0.
    if mult_bp <= 0 { return Some((client_mt, 0)); }
    // Работаем в RAW-нано (до наценки). Максимальная RAW-стоимость `x_max`, чья КЛИЕНТСКАЯ цена
    // (apply_multiplier, round-half-up) гарантированно ≤ bal: apply_multiplier(X)=(X·m+5000)/10000 ≤ bal
    // при X ≤ ⌊bal·10000/m⌋ (тогда X·m ≤ bal·10000 → (X·m+5000)/10000 ≤ bal, т.к. 5000<10000). Так
    // `hold=apply_multiplier(ceiling) ≤ bal` держится ПО ПОСТРОЕНИЮ, без клампа (клампа-щели округления нет).
    let x_max = bal.saturating_mul(10000) / (mult_bp as i128);
    let fixed_raw = input_est * p.cache_write_1h + web_buf; // вход worst-case + буфер поисков (RAW)
    if fixed_raw > x_max { return None; }                   // не тянет даже вход worst-case → 402
    let out = p.output.max(1);
    let affordable = ((x_max - fixed_raw) / out).max(0) as u64; // сколько output-токенов влезает в x_max
    if affordable == 0 { return None; }
    let eff_mt = client_mt.min(affordable);
    // ceiling_raw ≤ x_max (по построению) → hold = apply_multiplier(ceiling_raw) ≤ bal. А реальный
    // charge = apply_multiplier(real_raw) ≤ hold, т.к. real_raw ≤ ceiling_raw (реальных input-токенов
    // ≤ байт=input_est, ставка входа ≤ cw1h, output ≤ eff_mt). Итог: charge ≤ hold ≤ bal — жёстко.
    let ceiling_raw = fixed_raw + (eff_mt as i128) * p.output;
    let hold = metering::apply_multiplier(ceiling_raw, mult_bp) as i64;
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
    let obj = match v.as_object_mut() { Some(o) => o, None => return false };
    if !obj.contains_key("messages") { return false; } // не messages-запрос — не трогаем
    match obj.get("system").cloned() {
        None => { obj.insert("system".into(), serde_json::json!([{"type":"text","text":identity}])); }
        Some(Value::String(s)) => {
            if is_cc_marker(&s) { return false; }       // клиент прислал identity строкой — не дублируем
            obj.insert("system".into(),
                serde_json::json!([{"type":"text","text":identity},{"type":"text","text":s}]));
        }
        Some(Value::Array(mut arr)) => {
            let first_cc = arr.first()
                .and_then(|b| b.get("text")).and_then(|t| t.as_str())
                .map(is_cc_marker).unwrap_or(false);
            if first_cc { return false; }               // уже Claude-Code-запрос (напр. сам Claude Code)
            arr.insert(0, serde_json::json!({"type":"text","text":identity}));
            obj.insert("system".into(), Value::Array(arr));
        }
        _ => return false,
    }
    true
}

/// Слить anthropic-beta клиента с нашим (гарантируем присутствие oauth-беты).
fn merge_beta(client_beta: Option<&str>, default_beta: &str) -> String {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in default_beta.split(',').chain(client_beta.unwrap_or("").split(',')) {
        let p = part.trim();
        if !p.is_empty() && seen.insert(p.to_string()) { out.push(p.to_string()); }
    }
    out.join(", ")
}

/// Allowlist эндпоинтов Anthropic, работающих на квоте ПОДПИСКИ (что мы форвардим на пул):
/// `POST /v1/messages` (метерим), `POST /v1/messages/count_tokens` и `GET /v1/models[/{id}]` (проброс
/// без тарификации). Всё прочее (batches/files/agents/sessions/environments/skills/complete) на
/// подписочном OAuth-токене недоступно → чистый 404 на шлюзе. Управляющие роуты (`/health` и др.)
/// сюда НЕ доходят — их обслуживает `server` до fallback на `forward`.
fn is_supported_endpoint(method: &Method, path: &str) -> bool {
    match (method.as_str(), path) {
        ("POST", "/v1/messages") | ("POST", "/v1/messages/count_tokens") => true,
        ("GET", "/v1/models") => true,
        ("GET", p) if p.starts_with("/v1/models/") => true,
        _ => false,
    }
}

pub async fn forward(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    let (parts, body) = req.into_parts();
    let authz = authorize(&app, &parts.headers, &peer).await;
    match authz {
        Authz::Unauthorized =>
            return err_response(StatusCode::UNAUTHORIZED, "authentication_error", "invalid api key"),
        Authz::PaymentRequired =>
            return err_response(StatusCode::PAYMENT_REQUIRED, "invalid_request_error",
                                "insufficient balance — top up your key"),
        Authz::Admin | Authz::Metered { .. } => {}
    }
    // ALLOWLIST эндпоинтов: форвардим на пул ТОЛЬКО то, что доступно на квоте ПОДПИСКИ Claude Max
    // (messages/count_tokens/models). Batches/Files/Agents/Sessions требуют scope OAuth-токена
    // (user:batch/developer), которого у подписки НЕТ → на них Anthropic отдаёт 403/401/404. Не роутим
    // их на подписку (иначе застудили бы её и слили бы backend-scope в ошибке), а отдаём чистый 404.
    if !is_supported_endpoint(&parts.method, parts.uri.path()) {
        return err_response(StatusCode::NOT_FOUND, "not_found_error", "this endpoint is not available");
    }
    Metrics::inc(&app.metrics.requests);
    // fair-share ПО АККАУНТУ: не даём одному клиенту (профилю) набить флот бёрстом одновременных
    // запросов — лимитим по account_id, а НЕ по ключу, иначе юзер с N ключами обошёл бы конверт в N раз.
    // Слот держится всю обработку (гард освобождает на любом исходе/отмене). Админ — без лимита.
    let _key_guard = if let Authz::Metered { account_id, .. } = &authz {
        if !app.key_limiter.try_acquire(account_id, app.cfg.max_inflight_per_key) {
            Metrics::inc(&app.metrics.key_throttled);
            return err_retry(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error",
                             "too many concurrent requests — slow down", 1);
        }
        Some(KeyGuard { limiter: app.key_limiter.clone(), key: account_id.clone() })
    } else {
        None
    };
    let method: Method = parts.method.clone();
    let pq = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let url = format!("{}{}", app.cfg.upstream.trim_end_matches('/'), pq);

    let raw = match axum::body::to_bytes(body, BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => return err_response(StatusCode::BAD_REQUEST, "invalid_request_error", "body read error"),
    };

    // тело: один парс — вытаскиваем модель + max_tokens (для тарификации/резерва) и инжектим
    // identity (иначе токен подписки не пустят на /v1/messages). Держим `Bytes` (не Vec): clone на
    // каждую попытку ротации тогда O(1) refcount, а не копия до BODY_LIMIT (анти-амплификация памяти).
    let mut body_bytes: bytes::Bytes = raw.clone();
    let mut model = String::new();
    let mut max_tokens: u64 = 0;
    let mut session: Option<u64> = None; // sticky-ключ диалога (см. session_key) — для pool.pick_sticky
    let mut parsed = serde_json::from_slice::<Value>(&raw).ok();
    let mut body_dirty = false; // тело менялось (identity/cap max_tokens) → пересобрать перед форвардом
    let mut web_uses: u64 = 0;  // суммарный лимит web-поисков (для резерва их стоимости под баланс)
    if let Some(v) = parsed.as_mut() {
        model = v.get("model").and_then(Value::as_str).unwrap_or("").to_string();
        max_tokens = v.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
        // Резервируем стоимость web_search по РЕАЛЬНОМУ max_uses каждого инструмента (не фикс-буфер):
        // иначе клиент с max_uses>buf пробил бы hold. Без max_uses — консервативный дефолт.
        if let Some(tools) = v.get("tools").and_then(Value::as_array) {
            for t in tools {
                let is_web = t.get("type").and_then(Value::as_str)
                    .map(|s| s.contains("web_search")).unwrap_or(false);
                if is_web {
                    web_uses += t.get("max_uses").and_then(Value::as_u64).unwrap_or(DEFAULT_WEB_USES);
                }
            }
        }
        session = session_key(&parts.headers, v); // ДО инжекта — по исходному контенту клиента
        if app.cfg.inject_identity && inject_identity(v, &app.cfg.identity) { body_dirty = true; }
    }

    // Circuit breaker разомкнут (брауноут апстрима) → быстрый отбой ДО резерва: в аутейдж не делаем
    // лишних DB-записей (reserve+возврат) на каждый запрос thundering-herd. Резерва ещё нет — возвращать нечего.
    if let Some(retry) = app.breaker.open_for(pool::now()) {
        Metrics::inc(&app.metrics.breaker_rejects);
        return err_retry(StatusCode::SERVICE_UNAVAILABLE, "overloaded_error",
                         "upstream temporarily unavailable", retry);
    }

    // БАЛАНС-ЛИМИТ метерного ключа (точный контроль: клиент не получит ни токена/цента сверх баланса).
    // Идея: output ограничиваем ЗАРАНЕЕ — урезаем `max_tokens` под остаток баланса, и Anthropic сам
    // отрубает генерацию ровно на доступном токене (stop_reason: max_tokens). Вход считаем по ВЕРХНЕЙ
    // оценке (полные байты × cache_write_1h — токенов ≤ байт при любой корзине). Затем атомарно
    // резервируем потолок при УРЕЗАННОМ max_tokens (≤ баланса), фактику закрываем settle в finalize.
    let mut reserved: Option<(String, String, i64)> = None; // (account_id, key, hold)
    if let (Authz::Metered { account_id, key, mult_bp, balance_nano }, Some(billing)) = (&authz, &app.billing) {
        // баланс несём из authorize (свежая выборка) — без повторного чтения. Гонку с параллельными
        // запросами всё равно ловит АТОМАРНЫЙ reserve (WHERE balance>=hold): stale-баланс лишь мог бы
        // дать чуть больший cap, но reserve тогда честно откажет (402), в минус не уводя.
        let bal = *balance_nano as i128;
        // РЕЗЕРВ по model_prices_RESERVE: распознанная модель → её цена; нераспознанный алиас →
        // MAX-тариф. Иначе резерв по дешёвому дефолту, а списание по (дорогой) модели ОТВЕТА пробили
        // бы hold → баланс в минус до −2×. Списание (finalize) остаётся по реальной модели ответа.
        let p = metering::model_prices_reserve(&model);
        let input_est = ((raw.len() + app.cfg.identity.len()) as i128).max(1);
        // web-буфер = число разрешённых поисков × ставка (0 без web_search-инструмента → малобалансовым
        // не блокирует обычные запросы; при включённом — покрывает ровно заявленный max_uses).
        let web_buf = (web_uses as i128) * metering::WEB_SEARCH_NANO;
        let client_mt = if max_tokens > 0 { max_tokens.min(2_000_000) } else { 4096 };
        let (eff_mt, hold) = match cap_to_balance(bal, input_est, web_buf, &p, *mult_bp, client_mt) {
            Some(x) => x,
            None => return err_response(StatusCode::PAYMENT_REQUIRED, "invalid_request_error",
                                        "insufficient balance for this request — top up your key"),
        };
        // урезали под баланс → правим max_tokens в теле: Anthropic остановит генерацию ровно тут
        if eff_mt < max_tokens {
            if let Some(v) = parsed.as_mut() {
                v["max_tokens"] = serde_json::json!(eff_mt);
                body_dirty = true;
            }
        }
        // РЕЗЕРВ по АККАУНТУ (общий баланс на все ключи юзера); гонки атомарны на уровне аккаунта.
        match billing.reserve(account_id, hold).await {
            Some(_) => reserved = Some((account_id.clone(), key.clone(), hold)),
            None => return err_response(StatusCode::PAYMENT_REQUIRED, "invalid_request_error",
                                        "insufficient balance for this request — top up your key"),
        }
    }
    // Гард резерва: на любом не-успешном исходе И при отмене запроса вернёт hold клиенту. Создаём
    // ДО пересборки тела — если она упадёт и мы вернёмся, Drop гарда вернёт hold (без утечки).
    // Разоружим на успехе — там hold закрывает tee-метеринг. Снимает утечку денег при disconnect.
    let mut hold_guard = reserved.as_ref().map(|(acct, k, h)| HoldGuard {
        billing: app.billing.clone(), account_id: acct.clone(), key: k.clone(), hold: *h, armed: true,
    });
    // Пересобираем тело ОДИН раз после всех правок (identity + возможный cap max_tokens). Если правили,
    // но пересборка НЕ удалась — форвардить исходное тело НЕЛЬЗЯ: оно несёт СТАРЫЙ (большой) max_tokens
    // при малом hold → пробой баланса. Тогда отказ (hold вернёт hold_guard на return).
    if body_dirty {
        match parsed.as_ref().map(serde_json::to_vec) {
            Some(Ok(b)) => body_bytes = bytes::Bytes::from(b),
            _ => return err_response(StatusCode::INTERNAL_SERVER_ERROR, "api_error",
                                     "request could not be processed"),
        }
    }

    let version = parts.headers.get("anthropic-version")
        .and_then(|v| v.to_str().ok()).unwrap_or(&app.cfg.anthropic_version).to_string();
    let beta = merge_beta(
        parts.headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
        &app.cfg.default_beta);

    let mut tried: HashSet<String> = HashSet::new();
    // Один запрос вносит в breaker максимум ОДИН backend-фейл (не `max_tries`): иначе poison-запрос
    // (500 на каждой подписке) в одиночку размыкал бы глобальный breaker и клал сервис всем. Реальный
    // аутейдж тилит breaker числом РАЗНЫХ запросов, а не веером одного.
    let mut backend_fail_recorded = false;
    let mut last = err_response(StatusCode::SERVICE_UNAVAILABLE, "overloaded_error",
                               "no subscriptions available in pool");

    // Бюджет: ошибки ПОДПИСКИ (429/401/403 — бан/лимит конкретного аккаунта) НЕ тратят попытки,
    // крутимся дальше по флоту (клиенту такая ошибка идти не должна, пока есть здоровые подписки).
    // Бюджет `max_tries` тратят только BACKEND-фейлы (5xx/сеть — вероятный аутейдж апстрима). Верхний
    // предел итераций = «весь флот + запас» (пул сам исключает уже cooling/tried → быстро сходится).
    let hard_cap = app.pool.len().max(1) + 2;
    let mut attempt = 0usize;
    let mut backend_tries = 0usize;
    let mut auth_tries = 0usize; // 401/403: возможно вина запроса клиента, а не токена — см. ниже
    while attempt < hard_cap {
        // Первая попытка — cache-first роутинг сессии (пин/placement/спилл в `route`). Дальше —
        // load-based `pick` (дом/пробованные уже в `tried`; cooling-подписки пул исключает сам).
        let sub = match session.filter(|_| attempt == 0)
            .and_then(|s| app.pool.route(s))
            .or_else(|| app.pool.pick(&tried, false))
        {
            Some(s) => s,
            None => break,
        };
        // route/pick отдали cooling-персону → значит НЕТ ни одной не-cooling (весь оставшийся флот за
        // лимитом). НЕ шлём живой клиентский трафик на отлимиченный аккаунт: это гарантированный свежий
        // 429 (ban-signal + «автоматный» кластер, ровно то, чего избегаем). Быстрый прозрачный отбой
        // клиенту с точным Retry-After = soonest_ready (он откатится сам). hold вернёт hold_guard на return.
        if app.pool.is_cooling(&sub.email) {
            Metrics::inc(&app.metrics.exhausted);
            let retry = app.pool.soonest_ready().unwrap_or(app.cfg.cool_secs);
            return err_retry(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error",
                             "all subscriptions are rate-limited — retry shortly", retry);
        }
        attempt += 1;
        tried.insert(sub.email.clone());
        app.pool.mark_used(&sub.email);
        // Гард слота: закроет in-flight на ЛЮБОМ выходе из итерации (continue/return/ОТМЕНА запроса).
        // Разоружим только на успехе (слот перейдёт стриму). mark_cooling/mark_healthy in-flight не трогают.
        let mut guard = InflightGuard::new(app.pool.clone(), sub.email.clone());

        let client = match app.clients.get(&sub.proxy) {
            Ok(c) => c,
            Err(e) => {
                app.pool.mark_cooling(&sub.email, 10); // битый прокси → cooling (слот закроет guard)
                eprintln!("⚠ прокси {}: {e}", sub.email); // детали ТОЛЬКО в лог (не клиенту)
                last = err_response(StatusCode::BAD_GATEWAY, "api_error", "upstream connection error");
                continue;
            }
        };

        // per-persona UA: стабильный для подписки, но различный между подписками (антифингерпринт
        // флота). Клиентский user-agent НЕ пробрасываем (см. skip_req_header) — отпечаток наш.
        let ua = crate::upstream::persona_ua(&app.cfg, &sub.email);
        let mut rb = client.request(method.clone(), &url)
            .header("authorization", format!("Bearer {}", sub.token))
            .header("anthropic-version", &version)
            .header("anthropic-beta", &beta)
            .header("user-agent", &ua);
        for (name, value) in parts.headers.iter() {
            let n = name.as_str();
            if !skip_req_header(n) { rb = rb.header(n, value.as_bytes()); }
        }
        rb = rb.body(body_bytes.clone());

        let resp = match rb.send().await {
            Ok(r) => r,
            Err(e) => {
                // сетевой сбой: мог быть локальный прокси (короткий cooling подписки), а мог —
                // общий апстрим-аутейдж (тогда фейлят все прокси → брейкер разомкнётся). Тратит бюджет.
                app.pool.mark_cooling(&sub.email, 15);
                if !backend_fail_recorded { app.breaker.record_fail(pool::now(), &sub.email); backend_fail_recorded = true; }
                eprintln!("⚠ upstream {}: {e}", sub.email); // детали (email/сеть) ТОЛЬКО в лог
                last = err_response(StatusCode::BAD_GATEWAY, "api_error", "upstream unavailable");
                backend_tries += 1;
                if backend_tries >= app.cfg.max_tries.max(1) { break; }
                continue;
            }
        };

        let st = resp.status();
        let code = st.as_u16();

        // ПАССИВНЫЙ сбор лимитов из боевого ответа: свежий util/reset без лишних запросов
        // (обновляет polled_ts → активный поллер сам перестаёт трогать «живые» подписки).
        let lim = limits_from_headers(resp.headers());
        if lim.has_util() {
            app.pool.set_util(&sub.email, lim.util5h, lim.util7d, lim.status.clone(),
                              lim.reset5h, lim.reset7d);
        }

        let now = pool::now();
        // Классификация вины (важно: НЕ студить подписку за чужую вину):
        if code == 429 {
            // квота подписки: студим до сброса окна-виновника (см. cool_secs_429)
            Metrics::inc(&app.metrics.upstream_429);
            let secs = cool_secs_429(&resp, &lim, now);
            app.pool.mark_cooling(&sub.email, secs);
            eprintln!("↻ ротация: {} вернул 429 — cooling {}s", sub.email, secs);
            last = err_response(st, "overloaded_error", "upstream rate-limited"); // без email клиенту
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
            eprintln!("auth {} на {} (попытка {}) — НЕ студим (возможно вина запроса)", code, sub.email, auth_tries);
            // Пробуем ДРУГУЮ подписку ТОЛЬКО если она реально есть (вдруг дохлый токен этой). Если
            // другой нет (напр. пул из одной) или повтор — это вина запроса (scope/модель/путь) → отдаём
            // РЕАЛЬНЫЙ 401/403 Anthropic прозрачно, а НЕ маскируем в 429-исчерпание (был баг с 1 подпиской).
            if auth_tries < 2 && attempt < hard_cap && app.pool.pick(&tried, false).is_some() {
                // Уходим на ДРУГУЮ подписку → эта, возможно, с мёртвым токеном. Просим поллер проверить
                // её чистым probe СРАЗУ (не через LIVENESS_INTERVAL): revoked-токен перестанет быть
                // placement-магнитом за ~1 цикл. Без cooling здесь (crafted-запрос иначе студил бы флот).
                app.pool.request_probe(&sub.email);
                if let Some(p) = &app.probe_poke { p.notify_one(); }
                last = err_response(st, "overloaded_error", "upstream unavailable");
                continue;
            }
            app.pool.mark_healthy(&sub.email);          // повтор/нет альтернативы → вина запроса
            // НОРМАЛИЗУЕМ тело: сырой 401/403 подписочного OAuth-токена несёт «OAuth token does not meet
            // scope requirement…» — это раскрыло бы, что backend на подписках, а не на API-ключе. Отдаём
            // клиенту тот же КОД с generic Anthropic-shaped телом (backend скрыт, транспарентность цела).
            let kind = if code == 403 { "permission_error" } else { "authentication_error" };
            return err_response(st, kind, "this request is not permitted");
        }
        if st.is_server_error() || code == 408 || code == 409 || code == 425 {
            // вина АПСТРИМА, не подписки: НЕ студим подписку (слот закроет guard), кормим breaker
            // максимум раз на запрос (анти-DoS от poison-запроса).
            Metrics::inc(&app.metrics.upstream_5xx);
            if !backend_fail_recorded { app.breaker.record_fail(now, &sub.email); backend_fail_recorded = true; }
            eprintln!("↻ ротация: {} вернул {} — backend-fault (breaker+)", sub.email, code);
            last = err_response(st, "overloaded_error", "upstream unavailable"); // без email клиенту
            backend_tries += 1;                                    // backend тратит бюджет (аутейдж)
            if backend_tries >= app.cfg.max_tries.max(1) { break; }
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
            guard.disarm();                                   // слот переходит стриму (end_stream)
            if let Some(g) = hold_guard.as_mut() { g.disarm(); } // hold закроет tee-метеринг фактикой
            let bill = match (&authz, reserved.take()) {
                (Authz::Metered { mult_bp, .. }, Some((acct, key, hold))) =>
                    app.billing.clone().map(|billing| BillCtx {
                        billing, account_id: acct, key, mult_bp: *mult_bp, hold,
                        request_id: request_id_of(&resp),
                    }),
                _ => None,
            };
            Some(MeterCtx {
                pool: app.pool.clone(),
                email: sub.email.clone(),
                model: model.clone(),
                is_sse: is_event_stream(&resp),
                bill,
            })
        } else {
            // клиентская 4xx: подписка ни при чём. Слот закроет guard, резерв — hold_guard (на return).
            app.pool.mark_healthy(&sub.email);
            None
        };
        return stream_back(st, resp, meter);
    }
    // Резерв вернёт `hold_guard` на return; слот последней попытки уже закрыл её `guard`.
    if backend_tries >= app.cfg.max_tries.max(1) {
        // упёрлись в бюджет backend-фейлов → это аутейдж апстрима, отдаём последнюю upstream-ошибку
        // (breaker уже накапливает — скоро разомкнётся и будет быстрый отбой).
        last
    } else if tried.is_empty() {
        last // пул реально пуст
    } else {
        // перебрали подписки, но все за лимитом → прозрачный 429 + Retry-After (клиент откатится сам).
        // Именно это, а НЕ ошибка отдельной забаненной/лимитированной подписки, уходит клиенту.
        Metrics::inc(&app.metrics.exhausted);
        let retry = app.pool.soonest_ready().unwrap_or(app.cfg.cool_secs);
        err_retry(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error",
                  "all subscriptions are rate-limited — retry shortly", retry)
    }
}

/// Ответ — SSE-стрим? (по content-type). Определяет способ парсинга usage.
fn is_event_stream(resp: &reqwest::Response) -> bool {
    resp.headers().get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|c| c.contains("text/event-stream"))
        .unwrap_or(false)
}

/// request-id ответа Anthropic — кладём в ledger как `ref` списания (аудит-трейл «за что списано»).
fn request_id_of(resp: &reqwest::Response) -> Option<String> {
    resp.headers().get("request-id").and_then(|v| v.to_str().ok()).map(|s| s.to_string())
}

/// Явный заголовок `Retry-After` (самый авторитетный хинт — Anthropic сам говорит, когда можно).
fn retry_after_header(resp: &reqwest::Response) -> Option<i64> {
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
        if c.contains("day") && u7 >= 0.9 { return fut(lim.reset7d).or_else(|| fut(lim.reset5h)); }
        if c.contains("hour") && u5 >= 0.9 { return fut(lim.reset5h).or_else(|| fut(lim.reset7d)); }
    }
    // Фолбэк-эвристика, если заголовка claim нет: окно у потолка → студим до его reset.
    if u7 >= 0.95 { return fut(lim.reset7d).or_else(|| fut(lim.reset5h)); }
    if u5 >= 0.95 { return fut(lim.reset5h).or_else(|| fut(lim.reset7d)); }
    None
}

/// Сколько студить при 429: Retry-After (авторитетно) → окно-виновник (если квота выбита) →
/// короткий burst-дефолт (транзиентный лимит запросов/мин).
fn cool_secs_429(resp: &reqwest::Response, lim: &Limits, now: i64) -> i64 {
    retry_after_header(resp).or_else(|| window_cool(lim, now)).unwrap_or(BURST_COOL_SECS)
}

/// Отдать ответ апстрима клиенту байт-в-байт (стримом — работает и для SSE).
/// Если задан `meter` — оборачиваем тело в tee-метеринг: клиент получает те же байты,
/// а на завершении стрима списываем стоимость с ключа (тело клиенту НЕ задерживается).
fn stream_back(st: StatusCode, resp: reqwest::Response, meter: Option<MeterCtx>) -> Response {
    let mut builder = Response::builder().status(st);
    for (name, value) in resp.headers().iter() {
        if !skip_resp_header(name.as_str()) {
            builder = builder.header(name.as_str(), value.as_bytes());
        }
    }
    let stream = resp.bytes_stream().map(|chunk| {
        chunk.map_err(std::io::Error::other)
    });
    let body = match meter {
        Some(ctx) => Body::from_stream(TeeMeter::new(Box::pin(stream), ctx)),
        None => Body::from_stream(stream),
    };
    builder.body(body).unwrap_or_else(|_| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, "api_error", "response build error")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lim(u5: f64, u7: f64, claim: Option<&str>, r5: i64, r7: i64) -> Limits {
        Limits {
            util5h: Some(u5), util7d: Some(u7), status: None,
            reset5h: Some(r5), reset7d: Some(r7), claim: claim.map(|s| s.to_string()),
        }
    }

    #[test]
    fn window_cool_prefers_authoritative_claim() {
        let now = 1_000_000;
        let (r5, r7) = (now + 3600, now + 100_000);
        // claim=seven_day + 7d у потолка → студим до reset7d (не до 5h, хотя 5h тоже высок)
        assert_eq!(window_cool(&lim(0.97, 0.96, Some("seven_day"), r5, r7), now), Some(100_000));
        // claim=five_hour → до reset5h
        assert_eq!(window_cool(&lim(0.97, 0.96, Some("five_hour"), r5, r7), now), Some(3600));
        // claim есть, но окно НЕ у потолка (0.5) → burst-429 (rate), не quota → None (короткий дефолт)
        assert_eq!(window_cool(&lim(0.5, 0.5, Some("five_hour"), r5, r7), now), None);
        // нет claim → фолбэк-эвристика (7d≥0.95 → reset7d)
        assert_eq!(window_cool(&lim(0.1, 0.96, None, r5, r7), now), Some(100_000));
    }

    #[test]
    fn endpoint_allowlist() {
        use super::Method;
        assert!(is_supported_endpoint(&Method::POST, "/v1/messages"));
        assert!(is_supported_endpoint(&Method::POST, "/v1/messages/count_tokens"));
        assert!(is_supported_endpoint(&Method::GET, "/v1/models"));
        assert!(is_supported_endpoint(&Method::GET, "/v1/models/claude-haiku-4-5"));
        // недоступное на подписке — отклоняем
        assert!(!is_supported_endpoint(&Method::POST, "/v1/messages/batches"));
        assert!(!is_supported_endpoint(&Method::GET, "/v1/messages/batches"));
        assert!(!is_supported_endpoint(&Method::POST, "/v1/files"));
        assert!(!is_supported_endpoint(&Method::GET, "/v1/files"));
        assert!(!is_supported_endpoint(&Method::POST, "/v1/agents"));
        assert!(!is_supported_endpoint(&Method::POST, "/v1/complete")); // легаси
        assert!(!is_supported_endpoint(&Method::GET, "/v1/messages")); // messages только POST
        assert!(!is_supported_endpoint(&Method::DELETE, "/v1/models/x"));
    }

    #[test]
    fn ct_eq_is_correct() {
        assert!(ct_eq(b"secret-key", b"secret-key"));
        assert!(!ct_eq(b"secret-key", b"secret-keX"));
        assert!(!ct_eq(b"short", b"longer-key")); // разная длина
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn cap_to_balance_enforces_budget() {
        let p = metering::model_prices("claude-haiku-4-5"); // input 1000, output 5000, cw1h 2000
        // ИНВАРИАНТ на широком диапазоне (наценки, балансы): hold ≤ bal И charge(worst usage) ≤ hold,
        // И один лишний output-токен пробил бы баланс (точность отруба «ни на токен больше»).
        for &m in &[10000i64, 2000, 900, 33333] { // ×1.0, ×0.2 (прод), ×0.09, ×3.33
            for &bal in &[500_000i128, 2_000_000, 50_000_000, 10_000_000_000] {
                let ib = 137i128; // байты входа
                if let Some((eff, hold)) = cap_to_balance(bal, ib, 0, &p, m, 100_000) {
                    assert!((hold as i128) <= bal, "hold {hold} > bal {bal} (m={m})");
                    let real = ib * p.cache_write_1h + (eff as i128) * p.output; // worst-case usage
                    assert!(metering::apply_multiplier(real, m) <= hold as i128,
                            "charge > hold (m={m}, bal={bal}, eff={eff})");
                    // если урезали балансом (eff < запрошенного) — +1 токен обязан пробить баланс
                    if eff < 100_000 {
                        let over = ib * p.cache_write_1h + ((eff + 1) as i128) * p.output;
                        assert!(metering::apply_multiplier(over, m) > bal,
                                "eff+1 должен превышать баланс (m={m}, bal={bal}, eff={eff})");
                    }
                }
            }
        }
        // большой баланс + большой запрос → НЕ режем (eff == запрошенное)
        let (eff, _) = cap_to_balance(1_000_000_000, 100, 0, &p, 2000, 50).unwrap();
        assert_eq!(eff, 50);
        // бесплатный ключ (наценка 0) → не лимитируем, hold 0
        assert_eq!(cap_to_balance(1_000, 999_999, 0, &p, 0, 12345), Some((12345, 0)));
        // баланс не тянет даже вход → None (отказ, не отрицательный баланс)
        assert!(cap_to_balance(100, 100_000, 0, &p, 2000, 10).is_none());
        assert!(cap_to_balance(0, 10, 0, &p, 2000, 10).is_none());
        // web_buf ($0.20) режет бюджет только при web_search: малый баланс + буфер → None
        assert!(cap_to_balance(1_000_000, 100, 20 * metering::WEB_SEARCH_NANO, &p, 10000, 10).is_none());
        // переполнения нет: огромный баланс и max_tokens
        let (_, h) = cap_to_balance(i64::MAX as i128, 100, 0, &p, 2000, u64::MAX).unwrap();
        assert!(h >= 0);
    }
}
