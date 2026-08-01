//! Логика бота: команды, машина создания оффера, флоу продавца. Состояние — в SQLite (db).
//! Выплаты (Фаза 2) и выпуск setup-token (Фаза 3) пока заглушены — помечены TODO.

use crate::db::{AdminState, PurchaseBatch, SellerJob, SellerJobRef, Store};
use crate::gemini_oauth;
use crate::setup_token::{self, Outcome};
use crate::tg::{Bot, CallbackQuery, Keyboard};
use crate::Config;
use std::sync::Arc;

pub const WELCOME_NEW: &str =
    "👋 <b>Привет!</b>\nЭто бот закупки. Хочешь продавать — жми кнопку ниже, заявка уйдёт на модерацию.";
pub const ADMIN_HOME: &str = "🛠 <b>Дев-панель</b>\n\nБыстрая покупка: жми продукт на нижней \
    клавиатуре → выбери продавца, цену и источник прокси. Для нескольких подписок используй \
    🧺 Batch-покупку: продавец обработает позиции по очереди, оплата будет одной транзакцией.";

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

const PROXY_SOURCE_BUYER: &str = "buyer";
const PROXY_SOURCE_SELLER: &str = "seller";
const PROXY_SOURCE_LEGACY: &str = "legacy";

fn proxy_source_kb(prefix: &str) -> Keyboard {
    vec![
        vec![(
            "🧩 Мои прокси (я передам)".into(),
            format!("{prefix}:buyer"),
        )],
        vec![("👤 Прокси продавца".into(), format!("{prefix}:seller"))],
    ]
}

fn proxy_source_label(source: &str) -> &'static str {
    match source {
        PROXY_SOURCE_BUYER => "от покупателя",
        PROXY_SOURCE_SELLER => "от продавца",
        _ => "по старому сценарию",
    }
}

fn seller_job_label(job: &SellerJob) -> String {
    let phase = match job.phase.as_str() {
        "accepted" => "принято, ожидает оплаты",
        "paying" => "проверяется выплата",
        _ => "передача доступа",
    };
    if job.reference.kind == "batch" {
        if job.reference.item_no > 0 {
            format!(
                "🧺 Batch #{} · позиция {}/{} · {} · {}",
                job.reference.batch_id,
                job.reference.item_no,
                job.total,
                esc(&job.product),
                phase
            )
        } else {
            format!(
                "🧺 Batch #{} · {} подписок · {} · {}",
                job.reference.batch_id,
                job.total,
                esc(&job.product),
                phase
            )
        }
    } else {
        format!(
            "📦 Оффер #{} · {} · {}",
            job.reference.offer_id,
            esc(&job.product),
            phase
        )
    }
}

fn seller_busy_text(job: &SellerJob) -> String {
    format!(
        "⛔ <b>Нельзя смешивать две сделки.</b>\n\nСейчас активна:\n<b>{}</b>\n\nСначала заверши её. После этого можно принять и запустить следующую.",
        seller_job_label(job)
    )
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
pub(crate) enum HandoffKind {
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
        vec!["🧺 Batch-покупка"],
        vec!["📋 Активные сделки"],
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

fn active_job_kb(job: &SellerJob) -> Option<Keyboard> {
    if job.reference.kind != "batch" || job.phase != "processing" || job.reference.item_no <= 1 {
        return None;
    }
    Some(vec![vec![(
        format!(
            "↩️ Вернуть позицию {}/{}",
            job.reference.item_no - 1,
            job.total
        ),
        format!(
            "batchrewind:{}:{}:ask",
            job.reference.batch_id,
            job.reference.item_no - 1
        ),
    )]])
}

async fn show_active_jobs(bot: &Bot, store: &Store, chat: i64) {
    let jobs = store.active_seller_jobs().unwrap_or_default();
    if jobs.is_empty() {
        let _ = bot
            .send(
                chat,
                "📋 <b>Активных сделок нет.</b> Все продавцы свободны.",
            )
            .await;
        return;
    }
    let _ = bot
        .send(
            chat,
            &format!(
                "📋 <b>Активные сделки: {}</b>\nSingle и batch у одного продавца никогда не выполняются одновременно.",
                jobs.len()
            ),
        )
        .await;
    for job in jobs {
        let keyboard = active_job_kb(&job);
        let _ = bot
            .send_kb(
                chat,
                &format!(
                    "{}\nПродавец: {}",
                    seller_job_label(&job),
                    seller_label(store, job.seller_chat)
                ),
                keyboard.as_ref(),
            )
            .await;
    }
}

fn batch_product_kb() -> Keyboard {
    product_kb()
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(label, data)| {
                    let code = data.strip_prefix("noffer:").unwrap_or_default();
                    (label, format!("nbatch:{code}"))
                })
                .collect()
        })
        .collect()
}

const BATCH_PRODUCT_PICK: &str =
    "🧺 <b>Batch-покупка</b>\nВыбери вариант подписки для всех позиций:";
const BATCH_QUANTITY_PROMPT: &str = "Сколько подписок купить одним batch? Пришли целое число от <code>2</code> до <code>100</code>. /cancel — отмена.";
const BATCH_PROXY_PROMPT: &str = "Пришли прокси для позиции <b>{item}</b> из <b>{total}</b> одним сообщением: <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>. Прокси сохранится только в защищённой БД и будет передан продавцу после общей оплаты.";

async fn start_batch_product_pick(bot: &Bot, chat: i64) {
    let _ = bot
        .send_kb(chat, BATCH_PRODUCT_PICK, Some(&batch_product_kb()))
        .await;
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

fn accepted_next_step(product: &str, proxy_source: &str) -> &'static str {
    if proxy_source == PROXY_SOURCE_SELLER {
        return "После подтверждения выплаты бот попросит твой персональный прокси, затем даст подробную инструкцию. <b>До этого не создавай и не открывай аккаунт.</b>";
    }
    match handoff_kind(product) {
        HandoffKind::Claude => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай Claude-аккаунт.</b>",
        HandoffKind::Codex => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай ChatGPT-аккаунт.</b>",
        HandoffKind::Gemini => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай Google-аккаунт.</b>",
    }
}

fn offer_text(o: &crate::db::Offer) -> String {
    let guide = seller_offer_guide_for(&o.product, &o.proxy_source);
    format!(
        "📦 <b>Оффер #{}</b>\nПродукт: <b>{}</b>\nЦена: <b>{}</b>\nПрокси: <b>{}</b>{}",
        o.id,
        esc(&o.product),
        esc(&o.price),
        proxy_source_label(&o.proxy_source),
        if guide.is_empty() {
            String::new()
        } else {
            format!("\n\n{guide}")
        }
    )
}

fn seller_offer_guide_for(product: &str, proxy_source: &str) -> String {
    let guide = seller_offer_guide(product);
    let first_step = if proxy_source == PROXY_SOURCE_SELLER {
        "1. После подтверждения batch/оффера и выплаты прислать свой HTTP-прокси. Бот проверит его и закрепит за этим аккаунтом."
    } else {
        "1. Дождаться выплаты и персонального HTTP-прокси от покупателя/бота."
    };
    guide.replacen(
        "1. Дождаться выплаты и персонального HTTP-прокси от бота.",
        first_step,
        1,
    )
}

fn batch_offer_text(batch: &PurchaseBatch) -> String {
    let guide = seller_offer_guide_for(&batch.product, &batch.proxy_source);
    format!(
        "🧺 <b>Batch #{}</b>\nПродукт: <b>{}</b>\nКоличество: <b>{}</b>\nЦена за 1: <b>{}</b>\nИтого к выплате: <b>{}</b>\nПрокси: <b>{}</b>\n\n{}",
        batch.id,
        esc(&batch.product),
        batch.quantity,
        esc(&batch.unit_price),
        esc(&batch.total_price),
        proxy_source_label(&batch.proxy_source),
        guide
    )
}

fn batch_offer_kb(batch_id: i64) -> Keyboard {
    vec![vec![
        (
            "✅ Принять batch".into(),
            format!("batch:{batch_id}:accept"),
        ),
        ("❌ Отклонить".into(), format!("batch:{batch_id}:reject")),
    ]]
}

async fn send_batch_to(bot: &Bot, store: &Store, batch_id: i64, seller_chat: i64) -> bool {
    let batch = match store.get_batch(batch_id) {
        Ok(Some(batch)) => batch,
        _ => return false,
    };
    bot.send_kb(
        seller_chat,
        &batch_offer_text(&batch),
        Some(&batch_offer_kb(batch_id)),
    )
    .await
    .is_ok()
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
fn seller_pick_kb(store: &Store, sellers: &[crate::db::UserRow]) -> Keyboard {
    sellers
        .iter()
        .map(|s| {
            let mut label = if s.username.is_empty() {
                format!("id {}", s.uid)
            } else {
                format!("@{}", s.username)
            };
            if let Some(job) = store.active_seller_job(s.chat_id).ok().flatten() {
                let active = if job.reference.kind == "batch" {
                    if job.reference.item_no > 0 {
                        format!(
                            "batch #{} {}/{}",
                            job.reference.batch_id, job.reference.item_no, job.total
                        )
                    } else {
                        format!("batch #{} — ждёт оплаты", job.reference.batch_id)
                    }
                } else {
                    format!("оффер #{}", job.reference.offer_id)
                };
                label = format!("🟠 {label} — занят: {active}");
            } else {
                label = format!("🟢 {label} — свободен");
            }
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
            Some(&seller_pick_kb(store, &sellers)),
        )
        .await;
}

/// Batch-оффер проходит через того же выбранного продавца, но сохраняет отдельный режим wizard.
async fn start_batch_seller_pick(bot: &Bot, store: &Store, chat: i64, product: &str) {
    let sellers = store.by_status("approved").unwrap_or_default();
    if sellers.is_empty() {
        let _ = store.clear_admin_state(chat);
        let _ = bot
            .send(
                chat,
                "Пока нет одобренных продавцов — batch некому направить. Одобри заявку и повтори.",
            )
            .await;
        return;
    }
    let _ = store.set_admin_flow(&AdminState {
        chat_id: chat,
        step: "batch_seller".into(),
        product: product.into(),
        seller_chat: 0,
        mode: "batch".into(),
        quantity: 0,
        unit_price: String::new(),
        proxy_source: String::new(),
        draft_proxies: Vec::new(),
    });
    let _ = bot
        .send_kb(
            chat,
            &format!(
                "🧺 Продукт: <b>{}</b>\n\nКому отправить batch? Выбери продавца:",
                esc(product)
            ),
            Some(&seller_pick_kb(store, &sellers)),
        )
        .await;
}

async fn publish_single_offer(
    bot: &Bot,
    store: &Store,
    admin_chat: i64,
    product: &str,
    price: &str,
    seller_chat: i64,
    proxy_source: &str,
    buyer_proxy: &str,
) {
    if let Some(job) = store.active_seller_job(seller_chat).ok().flatten() {
        let _ = bot
            .send(
                admin_chat,
                &format!(
                    "🟠 Оффер не создан: продавец успел занять другую сделку.\n\n<b>{}</b>",
                    seller_job_label(&job)
                ),
            )
            .await;
        return;
    }
    let oid = match store.create_offer_with_proxy(
        product,
        price,
        admin_chat,
        seller_chat,
        proxy_source,
        buyer_proxy,
    ) {
        Ok(oid) => oid,
        Err(error) => {
            let _ = bot
                .send(
                    admin_chat,
                    &format!("❌ Не удалось создать оффер: {}", esc(&error.to_string())),
                )
                .await;
            return;
        }
    };
    let who = seller_label(store, seller_chat);
    let delivered = send_offer_to(bot, store, oid, seller_chat).await;
    let offer = store
        .get_offer(oid)
        .ok()
        .flatten()
        .map(|offer| offer_text(&offer))
        .unwrap_or_default();
    let message = if delivered {
        format!("✅ <b>Оффер #{oid} отправлен продавцу {who}.</b>\n\n{offer}")
    } else {
        format!(
            "⚠️ Оффер #{oid} создан, но доставить продавцу {who} не удалось (возможно, он не открывал бота).\n\n{offer}"
        )
    };
    let _ = bot.send(admin_chat, &message).await;
}

async fn publish_batch(
    bot: &Bot,
    store: &Store,
    admin_chat: i64,
    product: &str,
    unit_price: &str,
    quantity: i64,
    total_price: &str,
    seller_chat: i64,
    proxy_source: &str,
    proxies: &[String],
) {
    if let Some(job) = store.active_seller_job(seller_chat).ok().flatten() {
        let _ = bot
            .send(
                admin_chat,
                &format!(
                    "🟠 Batch не создан: продавец успел занять другую сделку.\n\n<b>{}</b>",
                    seller_job_label(&job)
                ),
            )
            .await;
        return;
    }
    let batch_id = match store.create_batch(
        product,
        unit_price,
        quantity,
        total_price,
        admin_chat,
        seller_chat,
        proxy_source,
        proxies,
    ) {
        Ok(id) => id,
        Err(error) => {
            let _ = bot
                .send(
                    admin_chat,
                    &format!("❌ Не удалось создать batch: {}", esc(&error.to_string())),
                )
                .await;
            return;
        }
    };
    let who = seller_label(store, seller_chat);
    let delivered = send_batch_to(bot, store, batch_id, seller_chat).await;
    let batch = store.get_batch(batch_id).ok().flatten();
    let summary = batch.as_ref().map(batch_offer_text).unwrap_or_default();
    let message = if delivered {
        format!("✅ <b>Batch #{batch_id} отправлен продавцу {who}.</b>\n\n{summary}")
    } else {
        format!(
            "⚠️ Batch #{batch_id} создан, но доставить его продавцу {who} не удалось (возможно, он не открывал бота).\n\n{summary}"
        )
    };
    let _ = bot.send(admin_chat, &message).await;
}

fn batch_pay_kb(batch_id: i64) -> Keyboard {
    vec![vec![(
        "💸 Оплатить batch целиком".into(),
        format!("batchpay:{batch_id}"),
    )]]
}

fn batch_payment_review_kb(batch_id: i64) -> Keyboard {
    vec![vec![(
        "🔁 Проверил: разрешить повторную оплату".into(),
        format!("batchpayretry:{batch_id}"),
    )]]
}

fn offer_payment_review_kb(offer_id: i64) -> Keyboard {
    vec![vec![(
        "🔁 Проверил: разрешить повторную оплату".into(),
        format!("offerpayretry:{offer_id}"),
    )]]
}

async fn notify_batch_payment_ready(
    bot: &Bot,
    cfg: &Config,
    batch: &PurchaseBatch,
    seller: &crate::db::UserRow,
) {
    notify_admins(
        bot,
        cfg,
        &format!(
            "✅ <b>Batch #{} принят продавцом.</b>\n{} × {}\nИтого: <b>{}</b> USDT\nАдрес: <code>{}</code>\nПрокси: {}",
            batch.id,
            batch.quantity,
            esc(&batch.product),
            esc(&batch.total_price),
            esc(&seller.address),
            proxy_source_label(&batch.proxy_source)
        ),
        Some(&batch_pay_kb(batch.id)),
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
        if text == "🧺 Batch-покупка" {
            start_batch_product_pick(bot, chat).await;
            return;
        }
        if text == "📋 Активные сделки" {
            show_active_jobs(bot, store, chat).await;
            return;
        }
        if text == "🛠 Панель" {
            show_admin_home(bot, store, chat).await;
            return;
        }
    }

    // машина создания оффера (persisted) — только админ, не команда
    if admin && !text.starts_with('/') {
        if let Ok(Some(mut flow)) = store.get_admin_flow(chat) {
            match flow.step.as_str() {
                "price" => {
                    let amount = match parse_amount(text) {
                        Some(a) if a.is_finite() && a > 0.0 => a,
                        _ => {
                            let _ = bot.send(chat, "Нужна <b>сумма в долларах</b> числом \
                                (например <code>20</code> или <code>15.5</code>). Пришли ещё раз или /cancel.").await;
                            return;
                        }
                    };
                    flow.unit_price = fmt_usd(amount);
                    flow.step = "proxy_source".into();
                    let _ = store.set_admin_flow(&flow);
                    let _ = bot.send_kb(
                        chat,
                        &format!(
                            "📦 <b>{}</b>\nПродавец выбран. Цена: <b>{}</b> за подписку.\n\nЧьи прокси использовать?",
                            esc(&flow.product), esc(&flow.unit_price)
                        ),
                        Some(&proxy_source_kb("proxy")),
                    ).await;
                    return;
                }
                "single_proxy" => {
                    let purl = proxy_url(text);
                    if purl.is_empty() {
                        let _ = bot.send(chat, "Не разобрал прокси. Пришли его в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
                        return;
                    }
                    let product = flow.product.clone();
                    let price = flow.unit_price.clone();
                    let seller_chat = flow.seller_chat;
                    let source = flow.proxy_source.clone();
                    let _ = store.clear_admin_state(chat);
                    publish_single_offer(
                        bot,
                        store,
                        chat,
                        &product,
                        &price,
                        seller_chat,
                        &source,
                        &purl,
                    )
                    .await;
                    return;
                }
                "batch_quantity" => {
                    let quantity = match parse_quantity(text) {
                        Some(value) if (2..=100).contains(&value) => value,
                        _ => {
                            let _ = bot.send(chat, BATCH_QUANTITY_PROMPT).await;
                            return;
                        }
                    };
                    flow.quantity = quantity;
                    flow.step = "batch_price".into();
                    let _ = store.set_admin_flow(&flow);
                    let _ = bot.send(chat, &format!(
                        "🧺 Batch из <b>{quantity}</b> подписок «{}».\n\nПришли цену <b>за одну подписку</b> в долларах (например <code>20</code>). Итого будет рассчитано автоматически. /cancel — отмена.",
                        esc(&flow.product)
                    )).await;
                    return;
                }
                "batch_price" => {
                    let amount = match parse_amount(text) {
                        Some(a) if a.is_finite() && a > 0.0 => a,
                        _ => {
                            let _ = bot.send(chat, "Нужна <b>цена за одну подписку</b> в долларах числом (например <code>20</code> или <code>15.5</code>). Пришли ещё раз или /cancel.").await;
                            return;
                        }
                    };
                    flow.unit_price = fmt_usd(amount);
                    flow.step = "batch_proxy_source".into();
                    let _ = store.set_admin_flow(&flow);
                    let normalized_amount = parse_amount(&flow.unit_price).unwrap_or(amount);
                    let total = fmt_usd(normalized_amount * flow.quantity as f64);
                    let _ = bot.send_kb(
                        chat,
                        &format!(
                            "🧺 <b>{} × {}</b>\nЗа 1: <b>{}</b>\nИтого: <b>{}</b>\n\nЧьи прокси использовать для каждой позиции?",
                            flow.quantity, esc(&flow.product), esc(&flow.unit_price), total
                        ),
                        Some(&proxy_source_kb("batchproxy")),
                    ).await;
                    return;
                }
                "batch_proxy_source" => {}
                "batch_proxies" => {
                    let purl = proxy_url(text);
                    if purl.is_empty() {
                        let item = flow.draft_proxies.len() as i64 + 1;
                        let _ = bot
                            .send(
                                chat,
                                &format!(
                                    "Не разобрал прокси.\n\n{}",
                                    BATCH_PROXY_PROMPT
                                        .replace("{item}", &item.to_string())
                                        .replace("{total}", &flow.quantity.to_string())
                                ),
                            )
                            .await;
                        return;
                    }
                    flow.draft_proxies.push(purl);
                    if flow.draft_proxies.len() < flow.quantity as usize {
                        let item = flow.draft_proxies.len() as i64 + 1;
                        let _ = store.set_admin_flow(&flow);
                        let _ = bot
                            .send(
                                chat,
                                &BATCH_PROXY_PROMPT
                                    .replace("{item}", &item.to_string())
                                    .replace("{total}", &flow.quantity.to_string()),
                            )
                            .await;
                        return;
                    }
                    let product = flow.product.clone();
                    let unit_price = flow.unit_price.clone();
                    let quantity = flow.quantity;
                    let seller_chat = flow.seller_chat;
                    let source = flow.proxy_source.clone();
                    let proxies = flow.draft_proxies.clone();
                    let total = parse_amount(&unit_price)
                        .map(|amount| fmt_usd(amount * quantity as f64))
                        .unwrap_or_default();
                    let _ = store.clear_admin_state(chat);
                    publish_batch(
                        bot,
                        store,
                        chat,
                        &product,
                        &unit_price,
                        quantity,
                        &total,
                        seller_chat,
                        &source,
                        &proxies,
                    )
                    .await;
                    return;
                }
                _ => {}
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
                let cancelled = store
                    .active_seller_job(chat)
                    .ok()
                    .flatten()
                    .filter(|job| {
                        seller_job_matches_handoff(job, &job.reference, HandoffKind::Gemini)
                    })
                    .is_some_and(|job| {
                        store
                            .cancel_gemini_oauth(chat, Some(&job.reference))
                            .unwrap_or(false)
                    });
                let _ = if cancelled {
                    bot.send(
                        chat,
                        &format!("Авторизация отменена.\n\n{GEMINI_PROXY_PROMPT}"),
                    )
                    .await
                } else {
                    bot.send(
                        chat,
                        "Эта авторизация уже не относится к активной сделке. Отправь /start.",
                    )
                    .await
                };
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
                } else if let Some(job) = store.active_seller_job(chat).ok().flatten() {
                    let next = match job.phase.as_str() {
                        "accepted" => "Сделка принята и закреплена за тобой. Ожидай одну выплату; после неё бот сам пришлёт первый шаг.",
                        "paying" => "Сейчас ничего не отправляй: администратор проверяет выплату. После подтверждения бот сам пришлёт следующий шаг.",
                        _ => "Продолжай по последней инструкции бота. Новая сделка не запустится, пока эта не завершена.",
                    };
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "👋 <b>Ты продавец.</b>\n\nСейчас активна:\n<b>{}</b>\n\n{}\n\n💼 Адрес выплат:\n<code>{}</code>",
                                seller_job_label(&job),
                                next,
                                esc(&rec.address)
                            ),
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
    if admin && text == "/batch" {
        start_batch_product_pick(bot, chat).await;
        return;
    }
    if admin && text == "/jobs" {
        show_active_jobs(bot, store, chat).await;
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
                    for batch in store.accepted_batches_for_seller(chat).unwrap_or_default() {
                        if let Some(seller) = store.get_user(chat).ok().flatten() {
                            notify_batch_payment_ready(bot, cfg, &batch, &seller).await;
                        }
                    }
                    for offer in store.accepted_offers_for_seller(chat).unwrap_or_default() {
                        let pay_kb: Keyboard = vec![vec![(
                            "💸 Оплатить".into(),
                            format!("pay:{}:{}", offer.id, chat),
                        )]];
                        notify_admins(
                            bot,
                            cfg,
                            &format!(
                                "✅ Адрес продавца сохранён для оффера #{} «{}».\nАдрес: <code>{}</code>",
                                offer.id,
                                esc(&offer.product),
                                esc(text)
                            ),
                            Some(&pay_kb),
                        )
                        .await;
                    }
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
                    let applied = store
                        .active_seller_job(chat)
                        .ok()
                        .flatten()
                        .filter(|job| {
                            seller_job_matches_handoff(job, &job.reference, HandoffKind::Claude)
                        })
                        .is_some_and(|job| {
                            store
                                .set_handoff_state_for_seller_job(
                                    chat,
                                    &job.reference,
                                    "ho_email",
                                    &purl,
                                    0,
                                )
                                .unwrap_or(false)
                        });
                    if applied {
                        let _ = bot
                            .send(chat, &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{CLAUDE_ACCOUNT_SETUP}"))
                            .await;
                    }
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
                } else {
                    do_start_token(bot, store, cfg, chat, text, &rec.hproxy, false).await;
                }
            }
            "cx_proxy" => {
                let purl = proxy_url(text);
                if purl.is_empty() {
                    let _ = bot.send(chat, "🤔 Не разобрал прокси. Пришли его одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
                } else {
                    let applied = store
                        .active_seller_job(chat)
                        .ok()
                        .flatten()
                        .filter(|job| {
                            seller_job_matches_handoff(job, &job.reference, HandoffKind::Codex)
                        })
                        .is_some_and(|job| {
                            store
                                .set_handoff_state_for_seller_job(
                                    chat,
                                    &job.reference,
                                    "cx_email",
                                    &purl,
                                    0,
                                )
                                .unwrap_or(false)
                        });
                    if applied {
                        let _ = bot
                            .send(chat, &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{CODEX_ACCOUNT_SETUP}"))
                            .await;
                    }
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
                } else if let Some((batch, item)) = active_batch_item(store, chat)
                    .filter(|(batch, _)| batch.proxy_source == PROXY_SOURCE_BUYER)
                {
                    // OAuth failure must retry with the buyer-selected egress, never silently
                    // switch a buyer-proxy batch to a seller-proxy flow.
                    let _ = bot.send(chat, &format!(
                        "🔁 Повторяю позицию <b>{}/{}</b> batch #{} через тот же прокси покупателя.",
                        item.item_no, batch.quantity, batch.id
                    )).await;
                    prepare_gemini_account(bot, store, chat, Some(&item.proxy), 0).await;
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
                        "Авторизация уже ждёт localhost callback. Заверши вход по первой ссылке, скопируй полный адрес из адресной строки, затем открой кнопку «Завершить подключение» и вставь URL в защищённую форму. В Telegram его не присылай. /cancel начнёт заново.",
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
        do_start_token(bot, store, cfg, chat, text, "", true).await;
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

fn parse_quantity(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
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
        let mut auth = authority.connect_with_application_name("claude-authbot")?;
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

async fn do_start_token(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    email: &str,
    proxy: &str,
    standalone: bool,
) -> bool {
    let expected_job = if standalone {
        None
    } else {
        let Some(job) = store.active_seller_job(chat).ok().flatten() else {
            let _ = bot
                .send(
                    chat,
                    "Эта инструкция уже не относится к активной сделке. Отправь /start.",
                )
                .await;
            return false;
        };
        if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Claude) {
            let _ = bot
                .send(
                    chat,
                    "Эта Claude-инструкция устарела. Открой актуальную сделку через /start.",
                )
                .await;
            return false;
        }
        let Some(rotated) = store
            .rotate_seller_job_token(chat, &job.reference)
            .ok()
            .flatten()
        else {
            let _ = bot
                .send(
                    chat,
                    "Активная сделка изменилась. Открой актуальную карточку через /start.",
                )
                .await;
            return false;
        };
        Some(rotated)
    };
    let (cb, config_dir, em, px) = (
        cfg.claude_bin.clone(),
        cfg.claude_config_dir.clone(),
        email.trim().to_string(),
        proxy.to_string(),
    );
    let session_job = expected_job.clone();
    let _ = bot.send(chat, "⏳ Готовлю авторизацию Claude…").await;
    match tokio::task::spawn_blocking(move || {
        setup_token::start(chat, &em, &px, &cb, &config_dir, session_job)
    })
    .await
    {
        Ok(Ok(url)) => {
            if let Some(expected) = expected_job.as_ref() {
                if !store
                    .set_want_for_seller_job(chat, expected, "ho_code")
                    .unwrap_or(false)
                {
                    setup_token::kill(chat);
                    return false;
                }
            }
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
        Ok(Ok(Outcome::Token(tok, email, proxy, expected_job))) => {
            match register_sub(cfg, &email, &tok, &proxy).await {
                Ok(_) => {
                    let current = expected_job.is_none()
                        || seller_handoff_is_current(
                            store,
                            chat,
                            expected_job.as_ref(),
                            HandoffKind::Claude,
                        );
                    if current {
                        let _ = bot.send(chat, &format!(
                        "✅ <b>Готово!</b> Доступ передан, подписка <code>{}</code> в системе. Спасибо за сделку! 🤝", esc(&email))).await;
                        notify_admins(bot, cfg, &format!(
                            "✅ <b>Claude-доступ получен</b>: аккаунт <code>{}</code> добавлен в пул (прокси: {}).",
                            esc(&email), if proxy.is_empty() { "нет" } else { "есть" }), None).await;
                    }
                    complete_seller_job_after_handoff(
                        bot,
                        store,
                        cfg,
                        chat,
                        expected_job,
                        HandoffKind::Claude,
                    )
                    .await;
                }
                Err(e) => {
                    let retry_is_current = expected_job.as_ref().map_or(true, |expected| {
                        store
                            .set_want_for_seller_job(chat, expected, "ho_email")
                            .unwrap_or(false)
                    });
                    if retry_is_current {
                        let _ = bot.send(chat,
                        "⚠️ Доступ получен, но добавить аккаунт не удалось. Пришли <b>email</b> заново — повторим вход.").await;
                    }
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
        Ok(Ok(Outcome::BadCode(expected_job))) => {
            let retry_is_current = expected_job.as_ref().map_or(true, |expected| {
                store
                    .set_want_for_seller_job(chat, expected, "ho_email")
                    .unwrap_or(false)
            });
            if retry_is_current {
                let _ = bot.send(chat,
                "❌ Код отклонён (неверный/истёк). Пришли <b>email</b> аккаунта заново — дам свежую ссылку.").await;
            }
        }
        Ok(Ok(Outcome::NoToken(expected_job))) => {
            let retry_is_current = expected_job.as_ref().map_or(true, |expected| {
                store
                    .set_want_for_seller_job(chat, expected, "ho_email")
                    .unwrap_or(false)
            });
            if retry_is_current {
                let _ = bot
                    .send(
                        chat,
                        "❌ Авторизация не завершилась вовремя. Пришли <b>email</b> заново.",
                    )
                    .await;
            }
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
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    let expected_job = job.job_ref();
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Gemini) {
        return;
    }
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let effective_proxy = proxy.unwrap_or(&user.hproxy);
    let effective_order = if proxy_order_id > 0 {
        proxy_order_id
    } else {
        user.hproxy_order
    };
    if effective_proxy.is_empty() {
        if !store
            .set_handoff_state_for_seller_job(chat, &expected_job, "gm_gproxy", "", effective_order)
            .unwrap_or(false)
        {
            return;
        }
        let _ = bot.send(chat, GEMINI_PROXY_PROMPT).await;
        return;
    }
    if !store
        .set_handoff_state_for_seller_job(
            chat,
            &expected_job,
            "gm_ready",
            effective_proxy,
            effective_order,
        )
        .unwrap_or(false)
    {
        return;
    }
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

/// Начать Antigravity OAuth после закрепления постоянного прокси и явного подтверждения продавца.
/// Google перенаправляет браузер на зарегистрированный localhost callback; продавец копирует его
/// полный URL в защищённую HTTPS-форму, а Telegram не получает ни URL, ни токен.
async fn start_gemini_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже не относится к активной сделке. Отправь /start.",
            )
            .await;
        return;
    };
    let expected_job = job.job_ref();
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Gemini) {
        let _ = bot
            .send(chat, "Текущая сделка не является Gemini-сделкой. Открой актуальную карточку через /start.")
            .await;
        return;
    }
    let Some(oauth) = cfg.gemini_oauth.as_ref() else {
        let _ = store.set_want_for_seller_job(chat, &expected_job, "gm_ready");
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
        let _ = store.set_want_for_seller_job(chat, &expected_job, "gm_gproxy");
        let _ = bot.send(chat, GEMINI_PROXY_PROMPT).await;
        return;
    }
    match gemini_oauth::begin(store, oauth, chat, &proxy, proxy_order_id) {
        Ok(links) => {
            if !seller_handoff_is_current(store, chat, links.job.as_ref(), HandoffKind::Gemini) {
                return;
            }
            let _ = bot
                .send_url_button(
                    chat,
                    "🔗 <b>Этап 3 из 3 — подтверди доступ Gemini</b>\n\n1️⃣ Не закрывая подготовленный антидетект-профиль и не меняя прокси, открой официальную ссылку ниже. <b>Не открывай её в Telegram, обычном браузере или на телефоне.</b>\n\n2️⃣ Войди именно в новый Google-аккаунт и подтверди доступ. Google перенаправит браузер на <code>localhost:51121</code>. Страница может не открыться — это нормально.",
                    "Авторизовать через Antigravity",
                    &links.authorize_url,
                )
                .await;
            let _ = bot
                .send_url_button(
                    chat,
                    "3️⃣ Скопируй <b>весь localhost URL</b> из адресной строки, нажми кнопку ниже и вставь его в защищённую форму. Не отправляй URL сообщением в Telegram.\n\n4️⃣ После отправки просто вернись в бот: активную подписку бот проверит автоматически.",
                    "Завершить подключение",
                    &links.submit_url,
                )
                .await;
        }
        Err(error) => {
            if matches!(&error, gemini_oauth::StartError::Proxy) {
                if store
                    .set_handoff_state_for_seller_job(
                        chat,
                        &expected_job,
                        "gm_gproxy",
                        "",
                        proxy_order_id,
                    )
                    .unwrap_or(false)
                {
                    let _ = bot.send(chat, error.public_message()).await;
                }
            } else {
                if store
                    .set_want_for_seller_job(chat, &expected_job, "gm_ready")
                    .unwrap_or(false)
                {
                    let _ = bot
                        .send_kb(chat, error.public_message(), Some(&gemini_ready_kb()))
                        .await;
                }
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
    handoff_steps_for_kind(kind)
}

fn handoff_steps_for_product(product: &str) -> (&'static str, &'static str) {
    handoff_steps_for_kind(handoff_kind(product))
}

fn handoff_steps_for_kind(kind: HandoffKind) -> (&'static str, &'static str) {
    match kind {
        HandoffKind::Claude => ("ho_proxy", "ho_email"),
        HandoffKind::Codex => ("cx_proxy", "cx_email"),
        HandoffKind::Gemini => ("gm_gproxy", "gm_ready"),
    }
}

fn active_batch_item(
    store: &Store,
    seller_chat: i64,
) -> Option<(PurchaseBatch, crate::db::BatchItem)> {
    let job = store.active_seller_job(seller_chat).ok().flatten()?;
    if job.reference.kind != "batch" || job.phase != "processing" {
        return None;
    }
    let batch = store.get_batch(job.reference.batch_id).ok().flatten()?;
    let item = store
        .get_batch_item(batch.id, job.reference.item_no)
        .ok()
        .flatten()?;
    Some((batch, item))
}

fn seller_job_matches_handoff(
    job: &SellerJob,
    expected: &SellerJobRef,
    completed_kind: HandoffKind,
) -> bool {
    job.phase == "processing"
        && job.reference == *expected
        && handoff_kind(&job.product) == completed_kind
}

/// Seller handoffs always carry a generation-bound job reference and may mutate seller state only
/// while it is still current. The admin's standalone Claude tool handles its `None` explicitly.
pub(crate) fn seller_handoff_is_current(
    store: &Store,
    seller_chat: i64,
    expected: Option<&SellerJobRef>,
    completed_kind: HandoffKind,
) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    store
        .active_seller_job(seller_chat)
        .ok()
        .flatten()
        .is_some_and(|job| seller_job_matches_handoff(&job, expected, completed_kind))
}

/// Start one persisted batch position. Exactly one item is `processing`, so the seller cannot
/// accidentally receive all ten account instructions at once.
async fn start_batch_item(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    batch_id: i64,
    item_no: i64,
    payment_hash: Option<&str>,
) {
    let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
        return;
    };
    let Some(item) = store.get_batch_item(batch_id, item_no).ok().flatten() else {
        return;
    };
    if !store.start_batch_item(batch_id, item_no).unwrap_or(false) {
        return;
    }
    let seller_chat = batch.seller_chat;
    let Some(job) = store.active_seller_job(seller_chat).ok().flatten() else {
        return;
    };
    if job.reference.kind != "batch"
        || job.reference.batch_id != batch_id
        || job.reference.item_no != item_no
        || job.phase != "processing"
    {
        return;
    }
    let expected_job = job.job_ref();
    let (proxy_step, next_step) = handoff_steps_for_product(&item.product);
    let payment_line = payment_hash
        .map(|hash| {
            format!(
                "💸 <b>Оплата batch отправлена!</b> tx: <code>{}</code>\n\n",
                esc(hash)
            )
        })
        .unwrap_or_default();
    let position = format!(
        "🧺 <b>Batch #{}</b> · позиция <b>{}/{}</b> · <b>{}</b>\n\n",
        batch.id,
        item.item_no,
        batch.quantity,
        esc(&item.product)
    );

    if batch.proxy_source == PROXY_SOURCE_BUYER {
        if item.proxy.is_empty() {
            let _ = bot
                .send(
                    seller_chat,
                    "⚠️ Для текущей позиции не найден прокси покупателя. Администратор уведомлён.",
                )
                .await;
            notify_admins(
                bot,
                cfg,
                &format!(
                    "⚠️ В batch #{} отсутствует прокси для позиции {}.",
                    batch.id, item.item_no
                ),
                None,
            )
            .await;
            return;
        }
        if !store
            .set_handoff_state_for_seller_job(seller_chat, &expected_job, next_step, &item.proxy, 0)
            .unwrap_or(false)
        {
            return;
        }
        let setup = if next_step == "gm_ready" {
            ""
        } else {
            account_setup_prompt(next_step)
        };
        let _ = bot
            .send(
                seller_chat,
                &format!(
            "{payment_line}{position}✅ Используем прокси покупателя:\n<code>{}</code>\n\n{}",
            esc(&item.proxy), setup
        ),
            )
            .await;
        if next_step == "gm_ready" {
            prepare_gemini_account(bot, store, seller_chat, None, 0).await;
        }
    } else {
        if !store
            .set_handoff_state_for_seller_job(seller_chat, &expected_job, proxy_step, "", 0)
            .unwrap_or(false)
        {
            return;
        }
        let _ = bot
            .send(
                seller_chat,
                &format!(
                    "{payment_line}{position}Теперь пришли прокси продавца для этой позиции.\n\n{}",
                    manual_proxy_prompt(proxy_step)
                ),
            )
            .await;
    }
}

/// Finalize only the exact work captured when this handoff started. The old implementation looked
/// up "any active batch for this seller" here; completing an unrelated single ChatGPT offer could
/// therefore advance a Google AI batch. Exact source + id + item + product-kind matching makes a
/// delayed callback harmless instead.
pub(crate) async fn complete_seller_job_after_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    seller_chat: i64,
    expected: Option<SellerJobRef>,
    completed_kind: HandoffKind,
) -> bool {
    let Some(expected) = expected else {
        if completed_kind == HandoffKind::Claude {
            return true;
        }
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Завершившийся {:?} handoff продавца {} не был привязан к сделке. Доступ мог быть опубликован, но состояние продавца и batch не изменены.",
                completed_kind,
                seller_label(store, seller_chat)
            ),
            None,
        )
        .await;
        return false;
    };
    let Some(job) = store.active_seller_job(seller_chat).ok().flatten() else {
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Завершившийся {:?} handoff продавца {} относится к уже закрытой или заменённой работе. Доступ уже опубликован, но состояние продавца и batch не изменены — нужна ручная сверка.",
                completed_kind,
                seller_label(store, seller_chat)
            ),
            None,
        )
        .await;
        return false;
    };
    if !seller_job_matches_handoff(&job, &expected, completed_kind) {
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Завершившийся {:?} handoff продавца {} не совпал с активной работой «{}». Доступ уже опубликован, но работа не закрыта и batch не сдвинут — нужна ручная сверка.",
                completed_kind,
                seller_label(store, seller_chat),
                seller_job_label(&job)
            ),
            None,
        )
        .await;
        return false;
    }
    if job.reference.kind == "offer" {
        if store
            .finish_offer_job(seller_chat, job.reference.offer_id, &job.reference.token)
            .unwrap_or(false)
        {
            notify_admins(
                bot,
                cfg,
                &format!(
                    "✅ <b>Оффер #{} завершён.</b> Продавец {} передал «{}». Он снова свободен.",
                    job.reference.offer_id,
                    seller_label(store, seller_chat),
                    esc(&job.product)
                ),
                None,
            )
            .await;
            return true;
        }
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Оффер #{} изменился одновременно с завершением handoff. Доступ опубликован, но работа не закрыта автоматически; проверь /jobs.",
                job.reference.offer_id
            ),
            None,
        )
        .await;
        return false;
    }
    let Some(batch) = store.get_batch(job.reference.batch_id).ok().flatten() else {
        return false;
    };
    let Some(progress) = store
        .finish_batch_item(
            job.reference.batch_id,
            job.reference.item_no,
            &job.reference.token,
        )
        .ok()
        .flatten()
    else {
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Batch #{} · позиция {} изменилась одновременно с завершением handoff. Курсор не сдвинут; проверь /jobs.",
                job.reference.batch_id, job.reference.item_no
            ),
            None,
        )
        .await;
        return false;
    };
    if progress.completed {
        let _ = bot
            .send(
                seller_chat,
                &format!(
                    "✅ <b>Batch #{} полностью готов.</b> Все {} подписок приняты. Спасибо! 🤝",
                    batch.id, progress.total
                ),
            )
            .await;
        notify_admins(
            bot,
            cfg,
            &format!(
                "✅ <b>Batch #{} завершён.</b> Приняты все {} подписок «{}».",
                batch.id,
                progress.total,
                esc(&batch.product)
            ),
            None,
        )
        .await;
    } else {
        let _ = bot
            .send(
                seller_chat,
                &format!(
                    "✅ Позиция <b>{}/{}</b> в batch #{} принята. Переходим к следующей позиции.",
                    progress.item_no, progress.total, batch.id
                ),
            )
            .await;
        start_batch_item(bot, store, cfg, batch.id, progress.item_no + 1, None).await;
    }
    true
}

/// Re-open the only states that can be stranded between two Telegram updates. An item that still
/// has a non-empty seller state is already in progress and must not receive a duplicate prompt.
pub async fn resume_batches(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>) {
    for job in store
        .active_seller_jobs()
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job.reference.kind == "offer" && job.phase == "paying")
    {
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Оффер #{} остался в состоянии оплаты после перезапуска. Сначала проверь BscScan/кошелёк продавца; повторную выплату разблокируй только если транзакции нет.",
                job.reference.offer_id
            ),
            Some(&offer_payment_review_kb(job.reference.offer_id)),
        )
        .await;
    }
    for batch in store.batches_needing_payment_review().unwrap_or_default() {
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ Batch #{} остался в состоянии оплаты после перезапуска. Транзакция могла уйти, поэтому сначала проверь BscScan/кошелёк продавца. Если выплаты нет, нажми кнопку для разблокировки повторной оплаты.",
                batch.id
            ),
            Some(&batch_payment_review_kb(batch.id)),
        )
        .await;
    }
    for (batch, item) in store.batches_needing_resume().unwrap_or_default() {
        let want = store
            .get_user(batch.seller_chat)
            .ok()
            .flatten()
            .map(|user| user.want)
            .unwrap_or_default();
        if batch.status == "paid" || want.is_empty() {
            let payment_hash = if batch.status == "paid" && !batch.payment_tx.is_empty() {
                Some(batch.payment_tx.as_str())
            } else {
                None
            };
            start_batch_item(bot, store, cfg, batch.id, item.item_no, payment_hash).await;
        }
    }
}

/// Передача ChatGPT-подписки: device-флоу в скрытый staging-каталог, затем seal в encrypted
/// roster движка (та же модель, что у Gemini).
///
/// В отличие от Claude здесь нет второго шага с `code#state`: codex сам опрашивает OpenAI и
/// завершается, поэтому продавцу достаточно открыть ссылку и ввести код. Ждём в фоне, чтобы не
/// заморозить приём сообщений; ни один секрет не попадает в Telegram или логи.
async fn start_codex_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    email: &str,
    proxy: &str,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        let _ = bot
            .send(
                chat,
                "Эта инструкция уже не относится к активной сделке. Отправь /start.",
            )
            .await;
        return;
    };
    if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Codex) {
        let _ = bot
            .send(
                chat,
                "Эта ChatGPT-инструкция устарела. Открой актуальную сделку через /start.",
            )
            .await;
        return;
    }
    let Some(expected_job) = store
        .rotate_seller_job_token(chat, &job.reference)
        .ok()
        .flatten()
    else {
        let _ = bot
            .send(
                chat,
                "Активная сделка изменилась. Открой актуальную карточку через /start.",
            )
            .await;
        return;
    };
    let expected_job = Some(expected_job);
    let Some(roster) = cfg.codex_roster.clone() else {
        let _ = bot
            .send(
                chat,
                "❌ Приём ChatGPT-аккаунтов не настроен (AUTH_BOT_CODEX_CREDENTIAL_KEYS). Сообщи администратору.",
            )
            .await;
        return;
    };
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
    if !expected_job.as_ref().is_some_and(|expected| {
        store
            .set_want_for_seller_job(chat, expected, "")
            .unwrap_or(false)
    }) {
        crate::codex_login::cancel(chat);
        notify_admins(
            bot,
            cfg,
            &format!(
                "⚠️ ChatGPT handoff продавца {} был отменён: активная работа изменилась во время запуска авторизации.",
                seller_label(store, chat)
            ),
            None,
        )
        .await;
        return;
    }
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
            tokio::task::spawn_blocking(move || crate::codex_login::wait(chat, &bin, &roster))
                .await;
        match outcome {
            Ok(crate::codex_login::Outcome::Authorized { label, has_proxy }) => {
                let current = seller_handoff_is_current(
                    &store2,
                    chat,
                    expected_job.as_ref(),
                    HandoffKind::Codex,
                );
                if current {
                    let _ = bot2.send(chat, &format!(
                        "✅ <b>Готово!</b> Доступ передан, подписка <code>{}</code> принята. Спасибо за сделку! 🤝",
                        esc(&label))).await;
                    notify_admins(&bot2, &cfg2, &format!(
                        "✅ <b>ChatGPT-доступ получен</b>: аккаунт <code>{}</code> добавлен в пул Codex (прокси: {}). \
                         Движок подхватит его ближайшим health-тиком.",
                        esc(&label), if has_proxy { "свой" } else { "общий" }), None).await;
                }
                complete_seller_job_after_handoff(
                    &bot2,
                    &store2,
                    &cfg2,
                    chat,
                    expected_job,
                    HandoffKind::Codex,
                )
                .await;
            }
            Ok(crate::codex_login::Outcome::Expired) => {
                if expected_job.as_ref().is_some_and(|expected| {
                    store2
                        .set_want_for_seller_job(chat, expected, "cx_email")
                        .unwrap_or(false)
                }) {
                    let _ = bot2.send(chat,
                        "❌ Вход не подтверждён — код истёк. Пришли <b>email</b> заново, дам свежий код.").await;
                }
            }
            Ok(crate::codex_login::Outcome::NotChatgpt) => {
                if expected_job.as_ref().is_some_and(|expected| {
                    store2
                        .set_want_for_seller_job(chat, expected, "cx_email")
                        .unwrap_or(false)
                }) {
                    let _ = bot2.send(chat,
                        "❌ Это не подписка ChatGPT (похоже на вход по API-ключу). Нужен аккаунт с активной \
                         подпиской Plus/Pro. Пришли <b>email</b> заново.").await;
                }
            }
            Ok(crate::codex_login::Outcome::Failed(why)) => {
                if expected_job.as_ref().is_some_and(|expected| {
                    store2
                        .set_want_for_seller_job(chat, expected, "cx_email")
                        .unwrap_or(false)
                }) {
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
            }
            Err(_) => {
                if expected_job.as_ref().is_some_and(|expected| {
                    store2
                        .set_want_for_seller_job(chat, expected, "cx_email")
                        .unwrap_or(false)
                }) {
                    let _ = bot2
                        .send(
                            chat,
                            "❌ Внутренняя ошибка ожидания. Пришли <b>email</b> заново.",
                        )
                        .await;
                }
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
    let Some(job) = store.active_seller_job(seller_chat).ok().flatten() else {
        return;
    };
    if job.reference.kind != "offer" || job.reference.offer_id != oid || job.phase != "processing" {
        return;
    }
    let expected_job = job.job_ref();
    let product = store
        .get_offer(oid)
        .ok()
        .flatten()
        .map(|offer| offer.product)
        .unwrap_or_default();
    let _ = bot
        .send(
            seller_chat,
            &format!(
                "📦 <b>Оффер #{} · {}</b>\n\n💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n⏳ Выпускаю прокси (UK ISP)…",
                oid,
                esc(&product),
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
            let gemini = next_step == "gm_ready";
            if !store
                .set_handoff_state_for_seller_job(
                    seller_chat,
                    &expected_job,
                    next_step,
                    &issued_proxy,
                    if gemini { px.order_id } else { 0 },
                )
                .unwrap_or(false)
            {
                notify_admins(
                    bot,
                    cfg,
                    &format!(
                        "⚠️ Прокси IPRoyal для оффера #{} выпущен (заказ #{}), но активная работа продавца уже изменилась. Прокси не отправлен; нужна ручная проверка.",
                        oid, px.order_id
                    ),
                    None,
                )
                .await;
                return;
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
            let retry_applied = store
                .set_handoff_state_for_seller_job(seller_chat, &expected_job, proxy_step, "", 0)
                .unwrap_or(false);
            if retry_applied {
                let prompt = manual_proxy_prompt(proxy_step).to_string();
                let _ = bot.send(seller_chat, &prompt).await;
            }
            notify_admins(
                bot,
                cfg,
                &format!(
                    "⚠️ Авто-выпуск прокси для оффера #{oid} не удался: {}\n{}",
                    esc(&e.to_string()),
                    if retry_applied {
                        "Продавцу предложен ручной ввод."
                    } else {
                        "Работа продавца уже изменилась; состояние не тронуто."
                    }
                ),
                None,
            )
            .await;
        }
    }
}

async fn start_buyer_offer_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    seller_chat: i64,
    oid: i64,
    hash: &str,
) {
    let Some(offer) = store.get_offer(oid).ok().flatten() else {
        return;
    };
    let Some(job) = store.active_seller_job(seller_chat).ok().flatten() else {
        return;
    };
    if job.reference.kind != "offer" || job.reference.offer_id != oid || job.phase != "processing" {
        return;
    }
    let expected_job = job.job_ref();
    if offer.buyer_proxy.is_empty() {
        let _ = bot
            .send(
                seller_chat,
                "⚠️ Для оффера не найден прокси покупателя. Администратор уведомлён.",
            )
            .await;
        notify_admins(
            bot,
            cfg,
            &format!("⚠️ В оффере #{} отсутствует прокси покупателя.", oid),
            None,
        )
        .await;
        return;
    }
    let (_, next_step) = handoff_steps(store, oid);
    if !store
        .set_handoff_state_for_seller_job(
            seller_chat,
            &expected_job,
            next_step,
            &offer.buyer_proxy,
            0,
        )
        .unwrap_or(false)
    {
        return;
    }
    let setup = if next_step == "gm_ready" {
        ""
    } else {
        account_setup_prompt(next_step)
    };
    let _ = bot.send(seller_chat, &format!(
        "📦 <b>Оффер #{} · {}</b>\n\n💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n✅ <b>Прокси покупателя для аккаунта:</b>\n<code>{}</code>\n\n{}",
        oid, esc(&offer.product), esc(hash), esc(&offer.buyer_proxy), setup
    )).await;
    if next_step == "gm_ready" {
        prepare_gemini_account(bot, store, seller_chat, None, 0).await;
    }
}

async fn start_seller_offer_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    seller_chat: i64,
    oid: i64,
    hash: &str,
) {
    let (proxy_step, _) = handoff_steps(store, oid);
    let product = store
        .get_offer(oid)
        .ok()
        .flatten()
        .map(|offer| offer.product)
        .unwrap_or_default();
    let Some(job) = store.active_seller_job(seller_chat).ok().flatten() else {
        return;
    };
    if job.reference.kind != "offer"
        || job.reference.offer_id != oid
        || job.phase != "processing"
        || !store
            .set_handoff_state_for_seller_job(seller_chat, &job.reference, proxy_step, "", 0)
            .unwrap_or(false)
    {
        return;
    }
    let seller_prompt = format!(
        "📦 <b>Оффер #{} · {}</b>\n\n💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n{}",
        oid,
        esc(&product),
        esc(hash),
        manual_proxy_prompt(proxy_step)
    );
    let _ = bot.send(seller_chat, &seller_prompt).await;
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

    // админ: выбрал продукт для batch → выбор продавца
    if let Some(code) = data.strip_prefix("nbatch:") {
        if !admin {
            return;
        }
        if let Some(name) = tier_name(code) {
            start_batch_seller_pick(bot, store, chat, name).await;
        }
        return;
    }

    // админ: выбрал продавца → перейти к цене
    if let Some(rest) = data.strip_prefix("oseller:") {
        if !admin {
            return;
        }
        let seller_chat: i64 = rest.parse().unwrap_or(0);
        if let Ok(Some(mut flow)) = store.get_admin_flow(chat) {
            let expected_step = if flow.mode == "batch" {
                "batch_seller"
            } else {
                "seller"
            };
            if flow.step != expected_step {
                return;
            }
            if let Some(job) = store.active_seller_job(seller_chat).ok().flatten() {
                let _ = bot
                    .send(
                        chat,
                        &format!(
                            "🟠 Этот продавец уже занят. Выбери свободного продавца или дождись завершения текущей сделки.\n\n<b>{}</b>",
                            seller_job_label(&job)
                        ),
                    )
                    .await;
                return;
            }
            let who = seller_label(store, seller_chat);
            flow.seller_chat = seller_chat;
            if flow.mode == "batch" {
                flow.step = "batch_quantity".into();
                let _ = store.set_admin_flow(&flow);
                let _ = bot
                    .send(
                        chat,
                        &format!(
                            "🧺 Продукт: <b>{}</b>\nПродавец: <b>{}</b>\n\n{}",
                            esc(&flow.product),
                            who,
                            BATCH_QUANTITY_PROMPT
                        ),
                    )
                    .await;
            } else {
                flow.step = "price".into();
                let _ = store.set_admin_flow(&flow);
                let _ = bot
                    .send(
                        chat,
                        &format!(
                            "📦 Продукт: <b>{}</b>\nПродавец: <b>{}</b>\n\n{}",
                            esc(&flow.product),
                            who,
                            PRICE_PROMPT
                        ),
                    )
                    .await;
            }
        }
        return;
    }

    // админ: выбрал источник прокси для одиночного оффера
    if let Some(source) = data.strip_prefix("proxy:") {
        if !admin || !matches!(source, PROXY_SOURCE_BUYER | PROXY_SOURCE_SELLER) {
            return;
        }
        if let Ok(mut flow) = store
            .get_admin_flow(chat)
            .map(|state| state.unwrap_or_default())
        {
            if flow.step != "proxy_source" {
                return;
            }
            flow.proxy_source = source.into();
            if source == PROXY_SOURCE_BUYER {
                flow.step = "single_proxy".into();
                let _ = store.set_admin_flow(&flow);
                let _ = bot.send(chat, "Пришли прокси покупателя для этого аккаунта одним сообщением: <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.").await;
            } else {
                let product = flow.product.clone();
                let price = flow.unit_price.clone();
                let seller_chat = flow.seller_chat;
                let _ = store.clear_admin_state(chat);
                publish_single_offer(bot, store, chat, &product, &price, seller_chat, source, "")
                    .await;
            }
        }
        return;
    }

    // админ: выбрал источник прокси для batch
    if let Some(source) = data.strip_prefix("batchproxy:") {
        if !admin || !matches!(source, PROXY_SOURCE_BUYER | PROXY_SOURCE_SELLER) {
            return;
        }
        if let Ok(mut flow) = store
            .get_admin_flow(chat)
            .map(|state| state.unwrap_or_default())
        {
            if flow.step != "batch_proxy_source" {
                return;
            }
            flow.proxy_source = source.into();
            if source == PROXY_SOURCE_BUYER {
                flow.step = "batch_proxies".into();
                let _ = store.set_admin_flow(&flow);
                let _ = bot
                    .send(
                        chat,
                        &BATCH_PROXY_PROMPT
                            .replace("{item}", "1")
                            .replace("{total}", &flow.quantity.to_string()),
                    )
                    .await;
            } else {
                let product = flow.product.clone();
                let unit_price = flow.unit_price.clone();
                let quantity = flow.quantity;
                let seller_chat = flow.seller_chat;
                let total = parse_amount(&unit_price)
                    .map(|amount| fmt_usd(amount * quantity as f64))
                    .unwrap_or_default();
                let _ = store.clear_admin_state(chat);
                publish_batch(
                    bot,
                    store,
                    chat,
                    &product,
                    &unit_price,
                    quantity,
                    &total,
                    seller_chat,
                    source,
                    &[],
                )
                .await;
            }
        }
        return;
    }

    // Админское восстановление: откат ровно на предыдущую позицию требует отдельного подтверждения.
    // Это исправляет данные, которые старая логика могла сдвинуть чужим single-handoff.
    if let Some(rest) = data.strip_prefix("batchrewind:") {
        if !admin {
            return;
        }
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() != 3 {
            return;
        }
        let batch_id = parts[0].parse::<i64>().unwrap_or(0);
        let target_item = parts[1].parse::<i64>().unwrap_or(0);
        let action = parts[2];
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        let valid_current = batch.status == "processing"
            && target_item > 0
            && batch.current_item == target_item + 1;
        if !valid_current {
            let _ = bot
                .send(chat, "Batch уже изменился; старая кнопка отката неактивна.")
                .await;
            return;
        }
        if action == "ask" {
            let confirm: Keyboard = vec![vec![(
                format!("⚠️ Да, вернуть к {}/{}", target_item, batch.quantity),
                format!("batchrewind:{batch_id}:{target_item}:confirm"),
            )]];
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "⚠️ <b>Подтверди откат batch #{}</b>\n\nСейчас: позиция <b>{}/{}</b>.\nСтанет: позиция <b>{}/{}</b>.\n\nТекущий ввод продавца будет сброшен, а предыдущая позиция снова станет незавершённой. Используй только если она была засчитана ошибочно.",
                        batch_id,
                        batch.current_item,
                        batch.quantity,
                        target_item,
                        batch.quantity
                    ),
                    Some(&confirm),
                )
                .await;
            return;
        }
        if action == "confirm" {
            let Some(rewound_to) = store
                .rewind_batch_to_previous(batch_id, batch.seller_chat)
                .ok()
                .flatten()
            else {
                let _ = bot
                    .send(
                        chat,
                        "Не удалось откатить batch: его состояние уже изменилось.",
                    )
                    .await;
                return;
            };
            // Stop process-local capabilities as well as the persisted Gemini session removed by
            // the DB transaction. Their eventual error callbacks are generation-guarded, but
            // cancelling eagerly avoids needless work and stale seller messages.
            setup_token::kill(batch.seller_chat);
            crate::codex_login::cancel(batch.seller_chat);
            let _ = bot
                .send(
                    chat,
                    &format!(
                        "✅ Batch #{} возвращён к позиции {}/{}. Продавцу отправлена новая точная карточка работы.",
                        batch_id, rewound_to, batch.quantity
                    ),
                )
                .await;
            let _ = bot
                .send(
                    batch.seller_chat,
                    &format!(
                        "↩️ <b>Администратор исправил очередь.</b> Batch #{} возвращён к позиции <b>{}/{}</b>. Предыдущая отметка отменена; выполняй только новую карточку ниже.",
                        batch_id, rewound_to, batch.quantity
                    ),
                )
                .await;
            start_batch_item(bot, store, cfg, batch_id, rewound_to, None).await;
            return;
        }
        return;
    }

    // Продавец: принять/отклонить batch целиком. Позиции продавцу будут открываться только после
    // одной общей выплаты и строго по одной.
    if let Some(rest) = data.strip_prefix("batch:") {
        let mut parts = rest.splitn(2, ':');
        let batch_id = parts
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let action = parts.next().unwrap_or_default();
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        if batch.seller_chat != chat {
            return;
        }
        if action == "reject" {
            if store.reject_batch(batch_id, chat).unwrap_or(false) {
                let _ = bot
                    .send(chat, &format!("Batch #{} отклонён.", batch_id))
                    .await;
                notify_admins(
                    bot,
                    cfg,
                    &format!("🚫 <b>Batch #{}</b> продавец отклонил.", batch_id),
                    None,
                )
                .await;
            }
            return;
        }
        if action == "accept" {
            if let Some(job) = store.active_seller_job(chat).ok().flatten() {
                let _ = bot.send(chat, &seller_busy_text(&job)).await;
                return;
            }
            if !store.accept_batch(batch_id, chat).unwrap_or(false) {
                let _ = bot
                    .send(
                        chat,
                        "Этот batch уже обработан либо у тебя уже есть принятая/активная сделка. Сначала заверши её.",
                    )
                    .await;
                return;
            }
            let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
            if rec.address.is_empty() {
                let _ = store.set_want(chat, "reg_address");
                let _ = bot.send(chat, &format!(
                    "✅ <b>Batch #{} принят.</b>\n\nСейчас пришли BEP-20 адрес для общей выплаты.\n\n<b>Затем:</b> {}",
                    batch_id, accepted_next_step(&batch.product, &batch.proxy_source)
                )).await;
                notify_admins(
                    bot,
                    cfg,
                    &format!(
                        "✅ <b>@{} принял batch #{}</b> «{}» — ждём адрес для общей выплаты.",
                        esc(&uname),
                        batch_id,
                        esc(&batch.product)
                    ),
                    None,
                )
                .await;
            } else {
                let _ = bot.send(chat, &format!(
                    "✅ <b>Batch #{} принят.</b> Адрес для выплаты уже сохранён. Ожидай одну общую оплату. {}",
                    batch_id, accepted_next_step(&batch.product, &batch.proxy_source)
                )).await;
                notify_batch_payment_ready(bot, cfg, &batch, &rec).await;
            }
            return;
        }
        return;
    }

    // Админ: после рестарта подтвердить, что зависшая выплата не ушла, и разблокировать retry.
    if let Some(rest) = data.strip_prefix("batchpayretry:") {
        if !admin {
            return;
        }
        let batch_id = rest.parse::<i64>().unwrap_or(0);
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        if batch.status != "paying" {
            let _ = bot
                .send(chat, "Для этого batch уже нет незавершённой выплаты.")
                .await;
            return;
        }
        if !store.reset_batch_payment(batch_id).unwrap_or(false) {
            let _ = bot
                .send(
                    chat,
                    "Не удалось разблокировать выплату — попробуй ещё раз.",
                )
                .await;
            return;
        }
        let Some(updated) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        let seller = store
            .get_user(updated.seller_chat)
            .ok()
            .flatten()
            .unwrap_or_default();
        let _ = bot
            .send(
                chat,
                &format!(
                    "✅ Batch #{} разблокирован. Если транзакции действительно нет, теперь можно запустить одну повторную выплату.",
                    batch_id
                ),
            )
            .await;
        if seller.address.is_empty() {
            let _ = bot
                .send(chat, "У продавца нет сохранённого BEP-20 адреса.")
                .await;
        } else {
            notify_batch_payment_ready(bot, cfg, &updated, &seller).await;
        }
        return;
    }

    // Админ: single-выплата тоже остаётся заблокированной после неопределённой ошибки. Retry
    // разрешается только явным подтверждением после проверки BscScan/кошелька.
    if let Some(rest) = data.strip_prefix("offerpayretry:") {
        if !admin {
            return;
        }
        let offer_id = rest.parse::<i64>().unwrap_or(0);
        let Some(offer) = store.get_offer(offer_id).ok().flatten() else {
            return;
        };
        let seller_chat = store
            .active_seller_jobs()
            .unwrap_or_default()
            .into_iter()
            .find(|job| job.reference.kind == "offer" && job.reference.offer_id == offer_id)
            .map(|job| job.seller_chat)
            .unwrap_or(offer.seller_chat);
        if !store
            .reset_offer_payment(offer_id, seller_chat)
            .unwrap_or(false)
        {
            let _ = bot
                .send(chat, "Для этого оффера уже нет зависшей выплаты.")
                .await;
            return;
        }
        let pay_keyboard: Keyboard = vec![vec![(
            "💸 Оплатить".into(),
            format!("pay:{}:{}", offer_id, seller_chat),
        )]];
        let _ = bot
            .send_kb(
                chat,
                &format!(
                    "✅ Выплата оффера #{} разблокирована. Повторяй только если транзакции действительно нет.",
                    offer_id
                ),
                Some(&pay_keyboard),
            )
            .await;
        return;
    }

    // Админ: одна выплата за весь batch. Статус paying делает callback идемпотентным.
    if let Some(rest) = data.strip_prefix("batchpay:") {
        if !admin {
            return;
        }
        let batch_id = rest.parse::<i64>().unwrap_or(0);
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        let seller = store
            .get_user(batch.seller_chat)
            .ok()
            .flatten()
            .unwrap_or_default();
        if seller.address.is_empty() {
            let _ = bot.send(chat, "У продавца нет BEP-20 адреса.").await;
            return;
        }
        if let Some(job) = store.active_seller_job(batch.seller_chat).ok().flatten() {
            let this_batch_waits_for_payment = job.reference.kind == "batch"
                && job.reference.batch_id == batch_id
                && job.reference.item_no == 0
                && job.phase == "accepted";
            if !this_batch_waits_for_payment {
                let _ = bot
                    .send(
                        chat,
                        &format!(
                            "⛔ Выплата batch #{} не запущена: у продавца уже активна другая работа.\n\n<b>{}</b>",
                            batch_id,
                            seller_job_label(&job)
                        ),
                    )
                    .await;
                return;
            }
        }
        let amount = match parse_amount(&batch.total_price) {
            Some(amount) if amount.is_finite() && amount > 0.0 => amount,
            _ => {
                let _ = bot
                    .send(
                        chat,
                        "Не понял итоговую сумму batch — проверь цену и создай его заново.",
                    )
                    .await;
                return;
            }
        };
        if !store.claim_batch_payment(batch_id).unwrap_or(false) {
            let _ = bot
                .send(chat, "Этот batch уже оплачен или сейчас оплачивается.")
                .await;
            return;
        }
        let _ = bot
            .send(
                chat,
                &format!(
                    "⏳ Отправляю <b>{}</b> USDT одной транзакцией на <code>{}</code>…",
                    amount,
                    esc(&seller.address)
                ),
            )
            .await;
        match pay(cfg, &seller.address, amount).await {
            Ok(hash) => {
                if !store.mark_batch_paid(batch_id, &hash).unwrap_or(false) {
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "⚠️ Транзакция batch отправлена, но состояние в БД не обновилось. tx: <code>{}</code>. Не нажимай оплату повторно — проверь транзакцию и сообщи администратору.",
                                esc(&hash)
                            ),
                        )
                        .await;
                    notify_admins(
                        bot,
                        cfg,
                        &format!(
                            "⚠️ Batch #{}: выплата отправлена, но не удалось сохранить статус paid. tx: <code>{}</code>",
                            batch_id,
                            esc(&hash)
                        ),
                        Some(&batch_payment_review_kb(batch_id)),
                    )
                    .await;
                    return;
                }
                let _ = bot.send(chat, &format!(
                    "✅ Batch #{} оплачен одной транзакцией. tx: <code>{}</code>\nhttps://bscscan.com/tx/{}",
                    batch_id, esc(&hash), esc(&hash)
                )).await;
                let _ = bot.send(seller.chat_id, &format!(
                    "💸 <b>Batch #{} оплачен целиком.</b> Сумма: <b>{}</b> USDT. Начинаем позиции по очереди.",
                    batch_id, esc(&batch.total_price)
                )).await;
                start_batch_item(bot, store, cfg, batch_id, 1, Some(&hash)).await;
            }
            Err(error) => {
                let _ = bot
                    .send(
                        chat,
                        &format!(
                            "❌ Оплата batch не подтверждена: {}\nСначала проверь BscScan/кошелёк продавца. Для повторной попытки используй кнопку только если транзакции действительно нет.",
                            esc(&error)
                        ),
                    )
                    .await;
                notify_admins(
                    bot,
                    cfg,
                    &format!(
                        "⚠️ Batch #{} остался заблокирован в статусе оплаты после ошибки: {}",
                        batch_id,
                        esc(&error)
                    ),
                    Some(&batch_payment_review_kb(batch_id)),
                )
                .await;
            }
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
            let callback_seller_chat: i64 = parts[1].parse().unwrap_or(0);
            let Some(o) = store.get_offer(oid).ok().flatten() else {
                let _ = bot.send(chat, "Оффер не найден.").await;
                return;
            };
            if o.seller_chat != 0 && o.seller_chat != callback_seller_chat {
                let _ = bot
                    .send(chat, "Продавец в этой кнопке не совпадает с оффером.")
                    .await;
                return;
            }
            let seller_chat = if o.seller_chat != 0 {
                o.seller_chat
            } else {
                callback_seller_chat
            };
            let seller = store
                .get_user(seller_chat)
                .ok()
                .flatten()
                .unwrap_or_default();
            if seller.address.is_empty() {
                let _ = bot.send(chat, "У продавца нет BEP-20 адреса.").await;
                return;
            }
            let amount = match parse_amount(&o.price) {
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
            if !store.claim_offer_payment(oid, seller_chat).unwrap_or(false) {
                if let Some(job) = store.active_seller_job(seller_chat).ok().flatten() {
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "⛔ Выплата оффера #{} не запущена: у продавца уже активна работа.\n\n<b>{}</b>",
                                oid,
                                seller_job_label(&job)
                            ),
                        )
                        .await;
                } else {
                    let _ = bot
                        .send(
                            chat,
                            "Этот оффер уже оплачивается, оплачен или больше не ожидает выплату.",
                        )
                        .await;
                }
                return;
            }
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
                    if !store.mark_offer_paid(oid, seller_chat).unwrap_or(false) {
                        let _ = bot
                            .send(
                                chat,
                                &format!(
                                    "⚠️ Транзакция отправлена, но состояние оффера не обновилось. tx: <code>{}</code>. Не оплачивай повторно — сначала проверь BscScan и сообщи администратору.",
                                    esc(&hash)
                                ),
                            )
                            .await;
                        notify_admins(
                            bot,
                            cfg,
                            &format!(
                                "⚠️ Оффер #{}: выплата отправлена, но handoff не запущен. tx: <code>{}</code>",
                                oid,
                                esc(&hash)
                            ),
                            Some(&offer_payment_review_kb(oid)),
                        )
                        .await;
                        return;
                    }
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
                    match o.proxy_source.as_str() {
                        PROXY_SOURCE_BUYER => {
                            start_buyer_offer_handoff(bot, store, cfg, seller_chat, oid, &hash)
                                .await;
                        }
                        PROXY_SOURCE_SELLER => {
                            start_seller_offer_handoff(bot, store, seller_chat, oid, &hash).await;
                        }
                        _ => {
                            // Старые офферы сохраняют прежний режим: при наличии IPRoyal бот
                            // выдаёт прокси сам, иначе просит его у продавца.
                            let already = store.offer_proxy_issued(oid).unwrap_or(false);
                            if amount > 10.0 && !already && !cfg.iproyal_key.is_empty() {
                                deliver_issued_proxy(
                                    bot,
                                    store,
                                    cfg,
                                    chat,
                                    seller_chat,
                                    oid,
                                    &hash,
                                )
                                .await;
                            } else {
                                start_seller_offer_handoff(bot, store, seller_chat, oid, &hash)
                                    .await;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "❌ Оплата не подтверждена: {}\nСначала проверь BscScan/кошелёк продавца. Повтор разрешай только если транзакции действительно нет.",
                                esc(&e)
                            ),
                        )
                        .await;
                    notify_admins(
                        bot,
                        cfg,
                        &format!(
                            "⚠️ Оффер #{} остался заблокирован в статусе оплаты после ошибки: {}",
                            oid,
                            esc(&e)
                        ),
                        Some(&offer_payment_review_kb(oid)),
                    )
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
                let decided = store.decide_offer(oid, uid, "rejected").unwrap_or(false);
                let _ = bot
                    .send(
                        chat,
                        if decided {
                            "Оффер отклонён."
                        } else {
                            "Этот оффер уже был обработан; старая кнопка неактивна."
                        },
                    )
                    .await;
                return;
            }
            if action == "accept" {
                if let Some(job) = store.active_seller_job(chat).ok().flatten() {
                    let _ = bot.send(chat, &seller_busy_text(&job)).await;
                    return;
                }
                if !store.accept_offer(oid, chat, uid).unwrap_or(false) {
                    let _ = bot
                        .send(
                            chat,
                            "Этот оффер уже обработан либо у тебя уже есть принятая/активная сделка. Сначала заверши её.",
                        )
                        .await;
                    return;
                }
                let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
                let o = store.get_offer(oid).ok().flatten();
                let prod = o.as_ref().map(|x| x.product.clone()).unwrap_or_default();
                let proxy_source = o
                    .as_ref()
                    .map(|offer| offer.proxy_source.as_str())
                    .unwrap_or(PROXY_SOURCE_LEGACY);
                if rec.address.is_empty() {
                    let _ = store.set_want(chat, "reg_address");
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "✅ <b>Оффер принят!</b>\n\n<b>Сейчас:</b> пришли одним сообщением свой BEP-20 адрес (<code>0x…</code>) для выплаты.\n\n<b>Затем:</b> {}",
                                accepted_next_step(&prod, proxy_source)
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
                                accepted_next_step(&prod, proxy_source)
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
    fn batch_product_menu_covers_every_subscription_variant() {
        let labels = batch_product_kb()
            .into_iter()
            .flatten()
            .map(|(label, data)| {
                let code = data
                    .strip_prefix("nbatch:")
                    .expect("batch product callback");
                (label, tier_name(code).expect("batch product code"))
            })
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 7);
        for (label, product) in labels {
            assert_eq!(label, product);
            assert!(matches!(
                handoff_kind(product),
                HandoffKind::Claude | HandoffKind::Codex | HandoffKind::Gemini
            ));
        }
    }

    #[test]
    fn proxy_source_is_visible_and_changes_seller_instructions() {
        let store = store();
        let seller_proxy = store
            .create_offer_with_proxy("Google AI Pro", "$20", 1, 2, PROXY_SOURCE_SELLER, "")
            .unwrap();
        let buyer_proxy = store
            .create_offer_with_proxy(
                "Google AI Pro",
                "$20",
                1,
                2,
                PROXY_SOURCE_BUYER,
                "http://user:pass@1.2.3.4:8080",
            )
            .unwrap();
        let seller_text = offer_text(&store.get_offer(seller_proxy).unwrap().unwrap());
        let buyer_text = offer_text(&store.get_offer(buyer_proxy).unwrap().unwrap());
        assert!(seller_text.contains("от продавца"));
        assert!(seller_text.contains("прислать свой HTTP-прокси"));
        assert!(buyer_text.contains("от покупателя"));
        assert!(buyer_text.contains("Дождаться выплаты"));
        assert!(
            !buyer_text.contains("user:pass@1.2.3.4"),
            "proxy must not leak into offer text"
        );
    }

    #[test]
    fn batch_source_keyboard_has_exactly_the_two_requested_flows() {
        let keyboard = proxy_source_kb("batchproxy");
        assert_eq!(keyboard.len(), 2);
        assert_eq!(keyboard[0][0].1, "batchproxy:buyer");
        assert_eq!(keyboard[1][0].1, "batchproxy:seller");
        assert_eq!(proxy_source_label(PROXY_SOURCE_BUYER), "от покупателя");
        assert_eq!(proxy_source_label(PROXY_SOURCE_SELLER), "от продавца");
    }

    #[test]
    fn batch_quantity_accepts_only_plain_integers() {
        assert_eq!(parse_quantity("10"), Some(10));
        assert_eq!(parse_quantity(" 10 "), Some(10));
        assert_eq!(parse_quantity("10.0"), None);
        assert_eq!(parse_quantity("10 подписок"), None);
        assert_eq!(parse_quantity(""), None);
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
            accepted_next_step("Google AI Pro", PROXY_SOURCE_LEGACY),
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

    #[test]
    fn handoff_completion_requires_exact_source_item_and_product_kind() {
        let job = SellerJob {
            seller_chat: 42,
            reference: SellerJobRef {
                kind: "batch".into(),
                offer_id: 0,
                batch_id: 7,
                item_no: 2,
                token: "generation-a".into(),
            },
            product: "Google AI Pro".into(),
            phase: "processing".into(),
            total: 5,
        };
        assert!(seller_job_matches_handoff(
            &job,
            &job.reference,
            HandoffKind::Gemini
        ));
        assert!(!seller_job_matches_handoff(
            &job,
            &SellerJobRef {
                kind: "offer".into(),
                offer_id: 7,
                batch_id: 0,
                item_no: 0,
                token: "generation-a".into(),
            },
            HandoffKind::Gemini
        ));
        assert!(!seller_job_matches_handoff(
            &job,
            &job.reference,
            HandoffKind::Codex
        ));
        let mut stale_generation = job.reference.clone();
        stale_generation.token = "generation-before-rewind".into();
        assert!(!seller_job_matches_handoff(
            &job,
            &stale_generation,
            HandoffKind::Gemini
        ));
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
