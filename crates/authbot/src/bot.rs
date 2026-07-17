//! Логика бота: команды, машина создания оффера, флоу продавца. Состояние — в SQLite (db).
//! Выплаты (Фаза 2) и выпуск setup-token (Фаза 3) пока заглушены — помечены TODO.

use crate::db::Store;
use crate::setup_token::{self, Outcome};
use crate::tg::{Bot, CallbackQuery, Keyboard};
use crate::Config;
use std::sync::Arc;

pub const WELCOME_NEW: &str =
    "👋 <b>Привет!</b>\nЭто бот закупки. Хочешь продавать — жми кнопку ниже, заявка уйдёт на модерацию.";
pub const ADMIN_HOME: &str = "🛠 <b>Дев-панель</b>\n\nБыстрая покупка: жми продукт на нижней \
    клавиатуре (Claude Pro / 5x / 20x) → пришли цену в $ → оффер уходит продавцам.";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn is_bep20(s: &str) -> bool {
    let s = s.trim();
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_admin(cfg: &Config, store: &Store, uid: i64, uname: &str) -> bool {
    if cfg.admins_id.contains(&uid) { return true; }
    let un = uname.to_lowercase();
    if !un.is_empty() && cfg.admins_name.contains(&un) { return true; }
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
const PRODUCT_PICK: &str = "📦 <b>Создание оффера</b>\nВыбери продукт:";
const PRICE_PROMPT: &str = "Теперь пришли <b>ЦЕНУ в долларах</b> одним сообщением \
     (например <code>20</code> или <code>15.5</code>). Это сумма выплаты продавцу. /cancel — отмена.";

fn product_kb() -> Keyboard {
    vec![
        vec![("Claude Pro".into(), "noffer:pro".into())],
        vec![("Claude 5x".into(), "noffer:5x".into())],
        vec![("Claude 20x".into(), "noffer:20x".into())],
    ]
}
fn tier_name(code: &str) -> Option<&'static str> {
    match code {
        "pro" => Some("Claude Pro"),
        "5x" => Some("Claude 5x"),
        "20x" => Some("Claude 20x"),
        _ => None,
    }
}
/// Текст закреплённой нижней кнопки продукта → имя продукта (для быстрой покупки админом).
fn admin_quick_tier(text: &str) -> Option<&'static str> {
    match text.trim() {
        "📦 Claude Pro" => Some("Claude Pro"),
        "📦 Claude 5x" => Some("Claude 5x"),
        "📦 Claude 20x" => Some("Claude 20x"),
        _ => None,
    }
}

/// Закреплённая нижняя клавиатура админа + список заявок на модерацию.
async fn show_admin_home(bot: &Bot, store: &Store, chat: i64) {
    let _ = bot
        .send_reply_kb(chat, ADMIN_HOME, &[
            vec!["📦 Claude Pro", "📦 Claude 5x", "📦 Claude 20x"],
            vec!["🛠 Панель"],
        ])
        .await;
    for u in store.by_status("pending").unwrap_or_default() {
        let _ = bot
            .send_kb(chat, &format!("🔔 Заявка в продавцы: @{} (id {})", esc(&u.username), u.uid),
                Some(&approve_kb(u.chat_id)))
            .await;
    }
}
/// Цена всегда в долларах: «$20» для целых, «$15.50» иначе.
fn fmt_usd(a: f64) -> String {
    if a.fract().abs() < 1e-9 { format!("${}", a as i64) } else { format!("${:.2}", a) }
}

async fn notify_admins(bot: &Bot, cfg: &Config, text: &str, kb: Option<&Keyboard>) {
    for id in &cfg.admins_id {
        let _ = bot.send_kb(*id, text, kb).await;
    }
}

fn offer_text(o: &crate::db::Offer) -> String {
    format!("📦 <b>Оффер #{}</b>\nПродукт: <b>{}</b>\nЦена: <b>{}</b>", o.id, esc(&o.product), esc(&o.price))
}

/// Отправить АДРЕСНЫЙ оффер одному продавцу. true — доставлено.
/// (Продавцы изолированы: оффер видит только адресат, не рассылается остальным.)
async fn send_offer_to(bot: &Bot, store: &Store, oid: i64, seller_chat: i64) -> bool {
    let o = match store.get_offer(oid) { Ok(Some(o)) => o, _ => return false };
    bot.send_kb(seller_chat, &offer_text(&o), Some(&offer_kb(oid))).await.is_ok()
}

/// Отображаемая метка продавца (@username или id) — уже HTML-экранирована.
fn seller_label(store: &Store, seller_chat: i64) -> String {
    let s = store.get_user(seller_chat).ok().flatten().unwrap_or_default();
    let id = if s.uid != 0 { s.uid } else { seller_chat };
    if s.username.is_empty() { format!("id {id}") } else { format!("@{}", esc(&s.username)) }
}

/// Клавиатура выбора продавца-адресата (по одному в строке).
fn seller_pick_kb(sellers: &[crate::db::UserRow]) -> Keyboard {
    sellers.iter().map(|s| {
        let label = if s.username.is_empty() { format!("id {}", s.uid) } else { format!("@{}", s.username) };
        vec![(label, format!("oseller:{}", s.chat_id))]
    }).collect()
}

/// Шаг «выбор продавца» после выбора продукта (перед ценой).
async fn start_seller_pick(bot: &Bot, store: &Store, chat: i64, product: &str) {
    let sellers = store.by_status("approved").unwrap_or_default();
    if sellers.is_empty() {
        let _ = store.clear_admin_state(chat);
        let _ = bot.send(chat, "Пока нет одобренных продавцов — оффер некому направить. Одобри заявку и повтори.").await;
        return;
    }
    let _ = store.set_admin_state(chat, "seller", product, 0);
    let _ = bot.send_kb(chat, &format!(
        "📦 Продукт: <b>{}</b>\n\nКому отправить оффер? Выбери продавца:", esc(product)),
        Some(&seller_pick_kb(&sellers))).await;
}

// ── сообщения ────────────────────────────────────────────────────────────────
pub async fn on_message(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>,
                        chat: i64, uid: i64, uname: &str, text: &str) {
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
                    let ot = store.get_offer(oid).ok().flatten().as_ref().map(offer_text).unwrap_or_default();
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
        let _ = bot.send(chat, if cleared { "Создание оффера отменено." } else { "Нечего отменять." }).await;
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
                    let _ = bot.send(chat, "👋 <b>Ты в системе как продавец.</b>\n\nПришли свой \
                        <b>BEP-20</b> адрес кошелька (<code>0x…</code>) для выплат.").await;
                } else {
                    let _ = bot.send(chat, &format!("👋 <b>Ты продавец.</b>\nКак появится оффер — пришлю сюда.\n\n\
                        💼 Адрес выплат:\n<code>{}</code>", esc(&rec.address))).await;
                }
            }
            "pending" => { let _ = bot.send(chat, "⏳ <b>Заявка на рассмотрении.</b> Как одобрят — сообщу.").await; }
            "rejected" => { let _ = bot.send(chat, "🚫 <b>Заявка отклонена.</b>").await; }
            _ => { let _ = bot.send_kb(chat, WELCOME_NEW, Some(&welcome_kb())).await; }
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
                    let _ = store.set_address(chat, text); let _ = store.set_want(chat, "");
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
                    let _ = store.set_hproxy(chat, &purl); let _ = store.set_want(chat, "ho_email");
                    let _ = bot.send(chat, "Прокси принят ✅\n<b>Шаг 2/3.</b> Пришли <b>email</b> аккаунта Claude.").await;
                }
            }
            "ho_email" => {
                if !looks_like_email(text) {
                    let _ = bot.send(chat, "Это не похоже на email. Пришли адрес аккаунта ещё раз.").await;
                } else if do_start_token(bot, cfg, chat, text, &rec.hproxy).await {
                    let _ = store.set_want(chat, "ho_code");
                }
            }
            "ho_code" => {
                match extract_code_state(text) {
                    Some(cs) => do_feed_token(bot, store, cfg, chat, &cs).await,
                    None => { let _ = bot.send(chat, "Пришли <b>адрес callback целиком</b> (…/callback?code=…&state=…) или строку <code>code#state</code>.").await; }
                }
            }
            _ => { let _ = bot.send(chat, "Доступна только команда /start.").await; }
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
        if ch.is_ascii_digit() { num.push(ch); }
        else if (ch == '.' || ch == ',') && !num.is_empty() && !seen_dot { num.push('.'); seen_dot = true; }
        else if !num.is_empty() { break; }
    }
    num.parse().ok()
}

/// Выплата USDT через проверенный bsc_pay (subprocess, web3). Возвращает txhash или текст ошибки.
async fn pay(cfg: &Config, to: &str, amount: f64) -> Result<String, String> {
    let (py, script, to, amt) = (cfg.bsc_python.clone(), cfg.bsc_script.clone(), to.to_string(), format!("{amount}"));
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&py).arg(&script).arg(&to).arg(&amt).output()
    }).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let last = stdout.lines().last().unwrap_or("").trim().to_string();
    if last.starts_with("0x") { Ok(last) }
    else if let Some(msg) = last.strip_prefix("PAYERR:") { Err(msg.trim().to_string()) }
    else { Err(format!("bsc_pay: {}", if last.is_empty() { String::from_utf8_lossy(&out.stderr).trim().to_string() } else { last })) }
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
            return Some(format!("{code}#{state}"));   // из callback-URL
        }
    }
    if s.contains('#') && !s.contains(char::is_whitespace) {
        return Some(s.to_string());                    // уже code#state
    }
    None
}

fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    if s.contains(char::is_whitespace) || s.matches('@').count() != 1 { return false; }
    let (user, dom) = s.split_once('@').unwrap();
    !user.is_empty() && dom.contains('.') && !dom.starts_with('.') && !dom.ends_with('.')
}

fn register_sub(cfg: &Config, email: &str, token: &str, proxy: &str) -> anyhow::Result<()> {
    // Пишем ПРЯМО в authority движка (Postgres, если задан CLAUDE_API_DATABASE_URL): движок
    // подхватит подписку своим reload_loop за ~30с. SQLite-путь остаётся fallback'ом.
    let mut auth = crate::authority_cfg(cfg).connect()?;
    auth.add(email, token, proxy, &cfg.fleet)?;
    Ok(())
}

/// host:port:user:pass | host:port | http(s)://… → http-URL (для реестра/прокси).
fn proxy_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() { return String::new(); }
    if raw.starts_with("http://") || raw.starts_with("https://") { return raw.to_string(); }
    let p: Vec<&str> = raw.split(':').collect();
    match p.len() {
        4 => format!("http://{}:{}@{}:{}", p[2], p[3], p[0], p[1]),
        2 => format!("http://{}:{}", p[0], p[1]),
        _ => String::new(),
    }
}

async fn do_start_token(bot: &Bot, cfg: &Arc<Config>, chat: i64, email: &str, proxy: &str) -> bool {
    let (cb, em, px) = (cfg.claude_bin.clone(), email.trim().to_string(), proxy.to_string());
    let _ = bot.send(chat, "⏳ Запускаю выпуск токена…").await;
    match tokio::task::spawn_blocking(move || setup_token::start(chat, &em, &px, &cb)).await {
        Ok(Ok(url)) => { let _ = bot.send(chat, &format!(
            "🔗 <b>Шаг 3/3.</b> Открой ссылку, залогинься нужным аккаунтом, затем пришли \
             <b>адрес callback целиком</b> (или строку <code>code#state</code>):\n\n{}", esc(&url))).await; true }
        Ok(Err(e)) => { let _ = bot.send(chat, &format!("❌ {}", esc(&e.to_string()))).await; false }
        Err(_) => { let _ = bot.send(chat, "❌ Внутренняя ошибка запуска.").await; false }
    }
}

async fn do_feed_token(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, chat: i64, codestate: &str) {
    let cs = codestate.trim().to_string();
    let _ = bot.send(chat, "⏳ Проверяю код и выпускаю токен…").await;
    match tokio::task::spawn_blocking(move || setup_token::feed(chat, &cs)).await {
        Ok(Ok(Outcome::Token(tok, email, proxy))) => match register_sub(cfg, &email, &tok, &proxy) {
            Ok(_) => {
                let _ = store.set_want(chat, "");
                let _ = store.set_hproxy(chat, "");
                let _ = bot.send(chat, &format!(
                    "✅ <b>Готово!</b> Доступ передан, подписка <code>{}</code> в системе. Спасибо за сделку! 🤝", esc(&email))).await;
                notify_admins(bot, cfg, &format!(
                    "✅ <b>Доступ получен</b>: аккаунт <code>{}</code> добавлен в пул (прокси: {}).",
                    esc(&email), if proxy.is_empty() { "нет" } else { "есть" }), None).await;
            }
            Err(e) => { let _ = bot.send(chat, &format!(
                "⚠️ Токен выпущен, но запись в пул не удалась: {}", esc(&e.to_string()))).await; }
        },
        Ok(Ok(Outcome::BadCode)) => { let _ = store.set_want(chat, "ho_email"); let _ = bot.send(chat,
            "❌ Код отклонён (неверный/истёк). Пришли <b>email</b> аккаунта заново — дам свежую ссылку.").await; }
        Ok(Ok(Outcome::NoToken)) => { let _ = store.set_want(chat, "ho_email"); let _ = bot.send(chat,
            "❌ Токен не получен вовремя. Пришли <b>email</b> заново.").await; }
        Ok(Err(e)) => { let _ = bot.send(chat, &format!("❌ {}", esc(&e.to_string()))).await; }
        Err(_) => { let _ = bot.send(chat, "❌ Внутренняя ошибка.").await; }
    }
}

/// После оплаты (сумма > $10): авто-выпуск UK ISP прокси через IPRoyal и красивая выдача
/// продавцу. Ставит его в шаг ho_email. При ошибке — фолбэк на ручной ввод прокси продавцом.
async fn deliver_issued_proxy(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>,
                              admin_chat: i64, seller_chat: i64, oid: i64, hash: &str) {
    let _ = bot.send(seller_chat, &format!(
        "💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n⏳ Выпускаю прокси (UK ISP)…", esc(hash))).await;
    match crate::iproyal::Iproyal::new(&cfg.iproyal_key).issue_uk_isp_30d().await {
        Ok(px) => {
            let _ = store.mark_offer_proxy_issued(oid);
            let _ = store.set_hproxy(seller_chat, &px.url());
            let _ = store.set_want(seller_chat, "ho_email");
            let _ = bot.send(seller_chat, &format!(
                "🔑 <b>Прокси выпущен</b> — UK · {city} (HTTP)\n\n\
                 <code>{compact}</code>\nURL: <code>{url}</code>\n\n\
                 <b>Шаг 2/3.</b> Зайди в аккаунт Claude через этот прокси (HTTP) и пришли <b>email</b> аккаунта.",
                city = esc(&px.city), compact = esc(&px.compact()), url = esc(&px.url()))).await;
            let _ = bot.send(admin_chat, &format!(
                "✅ Прокси по офферу #{oid} выпущен (UK · {}, заказ IPRoyal #{}) и отправлен продавцу.",
                esc(&px.city), px.order_id)).await;
        }
        Err(e) => {
            let _ = store.set_want(seller_chat, "ho_proxy");
            let _ = bot.send(seller_chat,
                "⚠️ Авто-выпуск прокси временно не удался. <b>Передача доступа, шаг 1/3.</b>\n\
                 Пришли <b>прокси</b> аккаунта: <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
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
        if rec.status == "approved" { let _ = bot.send(chat, "Ты уже продавец.").await; return; }
        let _ = store.set_status(chat, "pending");
        let _ = bot.send(chat, "📨 Заявка отправлена на модерацию. Как одобрят — сообщу.").await;
        notify_admins(bot, cfg, &format!("🔔 <b>Новая заявка в продавцы</b>: @{} (id {})", esc(&uname), uid),
                      Some(&approve_kb(chat))).await;
        return;
    }

    // админ: одобрить/отклонить продавца
    if let Some(rest) = data.strip_prefix("reg:approve:").or_else(|| data.strip_prefix("reg:reject:")) {
        if !admin { return; }
        let target: i64 = rest.parse().unwrap_or(0);
        let approve = data.starts_with("reg:approve:");
        let _ = store.set_status(target, if approve { "approved" } else { "rejected" });
        if approve {
            let _ = store.set_want(target, "reg_address");
            let _ = bot.send(target, "✅ <b>Заявка одобрена!</b> Ты продавец.\n\nПришли свой <b>BEP-20</b> \
                адрес (<code>0x…</code>) для выплат.").await;
        } else {
            let _ = bot.send(target, "🚫 Заявка отклонена.").await;
        }
        let _ = bot.send(chat, if approve { "✅ Продавец одобрен." } else { "🚫 Заявка отклонена." }).await;
        return;
    }

    // админ: создать оффер (кнопка) → выбор продукта кнопками
    if data == "admin:new_offer" {
        if !admin { return; }
        let _ = bot.send_kb(chat, PRODUCT_PICK, Some(&product_kb())).await;
        return;
    }

    // админ: выбрал продукт из кнопок → выбор продавца-адресата
    if let Some(code) = data.strip_prefix("noffer:") {
        if !admin { return; }
        if let Some(name) = tier_name(code) {
            start_seller_pick(bot, store, chat, name).await;
        }
        return;
    }

    // админ: выбрал продавца → перейти к цене
    if let Some(rest) = data.strip_prefix("oseller:") {
        if !admin { return; }
        let seller_chat: i64 = rest.parse().unwrap_or(0);
        if let Ok(Some((_, product, _))) = store.get_admin_state(chat) {
            let who = seller_label(store, seller_chat);
            let _ = store.set_admin_state(chat, "price", &product, seller_chat);
            let _ = bot.send(chat, &format!(
                "📦 Продукт: <b>{}</b>\nПродавец: <b>{}</b>\n\n{}", esc(&product), who, PRICE_PROMPT)).await;
        }
        return;
    }

    // админ: выплата продавцу (переиспользуем проверенный bsc_pay)
    if let Some(rest) = data.strip_prefix("pay:") {
        if !admin { return; }
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            let oid: i64 = parts[0].parse().unwrap_or(0);
            let seller_chat: i64 = parts[1].parse().unwrap_or(0);
            let o = store.get_offer(oid).ok().flatten();
            let seller = store.get_user(seller_chat).ok().flatten().unwrap_or_default();
            if seller.address.is_empty() { let _ = bot.send(chat, "У продавца нет BEP-20 адреса.").await; return; }
            let amount = match o.as_ref().and_then(|x| parse_amount(&x.price)) {
                Some(a) if a > 0.0 => a,
                _ => { let _ = bot.send(chat, "Не понял сумму из цены оффера — уточни цену или оплати вручную.").await; return; }
            };
            let _ = bot.send(chat, &format!("⏳ Отправляю <b>{}</b> USDT на <code>{}</code>…", amount, esc(&seller.address))).await;
            match pay(cfg, &seller.address, amount).await {
                Ok(hash) => {
                    let _ = store.set_response(oid, seller.uid, "paid");
                    let _ = bot.send(chat, &format!(
                        "✅ Оплачено. tx: <code>{}</code>\nhttps://bscscan.com/tx/{}", esc(&hash), esc(&hash))).await;
                    // Прокси авто-выпускается ТОЛЬКО после оплаты, если сумма сделки > $10 и по
                    // офферу его ещё не выпускали (1 оффер = 1 прокси). Иначе — ручной шаг продавца.
                    let already = store.offer_proxy_issued(oid).unwrap_or(false);
                    if amount > 10.0 && !already && !cfg.iproyal_key.is_empty() {
                        deliver_issued_proxy(bot, store, cfg, chat, seller_chat, oid, &hash).await;
                    } else {
                        let _ = store.set_want(seller_chat, "ho_proxy");
                        let _ = bot.send(chat, "Продавцу отправлена инструкция по передаче доступа (ручной прокси).").await;
                        let _ = bot.send(seller_chat, &format!(
                            "💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n<b>Передача доступа, шаг 1/3.</b>\n\
                             Пришли <b>прокси</b> аккаунта одним сообщением: <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.", esc(&hash))).await;
                    }
                }
                Err(e) => { let _ = bot.send(chat, &format!("❌ Оплата не прошла: {}", esc(&e))).await; }
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
                if off.seller_chat != 0 && off.seller_chat != chat { return; }
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
                    notify_admins(bot, cfg, &format!(
                        "✅ <b>@{} принял оффер #{oid}</b> «{}» — ждём от него адрес.", esc(&uname), esc(&prod)), None).await;
                } else {
                    let _ = bot.send(chat, "✅ Принято! Передал администратору на подтверждение оплаты.").await;
                    let pay_kb: Keyboard = vec![vec![("💸 Оплатить".into(), format!("pay:{oid}:{chat}"))]];
                    notify_admins(bot, cfg, &format!(
                        "✅ <b>@{} принял оффер #{oid}</b> «{}».\nАдрес: <code>{}</code>",
                        esc(&uname), esc(&prod), esc(&rec.address)), Some(&pay_kb)).await;
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_code_state;
    #[test]
    fn parse_callback_url_and_codestate() {
        let url = "https://platform.claude.com/oauth/code/callback?code=rmkUNDCtEG8zswTyaDn44qFTMN6qLWLOQxGi91XhKEsZhrBp&state=47eEhvUtKx6vcoYLVGCcmkCMCVR7mPDBQF3XBZbGTnk";
        assert_eq!(extract_code_state(url).as_deref(),
            Some("rmkUNDCtEG8zswTyaDn44qFTMN6qLWLOQxGi91XhKEsZhrBp#47eEhvUtKx6vcoYLVGCcmkCMCVR7mPDBQF3XBZbGTnk"));
        assert_eq!(extract_code_state(" abc#xyz ").as_deref(), Some("abc#xyz")); // уже code#state
        assert_eq!(extract_code_state("justcode"), None);                        // мусор
        // authorize-URL (code=true) не должен ловиться как код
        assert_eq!(extract_code_state("https://claude.com/cai/oauth/authorize?code=true&state=zzz"), None);
    }
}
