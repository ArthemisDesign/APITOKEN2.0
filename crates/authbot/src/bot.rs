//! Логика бота: команды, машина создания оффера, флоу продавца. Состояние — в SQLite (db).
//! Выплаты (Фаза 2) и выпуск setup-token (Фаза 3) пока заглушены — помечены TODO.

use crate::db::Store;
use crate::gemini_oauth;
use crate::setup_token::{self, Outcome};
use crate::tg::{Bot, CallbackQuery, Keyboard};
use crate::Config;
use std::sync::Arc;

pub const WELCOME_NEW: &str =
    "👋 <b>Привет!</b>\nЭто бот закупки. Хочешь продавать — жми кнопку ниже, заявка уйдёт на модерацию.";
pub const ADMIN_HOME: &str = "🛠 <b>Дев-панель</b>\n\nБыстрая покупка: жми продукт на нижней \
    клавиатуре → пришли цену в $ → оффер уходит выбранному продавцу.";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn is_bep20(s: &str) -> bool {
    let s = s.trim();
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_admin(cfg: &Config, store: &Store, uid: i64, uname: &str) -> bool {
    if cfg.admins_id.contains(&uid) {
        return true;
    }
    let un = uname.to_lowercase();
    if !un.is_empty() && cfg.admins_name.contains(&un) {
        return true;
    }
    store.is_persisted_admin(uid, uname).unwrap_or(false)
}

fn welcome_kb() -> Keyboard {
    vec![vec![("✅ Стать продавцом".into(), "reg:request".into())]]
}
fn offer_kb(oid: i64) -> Keyboard {
    vec![vec![
        ("✅ Принять".into(), format!("offer:{oid}:accept")),
        ("❌ Отклонить".into(), format!("offer:{oid}:reject")),
    ]]
}
fn approve_kb(target: i64) -> Keyboard {
    vec![vec![
        ("✅ Пустить".into(), format!("reg:approve:{target}")),
        ("🚫 Отклонить".into(), format!("reg:reject:{target}")),
    ]]
}

// ── создание оффера: продукт кнопками (без свободного текста) ────────────────
const PRODUCT_PICK: &str = "📦 <b>Создание оффера</b>\nВыбери продукт:\n\n\
    Gemini: только подтверждённые Google AI Pro/Ultra и организационные Code Assist планы; тип подписки после OAuth проверяет Google, а не выбор продавца.";
const PRICE_PROMPT: &str = "Теперь пришли <b>ЦЕНУ в долларах</b> одним сообщением \
     (например <code>20</code> или <code>15.5</code>). Это сумма выплаты продавцу. /cancel — отмена.";

fn product_kb() -> Keyboard {
    vec![
        vec![("Claude Pro".into(), "noffer:pro".into())],
        vec![("Claude 5x".into(), "noffer:5x".into())],
        vec![("Claude 20x".into(), "noffer:20x".into())],
        vec![("ChatGPT Plus".into(), "noffer:gptplus".into())],
        vec![("ChatGPT Pro".into(), "noffer:gptpro".into())],
        vec![("Google AI Pro".into(), "noffer:gemini_pro".into())],
        vec![("Google AI Ultra".into(), "noffer:gemini_ultra".into())],
        vec![(
            "Code Assist Standard".into(),
            "noffer:gemini_standard".into(),
        )],
        vec![(
            "Code Assist Enterprise".into(),
            "noffer:gemini_enterprise".into(),
        )],
        vec![(
            "Workspace AI Ultra".into(),
            "noffer:gemini_workspace_ultra".into(),
        )],
    ]
}

/// Три несовместимые единицы пополнения: Claude token, ChatGPT CODEX_HOME и зашифрованный
/// Gemini OAuth profile.
/// Явный enum не даёт новому продукту тихо провалиться в Claude setup-token ветку.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HandoffKind {
    Claude,
    Codex,
    Gemini,
}

fn handoff_kind(product: &str) -> HandoffKind {
    let p = product.to_lowercase();
    if p.contains("gemini")
        || p.contains("google ai")
        || p.contains("code assist")
        || p.contains("workspace ai")
    {
        HandoffKind::Gemini
    } else if p.contains("chatgpt") || p.contains("gpt") {
        HandoffKind::Codex
    } else {
        HandoffKind::Claude
    }
}
fn tier_name(code: &str) -> Option<&'static str> {
    match code {
        "pro" => Some("Claude Pro"),
        "5x" => Some("Claude 5x"),
        "20x" => Some("Claude 20x"),
        "gptplus" => Some("ChatGPT Plus"),
        "gptpro" => Some("ChatGPT Pro"),
        "gemini_pro" => Some("Google AI Pro"),
        "gemini_ultra" => Some("Google AI Ultra"),
        "gemini_standard" => Some("Code Assist Standard"),
        "gemini_enterprise" => Some("Code Assist Enterprise"),
        "gemini_workspace_ultra" => Some("Workspace AI Ultra"),
        _ => None,
    }
}
/// Текст закреплённой нижней кнопки продукта → имя продукта (для быстрой покупки админом).
fn admin_quick_tier(text: &str) -> Option<&'static str> {
    match text.trim() {
        "📦 Claude Pro" => Some("Claude Pro"),
        "📦 Claude 5x" => Some("Claude 5x"),
        "📦 Claude 20x" => Some("Claude 20x"),
        "📦 Google AI Pro" | "📦 Gemini Pro" => Some("Google AI Pro"),
        "📦 Google AI Ultra" | "📦 Gemini Ultra" => Some("Google AI Ultra"),
        "📦 Code Assist Standard" | "📦 Code Assist Std" => Some("Code Assist Standard"),
        "📦 Code Assist Enterprise" | "📦 Code Assist Ent" => Some("Code Assist Enterprise"),
        "📦 Workspace AI Ultra" | "📦 Workspace Ultra" => Some("Workspace AI Ultra"),
        _ => None,
    }
}

/// One source of truth for the persistent admin keyboard. Keeping this data out of the Telegram
/// call makes omissions visible to a unit test and preserves old button aliases during rollout.
fn admin_home_kb() -> Vec<Vec<&'static str>> {
    vec![
        vec!["📦 Claude Pro", "📦 Claude 5x", "📦 Claude 20x"],
        vec!["📦 Google AI Pro", "📦 Google AI Ultra"],
        vec!["📦 Code Assist Standard"],
        vec!["📦 Code Assist Enterprise"],
        vec!["📦 Workspace AI Ultra"],
        vec!["🛠 Панель"],
    ]
}

/// Закреплённая нижняя клавиатура админа + список заявок на модерацию.
async fn show_admin_home(bot: &Bot, store: &Store, chat: i64) {
    let _ = bot.send_reply_kb(chat, ADMIN_HOME, &admin_home_kb()).await;
    for u in store.by_status("pending").unwrap_or_default() {
        let _ = bot
            .send_kb(
                chat,
                &format!("🔔 Заявка в продавцы: @{} (id {})", esc(&u.username), u.uid),
                Some(&approve_kb(u.chat_id)),
            )
            .await;
    }
}
/// Цена всегда в долларах: «$20» для целых, «$15.50» иначе.
fn fmt_usd(a: f64) -> String {
    if a.fract().abs() < 1e-9 {
        format!("${}", a as i64)
    } else {
        format!("${:.2}", a)
    }
}

async fn notify_admins(bot: &Bot, cfg: &Config, text: &str, kb: Option<&Keyboard>) {
    for id in &cfg.admins_id {
        let _ = bot.send_kb(*id, text, kb).await;
    }
}

fn offer_text(o: &crate::db::Offer) -> String {
    format!(
        "📦 <b>Оффер #{}</b>\nПродукт: <b>{}</b>\nЦена: <b>{}</b>",
        o.id,
        esc(&o.product),
        esc(&o.price)
    )
}

/// Отправить АДРЕСНЫЙ оффер одному продавцу. true — доставлено.
/// (Продавцы изолированы: оффер видит только адресат, не рассылается остальным.)
async fn send_offer_to(bot: &Bot, store: &Store, oid: i64, seller_chat: i64) -> bool {
    let o = match store.get_offer(oid) {
        Ok(Some(o)) => o,
        _ => return false,
    };
    bot.send_kb(seller_chat, &offer_text(&o), Some(&offer_kb(oid)))
        .await
        .is_ok()
}

/// Отображаемая метка продавца (@username или id) — уже HTML-экранирована.
fn seller_label(store: &Store, seller_chat: i64) -> String {
    let s = store
        .get_user(seller_chat)
        .ok()
        .flatten()
        .unwrap_or_default();
    let id = if s.uid != 0 { s.uid } else { seller_chat };
    if s.username.is_empty() {
        format!("id {id}")
    } else {
        format!("@{}", esc(&s.username))
    }
}

/// Клавиатура выбора продавца-адресата (по одному в строке).
fn seller_pick_kb(sellers: &[crate::db::UserRow]) -> Keyboard {
    sellers
        .iter()
        .map(|s| {
            let label = if s.username.is_empty() {
                format!("id {}", s.uid)
            } else {
                format!("@{}", s.username)
            };
            vec![(label, format!("oseller:{}", s.chat_id))]
        })
        .collect()
}

/// Шаг «выбор продавца» после выбора продукта (перед ценой).
async fn start_seller_pick(bot: &Bot, store: &Store, chat: i64, product: &str) {
    let sellers = store.by_status("approved").unwrap_or_default();
    if sellers.is_empty() {
        let _ = store.clear_admin_state(chat);
        let _ = bot
            .send(
                chat,
                "Пока нет одобренных продавцов — оффер некому направить. Одобри заявку и повтори.",
            )
            .await;
        return;
    }
    let _ = store.set_admin_state(chat, "seller", product, 0);
    let _ = bot
        .send_kb(
            chat,
            &format!(
                "📦 Продукт: <b>{}</b>\n\nКому отправить оффер? Выбери продавца:",
                esc(product)
            ),
            Some(&seller_pick_kb(&sellers)),
        )
        .await;
}

// ── сообщения ────────────────────────────────────────────────────────────────
pub async fn on_message(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    uid: i64,
    uname: &str,
    _message_id: i64,
    text: &str,
) {
    let _ = store.register_user(chat, uid, uname);
    let text = text.trim();
    let admin = is_admin(cfg, store, uid, uname);

    // админ: закреплённые нижние кнопки — быстрый старт покупки подписки
    if admin {
        if let Some(name) = admin_quick_tier(text) {
            start_seller_pick(bot, store, chat, name).await;
            return;
        }
        if text == "🛠 Панель" {
            show_admin_home(bot, store, chat).await;
            return;
        }
    }

    // машина создания оффера (persisted) — только админ, не команда
    if admin && !text.starts_with('/') {
        if let Ok(Some((step, product, seller_chat))) = store.get_admin_state(chat) {
            if step == "price" {
                // Правило: цена — всегда в ДОЛЛАРАХ (число).
                let amount = match parse_amount(text) {
                    Some(a) if a > 0.0 => a,
                    _ => {
                        let _ = bot.send(chat, "Нужна <b>сумма в долларах</b> числом \
                            (например <code>20</code> или <code>15.5</code>). Пришли ещё раз или /cancel.").await;
                        return;
                    }
                };
                let _ = store.clear_admin_state(chat);
                if let Ok(oid) = store.create_offer(&product, &fmt_usd(amount), uid, seller_chat) {
                    let who = seller_label(store, seller_chat);
                    let ok = send_offer_to(bot, store, oid, seller_chat).await;
                    let ot = store
                        .get_offer(oid)
                        .ok()
                        .flatten()
                        .as_ref()
                        .map(offer_text)
                        .unwrap_or_default();
                    let _ = bot.send(chat, &if ok {
                        format!("✅ <b>Оффер #{oid} отправлен продавцу {who}.</b>\n\n{ot}")
                    } else {
                        format!("⚠️ Оффер #{oid} создан, но доставить продавцу {who} не удалось \
                            (возможно, он не открывал бота).\n\n{ot}")
                    }).await;
                }
                return;
            }
        }
    }

    if text == "/cancel" {
        let cleared = store.clear_admin_state(chat).unwrap_or(false);
        let _ = bot
            .send(
                chat,
                if cleared {
                    "Создание оффера отменено."
                } else {
                    "Нечего отменять."
                },
            )
            .await;
        return;
    }

    if text == "/start" || text == "/help" {
        if admin {
            show_admin_home(bot, store, chat).await;
            return;
        }
        // не-админ: по статусу
        let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
        match rec.status.as_str() {
            "approved" => {
                if rec.address.is_empty() {
                    let _ = store.set_want(chat, "reg_address");
                    let _ = bot
                        .send(
                            chat,
                            "👋 <b>Ты в системе как продавец.</b>\n\nПришли свой \
                        <b>BEP-20</b> адрес кошелька (<code>0x…</code>) для выплат.",
                        )
                        .await;
                } else {
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "👋 <b>Ты продавец.</b>\nКак появится оффер — пришлю сюда.\n\n\
                        💼 Адрес выплат:\n<code>{}</code>",
                                esc(&rec.address)
                            ),
                        )
                        .await;
                }
            }
            "pending" => {
                let _ = bot
                    .send(
                        chat,
                        "⏳ <b>Заявка на рассмотрении.</b> Как одобрят — сообщу.",
                    )
                    .await;
            }
            "rejected" => {
                let _ = bot.send(chat, "🚫 <b>Заявка отклонена.</b>").await;
            }
            _ => {
                let _ = bot.send_kb(chat, WELCOME_NEW, Some(&welcome_kb())).await;
            }
        }
        return;
    }

    if admin && text == "/offer" {
        let _ = bot.send_kb(chat, PRODUCT_PICK, Some(&product_kb())).await;
        return;
    }

    // не-админ: строгий режим — принимаем только ожидаемый сейчас ввод (state-machine)
    if !admin {
        let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
        match rec.want.as_str() {
            "reg_address" => {
                if is_bep20(text) {
                    let _ = store.set_address(chat, text);
                    let _ = store.set_want(chat, "");
                    let _ = bot.send(chat, "✅ Адрес сохранён. Жди оффер.").await;
                } else {
                    let _ = bot.send(chat, "Это не похоже на BEP-20 адрес (<code>0x</code> + 40 hex). Пришли ещё раз.").await;
                }
            }
            // передача доступа после оплаты: прокси → email → code#state
            "ho_proxy" => {
                let purl = proxy_url(text);
                if purl.is_empty() {
                    let _ = bot.send(chat, "Не похоже на прокси. Пришли <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
                } else {
                    let _ = store.set_hproxy(chat, &purl);
                    let _ = store.set_want(chat, "ho_email");
                    let _ = bot.send(chat, "Прокси принят ✅\n<b>Шаг 2/3.</b> Пришли <b>email</b> аккаунта Claude.").await;
                }
            }
            "ho_email" => {
                if !looks_like_email(text) {
                    let _ = bot
                        .send(
                            chat,
                            "Это не похоже на email. Пришли адрес аккаунта ещё раз.",
                        )
                        .await;
                } else if do_start_token(bot, cfg, chat, text, &rec.hproxy).await {
                    let _ = store.set_want(chat, "ho_code");
                }
            }
            "cx_proxy" => {
                let purl = proxy_url(text);
                if purl.is_empty() {
                    let _ = bot.send(chat, "Не похоже на прокси. Пришли <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
                } else {
                    let _ = store.set_hproxy(chat, &purl);
                    let _ = store.set_want(chat, "cx_email");
                    let _ = bot.send(chat, "Прокси принят ✅\n<b>Шаг 2/3.</b> Пришли <b>email</b> аккаунта ChatGPT.").await;
                }
            }
            "cx_email" => {
                if !looks_like_email(text) {
                    let _ = bot
                        .send(
                            chat,
                            "Это не похоже на email. Пришли адрес аккаунта ещё раз.",
                        )
                        .await;
                } else {
                    start_codex_handoff(bot, store, cfg, chat, text, &rec.hproxy).await;
                }
            }
            // Step-by-step Gemini onboarding: CLIENT_ID → CLIENT_SECRET → proxy, one field per
            // message. A pasted multi-line client (id/secret[/proxy]) is still accepted as a shortcut.
            "gm_gid" => {
                if let Some((client_id, client_secret, proxy_opt)) = parse_gemini_client(text) {
                    if let Ok(mut drafts) = cfg.gemini_client_drafts.lock() {
                        drafts.remove(&chat);
                    }
                    match proxy_opt {
                        Some(proxy_line) => {
                            let purl = proxy_url(&proxy_line);
                            if purl.is_empty() {
                                let _ = bot.send(chat, GEMINI_STEP_PROXY_RETRY).await;
                            } else {
                                start_gemini_handoff(
                                    bot, store, cfg, chat, Some(&purl), 0, &client_id, &client_secret,
                                )
                                .await;
                            }
                        }
                        None => {
                            gemini_finalize_or_ask_proxy(
                                bot, store, cfg, chat, &rec, &client_id, &client_secret,
                            )
                            .await
                        }
                    }
                } else {
                    let id = text.trim();
                    if id.ends_with(".apps.googleusercontent.com")
                        && id.len() >= 8
                        && !id.contains(char::is_whitespace)
                    {
                        if let Ok(mut drafts) = cfg.gemini_client_drafts.lock() {
                            drafts.insert(chat, (id.to_string(), String::new()));
                        }
                        let _ = store.set_want(chat, "gm_gsecret");
                        let _ = bot.send(chat, GEMINI_STEP_SECRET).await;
                    } else {
                        let _ = bot.send(chat, GEMINI_STEP_ID_RETRY).await;
                    }
                }
            }
            "gm_gsecret" => {
                let secret = text.trim();
                if secret.len() < 6 || secret.contains(char::is_whitespace) {
                    let _ = bot.send(chat, GEMINI_STEP_SECRET_RETRY).await;
                } else {
                    let client_id = cfg.gemini_client_drafts.lock().ok().and_then(|mut drafts| {
                        drafts.get_mut(&chat).map(|entry| {
                            entry.1 = secret.to_string();
                            entry.0.clone()
                        })
                    });
                    match client_id {
                        Some(client_id) => {
                            gemini_finalize_or_ask_proxy(
                                bot, store, cfg, chat, &rec, &client_id, secret,
                            )
                            .await
                        }
                        None => {
                            let _ = store.set_want(chat, "gm_gid");
                            let _ = bot.send(chat, GEMINI_RESTART).await;
                        }
                    }
                }
            }
            "gm_gproxy" => {
                let purl = proxy_url(text.trim());
                if purl.is_empty() {
                    let _ = bot.send(chat, GEMINI_STEP_PROXY_RETRY).await;
                } else {
                    let draft = cfg
                        .gemini_client_drafts
                        .lock()
                        .ok()
                        .and_then(|drafts| drafts.get(&chat).cloned());
                    match draft {
                        Some((client_id, client_secret)) if !client_secret.is_empty() => {
                            start_gemini_handoff(
                                bot, store, cfg, chat, Some(&purl), 0, &client_id, &client_secret,
                            )
                            .await;
                        }
                        _ => {
                            let _ = store.set_want(chat, "gm_gid");
                            let _ = bot.send(chat, GEMINI_RESTART).await;
                        }
                    }
                }
            }
            "gm_wait" => {
                let _ = bot
                    .send(
                        chat,
                        "OAuth-сессия уже ожидает callback Google. Заверши вход по выданной ссылке или используй /cancel и начни заново.",
                    )
                    .await;
            }
            "ho_code" => match extract_code_state(text) {
                Some(cs) => do_feed_token(bot, store, cfg, chat, &cs).await,
                None => {
                    let _ = bot.send(chat, "Пришли <b>адрес callback целиком</b> (…/callback?code=…&state=…) или строку <code>code#state</code>.").await;
                }
            },
            _ => {
                let _ = bot.send(chat, "Доступна только команда /start.").await;
            }
        }
        return;
    }

    // админ standalone: выпуск токена. Активная сессия + code#state/callback-URL → докормить.
    if setup_token::has(chat) {
        if let Some(cs) = extract_code_state(text) {
            do_feed_token(bot, store, cfg, chat, &cs).await;
            return;
        }
    }
    if looks_like_email(text) {
        do_start_token(bot, cfg, chat, text, "").await;
        return;
    }
    let _ = bot.send(chat, "Используй /start, кнопку «📦 Создать оффер», или пришли <b>email</b> аккаунта для выпуска токена.").await;
}

/// Первое число из строки цены («20$» → 20.0, «9 990₽» → 9.99? нет — берём слитную группу).
fn parse_amount(price: &str) -> Option<f64> {
    let mut num = String::new();
    let mut seen_dot = false;
    for ch in price.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else if (ch == '.' || ch == ',') && !num.is_empty() && !seen_dot {
            num.push('.');
            seen_dot = true;
        } else if !num.is_empty() {
            break;
        }
    }
    num.parse().ok()
}

/// Выплата USDT через проверенный bsc_pay (subprocess, web3). Возвращает txhash или текст ошибки.
async fn pay(cfg: &Config, to: &str, amount: f64) -> Result<String, String> {
    let (py, script, to, amt) = (
        cfg.bsc_python.clone(),
        cfg.bsc_script.clone(),
        to.to_string(),
        format!("{amount}"),
    );
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&py)
            .arg(&script)
            .arg(&to)
            .arg(&amt)
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let last = stdout.lines().last().unwrap_or("").trim().to_string();
    if last.starts_with("0x") {
        Ok(last)
    } else if let Some(msg) = last.strip_prefix("PAYERR:") {
        Err(msg.trim().to_string())
    } else {
        Err(format!(
            "bsc_pay: {}",
            if last.is_empty() {
                String::from_utf8_lossy(&out.stderr).trim().to_string()
            } else {
                last
            }
        ))
    }
}

/// Достать `code#state` из ввода: принимаем и весь callback-URL (…?code=X&state=Y),
/// и уже готовую строку code#state.
fn extract_code_state(s: &str) -> Option<String> {
    let s = s.trim();
    let val = |key: &str| -> Option<String> {
        s.split(|c| c == '?' || c == '&' || c == '#' || c == ' ')
            .find_map(|p| p.strip_prefix(key).map(|v| v.trim().to_string()))
            .filter(|v| !v.is_empty())
    };
    if let (Some(code), Some(state)) = (val("code="), val("state=")) {
        if code != "true" {
            return Some(format!("{code}#{state}")); // из callback-URL
        }
    }
    if s.contains('#') && !s.contains(char::is_whitespace) {
        return Some(s.to_string()); // уже code#state
    }
    None
}

fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    if s.contains(char::is_whitespace) || s.matches('@').count() != 1 {
        return false;
    }
    let (user, dom) = s.split_once('@').unwrap();
    !user.is_empty() && dom.contains('.') && !dom.starts_with('.') && !dom.ends_with('.')
}

async fn register_sub(cfg: &Config, email: &str, token: &str, proxy: &str) -> anyhow::Result<()> {
    // Пишем прямо в PostgreSQL authority движка; reload_loop подхватит подписку за ~30с.
    let (authority, email, token, proxy, fleet) = (
        crate::authority_cfg(cfg),
        email.to_string(),
        token.to_string(),
        proxy.to_string(),
        cfg.fleet.clone(),
    );
    tokio::task::spawn_blocking(move || {
        let mut auth = authority.connect()?;
        auth.add(&email, &token, &proxy, &fleet)
    })
    .await
    .map_err(|e| anyhow::anyhow!("PostgreSQL registration worker failed: {e}"))?
}

/// host:port:user:pass | host:port | http(s)://… → http-URL (для реестра/прокси).
fn proxy_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return raw.to_string();
    }
    let p: Vec<&str> = raw.split(':').collect();
    match p.len() {
        4 => format!("http://{}:{}@{}:{}", p[2], p[3], p[0], p[1]),
        2 => format!("http://{}:{}", p[0], p[1]),
        _ => String::new(),
    }
}

async fn do_start_token(bot: &Bot, cfg: &Arc<Config>, chat: i64, email: &str, proxy: &str) -> bool {
    let (cb, config_dir, em, px) = (
        cfg.claude_bin.clone(),
        cfg.claude_config_dir.clone(),
        email.trim().to_string(),
        proxy.to_string(),
    );
    let _ = bot.send(chat, "⏳ Запускаю выпуск токена…").await;
    match tokio::task::spawn_blocking(move || setup_token::start(chat, &em, &px, &cb, &config_dir))
        .await
    {
        Ok(Ok(url)) => {
            let _ = bot
                .send(
                    chat,
                    &format!(
            "🔗 <b>Шаг 3/3.</b> Открой ссылку, залогинься нужным аккаунтом, затем пришли \
             <b>адрес callback целиком</b> (или строку <code>code#state</code>):\n\n{}", esc(&url)),
                )
                .await;
            true
        }
        Ok(Err(e)) => {
            let _ = bot.send(chat, &format!("❌ {}", esc(&e.to_string()))).await;
            false
        }
        Err(_) => {
            let _ = bot.send(chat, "❌ Внутренняя ошибка запуска.").await;
            false
        }
    }
}

async fn do_feed_token(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    codestate: &str,
) {
    let cs = codestate.trim().to_string();
    let _ = bot.send(chat, "⏳ Проверяю код и выпускаю токен…").await;
    match tokio::task::spawn_blocking(move || setup_token::feed(chat, &cs)).await {
        Ok(Ok(Outcome::Token(tok, email, proxy))) => {
            match register_sub(cfg, &email, &tok, &proxy).await {
                Ok(_) => {
                    let _ = store.set_want(chat, "");
                    let _ = store.set_hproxy(chat, "");
                    let _ = bot.send(chat, &format!(
                    "✅ <b>Готово!</b> Доступ передан, подписка <code>{}</code> в системе. Спасибо за сделку! 🤝", esc(&email))).await;
                    notify_admins(bot, cfg, &format!(
                    "✅ <b>Доступ получен</b>: аккаунт <code>{}</code> добавлен в пул (прокси: {}).",
                    esc(&email), if proxy.is_empty() { "нет" } else { "есть" }), None).await;
                }
                Err(e) => {
                    let _ = store.set_want(chat, "ho_email");
                    let _ = bot.send(chat,
                    "⚠️ Токен выпущен, но сохранить его не удалось. Пришли <b>email</b> заново — повторим вход.").await;
                    notify_admins(
                        bot,
                        cfg,
                        &format!(
                            "⚠️ PostgreSQL registration failed for <code>{}</code>: {}",
                            esc(&email),
                            esc(&e.to_string())
                        ),
                        None,
                    )
                    .await;
                }
            }
        }
        Ok(Ok(Outcome::BadCode)) => {
            let _ = store.set_want(chat, "ho_email");
            let _ = bot.send(chat,
            "❌ Код отклонён (неверный/истёк). Пришли <b>email</b> аккаунта заново — дам свежую ссылку.").await;
        }
        Ok(Ok(Outcome::NoToken)) => {
            let _ = store.set_want(chat, "ho_email");
            let _ = bot
                .send(
                    chat,
                    "❌ Токен не получен вовремя. Пришли <b>email</b> заново.",
                )
                .await;
        }
        Ok(Err(e)) => {
            let _ = bot.send(chat, &format!("❌ {}", esc(&e.to_string()))).await;
        }
        Err(_) => {
            let _ = bot.send(chat, "❌ Внутренняя ошибка.").await;
        }
    }
}

/// Начать Google OAuth после закрепления постоянного прокси аккаунта. Ссылка содержит только
/// одноразовые state+PKCE параметры; токен приходит сервер-сервером в callback и никогда не
/// пересылается через Telegram.
async fn start_gemini_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
    client_id: &str,
    client_secret: &str,
) {
    // The wizard draft has served its purpose; drop the in-memory (id, secret) now that both are
    // captured as parameters and about to be sealed. Nothing else keeps the plaintext secret.
    if let Ok(mut drafts) = cfg.gemini_client_drafts.lock() {
        drafts.remove(&chat);
    }
    let Some(oauth) = cfg.gemini_oauth.as_ref() else {
        let _ = store.set_want(chat, "gm_gid");
        let _ = bot
            .send(
                chat,
                "⚠️ Приём Gemini OAuth сейчас выключен конфигурацией. Доступ не передавался; администратор уведомлён.",
            )
            .await;
        notify_admins(
            bot,
            cfg,
            "⚠️ Продавец дошёл до Gemini OAuth, но AUTH_BOT_GEMINI_* credential configuration отсутствует.",
            None,
        )
        .await;
        return;
    };
    let proxy = proxy
        .map(str::to_string)
        .or_else(|| store.get_user(chat).ok().flatten().map(|user| user.hproxy))
        .unwrap_or_default();
    if proxy.is_empty() {
        let _ = store.set_want(chat, "gm_gid");
        let _ = bot
            .send(
                chat,
                "Пришли прокси ещё раз, чтобы создать новую защищённую OAuth-сессию.",
            )
            .await;
        return;
    }
    match gemini_oauth::begin(
        store,
        oauth,
        chat,
        &proxy,
        proxy_order_id,
        Some(client_id.to_string()),
        Some(client_secret.to_string()),
    ) {
        Ok(url) => {
            // Delete the legacy/plain handoff slot as soon as the state-bound AEAD row exists.
            let _ = store.set_hproxy(chat, "");
            let _ = store.set_hproxy_order(chat, 0);
            let _ = store.set_want(chat, "gm_wait");
            let _ = bot
                .send_url_button(
                    chat,
                    "Данные приняты ✅\n\n<b>Шаг 2/2.</b> Открой ссылку в браузерном профиле этого Google-аккаунта, настроенном на тот же прокси. Подтверди доступ и дождись страницы успеха. Тип подписки и managed project бот определит у Google автоматически.",
                    "Авторизовать Gemini",
                    &url,
                )
                .await;
        }
        Err(error) => {
            let _ = store.set_want(chat, "gm_gid");
            let _ = bot.send(chat, error.public_message()).await;
        }
    }
}

/// Parse the seller's Gemini message. The seller creates their own Google Cloud OAuth *Web* client
/// and sends its id and secret; the id is the first non-empty line (…apps.googleusercontent.com),
/// the secret the second. A third line, when present, is the proxy (manual-proxy flow).
fn parse_gemini_client(text: &str) -> Option<(String, String, Option<String>)> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let client_id = lines.next()?.to_string();
    let client_secret = lines.next()?.to_string();
    let proxy = lines.next().map(str::to_string);
    if !client_id.ends_with(".apps.googleusercontent.com")
        || client_id.len() < 8
        || client_secret.len() < 6
    {
        return None;
    }
    Some((client_id, client_secret, proxy))
}

// ── Step-by-step Gemini onboarding prompts (one field per message) ─────────────────────────────
const GEMINI_STEP_ID: &str = "🔐 <b>Подключение Gemini — шаг 1 из 3: CLIENT&nbsp;ID</b>\n\n\
Заведи собственный OAuth-клиент Google (делается один раз, 2 минуты):\n\
1. Открой <b>Google Cloud Console → APIs &amp; Services → Credentials</b>\n\
2. <b>Create credentials → OAuth client ID → Application type: Web application</b>\n\
3. В поле <b>Authorized redirect URIs</b> добавь ровно:\n<code>https://gemini.api.apitoken.sale/oauth/callback</code>\n\
4. Нажми <b>Create</b>\n\n\
Пришли мне <b>Client ID</b> одним сообщением — он выглядит так:\n<code>1234567890-abcd.apps.googleusercontent.com</code>";

const GEMINI_STEP_ID_RETRY: &str = "🤔 Это не похоже на <b>Client ID</b>. Он всегда заканчивается на <code>.apps.googleusercontent.com</code>.\n\
Скопируй его из Google Cloud Console и пришли одним сообщением.";

const GEMINI_STEP_SECRET: &str = "✅ Client ID принят.\n\n\
🔑 <b>Шаг 2 из 3: CLIENT&nbsp;SECRET</b>\n\n\
В том же окне Google (рядом с Client ID) есть <b>Client secret</b>. Пришли его одним сообщением.\n\
Он выглядит примерно так: <code>GOCSPX-xxxxxxxxxxxxxxxx</code>\n\n\
🔒 Секрет нигде не сохраняется в открытом виде — он сразу шифруется.";

const GEMINI_STEP_SECRET_RETRY: &str = "🤔 Похоже, это не <b>Client secret</b> (обычно начинается с <code>GOCSPX-</code> и без пробелов).\n\
Пришли строку secret из Google Cloud Console одним сообщением.";

const GEMINI_STEP_PROXY: &str = "✅ Client secret принят.\n\n\
🌐 <b>Шаг 3 из 3: прокси</b>\n\n\
Пришли прокси, через который работает этот Google-аккаунт, одним сообщением в одном из форматов:\n\
<code>ip:port:user:pass</code>\n<code>http://user:pass@ip:port</code>\n\n\
Он нужен, чтобы OAuth и последующие запросы шли с одного адреса.";

const GEMINI_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси. Пришли его как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

const GEMINI_RESTART: &str = "⏱️ Сессия ввода сбросилась. Начнём заново с шага 1 — пришли <b>Client ID</b> (<code>…apps.googleusercontent.com</code>).";

/// If the seller already has an issued proxy, finish immediately; otherwise stash the (id, secret)
/// draft in RAM and ask for the proxy. Never persists the client secret to disk.
async fn gemini_finalize_or_ask_proxy(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    rec: &crate::db::UserRow,
    client_id: &str,
    client_secret: &str,
) {
    if !rec.hproxy.is_empty() || rec.hproxy_order != 0 {
        start_gemini_handoff(bot, store, cfg, chat, None, rec.hproxy_order, client_id, client_secret).await;
    } else {
        if let Ok(mut drafts) = cfg.gemini_client_drafts.lock() {
            drafts.insert(chat, (client_id.to_string(), client_secret.to_string()));
        }
        let _ = store.set_want(chat, "gm_gproxy");
        let _ = bot.send(chat, GEMINI_STEP_PROXY).await;
    }
}

/// Какой сценарий передачи доступа нужен по этому офферу.
///
/// Claude отдаётся токеном в реестр, ChatGPT — device-флоу в отдельный CODEX_HOME, Gemini —
/// OAuth-профилем в отдельный encrypted roster. Пространства имён шагов не пересекаются.
fn handoff_steps(store: &Store, oid: i64) -> (&'static str, &'static str) {
    let kind = store
        .get_offer(oid)
        .ok()
        .flatten()
        .map(|offer| handoff_kind(&offer.product))
        .unwrap_or(HandoffKind::Claude);
    match kind {
        HandoffKind::Claude => ("ho_proxy", "ho_email"),
        HandoffKind::Codex => ("cx_proxy", "cx_email"),
        HandoffKind::Gemini => ("gm_gid", "gm_gid"),
    }
}

/// Передача ChatGPT-подписки: device-флоу в свой CODEX_HOME.
///
/// В отличие от Claude здесь нет второго шага с `code#state`: codex сам опрашивает OpenAI и
/// завершается, поэтому продавцу достаточно открыть ссылку и ввести код. Ждём в фоне, чтобы не
/// заморозить приём сообщений, и ничего из auth store не читаем и не пересылаем.
async fn start_codex_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    email: &str,
    proxy: &str,
) {
    let (bin, dir, em, px) = (
        cfg.codex_bin.clone(),
        cfg.codex_homes_dir.clone(),
        email.trim().to_string(),
        proxy.to_string(),
    );
    let _ = bot.send(chat, "⏳ Готовлю авторизацию ChatGPT…").await;
    let started = tokio::task::spawn_blocking({
        let (em, px, bin, dir) = (em.clone(), px.clone(), bin.clone(), dir.clone());
        move || crate::codex_login::start(chat, &em, &px, &bin, &dir)
    })
    .await;
    let auth = match started {
        Ok(Ok(auth)) => auth,
        Ok(Err(e)) => {
            let _ = bot.send(chat, &format!("❌ {}", esc(&e.to_string()))).await;
            return;
        }
        Err(_) => {
            let _ = bot.send(chat, "❌ Внутренняя ошибка запуска.").await;
            return;
        }
    };
    let _ = store.set_want(chat, "");
    let _ = bot.send(chat, &format!(
        "🔗 <b>Шаг 3/3.</b> Открой ссылку, войди нужным аккаунтом ChatGPT и введи одноразовый код:\n\n\
         {url}\n\nКод: <code>{code}</code>\n\n\
         Код живёт 15 минут. Как только подтвердишь вход — сообщу здесь, ничего присылать не нужно.",
        url = esc(&auth.url), code = esc(&auth.code))).await;

    let (bot2, store2, cfg2) = (bot.clone(), store.clone(), cfg.clone());
    tokio::spawn(async move {
        let outcome =
            tokio::task::spawn_blocking(move || crate::codex_login::wait(chat, &bin)).await;
        match outcome {
            Ok(crate::codex_login::Outcome::Authorized { label, has_proxy }) => {
                let _ = store2.set_hproxy(chat, "");
                let _ = bot2.send(chat, &format!(
                    "✅ <b>Готово!</b> Доступ передан, подписка <code>{}</code> принята. Спасибо за сделку! 🤝",
                    esc(&label))).await;
                notify_admins(&bot2, &cfg2, &format!(
                    "✅ <b>ChatGPT-доступ получен</b>: аккаунт <code>{}</code> добавлен в пул Codex (прокси: {}). \
                     Движок подхватит его ближайшим health-тиком.",
                    esc(&label), if has_proxy { "свой" } else { "общий" }), None).await;
            }
            Ok(crate::codex_login::Outcome::Expired) => {
                let _ = store2.set_want(chat, "cx_email");
                let _ = bot2.send(chat,
                    "❌ Вход не подтверждён — код истёк. Пришли <b>email</b> заново, дам свежий код.").await;
            }
            Ok(crate::codex_login::Outcome::NotChatgpt) => {
                let _ = store2.set_want(chat, "cx_email");
                let _ = bot2.send(chat,
                    "❌ Это не подписка ChatGPT (похоже на вход по API-ключу). Нужен аккаунт с активной \
                     подпиской Plus/Pro. Пришли <b>email</b> заново.").await;
            }
            Ok(crate::codex_login::Outcome::Failed(why)) => {
                let _ = store2.set_want(chat, "cx_email");
                let _ = bot2
                    .send(
                        chat,
                        &format!(
                            "❌ Не получилось: {}. Пришли <b>email</b> заново.",
                            esc(&why)
                        ),
                    )
                    .await;
            }
            Err(_) => {
                let _ = bot2.send(chat, "❌ Внутренняя ошибка ожидания.").await;
            }
        }
    });
}

/// После оплаты (сумма > $10): авто-выпуск UK ISP прокси через IPRoyal и красивая выдача
/// продавцу. Ставит его в шаг ho_email. При ошибке — фолбэк на ручной ввод прокси продавцом.
async fn deliver_issued_proxy(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    admin_chat: i64,
    seller_chat: i64,
    oid: i64,
    hash: &str,
) {
    let _ = bot
        .send(
            seller_chat,
            &format!(
                "💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n⏳ Выпускаю прокси (UK ISP)…",
                esc(hash)
            ),
        )
        .await;
    match crate::iproyal::Iproyal::new(&cfg.iproyal_key)
        .issue_uk_isp_30d()
        .await
    {
        Ok(px) => {
            let _ = store.mark_offer_proxy_issued(oid);
            let (_, next_step) = handoff_steps(store, oid);
            let issued_proxy = px.url();
            // Store the handover proxy for every kind; the Gemini flow now defers begin() until the
            // seller sends their OAuth client, so the proxy (and its IPRoyal order) must persist.
            let _ = store.set_hproxy(seller_chat, &issued_proxy);
            if next_step == "gm_gid" {
                let _ = store.set_hproxy_order(seller_chat, px.order_id);
            }
            let _ = store.set_want(seller_chat, next_step);
            let next_prompt = match next_step {
                "cx_email" => {
                    "Зайди в аккаунт ChatGPT через этот прокси (HTTP) и пришли <b>email</b> аккаунта."
                }
                "gm_gid" => {
                    "Прокси закреплён за твоим Gemini-профилем ✅\nТеперь пришли <b>Client ID</b> своего Google OAuth-клиента одним сообщением (<code>…apps.googleusercontent.com</code>) — следующим шагом запрошу Client secret. В клиенте должен быть добавлен redirect URI <code>https://gemini.api.apitoken.sale/oauth/callback</code>."
                }
                _ => {
                    "Зайди в аккаунт Claude через этот прокси (HTTP) и пришли <b>email</b> аккаунта."
                }
            };
            let _ = bot
                .send(
                    seller_chat,
                    &format!(
                        "🔑 <b>Прокси выпущен</b> — UK · {city} (HTTP)\n\n\
                 <code>{compact}</code>\nURL: <code>{url}</code>\n\n\
                 <b>Шаг 2/3.</b> {next_prompt}",
                        city = esc(&px.city),
                        compact = esc(&px.compact()),
                        url = esc(&px.url())
                    ),
                )
                .await;
            let _ = bot.send(admin_chat, &format!(
                "✅ Прокси по офферу #{oid} выпущен (UK · {}, заказ IPRoyal #{}) и отправлен продавцу.",
                esc(&px.city), px.order_id)).await;
        }
        Err(e) => {
            let (proxy_step, _) = handoff_steps(store, oid);
            let _ = store.set_want(seller_chat, proxy_step);
            let prompt = if proxy_step == "gm_gid" {
                GEMINI_STEP_ID.to_string()
            } else {
                "⚠️ Авто-выпуск прокси временно не удался. <b>Передача доступа, шаг 1/3.</b>\n\
                 Пришли <b>прокси</b> аккаунта: <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.".to_string()
            };
            let _ = bot.send(seller_chat, &prompt).await;
            notify_admins(bot, cfg, &format!(
                "⚠️ Авто-выпуск прокси для оффера #{oid} не удался: {}\nПродавцу предложен ручной ввод.",
                esc(&e.to_string())), None).await;
        }
    }
}

// ── колбэки (кнопки) ─────────────────────────────────────────────────────────
pub async fn on_callback(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, cb: CallbackQuery) {
    let data = cb.data.clone().unwrap_or_default();
    let uid = cb.from.id;
    let uname = cb.from.username.clone().unwrap_or_default();
    let chat = cb.message.as_ref().map(|m| m.chat.id).unwrap_or(uid);
    let _ = bot.answer_callback(&cb.id, None).await;
    let _ = store.register_user(chat, uid, &uname);
    let admin = is_admin(cfg, store, uid, &uname);

    // продавец: подать заявку
    if data == "reg:request" {
        let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
        if rec.status == "approved" {
            let _ = bot.send(chat, "Ты уже продавец.").await;
            return;
        }
        let _ = store.set_status(chat, "pending");
        let _ = bot
            .send(
                chat,
                "📨 Заявка отправлена на модерацию. Как одобрят — сообщу.",
            )
            .await;
        notify_admins(
            bot,
            cfg,
            &format!(
                "🔔 <b>Новая заявка в продавцы</b>: @{} (id {})",
                esc(&uname),
                uid
            ),
            Some(&approve_kb(chat)),
        )
        .await;
        return;
    }

    // админ: одобрить/отклонить продавца
    if let Some(rest) = data
        .strip_prefix("reg:approve:")
        .or_else(|| data.strip_prefix("reg:reject:"))
    {
        if !admin {
            return;
        }
        let target: i64 = rest.parse().unwrap_or(0);
        let approve = data.starts_with("reg:approve:");
        let _ = store.set_status(target, if approve { "approved" } else { "rejected" });
        if approve {
            let _ = store.set_want(target, "reg_address");
            let _ = bot
                .send(
                    target,
                    "✅ <b>Заявка одобрена!</b> Ты продавец.\n\nПришли свой <b>BEP-20</b> \
                адрес (<code>0x…</code>) для выплат.",
                )
                .await;
        } else {
            let _ = bot.send(target, "🚫 Заявка отклонена.").await;
        }
        let _ = bot
            .send(
                chat,
                if approve {
                    "✅ Продавец одобрен."
                } else {
                    "🚫 Заявка отклонена."
                },
            )
            .await;
        return;
    }

    // админ: создать оффер (кнопка) → выбор продукта кнопками
    if data == "admin:new_offer" {
        if !admin {
            return;
        }
        let _ = bot.send_kb(chat, PRODUCT_PICK, Some(&product_kb())).await;
        return;
    }

    // админ: выбрал продукт из кнопок → выбор продавца-адресата
    if let Some(code) = data.strip_prefix("noffer:") {
        if !admin {
            return;
        }
        if let Some(name) = tier_name(code) {
            start_seller_pick(bot, store, chat, name).await;
        }
        return;
    }

    // админ: выбрал продавца → перейти к цене
    if let Some(rest) = data.strip_prefix("oseller:") {
        if !admin {
            return;
        }
        let seller_chat: i64 = rest.parse().unwrap_or(0);
        if let Ok(Some((_, product, _))) = store.get_admin_state(chat) {
            let who = seller_label(store, seller_chat);
            let _ = store.set_admin_state(chat, "price", &product, seller_chat);
            let _ = bot
                .send(
                    chat,
                    &format!(
                        "📦 Продукт: <b>{}</b>\nПродавец: <b>{}</b>\n\n{}",
                        esc(&product),
                        who,
                        PRICE_PROMPT
                    ),
                )
                .await;
        }
        return;
    }

    // админ: выплата продавцу (переиспользуем проверенный bsc_pay)
    if let Some(rest) = data.strip_prefix("pay:") {
        if !admin {
            return;
        }
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            let oid: i64 = parts[0].parse().unwrap_or(0);
            let seller_chat: i64 = parts[1].parse().unwrap_or(0);
            let o = store.get_offer(oid).ok().flatten();
            let seller = store
                .get_user(seller_chat)
                .ok()
                .flatten()
                .unwrap_or_default();
            if seller.address.is_empty() {
                let _ = bot.send(chat, "У продавца нет BEP-20 адреса.").await;
                return;
            }
            let amount = match o.as_ref().and_then(|x| parse_amount(&x.price)) {
                Some(a) if a > 0.0 => a,
                _ => {
                    let _ = bot
                        .send(
                            chat,
                            "Не понял сумму из цены оффера — уточни цену или оплати вручную.",
                        )
                        .await;
                    return;
                }
            };
            let _ = bot
                .send(
                    chat,
                    &format!(
                        "⏳ Отправляю <b>{}</b> USDT на <code>{}</code>…",
                        amount,
                        esc(&seller.address)
                    ),
                )
                .await;
            match pay(cfg, &seller.address, amount).await {
                Ok(hash) => {
                    let _ = store.set_response(oid, seller.uid, "paid");
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "✅ Оплачено. tx: <code>{}</code>\nhttps://bscscan.com/tx/{}",
                                esc(&hash),
                                esc(&hash)
                            ),
                        )
                        .await;
                    // Прокси авто-выпускается ТОЛЬКО после оплаты, если сумма сделки > $10 и по
                    // офферу его ещё не выпускали (1 оффер = 1 прокси). Иначе — ручной шаг продавца.
                    let already = store.offer_proxy_issued(oid).unwrap_or(false);
                    if amount > 10.0 && !already && !cfg.iproyal_key.is_empty() {
                        deliver_issued_proxy(bot, store, cfg, chat, seller_chat, oid, &hash).await;
                    } else {
                        let (proxy_step, _) = handoff_steps(store, oid);
                        let _ = store.set_want(seller_chat, proxy_step);
                        let _ = bot.send(chat, "Продавцу отправлена инструкция по передаче доступа (ручной прокси).").await;
                        let seller_prompt = if proxy_step == "gm_gid" {
                            format!(
                                "💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n{}",
                                esc(&hash), GEMINI_STEP_ID
                            )
                        } else {
                            format!(
                                "💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n<b>Передача доступа, шаг 1/3.</b>\n\
                             Пришли <b>прокси</b> аккаунта одним сообщением: <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.",
                                esc(&hash)
                            )
                        };
                        let _ = bot.send(seller_chat, &seller_prompt).await;
                    }
                }
                Err(e) => {
                    let _ = bot
                        .send(chat, &format!("❌ Оплата не прошла: {}", esc(&e)))
                        .await;
                }
            }
        }
        return;
    }

    // продавец: принять/отклонить оффер
    if let Some(rest) = data.strip_prefix("offer:") {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            let oid: i64 = parts[0].parse().unwrap_or(0);
            let action = parts[1];
            // Изоляция: реагировать может ТОЛЬКО адресат оффера.
            if let Ok(Some(off)) = store.get_offer(oid) {
                if off.seller_chat != 0 && off.seller_chat != chat {
                    return;
                }
            }
            if action == "reject" {
                let _ = store.set_response(oid, uid, "rejected");
                let _ = bot.send(chat, "Оффер отклонён.").await;
                return;
            }
            if action == "accept" {
                let _ = store.set_response(oid, uid, "accepted");
                let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
                let o = store.get_offer(oid).ok().flatten();
                let prod = o.as_ref().map(|x| x.product.clone()).unwrap_or_default();
                if rec.address.is_empty() {
                    let _ = store.set_want(chat, "reg_address");
                    let _ = bot.send(chat, "Принято! Сначала пришли <b>BEP-20</b> адрес (<code>0x…</code>) для выплаты.").await;
                    notify_admins(
                        bot,
                        cfg,
                        &format!(
                            "✅ <b>@{} принял оффер #{oid}</b> «{}» — ждём от него адрес.",
                            esc(&uname),
                            esc(&prod)
                        ),
                        None,
                    )
                    .await;
                } else {
                    let _ = bot
                        .send(
                            chat,
                            "✅ Принято! Передал администратору на подтверждение оплаты.",
                        )
                        .await;
                    let pay_kb: Keyboard =
                        vec![vec![("💸 Оплатить".into(), format!("pay:{oid}:{chat}"))]];
                    notify_admins(
                        bot,
                        cfg,
                        &format!(
                            "✅ <b>@{} принял оффер #{oid}</b> «{}».\nАдрес: <code>{}</code>",
                            esc(&uname),
                            esc(&prod),
                            esc(&rec.address)
                        ),
                        Some(&pay_kb),
                    )
                    .await;
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Arc<Store> {
        let directory = format!(
            "{}/authbot_bot_test_{}_{}",
            std::env::temp_dir().display(),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let _ = std::fs::remove_dir_all(&directory);
        let path = format!("{directory}/authbot.db");
        Arc::new(Store::open(&path).unwrap())
    }

    /// Продукт оффера разводит три несовместимые передачи доступа. В частности, Gemini никогда не
    /// должен попасть в `claude setup-token`, даже если позже изменится подпись кнопки.
    #[test]
    fn product_decides_which_handover_the_seller_gets() {
        for codex in [
            "ChatGPT Plus",
            "ChatGPT Pro",
            "chatgpt plus",
            "GPT-5 аккаунт",
        ] {
            assert_eq!(handoff_kind(codex), HandoffKind::Codex);
        }
        for claude in ["Claude Pro", "Claude 5x", "Claude 20x", "claude pro"] {
            assert_eq!(handoff_kind(claude), HandoffKind::Claude);
        }
        for gemini in [
            "Google AI Pro",
            "Google AI Ultra",
            "Code Assist Standard",
            "Code Assist Enterprise",
            "Workspace AI Ultra",
        ] {
            assert_eq!(handoff_kind(gemini), HandoffKind::Gemini);
        }
    }

    /// Каждая кнопка продукта должна резолвиться в имя, которое потом правильно классифицируется.
    /// Иначе новый ярлык в меню тихо уедет в Claude-ветку.
    #[test]
    fn every_product_button_resolves_and_classifies() {
        for row in product_kb() {
            for (label, data) in row {
                let code = data.strip_prefix("noffer:").expect("product button");
                let name = tier_name(code).expect("every button has a product name");
                assert_eq!(name, label, "button label and product name must match");
                let expected = if label.contains("Gemini")
                    || label.contains("Google AI")
                    || label.contains("Code Assist")
                    || label.contains("Workspace AI")
                {
                    HandoffKind::Gemini
                } else if label.contains("ChatGPT") {
                    HandoffKind::Codex
                } else {
                    HandoffKind::Claude
                };
                assert_eq!(handoff_kind(name), expected, "{label} classified wrongly");
            }
        }
    }

    #[test]
    fn persistent_admin_keyboard_lists_every_supported_gemini_line() {
        let buttons = admin_home_kb().into_iter().flatten().collect::<Vec<_>>();
        let expected = [
            ("📦 Google AI Pro", "Google AI Pro"),
            ("📦 Google AI Ultra", "Google AI Ultra"),
            ("📦 Code Assist Standard", "Code Assist Standard"),
            ("📦 Code Assist Enterprise", "Code Assist Enterprise"),
            ("📦 Workspace AI Ultra", "Workspace AI Ultra"),
        ];
        for (button, product) in expected {
            assert!(
                buttons.contains(&button),
                "missing persistent button {button}"
            );
            assert_eq!(admin_quick_tier(button), Some(product));
            assert_eq!(handoff_kind(product), HandoffKind::Gemini);
        }
    }

    /// The seller's Gemini message carries their OAuth client id and secret, and optionally a proxy;
    /// a non-Google client id or a missing/short secret is rejected before any OAuth session starts.
    #[test]
    fn parse_gemini_client_reads_id_secret_and_optional_proxy() {
        // Three lines: id, secret, proxy (manual-proxy flow).
        let (id, secret, proxy) = parse_gemini_client(
            "123456-abc.apps.googleusercontent.com\nGOCSPX-supersecretvalue\n1.2.3.4:8080:user:pass",
        )
        .unwrap();
        assert_eq!(id, "123456-abc.apps.googleusercontent.com");
        assert_eq!(secret, "GOCSPX-supersecretvalue");
        assert_eq!(proxy.as_deref(), Some("1.2.3.4:8080:user:pass"));

        // Two lines: id, secret (bot-issued proxy flow); blank lines tolerated.
        let (_, _, proxy) =
            parse_gemini_client("  123.apps.googleusercontent.com \n\nGOCSPX-value\n").unwrap();
        assert!(proxy.is_none());

        // A client id that is not a Google OAuth client id is rejected.
        assert!(parse_gemini_client("not-a-client\nsecret").is_none());
        // Missing secret is rejected.
        assert!(parse_gemini_client("123.apps.googleusercontent.com").is_none());
        // Too-short secret is rejected.
        assert!(parse_gemini_client("123.apps.googleusercontent.com\nshort").is_none());
    }

    #[test]
    fn handoff_steps_follow_the_offer_product() {
        let store = store();
        let claude = store.create_offer("Claude 20x", "$100", 1, 2).unwrap();
        let chatgpt = store.create_offer("ChatGPT Pro", "$200", 1, 2).unwrap();
        let gemini = store.create_offer("Google AI Ultra", "$300", 1, 2).unwrap();
        assert_eq!(handoff_steps(&store, claude), ("ho_proxy", "ho_email"));
        assert_eq!(handoff_steps(&store, chatgpt), ("cx_proxy", "cx_email"));
        assert_eq!(handoff_steps(&store, gemini), ("gm_gid", "gm_gid"));
        assert_eq!(handoff_steps(&store, 9_999), ("ho_proxy", "ho_email"));
    }

    /// Шаги трёх веток не должны пересекаться: одно и то же состояние в обеих отправило бы
    /// продавца в чужой обработчик после перезапуска бота.
    #[test]
    fn the_three_handovers_never_share_a_step_name() {
        let claude = ["ho_proxy", "ho_email", "ho_code"];
        let codex = ["cx_proxy", "cx_email"];
        let gemini = ["gm_gid", "gm_gsecret", "gm_gproxy", "gm_wait"];
        for step in claude {
            assert!(!codex.contains(&step) && !gemini.contains(&step));
        }
        for step in codex {
            assert!(!gemini.contains(&step));
        }
    }

    /// Прокси продавца приходит в разных формах; в реестр и в `proxy.url` должен уходить URL.
    #[test]
    fn seller_proxy_forms_normalise_to_a_url() {
        assert_eq!(
            proxy_url("1.2.3.4:8080:user:pass"),
            "http://user:pass@1.2.3.4:8080"
        );
        assert_eq!(proxy_url("1.2.3.4:8080"), "http://1.2.3.4:8080");
        assert_eq!(
            proxy_url("http://user:pass@1.2.3.4:8080"),
            "http://user:pass@1.2.3.4:8080"
        );
        assert_eq!(proxy_url("  "), "");
        assert_eq!(proxy_url("не прокси"), "");
    }

    use super::extract_code_state;
    #[test]
    fn parse_callback_url_and_codestate() {
        let url = "https://platform.claude.com/oauth/code/callback?code=rmkUNDCtEG8zswTyaDn44qFTMN6qLWLOQxGi91XhKEsZhrBp&state=47eEhvUtKx6vcoYLVGCcmkCMCVR7mPDBQF3XBZbGTnk";
        assert_eq!(extract_code_state(url).as_deref(),
            Some("rmkUNDCtEG8zswTyaDn44qFTMN6qLWLOQxGi91XhKEsZhrBp#47eEhvUtKx6vcoYLVGCcmkCMCVR7mPDBQF3XBZbGTnk"));
        assert_eq!(extract_code_state(" abc#xyz ").as_deref(), Some("abc#xyz")); // уже code#state
        assert_eq!(extract_code_state("justcode"), None); // мусор
                                                          // authorize-URL (code=true) не должен ловиться как код
        assert_eq!(
            extract_code_state("https://claude.com/cai/oauth/authorize?code=true&state=zzz"),
            None
        );
    }
}
