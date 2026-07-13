//! Апстрим: кэш reqwest-клиентов по прокси + опрос лимитов подписки.
//! Каждая подписка ходит на api.anthropic.com со СВОЕГО IP (через свой прокси).

use crate::config::ProxyConfig;
use reqwest::Client;
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

/// Понизить patch-версию первого токена вида `A.B.C` в UA на `seed % spread`. patch-релизы почти
/// наверняка существовали → правдоподобный разброс между персонами без смены major.minor.
fn vary_patch(base: &str, seed: u64, spread: u32) -> String {
    if spread <= 1 { return base.to_string(); }
    for token in base.split([' ', '/', '(', ')', ',']) {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
            if let (Ok(maj), Ok(min), Ok(patch)) =
                (parts[0].parse::<u32>(), parts[1].parse::<u32>(), parts[2].parse::<u32>())
            {
                let newpatch = patch.saturating_sub((seed % spread as u64) as u32);
                return base.replacen(token, &format!("{maj}.{min}.{newpatch}"), 1);
            }
        }
    }
    base.to_string()
}

/// **Per-persona UA:** стабильный во времени для подписки, но различный между подписками.
/// Пул задан списком (`user_agents` len>1) → пиним один по hash(email); иначе варьируем patch
/// базового UA на `ua_spread`. Флот из одинаковых UA сам по себе — отпечаток; это его убирает.
pub fn persona_ua(cfg: &ProxyConfig, email: &str) -> String {
    let seed = email_seed(email);
    if cfg.user_agents.len() > 1 {
        return cfg.user_agents[(seed as usize) % cfg.user_agents.len()].clone();
    }
    let base = cfg.user_agents.first().cloned().unwrap_or_else(|| cfg.user_agent.clone());
    vary_patch(&base, seed, cfg.ua_spread)
}

/// Кэш http-клиентов: один клиент на строку прокси (переиспользуем пулы соединений).
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
    /// Liveness обеспечиваем СОБЫТИЙНО, а не ожиданием большого idle-таймаута: HTTP/2 keep-alive PING
    /// непрерывно проверяет соединение (в т.ч. на простое), TCP keep-alive — на уровне сокета. Мёртвое
    /// соединение (чёрная дыра) обнаруживается в момент неответа PING, а не по выжиданию таймаута.
    pub fn get(&self, proxy: &str) -> reqwest::Result<Client> {
        if let Some(c) = self.map.lock().unwrap_or_else(|e| e.into_inner()).get(proxy) { return Ok(c.clone()); }
        let mut b = Client::builder()
            .connect_timeout(Duration::from_secs(self.connect_timeout))
            .user_agent(&self.user_agent)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .read_timeout(Duration::from_secs(120))               // idle между чтениями: ловит «подключился
            //   но молчит» до первого байта И зависший посреди стрима; сбрасывается на каждом чтении,
            //   поэтому живой поток данных (в т.ч. SSE с ping-ами Anthropic) не рвёт.
            .http2_keep_alive_interval(Duration::from_secs(30))   // PING каждые 30с…
            .http2_keep_alive_timeout(Duration::from_secs(20))    // …нет ответа за 20с → соединение мёртво
            .http2_keep_alive_while_idle(true);                   // проверять и на простое (idle-стрим/пул)
        if !proxy.is_empty() {
            b = b.proxy(reqwest::Proxy::all(proxy)?);
        }
        let c = b.build()?;
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(proxy.to_string(), c.clone());
        Ok(c)
    }
}

/// Утилизация приходит ДОЛЕЙ 0.0–1.0 — ПОДТВЕРЖДЕНО живым заголовком (`7d-utilization: 0.22` = 22%).
/// Значит «ровно 1% → 100%» невозможен: Anthropic шлёт `0.01` для 1%, а не `1`. Клампим в [0,1] от
/// шума; ветку `/100 при >1` держим лишь страховкой на случай смены формата (реально не срабатывает).
fn util(h: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
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
fn reset_ts(h: &reqwest::header::HeaderMap, name: &str) -> Option<i64> {
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
pub fn limits_from_headers(h: &reqwest::header::HeaderMap) -> Limits {
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
    let resp = match client.get(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("user-agent", ua)
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
        "system": [{"type": "text", "text": cfg.identity}],
        "messages": [{"role": "user", "content": content}]
    });
    let url = format!("{}/v1/messages", cfg.upstream.trim_end_matches('/'));
    let resp = client.post(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("anthropic-version", &cfg.anthropic_version)
        .header("anthropic-beta", &cfg.default_beta)
        .header("content-type", "application/json")
        .header("user-agent", ua) // per-persona UA — здоровье персоны идёт с тем же отпечатком, что и бой
        .json(&body)
        .timeout(Duration::from_secs(25))
        .send().await.ok()?;
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
    fn vary_patch_stable_per_email_varied_across() {
        let base = "claude-cli/2.1.195 (external, sdk-cli)";
        let a = vary_patch(base, email_seed("alice@x.io"), 8);
        // стабильность: тот же email → тот же UA
        assert_eq!(a, vary_patch(base, email_seed("alice@x.io"), 8));
        // major.minor не меняются, суффикс на месте
        assert!(a.starts_with("claude-cli/2.1.") && a.ends_with("(external, sdk-cli)"), "a={a}");
        let patch: u32 = a.split('/').nth(1).unwrap().split_whitespace().next().unwrap()
            .split('.').nth(2).unwrap().parse().unwrap();
        assert!((187..=195).contains(&patch), "patch={patch}");
    }

    #[test]
    fn vary_patch_disabled_when_spread_low() {
        let base = "claude-cli/2.1.195 (external, sdk-cli)";
        assert_eq!(vary_patch(base, 12345, 1), base);
        assert_eq!(vary_patch(base, 12345, 0), base);
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

    /// Разброс реально раскидывает флот по нескольким UA (не все в один).
    #[test]
    fn vary_patch_spreads_fleet() {
        let base = "claude-cli/2.1.195 (external, sdk-cli)";
        let mut seen = std::collections::HashSet::new();
        for i in 0..50 { seen.insert(vary_patch(base, email_seed(&format!("u{i}@x.io")), 8)); }
        assert!(seen.len() >= 3, "флот должен разложиться по UA, got {}", seen.len());
    }
}
