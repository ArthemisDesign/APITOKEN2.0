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
    billing: Option<Arc<registry::Billing>>,
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
            if let Some(b) = &self.billing { b.settle(&self.key, self.hold, 0); }
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

const BODY_LIMIT: usize = 64 * 1024 * 1024;

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

/// Результат авторизации запроса.
enum Authz {
    /// Админ (env-ключ/localhost) — без тарификации.
    Admin,
    /// Ключ клиента с балансом — тарифицируем ответ и списываем.
    Metered { key: String, mult_bp: i64 },
    /// Ключ есть, но баланс ≤ 0.
    PaymentRequired,
    /// Ключ неизвестен/заблокирован.
    Unauthorized,
}

/// Приоритет: сначала биллинг-ключ (из таблицы api_keys), иначе — админ (env/localhost).
fn authorize(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> Authz {
    if let (Some(billing), Some(k)) = (&app.billing, client_key(headers)) {
        if let Some(row) = billing.get(&k) {
            if row.status != "active" {
                return Authz::Unauthorized;
            }
            if row.balance_nano <= 0 {
                return Authz::PaymentRequired;
            }
            return Authz::Metered { key: k, mult_bp: row.mult_bp };
        }
    }
    if authed(app, headers, peer) { Authz::Admin } else { Authz::Unauthorized }
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

pub async fn forward(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    let (parts, body) = req.into_parts();
    let authz = authorize(&app, &parts.headers, &peer);
    match authz {
        Authz::Unauthorized =>
            return err_response(StatusCode::UNAUTHORIZED, "authentication_error", "invalid api key"),
        Authz::PaymentRequired =>
            return err_response(StatusCode::PAYMENT_REQUIRED, "invalid_request_error",
                                "insufficient balance — top up your key"),
        Authz::Admin | Authz::Metered { .. } => {}
    }
    Metrics::inc(&app.metrics.requests);
    // fair-share: не даём одному метерному ключу набить флот бёрстом одновременных запросов.
    // Слот держится всю обработку (гард освобождает на любом исходе/отмене). Админ — без лимита.
    let _key_guard = if let Authz::Metered { key, .. } = &authz {
        if !app.key_limiter.try_acquire(key, app.cfg.max_inflight_per_key) {
            Metrics::inc(&app.metrics.key_throttled);
            return err_retry(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error",
                             "too many concurrent requests — slow down", 1);
        }
        Some(KeyGuard { limiter: app.key_limiter.clone(), key: key.clone() })
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
    if let Ok(mut v) = serde_json::from_slice::<Value>(&raw) {
        model = v.get("model").and_then(Value::as_str).unwrap_or("").to_string();
        max_tokens = v.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
        session = session_key(&parts.headers, &v); // ДО инжекта — по исходному контенту клиента
        if app.cfg.inject_identity && inject_identity(&mut v, &app.cfg.identity) {
            if let Ok(b) = serde_json::to_vec(&v) { body_bytes = bytes::Bytes::from(b); }
        }
    }

    // РЕЗЕРВ баланса метерного ключа: атомарно списываем ПОТОЛОК стоимости запроса до начала
    // обслуживания. Устраняет гонку (конкурентные запросы не уводят баланс в минус), актуальную
    // стоимость закрываем в finalize (settle). Потолок: max_tokens по цене output + вход по САМОЙ
    // дорогой входной ставке (cache_write_1h) на ПОЛНЫЕ байты (токенов ≤ байт) → charge входа ≤ hold
    // при любой корзине. web_search — мягкий буфер (число вызовов заранее неизвестно), не абсолют.
    let mut reserved: Option<(String, i64)> = None;
    if let (Authz::Metered { key, mult_bp }, Some(billing)) = (&authz, &app.billing) {
        let p = metering::model_prices(&model);
        // max_tokens от клиента клампим сверху: абсурдное значение (≫ любого лимита модели, ~128k out)
        // иначе переполнило бы i128 в ceiling/apply_multiplier. 2M — заведомо выше реальных лимитов.
        let mt = (if max_tokens > 0 { max_tokens.min(2_000_000) } else { 4096 }) as i128;
        // ВЕРХНЯЯ оценка входа: ПОЛНЫЕ байты тела+identity по самой дорогой входной ставке
        // (cache_write_1h). Токенов ВСЕГДА ≤ байт (токен = ≥1 символ = ≥1 байт), поэтому input_est=bytes
        // по ставке cw1h покрывает charge входа при ЛЮБОЙ корзине — ВКЛЮЧАЯ 1h-cache-creation, где вход
        // тарифицируется по cw1h. (Прежний bytes/2 покрывал cw1h-вход лишь до 0.5 ток/байт и пробивался
        // плотным 1h-cache вводом — баланс мог уйти за резерв.)
        let input_est = ((raw.len() + app.cfg.identity.len()) as i128).max(1);
        // web_search: число вызовов заранее НЕИЗВЕСТНО (агентный цикл) → резерв на разумный потолок (20),
        // НЕ абсолютный предел. Overage сверх него уводит баланс чуть в минус, затем ключ блокируется
        // (≤0) — мягкая деградация; сама тарификация остаётся ТОЧНОЙ (charge = реальный usage).
        let web_buf = 20 * metering::WEB_SEARCH_NANO;
        let ceiling = mt * p.output + input_est * p.cache_write_1h + web_buf;
        let hold = metering::apply_multiplier(ceiling, *mult_bp).clamp(0, i64::MAX as i128) as i64;
        match billing.reserve(key, hold) {
            Some(_) => reserved = Some((key.clone(), hold)),
            None => return err_response(StatusCode::PAYMENT_REQUIRED, "invalid_request_error",
                                        "insufficient balance for this request — top up your key"),
        }
    }
    // Гард резерва: на любом не-успешном исходе И при отмене запроса вернёт hold клиенту.
    // Разоружим на успехе — там hold закрывает tee-метеринг. Снимает утечку денег при disconnect.
    let mut hold_guard = reserved.as_ref().map(|(k, h)| HoldGuard {
        billing: app.billing.clone(), key: k.clone(), hold: *h, armed: true,
    });

    let version = parts.headers.get("anthropic-version")
        .and_then(|v| v.to_str().ok()).unwrap_or(&app.cfg.anthropic_version).to_string();
    let beta = merge_beta(
        parts.headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
        &app.cfg.default_beta);

    // Circuit breaker разомкнут (брауноут апстрима) → быстрый отбой, НЕ веерим по всему пулу.
    // Резерв вернёт `hold_guard` на return.
    if let Some(retry) = app.breaker.open_for(pool::now()) {
        Metrics::inc(&app.metrics.breaker_rejects);
        return err_retry(StatusCode::SERVICE_UNAVAILABLE, "overloaded_error",
                         "upstream temporarily unavailable", retry);
    }

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
            if auth_tries < 2 && attempt < hard_cap {
                last = err_response(st, "overloaded_error", "upstream unavailable");
                continue; // единичный 401/403 — вдруг дохлый токен: пробуем другую подписку без cooling
            }
            app.pool.mark_healthy(&sub.email);          // повтор на разных подписках → вина запроса
            return stream_back(st, resp, None);          // прозрачно отдаём реальный 401/403 клиенту
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
                (Authz::Metered { key, mult_bp }, Some((_, hold))) =>
                    app.billing.clone().map(|billing| BillCtx {
                        billing, key: key.clone(), mult_bp: *mult_bp, hold,
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
    fn ct_eq_is_correct() {
        assert!(ct_eq(b"secret-key", b"secret-key"));
        assert!(!ct_eq(b"secret-key", b"secret-keX"));
        assert!(!ct_eq(b"short", b"longer-key")); // разная длина
        assert!(ct_eq(b"", b""));
    }
}
