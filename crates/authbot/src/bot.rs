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
const PRODUCT_PICK: &str = "📦 <b>Создание оффера</b>\nВыбери продукт:";
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
        "📦 ChatGPT Plus" => Some("ChatGPT Plus"),
        "📦 ChatGPT Pro" => Some("ChatGPT Pro"),
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
        vec!["📦 ChatGPT Plus", "📦 ChatGPT Pro"],
        vec!["📦 Google AI Pro", "📦 Google AI Ultra"],
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

const CLAUDE_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый аккаунт Claude и активировать тариф из оффера.\n\
4. Прислать боту email аккаунта, открыть ссылку авторизации в том же профиле, затем скопировать и прислать весь адрес из адресной строки.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, cookie, банковские данные и коды из почты бот не просит.";

const CODEX_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый аккаунт ChatGPT и активировать Plus/Pro из оффера.\n\
4. Прислать боту email, открыть ссылку OpenAI в том же профиле и ввести выданный одноразовый код.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, cookie, банковские данные и коды из почты бот не просит.";

const GEMINI_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый Google-аккаунт и активировать Google AI Pro/Ultra из оффера.\n\
4. Вернуться в бот, нажать «Аккаунт готов» и подтвердить доступ Google в том же профиле.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай Google-аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, cookie, банковские данные и коды из почты бот не просит.";

const CLAUDE_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь аккаунт Claude</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой <code>https://claude.ai</code> и самостоятельно зарегистрируй <b>новый</b> аккаунт. Если Google-аккаунта ещё нет, сначала создай его на <code>https://accounts.google.com</code> внутри этого же профиля, затем на Claude выбери «Continue with Google». Если входишь по email — подтверди письмо, не выходя из профиля.\n\n\
4️⃣ Подключи тариф, указанный в оффере, и проверь, что Claude открывается. Не меняй прокси и не закрывай профиль: он ещё понадобится для авторизации.\n\n\
5️⃣ Когда всё готово, пришли сюда <b>точный email аккаунта Claude</b> одним сообщением. Больше ничего присылать не нужно.";

const CODEX_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь аккаунт ChatGPT</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой <code>https://chatgpt.com</code> и самостоятельно зарегистрируй <b>новый</b> аккаунт. Если Google-аккаунта ещё нет, сначала создай его на <code>https://accounts.google.com</code> внутри этого же профиля, затем на ChatGPT выбери «Continue with Google». Если входишь по email — подтверди письмо, не выходя из профиля.\n\n\
4️⃣ Активируй подписку Plus или Pro из оффера и проверь, что ChatGPT открывается. Не меняй прокси и не закрывай профиль: он ещё понадобится для подтверждения входа.\n\n\
5️⃣ Когда всё готово, пришли сюда <b>точный email аккаунта ChatGPT</b> одним сообщением. Больше ничего присылать не нужно.";

const GEMINI_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь Google-аккаунт для Gemini</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой <code>https://accounts.google.com</code> и самостоятельно зарегистрируй <b>новый Google-аккаунт</b>. Подтверди почту или телефон, не выходя из этого профиля.\n\n\
4️⃣ В том же профиле открой <code>https://one.google.com</code>, активируй тариф Google AI Pro или Ultra, который указан в оффере, и проверь, что подписка появилась именно на новом аккаунте.\n\n\
5️⃣ Не закрывай профиль и не меняй прокси: они понадобятся на следующем этапе. Когда аккаунт и подписка готовы, нажми кнопку <b>«Аккаунт готов — продолжить»</b> ниже.\n\n\
🔒 Бот не попросит пароль, cookie, банковские данные или коды из почты.";

const CLAUDE_MANUAL_PROXY: &str = "⚠️ Автоматически выдать прокси сейчас не получилось.\n\n\
🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Claude</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

const CODEX_MANUAL_PROXY: &str = "⚠️ Автоматически выдать прокси сейчас не получилось.\n\n\
🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта ChatGPT</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

const GEMINI_MANUAL_PROXY: &str = "⚠️ Автоматически выдать прокси сейчас не получилось.\n\n\
🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Gemini</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй Google-аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

const GEMINI_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Gemini</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй Google-аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

fn gemini_ready_kb() -> Keyboard {
    vec![vec![(
        "✅ Аккаунт готов — продолжить".into(),
        "gemini:ready".into(),
    )]]
}

fn seller_offer_guide(product: &str) -> &'static str {
    match handoff_kind(product) {
        HandoffKind::Claude => CLAUDE_OFFER_GUIDE,
        HandoffKind::Codex => CODEX_OFFER_GUIDE,
        HandoffKind::Gemini => GEMINI_OFFER_GUIDE,
    }
}

fn account_setup_prompt(step: &str) -> &'static str {
    match step {
        "cx_email" => CODEX_ACCOUNT_SETUP,
        "ho_email" => CLAUDE_ACCOUNT_SETUP,
        "gm_ready" => GEMINI_ACCOUNT_SETUP,
        _ => "",
    }
}

fn manual_proxy_prompt(step: &str) -> &'static str {
    match step {
        "cx_proxy" => CODEX_MANUAL_PROXY,
        "gm_gproxy" => GEMINI_MANUAL_PROXY,
        _ => CLAUDE_MANUAL_PROXY,
    }
}

fn accepted_next_step(product: &str) -> &'static str {
    match handoff_kind(product) {
        HandoffKind::Claude => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай Claude-аккаунт.</b>",
        HandoffKind::Codex => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай ChatGPT-аккаунт.</b>",
        HandoffKind::Gemini => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай Google-аккаунт.</b>",
    }
}

fn offer_text(o: &crate::db::Offer) -> String {
    let guide = seller_offer_guide(&o.product);
    format!(
        "📦 <b>Оффер #{}</b>\nПродукт: <b>{}</b>\nЦена: <b>{}</b>{}",
        o.id,
        esc(&o.product),
        esc(&o.price),
        if guide.is_empty() {
            String::new()
        } else {
            format!("\n\n{guide}")
        }
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
        if !admin {
            let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
            if matches!(
                rec.want.as_str(),
                "gm_gid" | "gm_gsecret" | "gm_gproxy" | "gm_ready" | "gm_wait"
            ) {
                let _ = store.cancel_gemini_oauth(chat);
                let _ = bot
                    .send(
                        chat,
                        &format!("Авторизация отменена.\n\n{GEMINI_PROXY_PROMPT}"),
                    )
                    .await;
                return;
            }
        }
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
                    let _ = bot.send(chat, "🤔 Не разобрал прокси. Пришли его одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
                } else {
                    let _ = store.set_hproxy(chat, &purl);
                    let _ = store.set_want(chat, "ho_email");
                    let _ = bot
                        .send(chat, &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{CLAUDE_ACCOUNT_SETUP}"))
                        .await;
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
                    let _ = bot.send(chat, "🤔 Не разобрал прокси. Пришли его одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
                } else {
                    let _ = store.set_hproxy(chat, &purl);
                    let _ = store.set_want(chat, "cx_email");
                    let _ = bot
                        .send(chat, &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{CODEX_ACCOUNT_SETUP}"))
                        .await;
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
            // `gm_gid`/`gm_gsecret` are accepted only as restart compatibility for users who were
            // in the removed custom-client wizard during deployment. Every session now pauses at
            // account preparation before authorization, including users with a retained proxy.
            "gm_gid" | "gm_gsecret" | "gm_gproxy" => {
                if !rec.hproxy.is_empty() {
                    prepare_gemini_account(bot, store, chat, None, rec.hproxy_order).await;
                } else {
                    let purl = proxy_url(text.trim());
                    if purl.is_empty() {
                        let _ = bot.send(chat, GEMINI_STEP_PROXY_RETRY).await;
                    } else {
                        prepare_gemini_account(bot, store, chat, Some(&purl), rec.hproxy_order)
                            .await;
                    }
                }
            }
            "gm_ready" => {
                if text.to_lowercase() == "готово" {
                    continue_gemini_handoff(bot, store, cfg, chat).await;
                } else {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Когда новый Google-аккаунт создан и подписка из оффера активна, нажми кнопку ниже. До этого не меняй профиль или прокси.",
                            Some(&gemini_ready_kb()),
                        )
                        .await;
                }
            }
            "gm_wait" => {
                let _ = bot
                    .send(
                        chat,
                        "Авторизация уже ждёт одноразовый код. Заверши вход по первой ссылке, затем открой кнопку «Ввести код» и отправь код через защищённую форму. В Telegram код не присылай. /cancel начнёт заново.",
                    )
                    .await;
            }
            "ho_code" => match extract_code_state(text) {
                Some(cs) => do_feed_token(bot, store, cfg, chat, &cs).await,
                None => {
                    let _ = bot.send(chat, "Пришли <b>весь адрес страницы из адресной строки</b>: от <code>https://</code> до самого конца. Одного короткого кода недостаточно.").await;
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
    let _ = bot.send(chat, "⏳ Готовлю авторизацию Claude…").await;
    match tokio::task::spawn_blocking(move || setup_token::start(chat, &em, &px, &cb, &config_dir))
        .await
    {
        Ok(Ok(url)) => {
            let _ = bot
                .send(
                    chat,
                    &format!(
            "🔗 <b>Этап 3 из 3 — передай доступ Claude</b>\n\n\
             1️⃣ Не закрывая подготовленный антидетект-профиль и не меняя прокси, открой ссылку ниже. <b>Не открывай её в Telegram, обычном браузере или на телефоне.</b>\n\n\
             2️⃣ Войди именно в новый Claude-аккаунт и подтверди доступ.\n\n\
             3️⃣ После подтверждения браузер откроет новую страницу. Даже если она пустая или не загрузилась, скопируй <b>весь адрес из адресной строки</b> — от <code>https://</code> до самого конца.\n\n\
             4️⃣ Пришли этот адрес сюда одним сообщением. Пароль, cookie и коды из email не присылай.\n\n\
             <b>Ссылка авторизации:</b>\n{}", esc(&url)),
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
    let _ = bot.send(chat, "⏳ Проверяю авторизацию…").await;
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
                    "⚠️ Доступ получен, но добавить аккаунт не удалось. Пришли <b>email</b> заново — повторим вход.").await;
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
                    "❌ Авторизация не завершилась вовремя. Пришли <b>email</b> заново.",
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

/// Перевести Gemini-сделку на самостоятельную регистрацию аккаунта. Прокси сохраняется до явного
/// подтверждения готовности, поэтому авторизация не может случайно начаться раньше регистрации и
/// активации тарифа.
async fn prepare_gemini_account(
    bot: &Bot,
    store: &Arc<Store>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    if let Some(proxy) = proxy {
        let _ = store.set_hproxy(chat, proxy);
    }
    if proxy_order_id > 0 {
        let _ = store.set_hproxy_order(chat, proxy_order_id);
    }
    let has_proxy = store
        .get_user(chat)
        .ok()
        .flatten()
        .is_some_and(|user| !user.hproxy.is_empty());
    if !has_proxy {
        let _ = store.set_want(chat, "gm_gproxy");
        let _ = bot.send(chat, GEMINI_PROXY_PROMPT).await;
        return;
    }
    let _ = store.set_want(chat, "gm_ready");
    let _ = bot
        .send_kb(
            chat,
            &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{GEMINI_ACCOUNT_SETUP}"),
            Some(&gemini_ready_kb()),
        )
        .await;
}

/// Return the proxy only for the explicit Gemini readiness state. Callback buttons can be old or
/// forwarded, so neither the button itself nor a stored proxy alone authorizes a state transition.
fn gemini_ready_handoff(store: &Store, chat: i64) -> Option<(String, i64)> {
    let user = store.get_user(chat).ok().flatten()?;
    if user.want != "gm_ready" || user.hproxy.is_empty() {
        return None;
    }
    Some((user.hproxy, user.hproxy_order))
}

async fn continue_gemini_handoff(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, chat: i64) {
    let Some((proxy, proxy_order_id)) = gemini_ready_handoff(store, chat) else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    };
    start_gemini_handoff(bot, store, cfg, chat, Some(&proxy), proxy_order_id).await;
}

/// Начать официальную авторизацию Gemini после закрепления постоянного прокси и явного
/// подтверждения продавца. Google показывает одноразовый код, а продавец отправляет его через
/// защищённую HTTPS-форму Auth Bot; Telegram не получает ни код, ни токен.
async fn start_gemini_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    let Some(oauth) = cfg.gemini_oauth.as_ref() else {
        let _ = store.set_want(chat, "gm_ready");
        let _ = bot
            .send_kb(
                chat,
                "⚠️ Подключение Gemini сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз этой же кнопкой после исправления.",
                Some(&gemini_ready_kb()),
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
        let _ = store.set_want(chat, "gm_gproxy");
        let _ = bot.send(chat, GEMINI_PROXY_PROMPT).await;
        return;
    }
    match gemini_oauth::begin(store, oauth, chat, &proxy, proxy_order_id) {
        Ok(links) => {
            // Delete the legacy/plain handoff slot as soon as the state-bound AEAD row exists.
            let _ = store.set_hproxy(chat, "");
            let _ = store.set_want(chat, "gm_wait");
            let _ = bot
                .send_url_button(
                    chat,
                    "🔗 <b>Этап 3 из 3 — подтверди доступ Gemini</b>\n\n1️⃣ Не закрывая подготовленный антидетект-профиль и не меняя прокси, открой официальную ссылку ниже. <b>Не открывай её в Telegram, обычном браузере или на телефоне.</b>\n\n2️⃣ Войди именно в новый Google-аккаунт и подтверди доступ. Google покажет одноразовый код.",
                    "Авторизовать через Gemini CLI",
                    &links.authorize_url,
                )
                .await;
            let _ = bot
                .send_url_button(
                    chat,
                    "3️⃣ Скопируй одноразовый код со страницы Google, нажми кнопку ниже и вставь код в защищённую форму. Не отправляй его сообщением в Telegram.\n\n4️⃣ После отправки просто вернись в бот: активную подписку бот проверит автоматически.",
                    "Ввести одноразовый код",
                    &links.submit_url,
                )
                .await;
        }
        Err(error) => {
            if matches!(&error, gemini_oauth::StartError::Proxy) {
                let _ = store.set_want(chat, "gm_gproxy");
                let _ = bot.send(chat, error.public_message()).await;
            } else {
                let _ = store.set_want(chat, "gm_ready");
                let _ = bot
                    .send_kb(chat, error.public_message(), Some(&gemini_ready_kb()))
                    .await;
            }
        }
    }
}

const GEMINI_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси. Пришли его как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

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
        HandoffKind::Gemini => ("gm_gproxy", "gm_ready"),
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
        "🔗 <b>Этап 3 из 3 — подтверди доступ ChatGPT</b>\n\n\
         1️⃣ Не закрывая подготовленный антидетект-профиль и не меняя прокси, открой ссылку ниже. <b>Не открывай её в Telegram, обычном браузере или на телефоне.</b>\n\n\
         2️⃣ Войди именно в новый аккаунт ChatGPT.\n\n\
         3️⃣ Введи одноразовый код, который указан ниже, и подтверди вход.\n\n\
         4️⃣ Вернись в бот и просто подожди: ничего отправлять сюда не нужно, бот сам увидит подтверждение.\n\n\
         <b>Ссылка OpenAI:</b>\n{url}\n\n<b>Одноразовый код:</b> <code>{code}</code>\n\n\
         ⏱ Код действует 15 минут. Пароль, cookie и коды из email боту не нужны.",
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
/// продавцу. Переводит сценарий на подготовку аккаунта соответствующего продукта. При ошибке —
/// фолбэк на ручной ввод прокси продавцом.
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
            // Store the handover proxy for every kind. Gemini keeps it until the seller confirms
            // that registration and plan activation are complete.
            let _ = store.set_hproxy(seller_chat, &issued_proxy);
            let gemini = next_step == "gm_ready";
            if gemini {
                let _ = store.set_hproxy_order(seller_chat, px.order_id);
            }
            if !gemini {
                let _ = store.set_want(seller_chat, next_step);
            }
            let next_prompt = if gemini {
                "Сохрани эти данные: аккаунт нужно создать и подключить именно через этот прокси. Подробная инструкция придёт следующим сообщением."
            } else {
                account_setup_prompt(next_step)
            };
            let _ = bot
                .send(
                    seller_chat,
                    &format!(
                        "🔑 <b>Прокси выпущен</b> — UK · {city} (HTTP)\n\n\
                 <code>{compact}</code>\nURL: <code>{url}</code>\n\n\
                 {next_prompt}",
                        city = esc(&px.city),
                        compact = esc(&px.compact()),
                        url = esc(&px.url())
                    ),
                )
                .await;
            if gemini {
                prepare_gemini_account(bot, store, seller_chat, None, px.order_id).await;
            }
            let _ = bot.send(admin_chat, &format!(
                "✅ Прокси по офферу #{oid} выпущен (UK · {}, заказ IPRoyal #{}) и отправлен продавцу.",
                esc(&px.city), px.order_id)).await;
        }
        Err(e) => {
            let (proxy_step, _) = handoff_steps(store, oid);
            let _ = store.set_want(seller_chat, proxy_step);
            let prompt = manual_proxy_prompt(proxy_step).to_string();
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

    // Readiness is state-bound: an old or forwarded button cannot skip account preparation.
    if data == "gemini:ready" {
        continue_gemini_handoff(bot, store, cfg, chat).await;
        return;
    }

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
                        let seller_prompt = format!(
                            "💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n{}",
                            esc(&hash),
                            manual_proxy_prompt(proxy_step)
                        );
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
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "✅ <b>Оффер принят!</b>\n\n<b>Сейчас:</b> пришли одним сообщением свой BEP-20 адрес (<code>0x…</code>) для выплаты.\n\n<b>Затем:</b> {}",
                                accepted_next_step(&prod)
                            ),
                        )
                        .await;
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
                            &format!(
                                "✅ <b>Оффер принят!</b> Адрес для выплаты уже сохранён.\n\n⏳ Ожидай подтверждение оплаты. {}",
                                accepted_next_step(&prod)
                            ),
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
        static NEXT_STORE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = format!(
            "{}/authbot_bot_test_{}_{}_{}",
            std::env::temp_dir().display(),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_STORE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
    fn product_menus_show_only_the_two_operator_selected_gemini_plans() {
        let persistent_buttons = admin_home_kb().into_iter().flatten().collect::<Vec<_>>();
        let offer_buttons = product_kb()
            .into_iter()
            .flatten()
            .map(|(label, _)| label)
            .collect::<Vec<_>>();
        let visible = [
            ("📦 Google AI Pro", "Google AI Pro"),
            ("📦 Google AI Ultra", "Google AI Ultra"),
        ];
        for (button, product) in visible {
            assert!(
                persistent_buttons.contains(&button),
                "missing persistent button {button}"
            );
            assert!(offer_buttons.iter().any(|label| label == product));
            assert_eq!(admin_quick_tier(button), Some(product));
            assert_eq!(handoff_kind(product), HandoffKind::Gemini);
        }

        let hidden = [
            ("📦 Code Assist Standard", "Code Assist Standard"),
            ("📦 Code Assist Enterprise", "Code Assist Enterprise"),
            ("📦 Workspace AI Ultra", "Workspace AI Ultra"),
        ];
        for (button, product) in hidden {
            assert!(
                !persistent_buttons.contains(&button),
                "retired persistent button {button} is still visible"
            );
            assert!(!offer_buttons.iter().any(|label| label == product));
            // Old reply keyboards and callbacks remain routable during rollout.
            assert_eq!(admin_quick_tier(button), Some(product));
            assert_eq!(handoff_kind(product), HandoffKind::Gemini);
        }
    }

    #[test]
    fn persistent_admin_keyboard_exposes_both_chatgpt_products() {
        let buttons = admin_home_kb().into_iter().flatten().collect::<Vec<_>>();
        for (button, product) in [
            ("📦 ChatGPT Plus", "ChatGPT Plus"),
            ("📦 ChatGPT Pro", "ChatGPT Pro"),
        ] {
            assert!(buttons.contains(&button), "missing ChatGPT button {button}");
            assert_eq!(admin_quick_tier(button), Some(product));
            assert_eq!(handoff_kind(product), HandoffKind::Codex);
        }
    }

    #[test]
    fn every_subscription_offer_prepares_a_first_time_seller() {
        let store = store();
        let claude_id = store.create_offer("Claude Pro", "$20", 1, 2).unwrap();
        let chatgpt_id = store.create_offer("ChatGPT Plus", "$20", 1, 2).unwrap();
        let gemini_id = store.create_offer("Google AI Pro", "$20", 1, 2).unwrap();
        let claude = offer_text(&store.get_offer(claude_id).unwrap().unwrap());
        let chatgpt = offer_text(&store.get_offer(chatgpt_id).unwrap().unwrap());
        let gemini = offer_text(&store.get_offer(gemini_id).unwrap().unwrap());

        for guide in [&claude, &chatgpt, &gemini] {
            assert!(guide.contains("антидетект-браузере"));
            assert!(guide.contains("персонального HTTP-прокси"));
            assert!(guide.contains("Пароль, cookie, банковские данные"));
            assert!(guide.chars().count() < 3_500, "Telegram offer is too long");
        }
        assert!(claude.contains("Не регистрируй и не открывай аккаунт"));
        assert!(chatgpt.contains("Не регистрируй и не открывай аккаунт"));
        assert!(gemini.contains("Не регистрируй и не открывай Google-аккаунт"));
        assert!(claude.contains("весь адрес из адресной строки"));
        assert!(chatgpt.contains("одноразовый код"));
        assert!(gemini.contains("зарегистрировать новый Google-аккаунт"));
        assert!(gemini.contains("Аккаунт готов"));

        for setup in [
            CLAUDE_ACCOUNT_SETUP,
            CODEX_ACCOUNT_SETUP,
            GEMINI_ACCOUNT_SETUP,
        ] {
            assert!(setup.contains("новый чистый профиль"));
            assert!(setup.contains("https://accounts.google.com"));
            assert!(setup.contains("IP — первое поле"));
            assert!(setup.contains("Дополнительный VPN не включай"));
            assert!(setup.chars().count() < 3_500, "Telegram prompt is too long");
        }
        assert!(CLAUDE_ACCOUNT_SETUP.contains("Continue with Google"));
        assert!(CODEX_ACCOUNT_SETUP.contains("Continue with Google"));
        assert!(CLAUDE_ACCOUNT_SETUP.contains("точный email"));
        assert!(CODEX_ACCOUNT_SETUP.contains("точный email"));
        assert!(CLAUDE_ACCOUNT_SETUP.contains("https://claude.ai"));
        assert!(CODEX_ACCOUNT_SETUP.contains("https://chatgpt.com"));
        assert!(GEMINI_ACCOUNT_SETUP.contains("Google AI Pro или Ultra"));
        assert!(GEMINI_ACCOUNT_SETUP.contains("https://one.google.com"));
        assert!(GEMINI_ACCOUNT_SETUP.contains("Аккаунт готов — продолжить"));
        assert!(CLAUDE_MANUAL_PROXY.contains("аккаунта Claude"));
        assert!(CODEX_MANUAL_PROXY.contains("аккаунта ChatGPT"));
        assert!(GEMINI_MANUAL_PROXY.contains("аккаунта Gemini"));
        for fallback in [CLAUDE_MANUAL_PROXY, CODEX_MANUAL_PROXY, GEMINI_MANUAL_PROXY] {
            assert!(fallback.contains("регистрация и дальнейшая авторизация"));
            assert!(fallback.contains("ip:port:user:pass"));
        }
    }

    #[test]
    fn seller_copy_contains_actions_not_internal_implementation_notes() {
        assert_eq!(PRODUCT_PICK, "📦 <b>Создание оффера</b>\nВыбери продукт:");
        let seller_copy = [
            PRODUCT_PICK,
            CLAUDE_OFFER_GUIDE,
            CODEX_OFFER_GUIDE,
            GEMINI_OFFER_GUIDE,
            CLAUDE_ACCOUNT_SETUP,
            CODEX_ACCOUNT_SETUP,
            GEMINI_ACCOUNT_SETUP,
            CLAUDE_MANUAL_PROXY,
            CODEX_MANUAL_PROXY,
            GEMINI_MANUAL_PROXY,
            GEMINI_PROXY_PROMPT,
            accepted_next_step("Google AI Pro"),
        ];
        for copy in seller_copy {
            for internal_term in [
                "OAuth-клиент",
                "Cloud API",
                "Client ID",
                "Client secret",
                "managed project",
                "consumer project",
                "roster",
            ] {
                assert!(
                    !copy.contains(internal_term),
                    "seller copy contains internal term {internal_term}: {copy}"
                );
            }
        }
    }

    #[test]
    fn gemini_ready_button_requires_the_right_state_and_a_stored_proxy() {
        let store = store();
        let chat = 42;
        store.register_user(chat, chat, "gemini-seller").unwrap();
        store
            .set_hproxy(chat, "http://user:pass@1.2.3.4:8080")
            .unwrap();
        store.set_hproxy_order(chat, 17).unwrap();

        store.set_want(chat, "gm_gproxy").unwrap();
        assert!(gemini_ready_handoff(&store, chat).is_none());
        store.set_want(chat, "gm_ready").unwrap();
        assert_eq!(
            gemini_ready_handoff(&store, chat),
            Some(("http://user:pass@1.2.3.4:8080".into(), 17))
        );
        store.set_hproxy(chat, "").unwrap();
        assert!(gemini_ready_handoff(&store, chat).is_none());

        let keyboard = gemini_ready_kb();
        assert_eq!(keyboard[0][0].0, "✅ Аккаунт готов — продолжить");
        assert_eq!(keyboard[0][0].1, "gemini:ready");
    }

    #[test]
    fn handoff_steps_follow_the_offer_product() {
        let store = store();
        let claude = store.create_offer("Claude 20x", "$100", 1, 2).unwrap();
        let chatgpt = store.create_offer("ChatGPT Pro", "$200", 1, 2).unwrap();
        let gemini = store.create_offer("Google AI Ultra", "$300", 1, 2).unwrap();
        assert_eq!(handoff_steps(&store, claude), ("ho_proxy", "ho_email"));
        assert_eq!(handoff_steps(&store, chatgpt), ("cx_proxy", "cx_email"));
        assert_eq!(handoff_steps(&store, gemini), ("gm_gproxy", "gm_ready"));
        assert_eq!(handoff_steps(&store, 9_999), ("ho_proxy", "ho_email"));
    }

    /// Шаги трёх веток не должны пересекаться: одно и то же состояние в обеих отправило бы
    /// продавца в чужой обработчик после перезапуска бота.
    #[test]
    fn the_three_handovers_never_share_a_step_name() {
        let claude = ["ho_proxy", "ho_email", "ho_code"];
        let codex = ["cx_proxy", "cx_email"];
        let gemini = ["gm_gid", "gm_gsecret", "gm_gproxy", "gm_ready", "gm_wait"];
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
