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

pub const NANO_PER_USD: i128 = 1_000_000_000;
pub const WEB_SEARCH_NANO: i128 = 10_000_000; // $0.01 за web_search-запрос

/// Токены запроса по корзинам (u64 — точные счётчики, ничего не теряем).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,      // cache_read_input_tokens
    pub cache_write_5m_tokens: u64,  // cache_creation.ephemeral_5m_input_tokens
    pub cache_write_1h_tokens: u64,  // cache_creation.ephemeral_1h_input_tokens
    pub web_search_requests: u64,    // server_tool_use.web_search_requests
}

impl Usage {
    /// Сумма всех токеновых корзин — для контроля, что ничего не потеряли.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens
            + self.cache_write_5m_tokens + self.cache_write_1h_tokens
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

/// $/Mtoken → нано/токен. Цена в целых центах/десятитысячных → результат ЦЕЛЫЙ.
const fn nano(dollars_per_mtoken_x1000: i128) -> i128 { dollars_per_mtoken_x1000 }

/// Цены модели ($/Mtoken → нано/токен). Совпадают с официальными ставками Anthropic.
/// Матчим по подстроке имени (как это делает движок), дефолт = Opus 4.8.
pub fn model_prices(model: &str) -> Prices {
    let m = model.to_lowercase();
    // helper: значения = $/Mtoken × 1000 (нано/токен)
    let p = |inp, cr, cw5, cw1, out| Prices {
        input: nano(inp), cache_read: nano(cr), cache_write_5m: nano(cw5),
        cache_write_1h: nano(cw1), output: nano(out),
    };
    // Fable 5 / Mythos 5 — самые дорогие текущие модели ($10/$50). ДОЛЖНЫ идти до дефолта,
    // иначе считались бы как Opus 4.8 ($5/$25) — недосписание вдвое (теряем деньги).
    if m.contains("fable") || m.contains("mythos") {
        return p(10000, 1000, 12500, 20000, 50000);      // $10 / 1.0 / 12.5 / 20 / 50
    }
    if m.contains("sonnet-5") || m == "claude-sonnet-5" {
        return p(3000, 300, 3750, 6000, 15000);          // $3 / 0.30 / 3.75 / 6 / 15
    }
    if m.contains("opus-4-8") || m.contains("opus-4-7") || m.contains("opus-4-6") || m.contains("opus-4-5") {
        return p(5000, 500, 6250, 10000, 25000);         // $5 / 0.50 / 6.25 / 10 / 25
    }
    if m.contains("opus-4-1") || m.contains("opus-4-0") || m == "claude-opus-4" {
        return p(15000, 1500, 18750, 30000, 75000);      // $15 / 1.50 / 18.75 / 30 / 75
    }
    if m.contains("sonnet-4") {
        return p(3000, 300, 3750, 6000, 15000);          // Sonnet 4 = как Sonnet 5
    }
    if m.contains("haiku-4-5") {
        return p(1000, 100, 1250, 2000, 5000);           // $1 / 0.10 / 1.25 / 2 / 5
    }
    if m.contains("haiku-3-5") {
        return p(800, 80, 1000, 1600, 4000);             // $0.80 / 0.08 / 1.0 / 1.60 / 4
    }
    p(5000, 500, 6250, 10000, 25000)                     // дефолт = Opus 4.8
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

/// Применить множитель-наценку. `mult_bp` — множитель × 10000 (базисные пункты):
/// x1.0 = 10000, x2.78 = 27800. Округление до ближайшего нанодоллара (банковское — half-up).
pub fn apply_multiplier(nano: i128, mult_bp: i64) -> i128 {
    let m = mult_bp as i128;
    if nano >= 0 { (nano * m + 5000) / 10000 } else { (nano * m - 5000) / 10000 }
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
    let cw5_split = split.and_then(|c| c.get("ephemeral_5m_input_tokens")).and_then(Value::as_u64).unwrap_or(0);
    let cw1_split = split.and_then(|c| c.get("ephemeral_1h_input_tokens")).and_then(Value::as_u64).unwrap_or(0);
    let cw_total = g("cache_creation_input_tokens");
    let (cw5, cw1) = if cw5_split + cw1_split == 0 && cw_total > 0 {
        (cw_total, 0)
    } else {
        (cw5_split, cw1_split)
    };
    let web = u.get("server_tool_use")
        .and_then(|s| s.get("web_search_requests")).and_then(Value::as_u64).unwrap_or(0);
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
        let json = match line.strip_prefix("data:") { Some(r) => r.trim(), None => continue };
        if json.is_empty() || json == "[DONE]" { continue; }
        if let Ok(v) = serde_json::from_str::<Value>(json) {
            if v.get("type").and_then(Value::as_str) == Some("error") { return true; }
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

/// Из накопленного SSE-потока. input/cache — из `message_start`; output — из ПОСЛЕДНЕГО
/// `message_delta` (там output_tokens кумулятивный). web-search — из delta, если есть.
pub fn usage_from_sse(sse: &str) -> Usage {
    let mut u = Usage::default();
    let mut have_start = false;
    for raw in sse.lines() {
        let line = raw.trim_start();
        let json = match line.strip_prefix("data:") { Some(r) => r.trim(), None => continue };
        if json.is_empty() || json == "[DONE]" { continue; }
        let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => continue };
        match v.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(mu) = v.get("message").and_then(|m| m.get("usage")) {
                    let s = usage_from_value(mu);
                    u.input_tokens = s.input_tokens;
                    u.cache_read_tokens = s.cache_read_tokens;
                    u.cache_write_5m_tokens = s.cache_write_5m_tokens;
                    u.cache_write_1h_tokens = s.cache_write_1h_tokens;
                    u.output_tokens = s.output_tokens; // обычно 1 — перезапишется delta
                    if s.web_search_requests > 0 { u.web_search_requests = s.web_search_requests; }
                    have_start = true;
                }
            }
            Some("message_delta") => {
                if let Some(du) = v.get("usage") {
                    if let Some(o) = du.get("output_tokens").and_then(Value::as_u64) {
                        u.output_tokens = o; // кумулятивный — держим последний
                    }
                    if let Some(w) = du.get("server_tool_use")
                        .and_then(|s| s.get("web_search_requests")).and_then(Value::as_u64) {
                        u.web_search_requests = w;
                    }
                }
            }
            _ => {}
        }
    }
    let _ = have_start;
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── цены: 1M токенов каждой корзины = точная официальная ставка ────────────
    #[test]
    fn prices_exact_per_million() {
        let opus = model_prices("claude-opus-4-8");
        // 1M output Opus = ровно $25.00
        let u = Usage { output_tokens: 1_000_000, ..Default::default() };
        assert_eq!(cost_nanodollars(&u, &opus), 25 * NANO_PER_USD);
        // 1M input Opus = $5.00
        let u = Usage { input_tokens: 1_000_000, ..Default::default() };
        assert_eq!(cost_nanodollars(&u, &opus), 5 * NANO_PER_USD);
        // 1M cache_write_5m Opus = $6.25
        let u = Usage { cache_write_5m_tokens: 1_000_000, ..Default::default() };
        assert_eq!(cost_nanodollars(&u, &opus), 6_250_000_000);
        // 1M cache_read Opus = $0.50
        let u = Usage { cache_read_tokens: 1_000_000, ..Default::default() };
        assert_eq!(cost_nanodollars(&u, &opus), 500_000_000);
        // Haiku 4.5: 1M output = $5.00, 1M input = $1.00
        let hk = model_prices("claude-haiku-4-5-20251001");
        assert_eq!(cost_nanodollars(&Usage{output_tokens:1_000_000,..Default::default()}, &hk), 5*NANO_PER_USD);
        assert_eq!(cost_nanodollars(&Usage{input_tokens:1_000_000,..Default::default()}, &hk), 1*NANO_PER_USD);
        // Haiku 3.5 cache_read $0.08/Mtoken
        let h35 = model_prices("claude-haiku-3-5");
        assert_eq!(cost_nanodollars(&Usage{cache_read_tokens:1_000_000,..Default::default()}, &h35), 80_000_000);
        // Sonnet 5: 1M output $15
        assert_eq!(cost_nanodollars(&Usage{output_tokens:1_000_000,..Default::default()}, &model_prices("claude-sonnet-5")), 15*NANO_PER_USD);
        // Fable 5 / Mythos 5: 1M output = $50, 1M input = $10 (НЕ дефолт Opus)
        let fable = model_prices("claude-fable-5");
        assert_eq!(cost_nanodollars(&Usage{output_tokens:1_000_000,..Default::default()}, &fable), 50*NANO_PER_USD);
        assert_eq!(cost_nanodollars(&Usage{input_tokens:1_000_000,..Default::default()}, &fable), 10*NANO_PER_USD);
        assert_eq!(model_prices("claude-mythos-5"), fable);
        assert_ne!(fable, opus); // не свалилось в дефолт
        // неизвестная модель → дефолт Opus 4.8
        assert_eq!(model_prices("что-то-непонятное"), opus);
    }

    #[test]
    fn per_token_is_exact() {
        // 1 output-токен Opus = 25000 нано = $0.000025 — точно, без потерь
        let u = Usage { output_tokens: 1, ..Default::default() };
        assert_eq!(cost_nanodollars(&u, &model_prices("claude-opus-4-8")), 25_000);
        // 3 токена → ровно 3×
        let u = Usage { output_tokens: 3, ..Default::default() };
        assert_eq!(cost_nanodollars(&u, &model_prices("claude-opus-4-8")), 75_000);
    }

    #[test]
    fn full_usage_all_buckets_counted() {
        let u = Usage {
            input_tokens: 1000, output_tokens: 500, cache_read_tokens: 200,
            cache_write_5m_tokens: 100, cache_write_1h_tokens: 50, web_search_requests: 2,
        };
        let p = model_prices("claude-opus-4-8");
        // 1000*5000 + 500*25000 + 200*500 + 100*6250 + 50*10000 + 2*10_000_000
        let expect = 1000*5000 + 500*25000 + 200*500 + 100*6250 + 50*10000 + 2*10_000_000;
        assert_eq!(cost_nanodollars(&u, &p), expect as i128);
        // сумма токенов = ровно все корзины
        assert_eq!(u.total_tokens(), 1850);
    }

    #[test]
    fn multiplier_exact() {
        assert_eq!(apply_multiplier(NANO_PER_USD, 10000), NANO_PER_USD);      // x1.0
        assert_eq!(apply_multiplier(NANO_PER_USD, 27800), 2_780_000_000);     // x2.78 = $2.78
        assert_eq!(apply_multiplier(NANO_PER_USD, 5000), 500_000_000);        // x0.5
        // округление half-up
        assert_eq!(apply_multiplier(3, 15000), 5);                            // 3*1.5=4.5→5
    }

    #[test]
    fn parse_json_with_cache_split() {
        let body = br#"{"id":"m","type":"message","usage":{
            "input_tokens":41,"output_tokens":10,"cache_read_input_tokens":7,
            "cache_creation_input_tokens":30,
            "cache_creation":{"ephemeral_5m_input_tokens":20,"ephemeral_1h_input_tokens":10}}}"#;
        let u = usage_from_response_json(body);
        assert_eq!(u, Usage{input_tokens:41,output_tokens:10,cache_read_tokens:7,
            cache_write_5m_tokens:20,cache_write_1h_tokens:10,web_search_requests:0});
    }

    #[test]
    fn parse_json_cache_total_no_split_goes_to_5m() {
        let body = br#"{"usage":{"input_tokens":5,"output_tokens":3,"cache_creation_input_tokens":40}}"#;
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
        assert_eq!(u.output_tokens, 37);
    }

    #[test]
    fn sse_error_event_detected() {
        let ok = "event: message_stop\ndata: {\"type\":\"message_stop\"}";
        assert!(!sse_has_error(ok));
        let err = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\"}}";
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
        let u = Usage { output_tokens: 10_000_000_000, ..Default::default() };
        let c = cost_nanodollars(&u, &model_prices("claude-opus-4-8"));
        assert_eq!(c, 10_000_000_000i128 * 25_000); // = $250 000
        assert_eq!(nano_to_usd_string(c), "$250000.000000");
    }

    #[test]
    fn client_pays_9_percent_of_real_api() {
        // Бизнес-правило владельца: клиент платит 9% от реального USD-эквивалента API (×0.09,
        // mult_bp=900), независимо от модели. Реальные $1000 API → клиент тратит ровно $90.
        let real = 1000i128 * NANO_PER_USD;                 // $1000 реального API
        assert_eq!(apply_multiplier(real, 900), 90 * NANO_PER_USD); // $90 «нашего»
        // на реальном usage: 1M выходных токенов Opus = $25 API → клиент платит $2.25
        let u = Usage { output_tokens: 1_000_000, ..Default::default() };
        assert_eq!(cost_with_multiplier(&u, "claude-opus-4-8", 900), 2_250_000_000); // $2.25
    }

    #[test]
    fn usd_string_format() {
        assert_eq!(nano_to_usd_string(25_000), "$0.000025");
        assert_eq!(nano_to_usd_string(2_780_000_000), "$2.780000");
        assert_eq!(nano_to_usd_string(0), "$0.000000");
    }
}
