//! Апстрим: кэш reqwest-клиентов по прокси + опрос лимитов подписки.
//! Каждая подписка ходит на api.anthropic.com со СВОЕГО IP (через свой прокси).

use crate::config::ProxyConfig;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use wreq::Client;

/// Детерминированный seed персоны из email (стабилен во времени и между рестартами).
fn email_seed(email: &str) -> u64 {
    pool::stable_hash64(email.as_bytes())
}

/// **Per-persona UA.** Список реальных UA (`user_agents` len>1) → пиним один по hash(email). Иначе —
/// один ВАЛИДНЫЙ UA на весь флот. ВАЖНО: patch-версию НЕ варьируем. Раньше варьировали (`vary_patch`),
/// но это создавало НЕСУЩЕСТВУЮЩИЕ комбинации версий (UA говорит 2.1.188, а x-stainless-package-version/
/// identity — от 2.1.195) → аномалия ВНУТРИ одного запроса, хуже одинакового UA. Реальные юзеры на одной
/// версии CC делят точный UA — флот-константный валидный UA нормален. Различие аккаунтов даём через
/// egress-IP + `metadata.user_id` (per-подписка), а НЕ через фейковые версии. `ua_spread` больше не влияет.
pub fn persona_ua(cfg: &ProxyConfig, email: &str) -> String {
    let seed = email_seed(email);
    if cfg.user_agents.len() > 1 {
        return cfg.user_agents[(seed as usize) % cfg.user_agents.len()].clone();
    }
    cfg.user_agents
        .first()
        .cloned()
        .unwrap_or_else(|| cfg.user_agent.clone())
}

/// Добавить КОНСТАНТНЫЕ заголовки отпечатка Claude-Code-клиента (Stainless SDK): `x-app`, весь набор
/// `x-stainless-*`, `accept`. UA/anthropic-version/anthropic-beta/authorization каждый вызыватель ставит
/// сам. Применяется ОДИНАКОВО в бою (`forward`) и в probe/detect — иначе health-трафик имел бы иной
/// (урезанный) отпечаток, чем боевой. Реальный CC всегда шлёт полный набор; их отсутствие/чужие значения
/// (напр. `x-stainless-lang: python` от Python-SDK клиента) — мгновенный сигнал «это не Claude Code».
pub fn apply_persona_headers(rb: wreq::RequestBuilder, cfg: &ProxyConfig) -> wreq::RequestBuilder {
    rb.header("x-app", &cfg.x_app)
        .header("x-stainless-lang", &cfg.stainless_lang)
        .header("x-stainless-runtime", &cfg.stainless_runtime)
        .header(
            "x-stainless-runtime-version",
            &cfg.stainless_runtime_version,
        )
        .header(
            "x-stainless-package-version",
            &cfg.stainless_package_version,
        )
        .header("x-stainless-os", &cfg.stainless_os)
        .header("x-stainless-arch", &cfg.stainless_arch)
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-timeout", "600")
        .header("anthropic-dangerous-direct-browser-access", "true") // реальный CC всегда шлёт
        .header("accept", "application/json")
        // C6: рекламируем только кодеки, которые wreq гарантированно декодирует с текущими features.
        // Иначе deflate/zstd мог прийти сжатым, а downstream уже без Content-Encoding.
        .header("accept-encoding", "gzip, br")
        .header("connection", "keep-alive") // undici/CC шлёт явно; hyper на keep-alive его не пишет
}

/// Свежий CSPRNG UUIDv4 для `x-client-request-id` (реальный CC шлёт случайный на каждый запрос).
pub fn fresh_request_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("operating-system CSPRNG unavailable");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// Детерминированная hex-строка из seed (`words`×16 hex). Стабильна per-подписка.
fn hex_expand(seed: u64, words: usize) -> String {
    let mut out = String::with_capacity(words * 16);
    let mut s = seed;
    for _ in 0..words {
        s = s.wrapping_mul(0x9e3779b97f4a7c15).rotate_left(29) ^ 0xda3e_39cb_94b9_5bdb;
        out.push_str(&format!("{s:016x}"));
    }
    out
}
/// UUID-подобная строка (8-4-4-4-12 hex) из seed. Детерминирована.
fn uuid_from(seed: u64) -> String {
    let h = hex_expand(seed, 2); // 32 hex
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// `metadata.user_id` В ТОЧНОМ формате Claude Code — **stringified JSON**
/// `{"device_id":"<64hex>","account_uuid":"<uuid>","session_id":"<uuid>"}` (снято с живого claude).
/// device_id/account_uuid стабильны per-подписка (от email); session_id — от session-ключа диалога
/// (per-разговор, как у настоящего CC — одна сессия на диалог). Инжектим, перекрывая клиентский.
pub fn persona_user_id(email: &str, session: Option<u64>) -> String {
    let e = email_seed(email);
    let device_id = hex_expand(e, 4); // 64 hex
    let account_uuid = uuid_from(e.wrapping_add(0xA1)); // стабилен per-подписка
    let session_id = persona_session_id(email, session);
    format!(
        r#"{{"device_id":"{device_id}","account_uuid":"{account_uuid}","session_id":"{session_id}"}}"#
    )
}
/// session_id (UUID) — тот же, что в `metadata.user_id`, для заголовка `X-Claude-Code-Session-Id`.
pub fn persona_session_id(email: &str, session: Option<u64>) -> String {
    uuid_from(
        session
            .unwrap_or_else(|| email_seed(email))
            .wrapping_add(0x5E),
    )
}
/// cch (config-hash) для billing-header — 5 hex, СТАБИЛЕН per-подписка (per-install реализм: каждая
/// подписка = отдельная «инсталляция» claude со своим конфиг-хэшем; общий cch на флот был бы кластером).
pub fn persona_cch(email: &str) -> String {
    hex_expand(email_seed(email).wrapping_add(0xCC), 1)[0..5].to_string()
}
/// build-суффикс cc_version (`cc_version=<base>.d<NN>`). Живые захваты показали, что `.dNN` МЕНЯЕТСЯ
/// между запусками (d49, d80 — зависит от конфиг-окружения инстанса), т.е. это НЕ стабильный build-id.
/// Значит фиксировать один `.dNN` на весь флот = кластер. Делаем стабильным per-подписка (свой у каждой
/// «инсталляции»), 2 цифры (10..99) — как в наблюдаемом формате.
pub fn persona_ccbuild(email: &str) -> String {
    format!("d{}", 10 + (email_seed(email).wrapping_add(0xB1) % 90))
}

/// Кэш http-клиентов: один клиент на ПАРУ (прокси, подписка). Ключевание по email критично для
/// анти-фингерпринта: клиент по одной лишь строке прокси заставил бы подписки с общим/пустым прокси
/// делить ОДИН reqwest::Client → один пул TCP/HTTP2-соединений и TLS-session-store → токены разных
/// Max-аккаунтов мультиплексировались бы в одном TLS-соединении (сильнейший сигнал «это пул»). Свой
/// клиент на подписку изолирует соединения и TLS-сессии полностью.
pub struct Clients {
    map: Mutex<ClientCache>,
    connect_timeout: u64,
    read_timeout: u64,
    nonstream_read_timeout: u64,
    user_agent: String,
}

struct ClientCache {
    entries: HashMap<String, (Client, Instant)>,
}

const CLIENT_CACHE_CAPACITY: usize = 4_096;
const CLIENT_CACHE_IDLE_TTL: Duration = Duration::from_secs(30 * 60);

impl Clients {
    pub fn new(cfg: &ProxyConfig) -> Self {
        Clients {
            map: Mutex::new(ClientCache {
                entries: HashMap::new(),
            }),
            connect_timeout: cfg.connect_timeout,
            read_timeout: cfg.read_timeout,
            nonstream_read_timeout: cfg.nonstream_read_timeout,
            user_agent: cfg.user_agent.clone(),
        }
    }

    /// Пауза между чтениями для СТРИМА, если она задана. Anthropic шлёт SSE-ping, поэтому живой
    /// поток её не встречает, а зависший ловится быстро — это детектор, а не потолок.
    pub fn stream_read_timeout(&self) -> Option<Duration> {
        (self.read_timeout > 0).then(|| Duration::from_secs(self.read_timeout))
    }

    /// Пауза между чтениями для НЕ-стриминговых запросов, если она задана. По умолчанию её нет:
    /// апстрим молчит до конца генерации, «молчит» там не означает «умер», и никакое число не
    /// умеет их различить. Живость на этом пути держат TCP keep-alive и отмена по дисконнекту.
    pub fn nonstream_read_timeout(&self) -> Option<Duration> {
        (self.nonstream_read_timeout > 0).then(|| Duration::from_secs(self.nonstream_read_timeout))
    }

    /// Клиент для данного прокси ("" = напрямую). Без общего request-timeout — иначе рвал бы стримы.
    /// **HTTP/1.1-ONLY** — как настоящий Claude Code: захват реального ClientHello показал ALPN=`http/1.1`
    /// (undici/Node по умолчанию h1). h2 у нас был бы отличием (иной ALPN в JA4 + Akamai-h2-отпечаток +
    /// характерный 30с idle-PING). На h1 живость держим TCP keep-alive + read_timeout (мёртвое соединение
    /// = нет байтов → read_timeout рвёт; SSE-ping Anthropic сбрасывает таймер, живой стрим не рвётся).
    pub fn get(&self, proxy: &str, email: &str) -> wreq::Result<Client> {
        // Ключ = (proxy, email): своя изоляция соединений/TLS-сессий на КАЖДУЮ подписку (см. док структуры).
        let key = format!("{proxy}\x00{email}");
        {
            let mut cache = self.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((client, touched)) = cache.entries.get_mut(&key) {
                *touched = Instant::now();
                return Ok(client.clone());
            }
        }
        let _ = &self.user_agent; // UA ставим per-request (persona_ua); клиентский не нужен
        let mut b = Client::builder()
            .emulation(crate::nodetls::bun_emulation()) // ClientHello БАЙТ-В-БАЙТ как Claude Code
            // (Bun/BoringSSL) + ALPN=http/1.1 (см. nodetls)
            .connect_timeout(Duration::from_secs(self.connect_timeout))
            .pool_idle_timeout(Duration::from_secs(90))
            // Никакого клиентского read_timeout по умолчанию: границу выбирает каждый запрос сам,
            // и не-стриминговый выбирает «никакой». Значение на уровне клиента молча вернуло бы
            // потолок, ради снятия которого это разделение и сделано.
            .tcp_keepalive(Duration::from_secs(60));
        if !proxy.is_empty() {
            b = b.proxy(wreq::Proxy::all(proxy)?);
        }
        let c = b.build()?;
        let mut cache = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        cache
            .entries
            .retain(|_, (_, touched)| now.duration_since(*touched) < CLIENT_CACHE_IDLE_TTL);
        if cache.entries.len() >= CLIENT_CACHE_CAPACITY {
            if let Some(oldest) = cache
                .entries
                .iter()
                .min_by_key(|(_, (_, touched))| *touched)
                .map(|(key, _)| key.clone())
            {
                cache.entries.remove(&oldest);
            }
        }
        cache.entries.insert(key, (c.clone(), now));
        Ok(c)
    }
}

const FRACTION_SCALE: i64 = 100_000_000;

/// Exact quota endpoint value and the numeric resolution carried by its original decimal spelling.
/// Storage precision is always `10^-8`; this separate resolution prevents `0.22` from pretending
/// that Anthropic measured more finely than one hundredth of a full window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaFraction {
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
}

impl QuotaFraction {
    pub fn ratio(self) -> f64 {
        self.used_fraction_units as f64 / FRACTION_SCALE as f64
    }
}

fn pow10(exponent: usize) -> Option<i128> {
    (0..exponent).try_fold(1i128, |value, _| value.checked_mul(10))
}

/// Anthropic currently sends fractions (`0.22` = 22%). A decimal spelling is therefore always a
/// native fraction, including the observed exhausted-window overshoot `1.02` (= 102%, clamped to
/// full). Only an integer above one retains the historical whole-percent fallback (`22` = 22%).
/// Both paths are parsed as decimal integers without an `f64` round-trip.
fn parse_quota_fraction(raw: &str) -> Option<QuotaFraction> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('-') || raw.contains('e') || raw.contains('E') {
        return None;
    }
    let raw = raw.strip_prefix('+').unwrap_or(raw);
    let mut parts = raw.split('.');
    let whole_text = parts.next()?;
    let fraction_text = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole_text.is_empty()
        || !whole_text.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction_text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let whole = whole_text.parse::<i128>().ok()?;
    let is_native_fraction = raw.contains('.') || whole <= 1;
    let (unit_scale, max_decimals, max_whole) = if is_native_fraction {
        (i128::from(FRACTION_SCALE), 8usize, 1i128)
    } else {
        // One percentage point is 1e6 full-window fraction units.
        (i128::from(FRACTION_SCALE / 100), 6usize, 100i128)
    };
    if whole > max_whole {
        return None;
    }

    let (kept, excess) = fraction_text.split_at(fraction_text.len().min(max_decimals));
    if excess.bytes().any(|byte| byte != b'0') {
        return None;
    }
    let kept_value = if kept.is_empty() {
        0
    } else {
        kept.parse::<i128>().ok()?
    };
    let decimal_scale = pow10(kept.len())?;
    let units = whole.checked_mul(unit_scale)?.checked_add(
        kept_value
            .checked_mul(unit_scale)?
            .checked_div(decimal_scale)?,
    )?;
    let resolution = unit_scale.checked_div(decimal_scale)?.max(1);
    Some(QuotaFraction {
        used_fraction_units: i64::try_from(units).ok()?.clamp(0, FRACTION_SCALE),
        measurement_resolution_fraction_units: i64::try_from(resolution).ok()?,
    })
}

fn quota_fraction(h: &wreq::header::HeaderMap, name: &str) -> Option<QuotaFraction> {
    parse_quota_fraction(h.get(name)?.to_str().ok()?)
}
/// RFC3339 → Unix epoch (сек). Поддерживает `Z` и `±HH:MM`; дробные секунды игнорируем (нужна
/// секундная точность). Алгоритм civil→days — Howard Hinnant, без внешних зависимостей. None при
/// любом несоответствии формату.
fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |lo: usize, hi: usize| s.get(lo..hi)?.parse::<i64>().ok();
    if b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    // civil → дни от эпохи
    let yy = y - if mo <= 2 { 1 } else { 0 };
    let era = (if yy >= 0 { yy } else { yy - 399 }) / 400;
    let yoe = yy - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut epoch = days * 86400 + h * 3600 + mi * 60 + se;
    // зона: суффикс после секунд (пропускаем дробную часть)
    let tz = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    match tz.as_bytes().first() {
        None | Some(b'Z') | Some(b'z') => {} // UTC
        Some(&sign @ (b'+' | b'-')) => {
            let oh = tz.get(1..3)?.parse::<i64>().ok()?;
            let om = tz
                .get(4..6)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            let off = oh * 3600 + om * 60;
            epoch += if sign == b'+' { -off } else { off };
        }
        _ => return None,
    }
    Some(epoch)
}

/// Reset-заголовок → Unix epoch (сек). ПОДТВЕРЖДЕНО живым заголовком: числовой epoch (`5h-reset:
/// 1783884600`). Числовой путь — основной; RFC3339-fallback держим страховкой на смену формата.
/// Неизвестное → None → выше сработает fallback cooling, а не поедет вся математика окон.
fn reset_ts(h: &wreq::header::HeaderMap, name: &str) -> Option<i64> {
    let v = h.get(name)?.to_str().ok()?.trim();
    if let Ok(f) = v.parse::<f64>() {
        return Some(f as i64);
    }
    parse_rfc3339(v)
}

/// Снимок лимитов из unified-заголовков (без HTTP-кода). Приходят на КАЖДЫЙ ответ
/// api.anthropic.com — и на боевой, и на 429 — поэтому вытаскиваем их из реального
/// трафика бесплатно, а активный поллинг оставляем только для простаивающих подписок.
#[derive(Clone, Debug, Default)]
pub struct Limits {
    pub util5h: Option<f64>,
    pub util7d: Option<f64>,
    pub quota5h: Option<QuotaFraction>,
    pub quota7d: Option<QuotaFraction>,
    pub status: Option<String>,
    pub reset5h: Option<i64>,
    pub reset7d: Option<i64>,
    /// Какое окно Anthropic считает СВЯЗЫВАЮЩИМ прямо сейчас (`representative-claim`: "five_hour" |
    /// "seven_day"). Авторитетный источник для атрибуции 429-cooling нужному окну — вместо догадки
    /// «util≥0.95». Подтверждено живым заголовком (`unified-representative-claim: five_hour`).
    pub claim: Option<String>,
}

/// Разобрать unified-ratelimit из заголовков ответа.
pub fn limits_from_headers(h: &wreq::header::HeaderMap) -> Limits {
    let quota5h = quota_fraction(h, "anthropic-ratelimit-unified-5h-utilization");
    let quota7d = quota_fraction(h, "anthropic-ratelimit-unified-7d-utilization");
    Limits {
        util5h: quota5h.map(QuotaFraction::ratio),
        util7d: quota7d.map(QuotaFraction::ratio),
        quota5h,
        quota7d,
        status: h
            .get("anthropic-ratelimit-unified-status")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
        reset5h: reset_ts(h, "anthropic-ratelimit-unified-5h-reset"),
        reset7d: reset_ts(h, "anthropic-ratelimit-unified-7d-reset"),
        claim: h
            .get("anthropic-ratelimit-unified-representative-claim")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
    }
}

impl Limits {
    /// Есть ли хоть какое-то полезное значение утилизации (иначе не трогаем состояние пула).
    pub fn has_util(&self) -> bool {
        self.util5h.is_some() || self.util7d.is_some()
    }
}

pub struct PollResult {
    pub util5h: Option<f64>,
    pub util7d: Option<f64>,
    pub quota5h: Option<QuotaFraction>,
    pub quota7d: Option<QuotaFraction>,
    pub status: Option<String>,
    pub reset5h: Option<i64>,
    pub reset7d: Option<i64>,
    pub http: u16,
}

/// Результат детекта тарифа подписки.
#[derive(Debug, PartialEq, Eq)]
pub enum PlanDetect {
    Plan(String), // "pro" | "max5" | "max20"
    NoScope,      // 403 — у токена нет scope user:profile (частый случай setup-token)
    Err(String),  // сеть/парсинг/неизвестный тариф
}

/// Определить тариф подписки — как это делает Claude Code: GET /api/oauth/profile с Bearer
/// подписки (через её прокси). Только Authorization+Content-Type, GET, БЕЗ anthropic-beta.
/// user:inference-only токен профиль не отдаёт (403) → NoScope.
pub async fn detect_plan(client: &Client, cfg: &ProxyConfig, token: &str, ua: &str) -> PlanDetect {
    let url = format!("{}/api/oauth/profile", cfg.upstream.trim_end_matches('/'));
    let mut rb = client
        .get(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("user-agent", ua);
    rb = apply_persona_headers(rb, cfg); // единый отпечаток CC и на profile-запросе
    let resp = match rb.timeout(Duration::from_secs(20)).send().await {
        Ok(r) => r,
        Err(e) => return PlanDetect::Err(format!("net:{e}")),
    };
    let code = resp.status().as_u16();
    if code == 403 {
        return PlanDetect::NoScope;
    }
    if code != 200 {
        return PlanDetect::Err(format!("http{code}"));
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return PlanDetect::Err(format!("badjson:{e}")),
    };
    plan_from_profile(&v)
}

fn plan_from_profile(v: &serde_json::Value) -> PlanDetect {
    let acc = &v["account"];
    let org = &v["organization"];
    let tier = org["rate_limit_tier"].as_str().unwrap_or("");
    let otype = org["organization_type"].as_str().unwrap_or("");
    let has_max = acc["has_claude_max"].as_bool().unwrap_or(false);
    let has_pro = acc["has_claude_pro"].as_bool().unwrap_or(false);
    if has_max || otype == "claude_max" || tier.starts_with("default_claude_max") {
        return PlanDetect::Plan(
            match tier {
                "default_claude_max_20x" => "max20",
                "default_claude_max_5x" => "max5",
                _ => "max5", // редкий Max без явного tier — консервативно
            }
            .to_string(),
        );
    }
    if has_pro || otype == "claude_pro" || tier == "default_claude_pro" {
        return PlanDetect::Plan("pro".to_string());
    }
    PlanDetect::Err(format!(
        "unknown:{}",
        if otype.is_empty() { tier } else { otype }
    ))
}

/// Free count-tokens request used only to read liveness/rate-limit headers. It exercises the same
/// OAuth scope and request shape without creating synthetic model output or spending quota.
pub async fn poll_sub(
    client: &Client,
    cfg: &ProxyConfig,
    token: &str,
    ua: &str,
    email: &str,
) -> Option<PollResult> {
    let seed = email_seed(email);
    const PROMPTS: [&str; 8] = [
        "Hi",
        "Hello",
        "Hey",
        "Good morning",
        "Thanks!",
        "Quick test",
        "Ping",
        "Are you there?",
    ];
    let content = PROMPTS[(seed % PROMPTS.len() as u64) as usize];
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "system": [{"type": "text", "text": cfg.identity, "cache_control": {"type": "ephemeral"}}],
        "messages": [{"role": "user", "content": content}]
    });
    let url = format!(
        "{}/v1/messages/count_tokens",
        cfg.upstream.trim_end_matches('/')
    );
    let mut rb = client
        .post(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("anthropic-version", &cfg.anthropic_version)
        .header("anthropic-beta", &cfg.default_beta)
        .header("content-type", "application/json")
        .header("user-agent", ua); // per-persona UA — здоровье персоны идёт с тем же отпечатком, что и бой
    rb = apply_persona_headers(rb, cfg); // ТОТ ЖЕ x-app + x-stainless-* + accept, что и в бою
    let resp = match rb
        .json(&body)
        .timeout(Duration::from_secs(25))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            elog::warn("upstream", format!("poll_sub transport failure: {e}"));
            return None;
        }
    };
    let http = resp.status().as_u16();
    let l = limits_from_headers(resp.headers());
    Some(PollResult {
        util5h: l.util5h,
        util7d: l.util7d,
        quota5h: l.quota5h,
        quota7d: l.quota7d,
        status: l.status,
        reset5h: l.reset5h,
        reset7d: l.reset7d,
        http,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_user_id_is_cc_json() {
        let a = persona_user_id("alice@x.io", Some(42));
        assert_eq!(
            a,
            persona_user_id("alice@x.io", Some(42)),
            "стабилен per-(подписка,сессия)"
        );
        assert_ne!(
            a,
            persona_user_id("bob@x.io", Some(42)),
            "device/account различны между подписками"
        );
        // формат Claude Code: валидный JSON с тремя ключами
        let v: serde_json::Value = serde_json::from_str(&a).expect("user_id — валидный JSON");
        assert_eq!(
            v["device_id"].as_str().unwrap().len(),
            64,
            "device_id = 64 hex"
        );
        assert!(
            v["account_uuid"].as_str().unwrap().contains('-'),
            "account_uuid = uuid"
        );
        assert!(
            v["session_id"].as_str().unwrap().contains('-'),
            "session_id = uuid"
        );
        // session_id меняется с session-ключом, device/account — нет
        let b = persona_user_id("alice@x.io", Some(99));
        assert_ne!(
            serde_json::from_str::<serde_json::Value>(&a).unwrap()["session_id"],
            serde_json::from_str::<serde_json::Value>(&b).unwrap()["session_id"],
            "session_id per-диалог"
        );
    }

    #[test]
    fn rfc3339_epoch_conversions() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2000-01-01T00:00:00Z"), Some(946_684_800));
        // смещение зоны сводится к тому же инстанту
        assert_eq!(
            parse_rfc3339("2000-01-01T05:00:00+05:00"),
            Some(946_684_800)
        );
        assert_eq!(
            parse_rfc3339("1999-12-31T19:00:00-05:00"),
            Some(946_684_800)
        );
        // дробные секунды игнорируем (секундная точность)
        assert_eq!(parse_rfc3339("2000-01-01T00:00:00.123Z"), Some(946_684_800));
        // мусор / невалидные поля → None (сработает fallback, а не кривой epoch)
        assert_eq!(parse_rfc3339("not-a-date"), None);
        assert_eq!(parse_rfc3339("2000-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("2000-01-32T00:00:00Z"), None);
    }

    #[test]
    fn quota_fraction_preserves_units_and_provider_resolution_without_float() {
        assert_eq!(
            parse_quota_fraction("0.22"),
            Some(QuotaFraction {
                used_fraction_units: 22_000_000,
                measurement_resolution_fraction_units: 1_000_000,
            })
        );
        assert_eq!(
            parse_quota_fraction("0.22000001"),
            Some(QuotaFraction {
                used_fraction_units: 22_000_001,
                measurement_resolution_fraction_units: 1,
            })
        );
        assert_eq!(
            parse_quota_fraction("22"),
            Some(QuotaFraction {
                used_fraction_units: 22_000_000,
                measurement_resolution_fraction_units: 1_000_000,
            })
        );
        assert_eq!(
            parse_quota_fraction("1"),
            Some(QuotaFraction {
                used_fraction_units: 100_000_000,
                measurement_resolution_fraction_units: 100_000_000,
            })
        );
        assert_eq!(
            parse_quota_fraction("1.00000000"),
            Some(QuotaFraction {
                used_fraction_units: 100_000_000,
                measurement_resolution_fraction_units: 1,
            })
        );
        assert_eq!(
            parse_quota_fraction("1.02"),
            Some(QuotaFraction {
                used_fraction_units: 100_000_000,
                measurement_resolution_fraction_units: 1_000_000,
            }),
            "decimal values above one are native fraction overshoot, not historical percent",
        );
        assert_eq!(
            parse_quota_fraction("1.00000001"),
            Some(QuotaFraction {
                used_fraction_units: 100_000_000,
                measurement_resolution_fraction_units: 1,
            })
        );
        assert!(parse_quota_fraction("1.1e-2").is_none());
        assert!(parse_quota_fraction("2.0").is_none());
        assert!(parse_quota_fraction("101").is_none());
        assert!(parse_quota_fraction("0.123456789").is_none());
    }

    #[test]
    fn profile_plan_detection_accepts_only_known_paid_cohorts() {
        for (profile, expected) in [
            (
                serde_json::json!({
                    "account": {"has_claude_max": true},
                    "organization": {"rate_limit_tier": "default_claude_max_20x"}
                }),
                PlanDetect::Plan("max20".to_owned()),
            ),
            (
                serde_json::json!({
                    "account": {},
                    "organization": {
                        "organization_type": "claude_max",
                        "rate_limit_tier": "default_claude_max_5x"
                    }
                }),
                PlanDetect::Plan("max5".to_owned()),
            ),
            (
                serde_json::json!({
                    "account": {"has_claude_pro": true},
                    "organization": {}
                }),
                PlanDetect::Plan("pro".to_owned()),
            ),
        ] {
            assert_eq!(plan_from_profile(&profile), expected);
        }
        assert_eq!(
            plan_from_profile(&serde_json::json!({
                "account": {},
                "organization": {"rate_limit_tier": "free"}
            })),
            PlanDetect::Err("unknown:free".to_owned()),
        );
    }
}
