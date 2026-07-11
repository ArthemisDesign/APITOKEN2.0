//! Апстрим: кэш reqwest-клиентов по прокси + опрос лимитов подписки.
//! Каждая подписка ходит на api.anthropic.com со СВОЕГО IP (через свой прокси).

use crate::config::ProxyConfig;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

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

    /// Клиент для данного прокси ("" = напрямую). Без общего timeout — иначе рвал бы стримы;
    /// ограничиваем только установку соединения.
    pub fn get(&self, proxy: &str) -> reqwest::Result<Client> {
        if let Some(c) = self.map.lock().unwrap().get(proxy) { return Ok(c.clone()); }
        let mut b = Client::builder()
            .connect_timeout(Duration::from_secs(self.connect_timeout))
            .user_agent(&self.user_agent)
            .pool_idle_timeout(Duration::from_secs(90));
        if !proxy.is_empty() {
            b = b.proxy(reqwest::Proxy::all(proxy)?);
        }
        let c = b.build()?;
        self.map.lock().unwrap().insert(proxy.to_string(), c.clone());
        Ok(c)
    }
}

/// Утилизация приходит процентом (42 → 0.42); иногда уже долей (<=1) — нормализуем.
fn util(h: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    let v: f64 = h.get(name)?.to_str().ok()?.trim().parse().ok()?;
    Some(if v > 1.0 { v / 100.0 } else { v })
}
fn int(h: &reqwest::header::HeaderMap, name: &str) -> Option<i64> {
    h.get(name)?.to_str().ok()?.trim().parse::<f64>().ok().map(|x| x as i64)
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
pub async fn detect_plan(client: &Client, cfg: &ProxyConfig, token: &str) -> PlanDetect {
    let url = format!("{}/api/oauth/profile", cfg.upstream.trim_end_matches('/'));
    let resp = match client.get(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
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
pub async fn poll_sub(client: &Client, cfg: &ProxyConfig, token: &str) -> Option<PollResult> {
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 1,
        "system": [{"type": "text", "text": cfg.identity}],
        "messages": [{"role": "user", "content": "."}]
    });
    let url = format!("{}/v1/messages", cfg.upstream.trim_end_matches('/'));
    let resp = client.post(&url)
        .header("authorization", format!("Bearer {token}"))
        .header("anthropic-version", &cfg.anthropic_version)
        .header("anthropic-beta", &cfg.default_beta)
        .header("content-type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(25))
        .send().await.ok()?;
    let http = resp.status().as_u16();
    let h = resp.headers();
    Some(PollResult {
        util5h: util(h, "anthropic-ratelimit-unified-5h-utilization"),
        util7d: util(h, "anthropic-ratelimit-unified-7d-utilization"),
        status: h.get("anthropic-ratelimit-unified-status")
            .and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
        reset5h: int(h, "anthropic-ratelimit-unified-5h-reset"),
        reset7d: int(h, "anthropic-ratelimit-unified-7d-reset"),
        http,
    })
}
