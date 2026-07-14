//! Композиционный конфиг: читает ВСЁ окружение и собирает настройки сервера +
//! [`forward::ProxyConfig`]. Единственное место в проекте, где читается env.

use forward::{ProxyConfig, CLAUDE_CODE_IDENTITY};
use std::env;

pub struct Settings {
    pub db_path: String,
    pub bind: String,
    pub fleet: Option<String>,
    pub billing: bool,       // включён ли учёт баланса ключей (таблица api_keys)
    pub mult_bp: i64,        // дефолтная наценка для `key issue` (× 10000; 900 = ×0.09)
    pub cap5h_usd: f64,      // прайор ёмкости 5h окна (USD; 0 → дефолт пула под Max 20x)
    pub cap7d_usd: f64,      // прайор ёмкости 7d окна
    pub reserve5h: f64,      // запас 5h-окна (доля; деф 0.10 = бережём 10%)
    pub reserve7d: f64,      // запас 7d-окна (доля; деф 0.03)
    pub reserve_jitter: f64, // ± разброс порога между подписками (антифингерпринт; деф 0.02)
    pub proxy: ProxyConfig,
}

fn ev(k: &str) -> Option<String> {
    env::var(k).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn ev_or(k: &str, d: &str) -> String { ev(k).unwrap_or_else(|| d.to_string()) }
fn ev_bool(k: &str, d: bool) -> bool {
    match ev(k) { Some(v) => !matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"), None => d }
}
/// UA-список из env. Разделитель `|`, а НЕ `,`: реальный UA Claude Code содержит запятую
/// (`(external, sdk-cli)`), split(',') порвал бы одиночный UA на фрагменты → битый UA всему флоту.
fn split_ua_list(s: &str) -> Vec<String> {
    s.split('|').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}
/// Доля [0,1] из env с КЛАМПОМ + предупреждением при выходе за диапазон. Частая ошибка оператора:
/// `RESERVE_7D=3` (имел в виду 3%) → 3.0 → «резерв 300%» → пул считается исчерпанным → отказы всем.
/// Кламп страхует деньги/ёмкость; не-число → дефолт (с логом, не тихо).
fn ev_frac(k: &str, d: f64) -> f64 {
    match ev(k) { None => d, Some(v) => clamp_frac(k, &v, d) }
}
/// Чистая логика ev_frac (тестируемая без env): парс доли + кламп/предупреждение.
fn clamp_frac(k: &str, v: &str, d: f64) -> f64 {
    match v.parse::<f64>() {
        Ok(x) if (0.0..=1.0).contains(&x) => x,
        Ok(x) => { eprintln!("⚠ {k}={x} вне [0,1] — кламплю (это ДОЛЯ окна, не проценты)"); x.clamp(0.0, 1.0) }
        Err(_) => { eprintln!("⚠ {k}={v:?} не число — беру дефолт {d}"); d }
    }
}

impl Settings {
    pub fn from_env() -> Self {
        let cfg_dir = ev("SUB_CFG_DIR").unwrap_or_else(|| {
            let home = env::var("HOME").unwrap_or_default();
            format!("{home}/.config/claude-api")
        });
        let db_path = ev("SUBS_DB").unwrap_or_else(|| format!("{cfg_dir}/subscriptions.db"));
        let host = ev_or("CLAUDE_API_HOST", "0.0.0.0");
        let port = ev_or("CLAUDE_API_PORT", "8787");
        // Доверять loopback-админу — ТОЛЬКО при явном opt-in `CLAUDE_API_TRUST_LOOPBACK=1` И реальном
        // loopback-bind. Без opt-in — false даже на loopback: закрывает footgun «за реверс-прокси
        // (nginx→127.0.0.1) все пиры видны как 127.0.0.1 → аноним получает админ-доступ». Экспонированный
        // bind (0.0.0.0) не доверяет loopback никогда; управляющие роуты требуют CLAUDE_API_KEYS.
        let trust_loopback = ev("CLAUDE_API_TRUST_LOOPBACK").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
            && matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost");
        let parse_keys = |name: &str| ev(name).map(|s| {
            s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()
        }).unwrap_or_default();
        let api_keys = parse_keys("CLAUDE_API_KEYS");
        let control_keys = parse_keys("CLAUDE_API_CONTROL_KEY");
        let panel_keys = parse_keys("CLAUDE_API_PANEL_KEY");
        // Слабый управляющий ключ = онлайн-брутфорс денег/форвардинга (throttle нет — см. reverse-proxy).
        // Предупреждаем громко при старте, если admin/control-ключ короче 24 символов (наши генераторы
        // дают 48-hex; короткий = операторская парольная фраза, брутфорсибельна).
        for (name, keys) in [("CLAUDE_API_KEYS", &api_keys), ("CLAUDE_API_CONTROL_KEY", &control_keys)] {
            if keys.iter().any(|k| k.len() < 24) {
                eprintln!("⚠️  {name}: есть ключ короче 24 символов — слабый для управляющего доступа. \
                           Задай длинный случайный (напр. openssl rand -hex 24).");
            }
        }
        // Наценка по умолчанию (×0.20): нужна и в Settings, и в ProxyConfig (для /admin/account) →
        // считаем один раз в локальную переменную.
        let mult_bp = ev("CLAUDE_API_MULT_BP").and_then(|s| s.parse().ok()).unwrap_or(2000);
        Settings {
            db_path,
            bind: format!("{host}:{port}"),
            fleet: ev("SUBS_FLEET").filter(|f| f != "all"),
            billing: ev_bool("CLAUDE_API_BILLING", true),
            // Наценка по умолчанию: клиент платит 20% от реального API-эквивалента (×0.20).
            mult_bp,
            // Прайоры ёмкости окон (0 → дефолт пула под Max 20x; калибровка их уточняет).
            cap5h_usd: ev("CLAUDE_API_CAP5H_USD").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            cap7d_usd: ev("CLAUDE_API_CAP7D_USD").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            // Запас окон (headroom) с джиттером: бережём 10%/3%, порог отсечения слегка разный по
            // подпискам, чтобы флот не резался на одном проценте (антифингерпринт).
            reserve5h: ev_frac("CLAUDE_API_RESERVE_5H", 0.10),
            reserve7d: ev_frac("CLAUDE_API_RESERVE_7D", 0.03),
            reserve_jitter: ev_frac("CLAUDE_API_RESERVE_JITTER", 0.02),
            proxy: ProxyConfig {
                api_keys,
                control_keys,
                panel_keys,
                default_mult_bp: mult_bp,
                trust_loopback,
                upstream: ev_or("CLAUDE_API_UPSTREAM", "https://api.anthropic.com"),
                max_tries: ev("CLAUDE_API_MAX_TRIES").and_then(|s| s.parse().ok()).unwrap_or(3),
                // Fair-share: потолок одновременных запросов на клиентский ключ (кит не набивает флот).
                max_inflight_per_key: ev("CLAUDE_API_MAX_INFLIGHT_PER_KEY").and_then(|s| s.parse().ok()).unwrap_or(20),
                util_cap: ev_frac("CLAUDE_API_UTIL_CAP", 0.95),
                cool_secs: ev("CLAUDE_API_COOL_SECS").and_then(|s| s.parse().ok()).unwrap_or(300),
                poll: ev_bool("CLAUDE_API_POLL", true),
                inject_identity: ev_bool("CLAUDE_API_INJECT_IDENTITY", true),
                identity: ev_or("CLAUDE_API_IDENTITY", CLAUDE_CODE_IDENTITY),
                // billing-header (system[0]) — точный вид с живого claude 2.1.195 (mitm 2026-07-14):
                // `x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=<hex>;`
                // cc_version флот-константна (коген с UA), cch — per-персона (см. inject).
                inject_billing: ev_bool("CLAUDE_API_INJECT_BILLING", true),
                cc_version: ev_or("CLAUDE_API_CC_VERSION", "2.1.195.d49"),
                cc_entrypoint: ev_or("CLAUDE_API_CC_ENTRYPOINT", "sdk-cli"),
                // Полный CC-набор beta (не только oauth): без `claude-code-20250219` мы «OAuth-клиент, но
                // НЕ Claude Code». ТОЧНЫЙ актуальный набор снимает refresh-fingerprint.sh с живого claude
                // в config.env; это fallback. (Проверено живым /v1 — набор совместим с OAuth-подпиской.)
                // ТОЧНЫЙ набор снят с живого claude 2.1.195 (mitm-захват /v1/messages, 2026-07-14):
                // 10 бет, разделитель "," без пробела. Порядок ЗНАЧЕНИЙ внутри — Set-итерация (не
                // фингерпринт), важен НАБОР. extended-cache-ttl → ttl:"1h", prompt-caching-scope →
                // scope:"global" на cache_control (см. inject_identity). Проверено живым /v1 (200).
                default_beta: ev_or("CLAUDE_API_BETA",
                    "oauth-2025-04-20,interleaved-thinking-2025-05-14,thinking-token-count-2026-05-13,context-management-2025-06-27,prompt-caching-scope-2026-01-05,claude-code-20250219,advisor-tool-2026-03-01,advanced-tool-use-2025-11-20,extended-cache-ttl-2025-04-11,cache-diagnosis-2026-04-07"),
                // Дефолт-fallback; актуальное значение — env CLAUDE_API_UA (авто-рефреш скриптом).
                // CLAUDE_API_UA можно задать СПИСКОМ через `|` (пул реальных UA) — тогда каждая персона
                // пинит один. Иначе один UA + разброс patch-версии между персонами (ниже).
                // ВАЖНО: разделитель `|`, а НЕ `,` — реальный UA Claude Code содержит запятую
                // (`(external, sdk-cli)`), и split(',') порвал бы дефолт на фрагменты → битый UA флоту.
                user_agent: ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)"),
                user_agents: split_ua_list(&ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)")),
                // Разброс patch-версии UA между персонами (антифингерпринт флота). 0/1 → выключено.
                ua_spread: ev("CLAUDE_API_UA_SPREAD").and_then(|s| s.parse().ok()).unwrap_or(8),
                anthropic_version: ev_or("CLAUDE_API_ANTHROPIC_VERSION", "2023-06-01"),
                connect_timeout: ev("CLAUDE_API_CONNECT_TIMEOUT").and_then(|s| s.parse().ok()).unwrap_or(30),
                // Отпечаток Stainless-SDK клиента Claude Code. Дефолты — правдоподобные; ТОЧНЫЕ значения
                // снимаются с живого claude (refresh-fingerprint.sh) и кладутся в config.env. Флот-константны.
                x_app: ev_or("CLAUDE_API_X_APP", "cli"),
                stainless_lang: ev_or("CLAUDE_API_SL_LANG", "js"),
                stainless_runtime: ev_or("CLAUDE_API_SL_RUNTIME", "node"),
                // Сняты с живого claude 2.1.195 (mitm-захват 2026-07-14): runtime-version =
                // process.version бандл-Bun (v26.3.0), package-version = @anthropic-ai/sdk (0.94.0).
                // Коген с UA 2.1.195. (0.208.0 в бандле — версия ДРУГОГО пакета, не stainless.)
                stainless_runtime_version: ev_or("CLAUDE_API_SL_RT_VER", "v26.3.0"),
                stainless_package_version: ev_or("CLAUDE_API_SL_PKG_VER", "0.94.0"),
                stainless_os: ev_or("CLAUDE_API_SL_OS", "Linux"),
                stainless_arch: ev_or("CLAUDE_API_SL_ARCH", "x64"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ua_list_default_stays_single_not_fragments() {
        // Регресс CRIT-бага: дефолтный UA содержит запятую — split должен дать ОДИН UA, не фрагменты.
        let v = split_ua_list("claude-cli/2.1.195 (external, sdk-cli)");
        assert_eq!(v.len(), 1, "запятая в UA не должна рвать его на куски");
        assert_eq!(v[0], "claude-cli/2.1.195 (external, sdk-cli)");
        // Пул задаётся через '|'
        assert_eq!(split_ua_list("a/1.0|b/2.0|c/3.0").len(), 3);
        assert!(split_ua_list("").is_empty());
        assert!(split_ua_list("  |  ").is_empty()); // пустые куски отфильтрованы
    }

    #[test]
    fn frac_clamps_percent_typo_and_rejects_garbage() {
        assert_eq!(clamp_frac("k", "0.03", 0.5), 0.03);   // норм доля
        assert_eq!(clamp_frac("k", "3", 0.5), 1.0);       // «3» (проценты вместо доли) → кламп в 1.0
        assert_eq!(clamp_frac("k", "-1", 0.5), 0.0);      // отрицательное → 0
        assert_eq!(clamp_frac("k", "хлам", 0.5), 0.5);    // не число → дефолт
        assert_eq!(clamp_frac("k", "0,1", 0.5), 0.5);     // запятая-десятичная (опечатка) → дефолт
    }
}
