//! Апстрим: кэш reqwest-клиентов по прокси + опрос лимитов подписки.
//! Каждая подписка ходит на api.anthropic.com со СВОЕГО IP (через свой прокси).

use crate::config::ProxyConfig;
use wreq::Client;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

/// Детерминированный seed персоны из email (стабилен во времени и между рестартами).
fn email_seed(email: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    email.hash(&mut h);
    h.finish()
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
    cfg.user_agents.first().cloned().unwrap_or_else(|| cfg.user_agent.clone())
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
        .header("x-stainless-runtime-version", &cfg.stainless_runtime_version)
        .header("x-stainless-package-version", &cfg.stainless_package_version)
        .header("x-stainless-os", &cfg.stainless_os)
        .header("x-stainless-arch", &cfg.stainless_arch)
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-timeout", "600")
        .header("anthropic-dangerous-direct-browser-access", "true") // реальный CC всегда шлёт
        .header("accept", "application/json")
        .header("accept-encoding", "gzip, deflate, br, zstd") // точный набор Bun/undici (снято с 2.1.195)
}

/// Свежий per-request UUID для `x-client-request-id` (реальный CC шлёт случайный на КАЖДЫЙ запрос —
/// корреляционный id, не персона-стабильный). Мешаем время+счётчик, чтобы не был предсказуемо
/// последовательным. Значение семантически безразлично Anthropic; важно ПРИСУТСТВИЕ + вид uuid.
pub fn fresh_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0x1234_5678_9abc_def0);
    let n = CTR.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let lo = n ^ t.rotate_left(17);
    let hi = t ^ n.rotate_left(41);
    let a = hex_expand(hi, 2); // 32 hex из времени+счётчика
    let b = hex_expand(lo, 2);
    format!("{}-{}-4{}-{}-{}", &a[0..8], &a[8..12], &a[13..16], &b[0..4], &b[4..16])
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
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// `metadata.user_id` В ТОЧНОМ формате Claude Code — **stringified JSON**
/// `{"device_id":"<64hex>","account_uuid":"<uuid>","session_id":"<uuid>"}` (снято с живого claude).
/// device_id/account_uuid стабильны per-подписка (от email); session_id — от session-ключа диалога
/// (per-разговор, как у настоящего CC — одна сессия на диалог). Инжектим, перекрывая клиентский.
pub fn persona_user_id(email: &str, session: Option<u64>) -> String {
    let e = email_seed(email);
    let device_id = hex_expand(e, 4);                 // 64 hex
    let account_uuid = uuid_from(e.wrapping_add(0xA1)); // стабилен per-подписка
    let session_id = persona_session_id(email, session);
    format!(r#"{{"device_id":"{device_id}","account_uuid":"{account_uuid}","session_id":"{session_id}"}}"#)
}
/// session_id (UUID) — тот же, что в `metadata.user_id`, для заголовка `X-Claude-Code-Session-Id`.
pub fn persona_session_id(email: &str, session: Option<u64>) -> String {
    uuid_from(session.unwrap_or_else(|| email_seed(email)).wrapping_add(0x5E))
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
    map: Mutex<HashMap<String, Client>>,
    connect_timeout: u64,
    user_agent: String,
}

impl Clients {
    pub fn new(cfg: &ProxyConfig) -> Self {
        Clients {
            map: Mutex::new(HashMap::new()),
            connect_timeout: cfg.connect_timeout,
            user_agent: cfg.user_agent.clone(),
        }
    }

    /// Клиент для данного прокси ("" = напрямую). Без общего request-timeout — иначе рвал бы стримы.
    /// **HTTP/1.1-ONLY** — как настоящий Claude Code: захват реального ClientHello показал ALPN=`http/1.1`
    /// (undici/Node по умолчанию h1). h2 у нас был бы отличием (иной ALPN в JA4 + Akamai-h2-отпечаток +
    /// характерный 30с idle-PING). На h1 живость держим TCP keep-alive + read_timeout (мёртвое соединение
    /// = нет байтов → read_timeout рвёт; SSE-ping Anthropic сбрасывает таймер, живой стрим не рвётся).
    pub fn get(&self, proxy: &str, email: &str) -> wreq::Result<Client> {
        // Ключ = (proxy, email): своя изоляция соединений/TLS-сессий на КАЖДУЮ подписку (см. док структуры).
        let key = format!("{proxy}\x00{email}");
        if let Some(c) = self.map.lock().unwrap_or_else(|e| e.into_inner()).get(&key) { return Ok(c.clone()); }
        let _ = &self.user_agent; // UA ставим per-request (persona_ua); клиентский не нужен
        let mut b = Client::builder()
            .emulation(crate::nodetls::bun_emulation())           // ClientHello БАЙТ-В-БАЙТ как Claude Code
                                                                  // (Bun/BoringSSL) + ALPN=http/1.1 (см. nodetls)
            .connect_timeout(Duration::from_secs(self.connect_timeout))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .read_timeout(Duration::from_secs(120));              // idle между чтениями: ловит «подключился
            //   но молчит» до первого байта И зависший посреди стрима; сбрасывается на каждом чтении,
            //   поэтому живой поток данных (в т.ч. SSE с ping-ами Anthropic) не рвёт.
        if !proxy.is_empty() {
            b = b.proxy(wreq::Proxy::all(proxy)?);
        }
        let c = b.build()?;
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(key, c.clone());
        Ok(c)
    }
}

/// Утилизация приходит ДОЛЕЙ 0.0–1.0 — ПОДТВЕРЖДЕНО живым заголовком (`7d-utilization: 0.22` = 22%).
/// Значит «ровно 1% → 100%» невозможен: Anthropic шлёт `0.01` для 1%, а не `1`. Клампим в [0,1] от
/// шума; ветку `/100 при >1` держим лишь страховкой на случай смены формата (реально не срабатывает).
fn util(h: &wreq::header::HeaderMap, name: &str) -> Option<f64> {
    let v: f64 = h.get(name)?.to_str().ok()?.trim().parse().ok()?;
    Some((if v > 1.0 { v / 100.0 } else { v }).clamp(0.0, 1.0))
}
/// RFC3339 → Unix epoch (сек). Поддерживает `Z` и `±HH:MM`; дробные секунды игнорируем (нужна
/// секундная точность). Алгоритм civil→days — Howard Hinnant, без внешних зависимостей. None при
/// любом несоответствии формату.
fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 { return None; }
    let num = |lo: usize, hi: usize| s.get(lo..hi)?.parse::<i64>().ok();
    if b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':' || b[16] != b':' { return None; }
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
            let om = tz.get(4..6).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
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
    if let Ok(f) = v.parse::<f64>() { return Some(f as i64); }
    parse_rfc3339(v)
}

/// Снимок лимитов из unified-заголовков (без HTTP-кода). Приходят на КАЖДЫЙ ответ
/// api.anthropic.com — и на боевой, и на 429 — поэтому вытаскиваем их из реального
/// трафика бесплатно, а активный поллинг оставляем только для простаивающих подписок.
#[derive(Clone, Debug, Default)]
pub struct Limits {
    pub util5h: Option<f64>,
    pub util7d: Option<f64>,
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
    Limits {
        util5h: util(h, "anthropic-ratelimit-unified-5h-utilization"),
        util7d: util(h, "anthropic-ratelimit-unified-7d-utilization"),
        status: h.get("anthropic-ratelimit-unified-status")
            .and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
        reset5h: reset_ts(h, "anthropic-ratelimit-unified-5h-reset"),
        reset7d: reset_ts(h, "anthropic-ratelimit-unified-7d-reset"),
        claim: h.get("anthropic-ratelimit-unified-representative-claim")
            .and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
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
    pub status: Option<String>,
    pub reset5h: Option<i64>,
    pub reset7d: Option<i64>,
    pub http: u16,
}

/// Результат детекта тарифа подписки.
pub enum PlanDetect {
    Plan(String),   // "pro" | "max5" | "max20"
    NoScope,        // 403 — у токена нет scope user:profile (частый случай setup-token)
    Err(String),    // сеть/парсинг/неизвестный тариф
}

/// Определить тариф подписки — как это делает Claude Code: GET /api/oauth/profile с Bearer
/// подписки (через её прокси). Только Authorization+Content-Type, GET, БЕЗ anthropic-beta.
/// user:inference-only токен профиль не отдаёт (403) → NoScope.
pub async fn detect_plan(client: &Client, cfg: &ProxyConfig, token: &str, ua: &str) -> PlanDetect {
    let url = format!("{}/api/oauth/profile", cfg.upstream.trim_end_matches('/'));
    let mut rb = client.get(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("user-agent", ua);
    rb = apply_persona_headers(rb, cfg); // единый отпечаток CC и на profile-запросе
    let resp = match rb
        .timeout(Duration::from_secs(20))
        .send().await
    {
        Ok(r) => r,
        Err(e) => return PlanDetect::Err(format!("net:{e}")),
    };
    let code = resp.status().as_u16();
    if code == 403 { return PlanDetect::NoScope; }
    if code != 200 { return PlanDetect::Err(format!("http{code}")); }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v, Err(e) => return PlanDetect::Err(format!("badjson:{e}")),
    };
    let acc = &v["account"];
    let org = &v["organization"];
    let tier = org["rate_limit_tier"].as_str().unwrap_or("");
    let otype = org["organization_type"].as_str().unwrap_or("");
    let has_max = acc["has_claude_max"].as_bool().unwrap_or(false);
    let has_pro = acc["has_claude_pro"].as_bool().unwrap_or(false);
    if has_max || otype == "claude_max" || tier.starts_with("default_claude_max") {
        return PlanDetect::Plan(match tier {
            "default_claude_max_20x" => "max20",
            "default_claude_max_5x" => "max5",
            _ => "max5", // редкий Max без явного tier — консервативно
        }.to_string());
    }
    if has_pro || otype == "claude_pro" || tier == "default_claude_pro" {
        return PlanDetect::Plan("pro".to_string());
    }
    PlanDetect::Err(format!("unknown:{}", if otype.is_empty() { tier } else { otype }))
}

/// Минимальный запрос → читаем unified-ratelimit из ЗАГОЛОВКОВ (приходят и на 400/429).
/// Идентичность Claude Code включена, чтобы запрос был валиден и вернул реальные лимиты.
pub async fn poll_sub(client: &Client, cfg: &ProxyConfig, token: &str, ua: &str, email: &str) -> Option<PollResult> {
    // Анти-фингерпринт: тело probe РАЗНОЕ по персоне (стабильно во времени, но различно между подписками),
    // а не байт-в-байт "." / max_tokens:1 у всего флота. Иначе — два сигнала аномалии для Anthropic:
    // (а) один аккаунт шлёт роботизированный микрозапрос по расписанию; (б) N аккаунтов шлют ИДЕНТИЧНЫЙ
    // запрос (кросс-аккаунт корреляция). Набор — короткие человечные строки; max_tokens 1..=5.
    let seed = email_seed(email);
    const PROMPTS: [&str; 8] = ["Hi", "Hello", "Hey", "Good morning", "Thanks!", "Quick test", "Ping", "Are you there?"];
    let content = PROMPTS[(seed % PROMPTS.len() as u64) as usize];
    let max_tokens = 1 + (seed >> 8) % 5; // 1..=5
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": max_tokens,
        "system": [{"type": "text", "text": cfg.identity, "cache_control": {"type": "ephemeral"}}],
        "messages": [{"role": "user", "content": content}],
        "metadata": {"user_id": persona_user_id(email, None)} // тот же per-подписка user_id, что и в бою
    });
    let url = format!("{}/v1/messages", cfg.upstream.trim_end_matches('/'));
    let mut rb = client.post(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("anthropic-version", &cfg.anthropic_version)
        .header("anthropic-beta", &cfg.default_beta)
        .header("content-type", "application/json")
        .header("user-agent", ua); // per-persona UA — здоровье персоны идёт с тем же отпечатком, что и бой
    rb = apply_persona_headers(rb, cfg);          // ТОТ ЖЕ x-app + x-stainless-* + accept, что и в бою
    let resp = rb.json(&body).timeout(Duration::from_secs(25)).send().await.ok()?;
    let http = resp.status().as_u16();
    let l = limits_from_headers(resp.headers());
    Some(PollResult {
        util5h: l.util5h, util7d: l.util7d, status: l.status,
        reset5h: l.reset5h, reset7d: l.reset7d, http,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_user_id_is_cc_json() {
        let a = persona_user_id("alice@x.io", Some(42));
        assert_eq!(a, persona_user_id("alice@x.io", Some(42)), "стабилен per-(подписка,сессия)");
        assert_ne!(a, persona_user_id("bob@x.io", Some(42)), "device/account различны между подписками");
        // формат Claude Code: валидный JSON с тремя ключами
        let v: serde_json::Value = serde_json::from_str(&a).expect("user_id — валидный JSON");
        assert_eq!(v["device_id"].as_str().unwrap().len(), 64, "device_id = 64 hex");
        assert!(v["account_uuid"].as_str().unwrap().contains('-'), "account_uuid = uuid");
        assert!(v["session_id"].as_str().unwrap().contains('-'), "session_id = uuid");
        // session_id меняется с session-ключом, device/account — нет
        let b = persona_user_id("alice@x.io", Some(99));
        assert_ne!(serde_json::from_str::<serde_json::Value>(&a).unwrap()["session_id"],
                   serde_json::from_str::<serde_json::Value>(&b).unwrap()["session_id"], "session_id per-диалог");
    }

    #[test]
    fn rfc3339_epoch_conversions() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2000-01-01T00:00:00Z"), Some(946_684_800));
        // смещение зоны сводится к тому же инстанту
        assert_eq!(parse_rfc3339("2000-01-01T05:00:00+05:00"), Some(946_684_800));
        assert_eq!(parse_rfc3339("1999-12-31T19:00:00-05:00"), Some(946_684_800));
        // дробные секунды игнорируем (секундная точность)
        assert_eq!(parse_rfc3339("2000-01-01T00:00:00.123Z"), Some(946_684_800));
        // мусор / невалидные поля → None (сработает fallback, а не кривой epoch)
        assert_eq!(parse_rfc3339("not-a-date"), None);
        assert_eq!(parse_rfc3339("2000-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("2000-01-32T00:00:00Z"), None);
    }

}
