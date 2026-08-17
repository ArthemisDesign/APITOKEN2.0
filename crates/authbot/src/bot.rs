//! Логика бота: команды, машина создания оффера, флоу продавца. Состояние — в SQLite (db).
//! Выплаты (Фаза 2) и выпуск setup-token (Фаза 3) пока заглушены — помечены TODO.

use crate::db::{
    AdminState, BatchOverview, ProxyAuthorityStatus, PurchaseBatch, SellerJob, SellerJobRef, Store,
};
use crate::gemini_oauth;
use crate::glm_key;
use crate::glm_roster;
use crate::kimi_oauth;
use crate::kimi_roster;
use crate::setup_token::{self, Outcome};
use crate::suno_roster;
use crate::suno_session;
use crate::tg::{Bot, CallbackQuery, Keyboard};
use crate::tripo3d_key;
use crate::tripo3d_roster;
use crate::Config;
use std::collections::HashSet;
use std::sync::Arc;

pub const WELCOME_NEW: &str =
    "👋 <b>Привет!</b>\nЭто бот закупки. Хочешь продавать — жми кнопку ниже, заявка уйдёт на модерацию.";
pub const ADMIN_HOME: &str = "🛠 <b>Дев-панель</b>\n\nБыстрая покупка: жми продукт на нижней \
    клавиатуре → выбери продавца, цену и источник прокси. Для нескольких подписок используй \
    🧺 Batch-покупку: продавец обработает позиции по очереди, оплата будет одной транзакцией.";

pub(crate) fn esc(s: &str) -> String {
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
        vec![("Kimi Andante".into(), "noffer:kimi_andante".into())],
        vec![("Kimi Moderato".into(), "noffer:kimi_moderato".into())],
        vec![("Kimi Allegretto".into(), "noffer:kimi_allegretto".into())],
        vec![("Kimi Allegro".into(), "noffer:kimi_allegro".into())],
        vec![("Kimi Vivace".into(), "noffer:kimi_vivace".into())],
        vec![("GLM Coding Plan Lite".into(), "noffer:glm_lite".into())],
        vec![("GLM Coding Plan Pro".into(), "noffer:glm_pro".into())],
        vec![("GLM Coding Plan Max".into(), "noffer:glm_max".into())],
        vec![("Tripo3D API $25".into(), "noffer:tripo3d_api_25".into())],
        vec![("Tripo3D API $50".into(), "noffer:tripo3d_api_50".into())],
        vec![("Tripo3D API $100".into(), "noffer:tripo3d_api_100".into())],
        vec![("Suno Pro".into(), "noffer:suno_pro".into())],
        vec![("Suno Premier".into(), "noffer:suno_premier".into())],
    ]
}

/// Несовместимые единицы пополнения: Claude token, ChatGPT CODEX_HOME, зашифрованный
/// Gemini OAuth profile, зашифрованный KIMI (Kimi Code) OAuth profile, статический GLM
/// (Zhipu AI / Z.ai Coding Plan) API-ключ, статический Tripo3D (VAST / Holymolly) API-ключ
/// и зашифрованная Suno (suno.com) сессия подписки.
/// Явный enum не даёт новому продукту тихо провалиться в Claude setup-token ветку.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandoffKind {
    Claude,
    Codex,
    Gemini,
    Kimi,
    Glm,
    Tripo3d,
    Suno,
}

fn handoff_kind(product: &str) -> HandoffKind {
    let p = product.to_lowercase();
    // GLM is matched first: its tier names are generic (Lite/Pro/Max — Claude sells a Max too),
    // so no later substring rule or the Claude fallback must ever be able to claim one of them.
    // Classification keys on the provider/platform words, never on the bare tier word.
    if p.contains("glm")
        || p.contains("zhipu")
        || p.contains("z.ai")
        || p.contains("bigmodel")
        || p.contains("coding plan")
    {
        HandoffKind::Glm
    // KIMI is matched before the rest: its plan names are generic musical terms, so a later
    // substring rule must never be able to claim one of them.
    } else if p.contains("kimi") || p.contains("moonshot") {
        HandoffKind::Kimi
    // Tripo3D keys on the provider word: its cohort names are bare top-up amounts ("$50"),
    // which mean nothing without the provider name. No existing rule contains "tripo", and a
    // Tripo3D product name contains none of the earlier provider words.
    } else if p.contains("tripo3d") || p.contains("tripo") {
        HandoffKind::Tripo3d
    // Suno keys on the provider word: its plan names (Pro/Premier) are generic tier words,
    // so no later substring rule or the Claude fallback must ever be able to claim one of
    // them. There is a single platform (suno.com) and no region fork.
    } else if p.contains("suno") {
        HandoffKind::Suno
    } else if p.contains("gemini")
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
        // Kimi Code plan names as published by the provider's own Kimi Code docs. Only the
        // capability ladder is official; the prices differ between the provider's USD and CNY
        // pages, which is why a price is entered per offer and never assumed here.
        "kimi_andante" => Some("Kimi Andante"),
        "kimi_moderato" => Some("Kimi Moderato"),
        "kimi_allegretto" => Some("Kimi Allegretto"),
        "kimi_allegro" => Some("Kimi Allegro"),
        "kimi_vivace" => Some("Kimi Vivace"),
        // GLM Coding Plan tier names as published by the provider (Z.ai / Zhipu). Only the
        // individual credits ladder is supported; Team and legacy prompts tiers fail closed
        // at validation. Prices are entered per offer and never assumed.
        "glm_lite" => Some("GLM Coding Plan Lite"),
        "glm_pro" => Some("GLM Coding Plan Pro"),
        "glm_max" => Some("GLM Coding Plan Max"),
        // Tripo3D API-platform top-up cohorts. There is no plan ladder on the API side
        // (prepaid credits, $0.01/credit), so the product names the declared top-up cohort;
        // the price of the offer is still entered per offer and never assumed.
        "tripo3d_api_25" => Some("Tripo3D API $25"),
        "tripo3d_api_50" => Some("Tripo3D API $50"),
        "tripo3d_api_100" => Some("Tripo3D API $100"),
        // Suno paid plans as published by the provider. The Free tier is excluded by design
        // (no commercial rights, a daily credit drip, an explicit anti-pooling clause —
        // docs/engine/SUNO_PROVIDER.md §1). Prices are entered per offer and never assumed.
        "suno_pro" => Some("Suno Pro"),
        "suno_premier" => Some("Suno Premier"),
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
        "📦 Kimi Andante" => Some("Kimi Andante"),
        "📦 Kimi Moderato" => Some("Kimi Moderato"),
        "📦 Kimi Allegretto" => Some("Kimi Allegretto"),
        "📦 Kimi Allegro" => Some("Kimi Allegro"),
        "📦 Kimi Vivace" => Some("Kimi Vivace"),
        "📦 GLM Coding Plan Lite" => Some("GLM Coding Plan Lite"),
        "📦 GLM Coding Plan Pro" => Some("GLM Coding Plan Pro"),
        "📦 GLM Coding Plan Max" => Some("GLM Coding Plan Max"),
        "📦 Tripo3D API $25" => Some("Tripo3D API $25"),
        "📦 Tripo3D API $50" => Some("Tripo3D API $50"),
        "📦 Tripo3D API $100" => Some("Tripo3D API $100"),
        "📦 Suno Pro" => Some("Suno Pro"),
        "📦 Suno Premier" => Some("Suno Premier"),
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
        vec!["📦 Kimi Andante", "📦 Kimi Moderato", "📦 Kimi Allegretto"],
        vec!["📦 Kimi Allegro", "📦 Kimi Vivace"],
        vec![
            "📦 GLM Coding Plan Lite",
            "📦 GLM Coding Plan Pro",
            "📦 GLM Coding Plan Max",
        ],
        vec!["📦 Tripo3D API $25", "📦 Tripo3D API $50", "📦 Tripo3D API $100"],
        vec!["📦 Suno Pro", "📦 Suno Premier"],
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

fn batch_status_label(batch: &PurchaseBatch) -> &'static str {
    match batch.status.as_str() {
        "offered" => "📨 ожидает решения продавца",
        "accepted" => "💳 принят, ожидает общей оплаты",
        "paying" => "🔒 выплата проверяется",
        "paid" => "✅ оплачен, запускается",
        "processing" => "▶️ выполняется",
        "paused" => "⏸ на паузе",
        _ => "неизвестный статус",
    }
}

fn progress_bar(completed: i64, total: i64) -> String {
    let filled = if total > 0 {
        (completed.clamp(0, total) * 10 / total) as usize
    } else {
        0
    };
    format!("{}{}", "▓".repeat(filled), "░".repeat(10 - filled))
}

fn batch_jobs_text(store: &Store, overview: &BatchOverview, admin: bool) -> String {
    let batch = &overview.batch;
    let current = if matches!(batch.status.as_str(), "processing" | "paused") {
        format!(
            "\n🎯 Текущая позиция: <b>{}/{}</b>",
            batch.current_item, batch.quantity
        )
    } else {
        String::new()
    };
    let seller = if admin {
        format!("\n👤 Продавец: {}", seller_label(store, batch.seller_chat))
    } else {
        String::new()
    };
    format!(
        "🧺 <b>Batch #{} · {}</b>\n{}\n\n<code>{}</code>  <b>{}/{}</b>\n✅ Выполнено: <b>{}</b>\n⏳ Осталось: <b>{}</b>{current}\n💵 Общая выплата: <b>{}</b>\n🌐 Прокси: {}{seller}",
        batch.id,
        esc(&batch.product),
        batch_status_label(batch),
        progress_bar(overview.completed, batch.quantity),
        overview.completed,
        batch.quantity,
        overview.completed,
        overview.remaining,
        esc(&batch.total_price),
        proxy_source_label(&batch.proxy_source),
    )
}

fn batch_jobs_kb(overview: &BatchOverview, admin: bool) -> Option<Keyboard> {
    let batch = &overview.batch;
    let mut keyboard = Vec::new();
    if batch.status == "processing" {
        keyboard.push(vec![(
            "⏸ Поставить на паузу".into(),
            format!("batchpause:{}:ask", batch.id),
        )]);
        if admin && batch.current_item > 1 {
            keyboard.push(vec![(
                format!(
                    "↩️ Вернуть позицию {}/{}",
                    batch.current_item - 1,
                    batch.quantity
                ),
                format!("batchrewind:{}:{}:ask", batch.id, batch.current_item - 1),
            )]);
        }
    } else if batch.status == "paused" {
        keyboard.push(vec![(
            "▶️ Продолжить batch".into(),
            format!("batchresume:{}", batch.id),
        )]);
    }
    if admin && batch.status != "paying" {
        keyboard.push(vec![(
            "🗑 Удалить batch".into(),
            format!("batchdelete:{}:ask", batch.id),
        )]);
    }
    (!keyboard.is_empty()).then_some(keyboard)
}

fn single_job_kb(job: &SellerJob, admin: bool) -> Option<Keyboard> {
    (admin && matches!(job.phase.as_str(), "accepted" | "processing")).then(|| {
        let mut keyboard = Vec::new();
        // Выплата обязана быть доступна из списка сделок: push с кнопкой одноразовый, и админ,
        // который его не получил или потерял, иначе не может оплатить принятый оффер вообще.
        // Повторное нажатие безопасно — claim_offer_payment пускает только фазу `accepted`.
        if job.phase == "accepted" {
            keyboard.push(vec![(
                "💸 Оплатить".into(),
                format!("pay:{}:{}", job.reference.offer_id, job.seller_chat),
            )]);
        }
        keyboard.push(vec![(
            "🗑 Удалить оффер".into(),
            format!("odel:{}:ask", job.reference.offer_id),
        )]);
        keyboard
    })
}

async fn show_jobs(bot: &Bot, store: &Store, chat: i64, admin: bool) {
    let singles = store
        .active_seller_jobs()
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job.reference.kind == "offer" && (admin || job.seller_chat == chat))
        .collect::<Vec<_>>();
    let batches = store
        .open_batch_overviews(if admin { 0 } else { chat })
        .unwrap_or_default();
    if singles.is_empty() && batches.is_empty() {
        let message = if admin {
            "📋 <b>Активных сделок и batch на паузе нет.</b> Все продавцы свободны."
        } else {
            "📋 <b>У тебя нет активных сделок или batch на паузе.</b>"
        };
        let _ = bot.send(chat, message).await;
        return;
    }
    let _ = bot
        .send(
            chat,
            if admin {
                "📋 <b>Сделки и batch</b>\n\nЗдесь можно контролировать прогресс, паузу и продолжение. Удаление сохраняет финансовую историю в архиве."
            } else {
                "📋 <b>Мои сделки</b>\n\nBatch можно поставить на паузу, выполнить одиночный заказ и затем продолжить с той же незавершённой позиции."
            },
        )
        .await;
    for job in singles {
        let keyboard = single_job_kb(&job, admin);
        let seller = if admin {
            format!("\n👤 Продавец: {}", seller_label(store, job.seller_chat))
        } else {
            String::new()
        };
        let _ = bot
            .send_kb(
                chat,
                &format!("{}{}", seller_job_label(&job), seller),
                keyboard.as_ref(),
            )
            .await;
    }
    for overview in batches {
        let keyboard = batch_jobs_kb(&overview, admin);
        let _ = bot
            .send_kb(
                chat,
                &batch_jobs_text(store, &overview, admin),
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

/// Получатели админской рассылки: env-список ПЛЮС рантайм-админы из БД (`role='admin'`).
/// `is_admin` уже признаёт роль из БД, поэтому рассылка обязана признавать её тоже — иначе выданная
/// админка даёт права в обработчиках, но ни одного собственного уведомления, а кнопка выплаты живёт
/// именно в них.
pub(crate) fn admin_recipients(env_admins: &HashSet<i64>, store: &Store) -> Vec<i64> {
    let mut ids: Vec<i64> = env_admins.iter().copied().filter(|id| *id != 0).collect();
    ids.extend(
        store
            .admin_chat_ids()
            .unwrap_or_default()
            .into_iter()
            .filter(|id| *id != 0),
    );
    ids.sort_unstable();
    ids.dedup();
    ids
}

async fn notify_admins(
    bot: &Bot,
    cfg: &Config,
    store: &Store,
    text: &str,
    kb: Option<&Keyboard>,
) {
    for id in admin_recipients(&cfg.admins_id, store) {
        let _ = bot.send_kb(id, text, kb).await;
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
4. Вернуться в бот, нажать «Аккаунт готов» и в том же профиле подтвердить доступ Antigravity.\n\n\
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

const MANUAL_PROXY_WARNING: &str = "⚠️ Автоматически выдать прокси сейчас не получилось.\n\n";

const CLAUDE_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Claude</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

const KIMI_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый аккаунт Kimi и оформить тариф Kimi Code из оффера.\n\
4. Вернуться в бот, нажать «Аккаунт готов», открыть присланную ссылку в том же профиле и подтвердить показанный код.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, cookie, банковские данные и коды из почты бот не просит.";

const KIMI_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь аккаунт Kimi</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой <code>https://www.kimi.com</code> и самостоятельно зарегистрируй <b>новый</b> аккаунт.\n\n\
4️⃣ В том же профиле оформи тариф <b>Kimi Code</b>, указанный в оффере, и убедись, что подписка появилась именно на этом аккаунте. Обычная подписка на чат Kimi не подходит — нужен именно Kimi Code.\n\n\
5️⃣ Не закрывай профиль и не меняй прокси: они понадобятся на следующем этапе. Когда аккаунт и подписка готовы, нажми кнопку <b>«Аккаунт готов — продолжить»</b> ниже.";

const KIMI_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Kimi</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт Kimi до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

const GLM_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый аккаунт на выбранной площадке GLM (Z.ai или bigmodel.cn) и оформить <b>Individual Coding Plan</b> строго того тарифа, что в оффере.\n\
4. В консоли плана создать API-ключ, вернуться в бот, нажать «Аккаунт готов» и прислать ключ одним сообщением.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, cookie, банковские данные и коды из почты бот не просит — единственное, что нужно прислать, это API-ключ из консоли.";

const GLM_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь аккаунт GLM (Z.ai / Zhipu)</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой сайт выбранной площадки — международную <code>https://z.ai</code> или китайскую <code>https://open.bigmodel.cn</code> — и самостоятельно зарегистрируй <b>новый</b> аккаунт. Аккаунт и ключ обязаны жить на одной площадке: ключ z.ai не работает на bigmodel.cn и наоборот.\n\n\
4️⃣ В том же профиле оформи <b>Individual Coding Plan</b> ровно того тарифа, что указан в оффере (Lite/Pro/Max). <b>Team-версия и обычные pay-as-you-go пакеты не подходят</b> — нужен именно Individual Coding Plan.\n\n\
5️⃣ В консоли открой <b>Individual Coding Plan → Plan Overview</b> и создай <b>API Key</b>. Сам ключ пока никуда не отправляй. Не закрывай профиль и не меняй прокси: они понадобятся на следующем этапе. Когда аккаунт, план и ключ готовы, нажми кнопку <b>«Аккаунт готов — продолжить»</b> ниже.\n\n\
🔒 Бот никогда не попросит пароль, коды 2FA, cookie или банковские данные — только сам API-ключ.";

const GLM_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта GLM (Z.ai / Zhipu)</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт Z.ai или bigmodel.cn до подтверждения прокси ботом: регистрация и дальнейшая проверка ключа должны пройти с одного IP.";

const GLM_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси. Пришли его как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

/// Промпт шага `glm_wait`: API-ключ — единственный credential-артефакт этой ветки, как
/// `sk-ant-oat01-…` у Claude. Продавец присылает только ключ, и бот подчёркивает это явно.
const GLM_KEY_PROMPT: &str = "🔐 <b>Этап 3 из 3 — пришли API-ключ GLM</b>\n\n\
В консоли выбранной площадки открой <b>Individual Coding Plan → Plan Overview</b>, создай <b>API Key</b> и пришли его сюда <b>одним сообщением</b>.\n\n\
Бот проверит ключ (бесплатный запрос квоты и один минимальный вызов модели) и сразу подключит аккаунт. Ключ нигде не сохраняется в открытом виде и не пересылается дальше.\n\n\
Больше ничего присылать не нужно: ни пароль, ни коды 2FA, ни cookie, ни банковские данные — только сам ключ.";

/// Типовые подсказки шага `glm_wait`. Все статические: текст продавца (и тем более ключ) в
/// ответ бота и в журнал не подставляется никогда.
const GLM_KEY_MALFORMED: &str = "🤔 Это не похоже на API-ключ. Пришли ключ одной строкой, без пробелов и переносов — ровно так, как его показывает консоль.";
const GLM_KEY_REJECTED: &str = "❌ Провайдер отклонил этот ключ. Проверь, что ключ скопирован из консоли полностью (Individual Coding Plan → Plan Overview → API Key), без лишних символов. Доступ не передан и выплата не завершена. Нажми «Аккаунт готов — продолжить» и пришли ключ ещё раз.";
const GLM_PLAN_MISMATCH: &str = "❌ План этого ключа не совпадает с оффером: по квоте аккаунта виден другой тариф. Купи ровно тот Individual Coding Plan, что указан в оффере, или пришли ключ от аккаунта с нужным планом. Доступ не передан и выплата не завершена.";
const GLM_PLAN_SHAPE: &str = "❌ Форма квоты этого аккаунта не похожа на Individual Coding Plan (возможно, оформлен Team или устаревший prompts-тариф). Нужен именно Individual Coding Plan. Доступ не передан и выплата не завершена.";
const GLM_QUOTA_EXHAUSTED: &str = "⏳ Ключ действителен, но квота плана сейчас исчерпана. Это не ошибка ключа: пришли его ещё раз после сброса окна квоты (окно — 5 часов) или пришли ключ другого аккаунта. Доступ не передан и выплата не завершена.";
const GLM_VALIDATION_TRANSPORT: &str = "⚠️ Не удалось проверить ключ: сервис провайдера временно недоступен. Ключ не отклонён и никуда не отправлен; платный вызов автоматически не повторялся. Нажми «Аккаунт готов — продолжить» и пришли ключ ещё раз чуть позже.";

/// Подсказка по классу отказа платной проверки. Чистая функция: покрытие всех классов и их
/// тексты проверяются тестом без единого сетевого вызова.
fn glm_invalid_key_guidance(reason: glm_key::InvalidKeyReason) -> &'static str {
    match reason {
        glm_key::InvalidKeyReason::Auth => "❌ Ключ не прошёл проверку: провайдер его не принимает. Пересоздай ключ в консоли (Individual Coding Plan → Plan Overview → API Key) и пришли новый. Доступ не передан и выплата не завершена.",
        glm_key::InvalidKeyReason::OutOfPlanBalance => "❌ Ключ оказался вне баланса или endpoint'ов своего плана. Убедись, что оформлен именно Individual Coding Plan, и пришли ключ ещё раз. Доступ не передан и выплата не завершена.",
        glm_key::InvalidKeyReason::PlanExpired => "❌ Подписка за этим ключом истекла. Продли Individual Coding Plan на том же аккаунте и пришли ключ ещё раз. Доступ не передан и выплата не завершена.",
        glm_key::InvalidKeyReason::ModelOutOfPlan => "❌ Тариф этого ключа не обслуживает нужную модель. Нужен Individual Coding Plan (Lite/Pro/Max) ровно того тарифа, что в оффере. Доступ не передан и выплата не завершена.",
        glm_key::InvalidKeyReason::FairUse => "❌ Аккаунт отмечен риск-контролем провайдера (fair-use), такой ключ подключить нельзя. Нужен другой аккаунт с Individual Coding Plan. Доступ не передан и выплата не завершена.",
        glm_key::InvalidKeyReason::WrongKeyKind => "❌ Этот ключ привязан к Team/enterprise-сценарию, а не к Individual Coding Plan. Нужен ключ individual-плана. Доступ не передан и выплата не завершена.",
    }
}

const TRIPO3D_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый аккаунт на выбранной площадке Tripo3D и пополнить баланс API <b>ровно на сумму, указанную в оффере</b>.\n\
4. В консоли создать API-ключ, вернуться в бот, нажать «Аккаунт готов» и прислать ключ одним сообщением.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, cookie, банковские данные и коды из почты бот не просит — единственное, что нужно прислать, это API-ключ из консоли.";

const TRIPO3D_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь аккаунт Tripo3D</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой сайт выбранной площадки — международную <code>https://platform.tripo3d.ai</code> или китайскую <code>https://platform.tripo3d.com</code> — и самостоятельно зарегистрируй <b>новый</b> аккаунт. Аккаунт и ключ обязаны жить на одной площадке: ключ одной площадки не работает на другой.\n\n\
4️⃣ В том же профиле пополни баланс API <b>ровно на сумму, указанную в оффере</b> — ни больше, ни меньше. Пополнение идёт на API-платформу, а не на подписку Studio.\n\n\
5️⃣ Открой <b>API Keys</b> (<code>/api-keys</code>) и создай <b>API-ключ</b> — он начинается с <code>tsk_</code>. Client ID вида <code>tcli_</code> не подходит. Сам ключ пока никуда не отправляй. Не закрывай профиль и не меняй прокси: они понадобятся на следующем этапе. Когда аккаунт, пополнение и ключ готовы, нажми кнопку <b>«Аккаунт готов — продолжить»</b> ниже.\n\n\
🔒 Бот никогда не попросит пароль, коды 2FA, cookie или банковские данные — только сам API-ключ.";

const TRIPO3D_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Tripo3D</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт Tripo3D до подтверждения прокси ботом: регистрация и дальнейшая проверка ключа должны пройти с одного IP.";

const TRIPO3D_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси. Пришли его как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

/// Промпт шага `t3_wait`: API-ключ — единственный credential-артефакт этой ветки, как
/// `sk-ant-oat01-…` у Claude. Продавец присылает только ключ, и бот подчёркивает это явно.
const TRIPO3D_KEY_PROMPT: &str = "🔐 <b>Этап 3 из 3 — пришли API-ключ Tripo3D</b>\n\n\
В консоли выбранной площадки открой <b>API Keys</b>, создай ключ (начинается с <code>tsk_</code>) и пришли его сюда <b>одним сообщением</b>.\n\n\
Бот проверит ключ бесплатным запросом баланса и сразу подключит аккаунт. Ключ нигде не сохраняется в открытом виде и не пересылается дальше.\n\n\
Больше ничего присылать не нужно: ни пароль, ни коды 2FA, ни cookie, ни банковские данные — только сам ключ.";

/// Типовые подсказки шага `t3_wait`. Все статические: текст продавца (и тем более ключ) в
/// ответ бота и в журнал не подставляется никогда.
const TRIPO3D_KEY_MALFORMED: &str = "🤔 Это не похоже на API-ключ. Пришли ключ одной строкой, без пробелов и переносов — ровно так, как его показывает консоль.";
const TRIPO3D_KEY_REJECTED: &str = "❌ Провайдер отклонил этот ключ. Проверь, что ключ скопирован из консоли полностью (API Keys, ключ вида <code>tsk_…</code>), без лишних символов. Доступ не передан и выплата не завершена. Нажми «Аккаунт готов — продолжить» и пришли ключ ещё раз.";
const TRIPO3D_BALANCE_UNREADABLE: &str = "❌ Не удалось прочитать баланс этого аккаунта: ответ провайдера не похож на обычный prepaid-баланс API. Убедись, что баланс API пополнен ровно на сумму из оффера, и пришли ключ ещё раз. Доступ не передан и выплата не завершена.";
const TRIPO3D_VALIDATION_TRANSPORT: &str = "⚠️ Не удалось проверить ключ: сервис провайдера временно недоступен. Ключ не отклонён и никуда не отправлен. Нажми «Аккаунт готов — продолжить» и пришли ключ ещё раз чуть позже.";

/// Подсказка по классу отказа приёма ключа. Чистая функция: покрытие всех классов и их
/// тексты проверяются тестом без единого сетевого вызова.
fn tripo3d_invalid_key_guidance(reason: tripo3d_key::InvalidKeyReason) -> &'static str {
    match reason {
        tripo3d_key::InvalidKeyReason::Auth => "❌ Ключ не прошёл проверку: провайдер его не принимает. Пересоздай ключ в консоли (API Keys, ключ вида <code>tsk_…</code>) и пришли новый. Доступ не передан и выплата не завершена.",
        tripo3d_key::InvalidKeyReason::ClientIdMisuse => "❌ Это Client ID (вида <code>tcli_…</code>), а не API-ключ. В консоли открой API Keys и создай именно API-ключ (вида <code>tsk_…</code>), затем пришли его. Доступ не передан и выплата не завершена.",
    }
}

const SUNO_OFFER_GUIDE: &str = "🧭 <b>Что нужно будет сделать после принятия</b>\n\
1. Дождаться выплаты и персонального HTTP-прокси от бота.\n\
2. Создать <b>новый чистый профиль</b> в антидетект-браузере и подключить к нему этот прокси.\n\
3. Только через этот профиль самостоятельно зарегистрировать новый аккаунт на <code>suno.com</code> и активировать <b>ровно тот план, что в оффере</b> (Pro или Premier).\n\
4. В том же профиле скопировать cookie сессии, вернуться в бот, нажать «Аккаунт готов» и прислать cookie одним сообщением.\n\n\
Если автоматическая выдача прокси временно недоступна, бот отдельно попросит прокси и продолжит только после его проверки.\n\n\
⚠️ <b>Не регистрируй и не открывай аккаунт до получения прокси.</b> До завершения не меняй профиль, прокси или устройство. Пароль, банковские данные и коды из почты бот не просит — единственное, что нужно прислать, это cookie сессии из твоего собственного браузера.";

const SUNO_ACCOUNT_SETUP: &str = "🧩 <b>Этап 2 из 3 — подготовь аккаунт Suno</b>\n\n\
1️⃣ Открой антидетект-браузер (например, Dolphin или AdsPower) и создай <b>новый чистый профиль</b>. Не используй обычный браузер, старый профиль или телефон.\n\n\
2️⃣ В настройках профиля выбери тип прокси <b>HTTP</b> и вставь данные, которые бот прислал выше. Если браузер просит отдельные поля, строка <code>ip:port:user:pass</code> означает: IP — первое поле, порт — второе, логин — третье, пароль — четвёртое. Нажми проверку и продолжай только если прокси работает и IP изменился. Дополнительный VPN не включай.\n\n\
3️⃣ В этом же профиле открой <code>https://suno.com</code> и самостоятельно зарегистрируй <b>новый</b> аккаунт.\n\n\
4️⃣ В том же профиле активируй <b>ровно тот план, что указан в оффере — Pro или Premier</b>. Бесплатный тариф не подходит.\n\n\
5️⃣ Не выходя из аккаунта и не меняя профиль, открой инструменты разработчика браузера (DevTools → Application → Cookies → <code>suno.com</code>) и скопируй значение cookie <code>__client</code> целиком. Это разрешённый одноразовый артефакт сессии — как setup-token у Claude: без него технически нет другого способа передать доступ. Пока никуда не отправляй. Когда аккаунт и план готовы, нажми кнопку <b>«Аккаунт готов — продолжить»</b> ниже.\n\n\
🔒 Бот никогда не попросит пароль, коды 2FA или банковские данные — только cookie <code>__client</code> из твоего собственного браузера.";

const SUNO_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Suno</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт Suno до подтверждения прокси ботом: регистрация и дальнейшая проверка сессии должны пройти с одного IP.";

const SUNO_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси. Пришли его как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

/// Промпт шага `su_wait`: cookie сессии — единственный credential-артефакт этой ветки, того
/// же класса, что `sk-ant-oat01-…` у Claude (задокументированное отклонение от общего правила
/// «продавец не присылает cookie» — другого credential-интерфейса у Suno нет, manifest §2).
const SUNO_COOKIE_PROMPT: &str = "🔐 <b>Этап 3 из 3 — пришли cookie сессии Suno</b>\n\n\
В том же профиле браузера открой DevTools → Application → Cookies → <code>suno.com</code>, скопируй значение cookie <code>__client</code> и пришли его сюда <b>одним сообщением</b> в формате <code>__client=значение</code> (можно прислать и всю строку cookie целиком).\n\n\
Бот проверит сессию бесплатными запросами (вход, токен, баланс подписки) и сразу подключит аккаунт. Cookie нигде не сохраняется в открытом виде и не пересылается дальше.\n\n\
Больше ничего присылать не нужно: ни пароль, ни коды 2FA, ни банковские данные — только cookie.";

/// Типовые подсказки шага `su_wait`. Все статические: текст продавца (и тем более cookie) в
/// ответ бота и в журнал не подставляется никогда.
const SUNO_COOKIE_MALFORMED: &str = "🤔 Это не похоже на cookie сессии. Пришли одну строку с непустым значением <code>__client=…</code> — ровно так, как её показывает браузер, без переносов строк.";
const SUNO_SESSION_REJECTED: &str = "❌ Сессия отклонена: вход по этой cookie не удался. В том же профиле браузера обнови страницу <code>suno.com</code>, убедись, что выполнен вход в аккаунт с нужным планом, скопируй свежее значение <code>__client</code> и пришли его ещё раз. Доступ не передан и выплата не завершена.";
const SUNO_PLAN_MISMATCH: &str = "❌ План этого аккаунта не совпадает с оффером: по балансу подписки виден другой тариф. Активируй ровно тот план, что указан в оффере (Pro или Premier), или пришли cookie аккаунта с нужным планом. Доступ не передан и выплата не завершена.";
const SUNO_BILLING_UNREADABLE: &str = "❌ Не удалось прочитать баланс этого аккаунта: ответ провайдера не похож на обычный месячный лимит подписки. Убедись, что план из оффера активен, и пришли свежую cookie ещё раз. Доступ не передан и выплата не завершена.";
const SUNO_VALIDATION_TRANSPORT: &str = "⚠️ Не удалось проверить сессию: сервис провайдера временно недоступен. Cookie не отклонена и никуда не отправлена. Нажми «Аккаунт готов — продолжить» и пришли её ещё раз чуть позже.";

/// Подсказка по классу отказа приёма сессии. Чистая функция: покрытие всех классов и их
/// тексты проверяются тестом без единого сетевого вызова.
fn suno_invalid_session_guidance(reason: suno_session::InvalidKeyReason) -> &'static str {
    match reason {
        suno_session::InvalidKeyReason::Auth => SUNO_SESSION_REJECTED,
    }
}

const CODEX_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта ChatGPT</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

const GEMINI_PROXY_PROMPT: &str = "🔐 <b>Этап 1 из 3 — пришли HTTP-прокси для аккаунта Gemini</b>\n\
Одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.\n\n\
Не регистрируй Google-аккаунт до подтверждения прокси ботом: регистрация и дальнейшая авторизация должны пройти с одного IP.";

fn kimi_ready_kb(back: Option<&HandoffStepBack>) -> Keyboard {
    let mut keyboard = vec![vec![(
        "✅ Аккаунт готов — продолжить".into(),
        "kimi:ready".into(),
    )]];
    if let Some(step) = back {
        keyboard.push(handoff_back_row(step, "km_ready"));
    }
    keyboard
}

/// Площадка GLM-аккаунта: международная `api.z.ai` (default) или китайская `open.bigmodel.cn`.
/// Ключ одной площадки не работает на другой, поэтому выбор доживает до `credential_from`.
fn glm_region_row() -> Vec<(String, String)> {
    vec![
        ("🌐 Площадка: z.ai".into(), "glm:region:int".into()),
        ("🌐 Площадка: bigmodel.cn".into(), "glm:region:cn".into()),
    ]
}

fn glm_ready_kb(back: Option<&HandoffStepBack>) -> Keyboard {
    let mut keyboard = vec![
        vec![("✅ Аккаунт готов — продолжить".into(), "glm:ready".into())],
        glm_region_row(),
    ];
    if let Some(step) = back {
        keyboard.push(handoff_back_row(step, "glm_ready"));
    }
    keyboard
}

/// Площадка Tripo3D-аккаунта: международная `api.tripo3d.ai` (default) или китайская
/// `api.tripo3d.com`. Ключ одной площадки не работает на другой, поэтому выбор доживает до
/// `credential_from`.
fn tripo3d_region_row() -> Vec<(String, String)> {
    vec![
        ("🌐 Площадка: tripo3d.ai".into(), "t3:region:global".into()),
        ("🌐 Площадка: tripo3d.com".into(), "t3:region:cn".into()),
    ]
}

fn tripo3d_ready_kb(back: Option<&HandoffStepBack>) -> Keyboard {
    let mut keyboard = vec![
        vec![("✅ Аккаунт готов — продолжить".into(), "t3:ready".into())],
        tripo3d_region_row(),
    ];
    if let Some(step) = back {
        keyboard.push(handoff_back_row(step, "t3_ready"));
    }
    keyboard
}

fn suno_ready_kb(back: Option<&HandoffStepBack>) -> Keyboard {
    // Одна площадка (suno.com), поэтому выбора региона нет.
    let mut keyboard = vec![vec![(
        "✅ Аккаунт готов — продолжить".into(),
        "su:ready".into(),
    )]];
    if let Some(step) = back {
        keyboard.push(handoff_back_row(step, "su_ready"));
    }
    keyboard
}

fn gemini_ready_kb(back: Option<&HandoffStepBack>) -> Keyboard {
    let mut keyboard = vec![vec![(
        "✅ Аккаунт готов — продолжить".into(),
        "gemini:ready".into(),
    )]];
    if let Some(step) = back {
        keyboard.push(handoff_back_row(step, "gm_ready"));
    }
    keyboard
}

/// Offered only next to an `account_validation_required` answer, and only while the account is
/// actually parked: one press runs one real acceptance generation with the tokens consent already
/// produced, instead of walking the seller through both Google consents again.
pub(crate) fn gemini_verified_kb() -> Keyboard {
    vec![vec![(
        "✅ Я подтвердил аккаунт — проверить".into(),
        "gemini:verified".into(),
    )]]
}

fn seller_offer_guide(product: &str) -> &'static str {
    match handoff_kind(product) {
        HandoffKind::Claude => CLAUDE_OFFER_GUIDE,
        HandoffKind::Codex => CODEX_OFFER_GUIDE,
        HandoffKind::Gemini => GEMINI_OFFER_GUIDE,
        HandoffKind::Kimi => KIMI_OFFER_GUIDE,
        HandoffKind::Glm => GLM_OFFER_GUIDE,
        HandoffKind::Tripo3d => TRIPO3D_OFFER_GUIDE,
        HandoffKind::Suno => SUNO_OFFER_GUIDE,
    }
}

fn account_setup_prompt(step: &str) -> &'static str {
    match step {
        "cx_email" => CODEX_ACCOUNT_SETUP,
        "ho_email" => CLAUDE_ACCOUNT_SETUP,
        "gm_ready" => GEMINI_ACCOUNT_SETUP,
        "km_ready" => KIMI_ACCOUNT_SETUP,
        "glm_ready" => GLM_ACCOUNT_SETUP,
        "t3_ready" => TRIPO3D_ACCOUNT_SETUP,
        "su_ready" => SUNO_ACCOUNT_SETUP,
        _ => "",
    }
}

/// Чистый промпт шага ввода прокси, без причины, по которой продавец на нём оказался.
fn proxy_prompt(step: &str) -> &'static str {
    match step {
        "cx_proxy" => CODEX_PROXY_PROMPT,
        "gm_gproxy" => GEMINI_PROXY_PROMPT,
        "km_proxy" => KIMI_PROXY_PROMPT,
        "glm_proxy" => GLM_PROXY_PROMPT,
        "t3_proxy" => TRIPO3D_PROXY_PROMPT,
        "su_proxy" => SUNO_PROXY_PROMPT,
        _ => CLAUDE_PROXY_PROMPT,
    }
}

/// Тот же промпт, но с предупреждением о неудавшейся автоматической выдаче. Осознанный шаг назад
/// продавца этим предупреждением сопровождать нельзя: там ничего не ломалось.
fn manual_proxy_prompt(step: &str) -> String {
    format!("{MANUAL_PROXY_WARNING}{}", proxy_prompt(step))
}

fn accepted_next_step(product: &str, proxy_source: &str) -> &'static str {
    if proxy_source == PROXY_SOURCE_SELLER {
        return "После подтверждения выплаты бот попросит твой персональный прокси, затем даст подробную инструкцию. <b>До этого не создавай и не открывай аккаунт.</b>";
    }
    match handoff_kind(product) {
        HandoffKind::Claude => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай Claude-аккаунт.</b>",
        HandoffKind::Codex => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай ChatGPT-аккаунт.</b>",
        HandoffKind::Gemini => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай Google-аккаунт.</b>",
        HandoffKind::Kimi => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай аккаунт Kimi.</b>",
        HandoffKind::Glm => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай аккаунт Z.ai или bigmodel.cn.</b>",
        HandoffKind::Tripo3d => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай аккаунт Tripo3D.</b>",
        HandoffKind::Suno => "После подтверждения выплаты бот выдаст персональный прокси и подробную инструкцию. <b>До этого не создавай и не открывай аккаунт Suno.</b>",
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
fn seller_pick_kb(store: &Store, sellers: &[crate::db::UserRow], mode: &str) -> Keyboard {
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
            } else if let Some(paused) = store.paused_batch_for_seller(s.chat_id).ok().flatten() {
                label = if mode == "batch" {
                    format!("⏸ {label} — batch #{} на паузе", paused.id)
                } else {
                    format!(
                        "🔵 {label} — batch #{} на паузе, single доступен",
                        paused.id
                    )
                };
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
            Some(&seller_pick_kb(store, &sellers, "single")),
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
            Some(&seller_pick_kb(store, &sellers, "batch")),
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
    if let Some(paused) = store.paused_batch_for_seller(seller_chat).ok().flatten() {
        let _ = bot
            .send(
                admin_chat,
                &format!(
                    "⏸ Batch не создан: у продавца уже стоит на паузе batch #{}. Пока он не продолжен или не удалён, этому продавцу можно направлять только одиночные офферы.",
                    paused.id
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
    store: &Store,
    batch: &PurchaseBatch,
    seller: &crate::db::UserRow,
) {
    notify_admins(
        bot,
        cfg,
        store,
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
            show_jobs(bot, store, chat, true).await;
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

    if is_cancel_command(text) {
        // Для продавца «отмена» всегда означала «верни меня назад», а не «расторгни сделку».
        // Gemini — явное исключение: callback уже мог забрать одноразовый код, поэтому /cancel
        // не ждёт его и не переиспользует. Он атомарно ротирует generation, гасит старую OAuth
        // capability и сразу выдаёт полностью новые ссылки на том же egress.
        if restart_gemini_oauth_attempt(bot, store, cfg, chat, GeminiRestart::Requested).await {
            return;
        }
        if !admin && store.active_seller_job(chat).ok().flatten().is_some() {
            offer_handoff_back(bot, store, cfg, chat, false).await;
            return;
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
                } else if let Some(paused) = store.paused_batch_for_seller(chat).ok().flatten() {
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "👋 <b>Ты продавец.</b>\n\n⏸ Batch #{} стоит на паузе. Пока можешь принять одиночный оффер; чтобы вернуться к batch, открой /jobs и нажми «Продолжить».\n\n💼 Адрес выплат:\n<code>{}</code>",
                                paused.id,
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
    if text == "/jobs" {
        let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
        if admin || rec.status == "approved" {
            show_jobs(bot, store, chat, admin).await;
        } else {
            let _ = bot
                .send(
                    chat,
                    "Команда /jobs доступна администратору и одобренным продавцам.",
                )
                .await;
        }
        return;
    }

    // не-админ: строгий режим — принимаем только ожидаемый сейчас ввод (state-machine)
    if !admin {
        let rec = store.get_user(chat).ok().flatten().unwrap_or_default();
        // Слово «назад» проверяем ДО разбора шага: иначе на шаге ввода прокси его съест парсер
        // прокси и ответит «не разобрал», а продавец останется без выхода.
        if is_handoff_back(text) && store.active_seller_job(chat).ok().flatten().is_some() {
            offer_handoff_back(bot, store, cfg, chat, false).await;
            return;
        }
        match rec.want.as_str() {
            "reg_address" => {
                if is_bep20(text) {
                    let _ = store.set_address(chat, text);
                    let _ = store.set_want(chat, "");
                    let _ = bot.send(chat, "✅ Адрес сохранён. Жди оффер.").await;
                    for batch in store.accepted_batches_for_seller(chat).unwrap_or_default() {
                        if let Some(seller) = store.get_user(chat).ok().flatten() {
                            notify_batch_payment_ready(bot, cfg, store, &batch, &seller).await;
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
                            store,
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
                    let _ = bot
                        .send_kb(
                            chat,
                            "🤔 Не разобрал прокси. Пришли его одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.",
                            handoff_back_kb(store, cfg, chat).as_ref(),
                        )
                        .await;
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
                            .send_kb(
                                chat,
                                &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{CLAUDE_ACCOUNT_SETUP}"),
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
                            .await;
                    }
                }
            }
            "ho_email" => {
                if !looks_like_email(text) {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Это не похоже на email. Пришли адрес аккаунта ещё раз.",
                            handoff_back_kb(store, cfg, chat).as_ref(),
                        )
                        .await;
                } else {
                    do_start_token(bot, store, cfg, chat, text, &rec.hproxy, false).await;
                }
            }
            "cx_proxy" => {
                let purl = proxy_url(text);
                if purl.is_empty() {
                    let _ = bot
                        .send_kb(
                            chat,
                            "🤔 Не разобрал прокси. Пришли его одним сообщением в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.",
                            handoff_back_kb(store, cfg, chat).as_ref(),
                        )
                        .await;
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
                            .send_kb(
                                chat,
                                &format!("✅ Прокси принят и закреплён за аккаунтом.\n\n{CODEX_ACCOUNT_SETUP}"),
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
                            .await;
                    }
                }
            }
            "cx_email" => {
                if !looks_like_email(text) {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Это не похоже на email. Пришли адрес аккаунта ещё раз.",
                            handoff_back_kb(store, cfg, chat).as_ref(),
                        )
                        .await;
                } else {
                    start_codex_handoff(bot, store, cfg, chat, text, &rec.hproxy).await;
                }
            }
            // Device-флоу опрашивает OpenAI в фоне: от продавца здесь не ждут ни одного сообщения.
            // Шаг существует явно, чтобы у ожидания был предшественник и продавец не оказался в
            // общем `_ =>` арме без единой подсказки.
            "cx_wait" => {
                let _ = bot
                    .send_kb(
                        chat,
                        "Подтверждение ChatGPT ещё идёт: открой выданную ссылку и введи одноразовый код. Присылать сюда ничего не нужно — бот сам сообщит результат.",
                        handoff_back_kb(store, cfg, chat).as_ref(),
                    )
                    .await;
            }
            // `gm_gid`/`gm_gsecret` are accepted only as restart compatibility for users who were
            // in the removed custom-client wizard during deployment.
            "gm_gid" | "gm_gsecret" => {
                prepare_gemini_account(bot, store, cfg, chat, None, rec.hproxy_order).await;
            }
            "gm_gproxy" => {
                let Some(job) = store.active_seller_job(chat).ok().flatten() else {
                    return;
                };
                if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Gemini) {
                    return;
                }
                match select_gemini_proxy_retry(
                    store,
                    &job.reference,
                    &rec.hproxy,
                    rec.hproxy_order,
                    text,
                ) {
                    GeminiProxyRetry::SellerSupplied(proxy, credentials) => {
                        if !credentials {
                            // Прокси с авторизацией по IP законен, но у продавца это почти всегда
                            // обрезанная вставка. Без явного предупреждения ошибка проявится только
                            // как CONNECT 407 внутри OAuth, когда одноразовый код уже потрачен.
                            let _ = bot
                                .send(
                                    chat,
                                    "ℹ️ В этом прокси не распознаны логин и пароль — подключение пойдёт с авторизацией по IP. Если у прокси есть логин и пароль, пришли его целиком в формате <code>ip:port:user:pass</code>.",
                                )
                                .await;
                        }
                        // A manually supplied replacement is unrelated to any prior IPRoyal order.
                        prepare_gemini_account(bot, store, cfg, chat, Some(&proxy), 0).await;
                    }
                    GeminiProxyRetry::Retained(proxy, proxy_order_id) => {
                        let _ = bot
                            .send(
                                chat,
                                "🔁 Использую сохранённый прокси для этой же позиции. В следующем сообщении нажми <b>«Аккаунт готов — продолжить»</b>, чтобы получить новую ссылку авторизации.",
                            )
                            .await;
                        prepare_gemini_account(bot, store, cfg, chat, Some(&proxy), proxy_order_id)
                            .await;
                    }
                    GeminiProxyRetry::Fixed(proxy, proxy_order_id) => {
                        let _ = bot
                            .send(
                                chat,
                                "🔁 Использую закреплённый за этой позицией прокси. В следующем сообщении нажми <b>«Аккаунт готов — продолжить»</b>, чтобы получить новую ссылку авторизации. Сообщением продавца этот прокси заменить нельзя.",
                            )
                            .await;
                        prepare_gemini_account(bot, store, cfg, chat, Some(&proxy), proxy_order_id)
                            .await;
                    }
                    GeminiProxyRetry::Invalid => {
                        elog::error("authbot", format!("[gemini-proxy] chat={} rejected seller proxy input: {}", chat,
                            proxy_input_fingerprint(text)));
                        let _ = bot
                            .send_kb(
                                chat,
                                GEMINI_STEP_PROXY_RETRY,
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
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
                            Some(&gemini_ready_kb(current_handoff_back(store, cfg, chat).as_ref())),
                        )
                        .await;
                }
            }
            "gm_wait" => {
                let _ = bot
                    .send_kb(
                        chat,
                        "Авторизация уже ждёт localhost callback. Заверши вход по первой ссылке, скопируй полный адрес из адресной строки, затем открой кнопку «Завершить подключение» и вставь URL в защищённую форму. В Telegram его не присылай.",
                        handoff_back_kb(store, cfg, chat).as_ref(),
                    )
                    .await;
            }
            "km_proxy" => {
                let Some(job) = store.active_seller_job(chat).ok().flatten() else {
                    return;
                };
                if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Kimi) {
                    return;
                }
                match select_kimi_proxy_input(
                    store,
                    &job.reference,
                    &rec.hproxy,
                    rec.hproxy_order,
                    text,
                ) {
                    KimiProxyInput::SellerSupplied(proxy, credentials) => {
                        if !credentials {
                            // Тот же риск, что и у Gemini: обрезанная вставка без userinfo иначе
                            // проявится только как отказ прокси внутри device-flow.
                            let _ = bot
                                .send(
                                    chat,
                                    "ℹ️ В этом прокси не распознаны логин и пароль — подключение пойдёт с авторизацией по IP. Если у прокси есть логин и пароль, пришли его целиком в формате <code>ip:port:user:pass</code>.",
                                )
                                .await;
                        }
                        // A manually supplied replacement is unrelated to any prior IPRoyal order.
                        prepare_kimi_account(bot, store, cfg, chat, Some(&proxy), 0).await;
                    }
                    KimiProxyInput::Fixed(..) => {
                        let _ = bot
                            .send(
                                chat,
                                "🔁 Использую закреплённый за этой позицией прокси. В следующем сообщении нажми <b>«Аккаунт готов — продолжить»</b>, чтобы получить код подтверждения. Сообщением продавца этот прокси заменить нельзя.",
                            )
                            .await;
                        prepare_kimi_account(bot, store, cfg, chat, None, rec.hproxy_order).await;
                    }
                    KimiProxyInput::Invalid => {
                        elog::error("authbot", format!("[kimi-proxy] chat={} rejected seller proxy input: {}", chat,
                            proxy_input_fingerprint(text)));
                        let _ = bot
                            .send_kb(
                                chat,
                                KIMI_STEP_PROXY_RETRY,
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
                            .await;
                    }
                }
            }
            "glm_proxy" => {
                let Some(job) = store.active_seller_job(chat).ok().flatten() else {
                    return;
                };
                if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Glm) {
                    return;
                }
                match select_glm_proxy_input(
                    store,
                    &job.reference,
                    &rec.hproxy,
                    rec.hproxy_order,
                    text,
                ) {
                    GlmProxyInput::SellerSupplied(proxy, credentials) => {
                        if !credentials {
                            // Тот же риск, что и у KIMI: обрезанная вставка без userinfo иначе
                            // проявится только как отказ прокси внутри проверки ключа.
                            let _ = bot
                                .send(
                                    chat,
                                    "ℹ️ В этом прокси не распознаны логин и пароль — подключение пойдёт с авторизацией по IP. Если у прокси есть логин и пароль, пришли его целиком в формате <code>ip:port:user:pass</code>.",
                                )
                                .await;
                        }
                        // A manually supplied replacement is unrelated to any prior IPRoyal order.
                        prepare_glm_account(bot, store, cfg, chat, Some(&proxy), 0).await;
                    }
                    GlmProxyInput::Fixed(..) => {
                        let _ = bot
                            .send(
                                chat,
                                "🔁 Использую закреплённый за этой позицией прокси. В следующем сообщении нажми <b>«Аккаунт готов — продолжить»</b>, чтобы прислать API-ключ. Сообщением продавца этот прокси заменить нельзя.",
                            )
                            .await;
                        prepare_glm_account(bot, store, cfg, chat, None, rec.hproxy_order).await;
                    }
                    GlmProxyInput::Invalid => {
                        elog::error("authbot", format!("[glm-proxy] chat={} rejected seller proxy input: {}", chat,
                            proxy_input_fingerprint(text)));
                        let _ = bot
                            .send_kb(
                                chat,
                                GLM_STEP_PROXY_RETRY,
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
                            .await;
                    }
                }
            }
            "glm_ready" => {
                if text.to_lowercase() == "готово" {
                    continue_glm_handoff(bot, store, cfg, chat).await;
                } else {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Когда аккаунт GLM создан, Individual Coding Plan из оффера активен и API-ключ создан в консоли, нажми кнопку ниже. До этого не меняй профиль или прокси.",
                            Some(&glm_ready_kb(current_handoff_back(store, cfg, chat).as_ref())),
                        )
                        .await;
                }
            }
            "glm_wait" => {
                handle_glm_key_message(bot, store, cfg, chat, text).await;
            }
            "t3_proxy" => {
                let Some(job) = store.active_seller_job(chat).ok().flatten() else {
                    return;
                };
                if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Tripo3d) {
                    return;
                }
                match select_tripo3d_proxy_input(
                    store,
                    &job.reference,
                    &rec.hproxy,
                    rec.hproxy_order,
                    text,
                ) {
                    Tripo3dProxyInput::SellerSupplied(proxy, credentials) => {
                        if !credentials {
                            // Тот же риск, что и у GLM: обрезанная вставка без userinfo иначе
                            // проявится только как отказ прокси внутри проверки ключа.
                            let _ = bot
                                .send(
                                    chat,
                                    "ℹ️ В этом прокси не распознаны логин и пароль — подключение пойдёт с авторизацией по IP. Если у прокси есть логин и пароль, пришли его целиком в формате <code>ip:port:user:pass</code>.",
                                )
                                .await;
                        }
                        // A manually supplied replacement is unrelated to any prior IPRoyal order.
                        prepare_tripo3d_account(bot, store, cfg, chat, Some(&proxy), 0).await;
                    }
                    Tripo3dProxyInput::Fixed(..) => {
                        let _ = bot
                            .send(
                                chat,
                                "🔁 Использую закреплённый за этой позицией прокси. В следующем сообщении нажми <b>«Аккаунт готов — продолжить»</b>, чтобы прислать API-ключ. Сообщением продавца этот прокси заменить нельзя.",
                            )
                            .await;
                        prepare_tripo3d_account(bot, store, cfg, chat, None, rec.hproxy_order).await;
                    }
                    Tripo3dProxyInput::Invalid => {
                        elog::error("authbot", format!("[tripo3d-proxy] chat={} rejected seller proxy input: {}", chat,
                            proxy_input_fingerprint(text)));
                        let _ = bot
                            .send_kb(
                                chat,
                                TRIPO3D_STEP_PROXY_RETRY,
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
                            .await;
                    }
                }
            }
            "t3_ready" => {
                if text.to_lowercase() == "готово" {
                    continue_tripo3d_handoff(bot, store, cfg, chat).await;
                } else {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Когда аккаунт Tripo3D создан, баланс API пополнен ровно на сумму из оффера и API-ключ создан в консоли, нажми кнопку ниже. До этого не меняй профиль или прокси.",
                            Some(&tripo3d_ready_kb(current_handoff_back(store, cfg, chat).as_ref())),
                        )
                        .await;
                }
            }
            "t3_wait" => {
                handle_tripo3d_key_message(bot, store, cfg, chat, text).await;
            }
            "su_proxy" => {
                let Some(job) = store.active_seller_job(chat).ok().flatten() else {
                    return;
                };
                if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Suno) {
                    return;
                }
                match select_suno_proxy_input(
                    store,
                    &job.reference,
                    &rec.hproxy,
                    rec.hproxy_order,
                    text,
                ) {
                    SunoProxyInput::SellerSupplied(proxy, credentials) => {
                        if !credentials {
                            // Тот же риск, что и у GLM: обрезанная вставка без userinfo иначе
                            // проявится только как отказ прокси внутри проверки сессии.
                            let _ = bot
                                .send(
                                    chat,
                                    "ℹ️ В этом прокси не распознаны логин и пароль — подключение пойдёт с авторизацией по IP. Если у прокси есть логин и пароль, пришли его целиком в формате <code>ip:port:user:pass</code>.",
                                )
                                .await;
                        }
                        // A manually supplied replacement is unrelated to any prior IPRoyal order.
                        prepare_suno_account(bot, store, cfg, chat, Some(&proxy), 0).await;
                    }
                    SunoProxyInput::Fixed(..) => {
                        let _ = bot
                            .send(
                                chat,
                                "🔁 Использую закреплённый за этой позицией прокси. В следующем сообщении нажми <b>«Аккаунт готов — продолжить»</b>, чтобы прислать cookie сессии. Сообщением продавца этот прокси заменить нельзя.",
                            )
                            .await;
                        prepare_suno_account(bot, store, cfg, chat, None, rec.hproxy_order).await;
                    }
                    SunoProxyInput::Invalid => {
                        elog::error("authbot", format!("[suno-proxy] chat={} rejected seller proxy input: {}", chat,
                            proxy_input_fingerprint(text)));
                        let _ = bot
                            .send_kb(
                                chat,
                                SUNO_STEP_PROXY_RETRY,
                                handoff_back_kb(store, cfg, chat).as_ref(),
                            )
                            .await;
                    }
                }
            }
            "su_ready" => {
                if text.to_lowercase() == "готово" {
                    continue_suno_handoff(bot, store, cfg, chat).await;
                } else {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Когда аккаунт Suno создан, план из оффера активен и значение cookie __client скопировано, нажми кнопку ниже. До этого не меняй профиль или прокси.",
                            Some(&suno_ready_kb(current_handoff_back(store, cfg, chat).as_ref())),
                        )
                        .await;
                }
            }
            "su_wait" => {
                handle_suno_cookie_message(bot, store, cfg, chat, text).await;
            }
            "ho_code" => match extract_code_state(text) {
                Some(cs) => do_feed_token(bot, store, cfg, chat, &cs).await,
                None => {
                    let _ = bot
                        .send_kb(
                            chat,
                            "Пришли <b>весь адрес страницы из адресной строки</b>: от <code>https://</code> до самого конца. Одного короткого кода недостаточно.",
                            handoff_back_kb(store, cfg, chat).as_ref(),
                        )
                        .await;
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

/// Telegram appends `@bot_username` to commands sent from group chats. Treat that syntax as the
/// same exact command, but reject arguments and malformed suffixes so ordinary text cannot trigger
/// a destructive OAuth generation rotation.
fn is_cancel_command(text: &str) -> bool {
    if text == "/cancel" {
        return true;
    }
    text.strip_prefix("/cancel@").is_some_and(|username| {
        !username.is_empty()
            && username
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

#[derive(Clone, Copy)]
enum GeminiRestart {
    Requested,
    Interrupted,
}

/// Replace one Gemini OAuth generation without touching the paid deal or its egress.
///
/// The publication lock gives cancellation a linearization point against the final credential
/// write. If cancellation wins, `rewind_handoff_step` rotates the seller-job token before the old
/// task is aborted, so that task can neither publish nor mutate the new attempt. If publication
/// already won, the completed job no longer matches and cancellation becomes a harmless no-op.
async fn restart_gemini_oauth_attempt(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    reason: GeminiRestart,
) -> bool {
    let Some(oauth) = cfg.gemini_oauth.as_ref() else {
        return false;
    };
    let _terminal_guard = oauth.terminal_guard().await;
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return false;
    };
    if handoff_kind(&job.product) != HandoffKind::Gemini || job.phase != "processing" {
        return false;
    }
    let Some(user) = store.get_user(chat).ok().flatten() else {
        return false;
    };
    let egress = gemini_oauth::active_egress(store, oauth, chat)
        .or_else(|| (!user.hproxy.is_empty()).then(|| (user.hproxy.clone(), user.hproxy_order)))
        .or_else(|| pinned_job_egress(store, &job.reference));
    let Some((proxy, proxy_order_id)) = egress else {
        // Corrupt or externally removed egress must not keep an already-claimed callback alive.
        // Fence the generation anyway; a seller-owned proxy can then be entered again, while a
        // fixed-proxy position remains stopped for operator repair instead of hanging forever.
        let fenced = store
            .rewind_handoff_step(
                chat,
                &job.reference,
                &user.want,
                "gm_gproxy",
                Some(("", user.hproxy_order)),
            )
            .unwrap_or(None)
            .is_some();
        if fenced {
            oauth.abort_inflight(chat);
        }
        drop(_terminal_guard);
        elog::error("authbot", format!("[gemini-oauth] chat={} restart {} but could not recover the sealed or pinned egress", chat,
            if fenced {
                "fenced the old generation"
            } else {
                "was rejected"
            }));
        if fenced {
            let replaceable = job_accepts_seller_proxy(store, &job.reference, user.hproxy_order);
            let _ = bot
                .send(
                    chat,
                    if replaceable {
                        "🔄 <b>Старая Gemini-авторизация полностью остановлена.</b> Сохранённый прокси оказался недоступен, поэтому пришли его заново — после этого бот создаст новую попытку."
                    } else {
                        "🔄 <b>Старая Gemini-авторизация полностью остановлена.</b> Закреплённый прокси не удалось восстановить; новая попытка не запущена, администратор уже уведомлён."
                    },
                )
                .await;
            notify_admins(
                bot,
                cfg,
                store,
                "⚠️ Gemini /cancel погасил зависшее поколение, но не смог восстановить его запечатанный или закреплённый egress. Проверь источник прокси текущей позиции; старый callback уже недействителен.",
                None,
            )
            .await;
        }
        return fenced;
    };
    let fresh_job = store
        .rewind_handoff_step(
            chat,
            &job.reference,
            &user.want,
            "gm_ready",
            Some((proxy.as_str(), proxy_order_id)),
        )
        .unwrap_or(None);
    if fresh_job.is_none() {
        return false;
    }
    oauth.abort_inflight(chat);
    let message = match reason {
        GeminiRestart::Requested => {
            "🔄 <b>Старая Gemini-авторизация полностью остановлена.</b> Её ссылки и callback больше ничего не могут опубликовать. Ниже — новая попытка с тем же прокси."
        }
        GeminiRestart::Interrupted => {
            "🔄 <b>Прерванная Gemini-проверка безопасно восстановлена.</b> Старое поколение погашено. Используй только новые ссылки ниже; прокси сохранён."
        }
    };
    let _ = bot.send(chat, message).await;
    start_gemini_handoff(bot, store, cfg, chat, Some(&proxy), proxy_order_id).await;
    true
}

/// A claimed Google code is never replayed after an authbot restart or detached-task panic.
/// Instead, the normal `/cancel` fence rotates the exact seller generation and issues new PKCE.
pub(crate) async fn restart_interrupted_gemini_oauth(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
) -> bool {
    restart_gemini_oauth_attempt(bot, store, cfg, chat, GeminiRestart::Interrupted).await
}

pub(crate) async fn recover_interrupted_gemini_oauth(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
) -> usize {
    let mut recovered = 0;
    for chat in store.interrupted_gemini_chats().unwrap_or_default() {
        if restart_interrupted_gemini_oauth(bot, store, cfg, chat).await {
            recovered += 1;
        }
    }
    recovered
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

fn opaque_claude_local_id(email: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("claude_{:x}", Sha256::digest(email.as_bytes()))
}

struct ClaudePublication {
    added_ts: i64,
    canonical_ip: Option<std::net::IpAddr>,
}

fn literal_proxy_ip(proxy: &str) -> anyhow::Result<Option<std::net::IpAddr>> {
    if proxy.is_empty() {
        return Ok(None);
    }
    Ok(reqwest::Url::parse(proxy)
        .map_err(|_| anyhow::anyhow!("invalid canonical proxy URL"))?
        .host_str()
        .map(|host| host.trim_matches(['[', ']']))
        .and_then(|host| host.parse().ok()))
}

async fn register_sub(
    cfg: &Config,
    email: &str,
    token: &str,
    proxy: &str,
    _proxy_order_id: i64,
) -> anyhow::Result<ClaudePublication> {
    // Пишем прямо в PostgreSQL authority движка; reload_loop подхватит подписку за ~30с.
    let canonical_ip = literal_proxy_ip(proxy)?;
    let (authority, email, token, proxy, fleet) = (
        crate::authority_cfg(cfg),
        email.to_string(),
        token.to_string(),
        proxy.to_string(),
        cfg.fleet.clone(),
    );
    let added_ts = tokio::task::spawn_blocking(move || {
        let mut auth = authority.connect_with_application_name("claude-authbot")?;
        auth.add(&email, &token, &proxy, &fleet)?;
        auth.load_claude_lifecycle()?
            .into_iter()
            .find(|profile| profile.email == email)
            .map(|profile| profile.added_ts)
            .filter(|timestamp| *timestamp > 0)
            .ok_or_else(|| anyhow::anyhow!("registered Claude lifecycle row is unavailable"))
    })
    .await
    .map_err(|e| anyhow::anyhow!("PostgreSQL registration worker failed: {e}"))??;
    Ok(ClaudePublication {
        added_ts,
        canonical_ip,
    })
}

/// Разобранный ввод прокси: канонический URL плюс факт наличия учётных данных.
///
/// Флаг нужен продавцовской ветке: `ip:port` — валидный прокси с авторизацией по IP, но для
/// Gemini это почти всегда обрезанная вставка, и без явного предупреждения она превращается в
/// CONNECT 407, неотличимый в журнале от мёртвого прокси.
#[derive(Debug, PartialEq, Eq)]
struct ProxyInput {
    url: String,
    credentials: bool,
}

impl ProxyInput {
    fn invalid() -> Self {
        Self {
            url: String::new(),
            credentials: false,
        }
    }
}

/// host:port:user:pass | host:port | http(s)://… → http-URL (для реестра/прокси).
///
/// Реконструкция обязана быть ОБРАТИМОЙ: пароль продавца — произвольная строка, и всё, что здесь
/// потеряно, позже утекает в CONNECT как чужой пароль. Поэтому режем ровно на четыре поля (пароль
/// может содержать `:`) и процент-кодируем userinfo. Без кодирования `normalize_proxy_url` ниже по
/// стеку ДЕКОДИРУЕТ литеральный `%41` в `A`, а `/`, `?`, `#` в пароле рвут разбор authority —
/// в обоих случаях бот уходит на прокси не с тем паролем, который прислал продавец.
fn parse_proxy_input(raw: &str) -> ProxyInput {
    let raw = raw.trim();
    if raw.is_empty() {
        return ProxyInput::invalid();
    }
    if let Some(rest) = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("https://"))
    {
        let authority = rest.split('/').next().unwrap_or_default();
        return ProxyInput {
            url: raw.to_string(),
            credentials: authority.contains('@'),
        };
    }
    let fields: Vec<&str> = raw.splitn(4, ':').collect();
    let (host, port) = match fields.as_slice() {
        [host, port] | [host, port, _, _] => (*host, *port),
        _ => return ProxyInput::invalid(),
    };
    if !valid_proxy_host(host) || !valid_proxy_port(port) {
        return ProxyInput::invalid();
    }
    match fields.as_slice() {
        [_, _, user, password] if !(user.is_empty() && password.is_empty()) => ProxyInput {
            url: format!(
                "http://{}:{}@{host}:{port}",
                encode_userinfo(user),
                encode_userinfo(password)
            ),
            credentials: true,
        },
        _ => ProxyInput {
            url: format!("http://{host}:{port}"),
            credentials: false,
        },
    }
}

pub(crate) fn proxy_url(raw: &str) -> String {
    parse_proxy_input(raw).url
}

fn valid_proxy_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.bytes().any(|byte| {
            matches!(byte, b'@' | b'/' | b'?' | b'#' | b'[' | b']' | b'%') || byte <= b' '
        })
}

fn valid_proxy_port(port: &str) -> bool {
    port.parse::<u16>().map(|port| port > 0).unwrap_or(false)
}

/// Процент-кодирование в самый узкий безопасный набор: всё, кроме unreserved, экранируется, поэтому
/// `decode_proxy_component` и `decodeURIComponent` в helper возвращают ровно исходные байты.
fn encode_userinfo(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Бесключевой отпечаток отвергнутого ввода: только форма, хост, порт и длины. Позволяет разобрать
/// инцидент, не сохраняя и не печатая секрет продавца.
fn proxy_input_fingerprint(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return "shape=empty".into();
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return format!("shape=url len={}", raw.len());
    }
    let fields: Vec<&str> = raw.splitn(4, ':').collect();
    match fields.as_slice() {
        [host, port] => format!(
            "shape=host:port host_ok={} port_ok={} credentials=no",
            valid_proxy_host(host),
            valid_proxy_port(port)
        ),
        [host, port, user, password] => format!(
            "shape=host:port:user:pass host_ok={} port_ok={} user_len={} pass_len={}",
            valid_proxy_host(host),
            valid_proxy_port(port),
            user.len(),
            password.len()
        ),
        _ => format!(
            "shape=unrecognised fields={} len={}",
            fields.len(),
            raw.len()
        ),
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
    let proxy_order_id = if standalone {
        0
    } else {
        store
            .get_user(chat)
            .ok()
            .flatten()
            .map(|user| user.hproxy_order)
            .unwrap_or(0)
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
        setup_token::start(
            chat,
            &em,
            &px,
            proxy_order_id,
            &cb,
            &config_dir,
            session_job,
        )
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
        Ok(Ok(Outcome::Token {
            token,
            email,
            proxy,
            proxy_order_id,
            job: expected_job,
        })) => {
            let current = expected_job.is_none()
                || seller_handoff_is_current(
                    store,
                    chat,
                    expected_job.as_ref(),
                    HandoffKind::Claude,
                );
            if !current {
                return;
            }
            match register_sub(cfg, &email, &token, &proxy, proxy_order_id).await {
                Ok(publication) => {
                    let binding = match (proxy_order_id, publication.canonical_ip) {
                        (0, _) => Ok(()),
                        (_, Some(allocation_ip)) => store
                            .upsert_proxy_binding_allocation(
                                "claude",
                                &opaque_claude_local_id(&email),
                                proxy_order_id,
                                &allocation_ip.to_string(),
                                publication.added_ts,
                                ProxyAuthorityStatus::Local,
                            )
                            .map(|_| ()),
                        (_, None) => Err(anyhow::anyhow!(
                            "managed Claude proxy host is not a literal allocation IP"
                        )),
                    };
                    if binding.is_err() {
                        notify_admins(
                            bot,
                            cfg,
                            store,
                            "⚠️ Claude опубликован в registry, но lifecycle binding не записан. Сделка оставлена незавершённой; публикацию не откатывать, требуется reconciliation.",
                            None,
                        )
                        .await;
                        return;
                    }
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
                        notify_admins(bot, cfg, store, &format!(
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
                        store,
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
/// Put the seller on the KIMI "account ready" step and show the button that starts the device
/// flow. Mirrors `prepare_gemini_account`, but the KIMI branch needs no OAuth service config to
/// reach this step: the keyring is only required once something is actually sealed.
async fn prepare_kimi_account(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    let expected_job = job.job_ref();
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Kimi) {
        return;
    }
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let effective_proxy = proxy.unwrap_or(&user.hproxy);
    let effective_order = if proxy.is_some() {
        proxy_order_id
    } else {
        user.hproxy_order
    };
    if effective_proxy.is_empty() {
        // Without an egress the seller must not open the account yet: registration and
        // authorization have to come from the same IP.
        if !store
            .set_handoff_state_for_seller_job(chat, &expected_job, "km_proxy", "", effective_order)
            .unwrap_or(false)
        {
            return;
        }
        let _ = bot.send(chat, KIMI_PROXY_PROMPT).await;
        return;
    }
    // A proxy that only passed the shape check would strand the seller on km_ready: the device
    // flow would fail against an egress that can never work, with no way to fix it in place.
    // Canonicalise before pinning, exactly like the Gemini branch does.
    let replaceable_proxy = job_accepts_seller_proxy(store, &expected_job, effective_order);
    let effective_proxy = match kimi_credential::normalize_proxy_url(effective_proxy) {
        Ok(proxy) => proxy,
        Err(_) => {
            elog::error("authbot", format!("[kimi-proxy] chat={} canonicalisation rejected proxy: {}", chat,
                proxy_input_fingerprint(effective_proxy)));
            let (retry_proxy, retry_order) = if replaceable_proxy {
                ("", 0)
            } else {
                (effective_proxy, effective_order)
            };
            if !store
                .set_handoff_state_for_seller_job(
                    chat,
                    &expected_job,
                    "km_proxy",
                    retry_proxy,
                    retry_order,
                )
                .unwrap_or(false)
            {
                return;
            }
            let seller_message = if replaceable_proxy {
                "❌ Не удалось разобрать этот прокси. Авторизация не начата — пришли его заново в указанном формате."
            } else {
                "⚠️ Закреплённый за этой позицией прокси имеет неверный формат. Авторизация не начата; администратор уведомлён."
            };
            let _ = bot.send(chat, seller_message).await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ KIMI proxy не прошёл локальную проверку формата для {}. Сетевых запросов не выполнялось; секреты прокси не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };
    if !store
        .set_handoff_state_for_seller_job(
            chat,
            &expected_job,
            "km_ready",
            &effective_proxy,
            effective_order,
        )
        .unwrap_or(false)
    {
        return;
    }
    let _ = bot
        .send_kb(
            chat,
            KIMI_ACCOUNT_SETUP,
            Some(&kimi_ready_kb(
                current_handoff_back(store, cfg, chat).as_ref(),
            )),
        )
        .await;
}

async fn prepare_gemini_account(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
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
    let effective_order = if proxy.is_some() {
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
    let replaceable_proxy = job_accepts_seller_proxy(store, &expected_job, effective_order);
    let effective_proxy = match gemini_oauth::normalize_proxy_url(effective_proxy) {
        Ok(proxy) => proxy,
        Err(_) => {
            elog::error("authbot", format!("[gemini-proxy] chat={} canonicalisation rejected proxy: {}", chat,
                proxy_input_fingerprint(effective_proxy)));
            let (retry_proxy, retry_order) = if replaceable_proxy {
                ("", 0)
            } else {
                (effective_proxy, effective_order)
            };
            if !store
                .set_handoff_state_for_seller_job(
                    chat,
                    &expected_job,
                    "gm_gproxy",
                    retry_proxy,
                    retry_order,
                )
                .unwrap_or(false)
            {
                return;
            }
            let seller_message = if replaceable_proxy {
                "❌ Не удалось разобрать этот прокси. Авторизация не начата — пришли его заново в указанном формате."
            } else {
                "⚠️ Закреплённый за этой позицией прокси имеет неверный формат. Авторизация не начата; администратор уведомлён."
            };
            let _ = bot.send(chat, seller_message).await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ Gemini proxy не прошёл локальную проверку формата для {}. Сетевых запросов не выполнялось; секреты прокси не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };
    if !store
        .set_handoff_state_for_seller_job(
            chat,
            &expected_job,
            "gm_ready",
            &effective_proxy,
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
            Some(&gemini_ready_kb(
                current_handoff_back(store, cfg, chat).as_ref(),
            )),
        )
        .await;
}

/// Return the proxy only for the explicit Gemini readiness state. Callback buttons can be old or
/// forwarded, so neither the button itself nor a stored proxy alone authorizes a state transition.
fn kimi_ready_handoff(store: &Store, chat: i64) -> Option<(String, i64)> {
    let user = store.get_user(chat).ok().flatten()?;
    if user.want != "km_ready" || user.hproxy.is_empty() {
        return None;
    }
    Some((user.hproxy, user.hproxy_order))
}

fn gemini_ready_handoff(store: &Store, chat: i64) -> Option<(String, i64)> {
    let user = store.get_user(chat).ok().flatten()?;
    if user.want != "gm_ready" || user.hproxy.is_empty() {
        return None;
    }
    Some((user.hproxy, user.hproxy_order))
}

async fn continue_kimi_handoff(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, chat: i64) {
    let Some((proxy, _proxy_order_id)) = kimi_ready_handoff(store, chat) else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    };
    start_kimi_handoff(bot, store, cfg, chat, &proxy).await;
}

/// Run the KIMI device-code acquisition for the seller's current deal.
///
/// The whole exchange happens on the seller's assigned egress, because the account was opened
/// there and authorizing from a different IP is what trips the provider's risk checks. The seller
/// only ever sees a short user code and a verification URL: no password, 2FA, cookie or token
/// crosses Telegram, and the device code itself is never shown.
async fn start_kimi_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: &str,
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
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Kimi) {
        let _ = bot
            .send(
                chat,
                "Текущая сделка не является KIMI-сделкой. Открой актуальную карточку через /start.",
            )
            .await;
        return;
    }
    let Some(roster) = cfg.kimi_roster.as_ref() else {
        // Without a keyring nothing can be sealed, so the seller must not be sent through a flow
        // whose result we would have to throw away.
        let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
        let _ = bot
            .send_kb(
                chat,
                "⚠️ Подключение KIMI сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз этой же кнопкой после исправления.",
                Some(&kimi_ready_kb(None)),
            )
            .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ KIMI handoff недоступен: не настроен AEAD keyring (AUTH_BOT_KIMI_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    };

    let authorization = match kimi_oauth::request_device_authorization(proxy).await {
        Ok(authorization) => authorization,
        Err(_) => {
            // The provider body may echo account details, so the seller gets a bounded message.
            let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
            let _ = bot
                .send_kb(
                    chat,
                    "⚠️ Не удалось начать авторизацию KIMI через твой прокси. Доступ не передан. Нажми кнопку ещё раз; если повторяется — напиши администратору.",
                    Some(&kimi_ready_kb(None)),
                )
                .await;
            return;
        }
    };

    let (user_code, verification_url) = authorization.seller_prompt();
    let _ = store.set_want_for_seller_job(chat, &expected_job, "km_wait");
    let _ = bot
        .send(
            chat,
            &format!(
                "🔐 <b>Этап 3 из 3 — подтверди устройство</b>\n\n                 1️⃣ В <b>том же профиле антидетект-браузера</b>, где ты создал аккаунт Kimi, открой:\n                 <code>{}</code>\n\n                 2️⃣ Проверь, что показан код <b><code>{}</code></b>, и подтверди вход.\n\n                 Не меняй профиль, прокси и устройство. Ничего присылать в чат не нужно — бот сам увидит подтверждение.",
                esc(verification_url),
                esc(user_code)
            ),
        )
        .await;

    let deadline = std::time::Instant::now() + kimi_oauth::acquisition_deadline(&authorization);
    let mut interval = authorization.interval;
    let tokens = loop {
        if std::time::Instant::now() >= deadline {
            let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
            let _ = bot
                .send_kb(
                    chat,
                    "⌛ Время подтверждения истекло. Доступ не передан и выплата не завершена. Нажми кнопку ещё раз — бот выдаст новую ссылку и новый код.",
                    Some(&kimi_ready_kb(None)),
                )
                .await;
            return;
        }
        tokio::time::sleep(interval).await;

        // Re-check the deal on every poll: a cancel, a step back or a restart must be able to
        // stop an in-flight acquisition rather than have it publish into a deal that moved on.
        if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Kimi) {
            return;
        }

        match kimi_oauth::poll_device_token(proxy, &authorization.device_code, kimi_now_unix())
            .await
        {
            Ok(kimi_oauth::DevicePoll::Pending) => continue,
            Ok(kimi_oauth::DevicePoll::SlowDown) => {
                interval = kimi_oauth::backed_off(interval);
                continue;
            }
            Ok(kimi_oauth::DevicePoll::Granted(tokens)) => break tokens,
            Ok(kimi_oauth::DevicePoll::Expired) => {
                let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
                let _ = bot
                    .send_kb(
                        chat,
                        "⌛ Код подтверждения истёк. Доступ не передан. Нажми кнопку ещё раз — бот выдаст новый.",
                        Some(&kimi_ready_kb(None)),
                    )
                    .await;
                return;
            }
            Ok(kimi_oauth::DevicePoll::Denied) => {
                let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
                let _ = bot
                    .send_kb(
                        chat,
                        "🚫 Подтверждение отклонено в браузере. Доступ не передан и выплата не завершена. Нажми кнопку ещё раз, если это была ошибка.",
                        Some(&kimi_ready_kb(None)),
                    )
                    .await;
                return;
            }
            Err(_) => {
                // Polling the token endpoint is read-only, so a transport failure is safe to
                // retry until the deadline; it never replays a paid or one-shot operation.
                continue;
            }
        }
    };

    let identity = match kimi_oauth::fetch_identity(proxy, &tokens.access_token).await {
        Ok(identity) => identity,
        Err(_) => {
            let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
            let _ = bot
                .send_kb(
                    chat,
                    "⚠️ Не удалось подтвердить аккаунт Kimi после входа. Доступ не передан и выплата не завершена. Убедись, что оформлен тариф <b>Kimi Code</b>, и нажми кнопку ещё раз.",
                    Some(&kimi_ready_kb(None)),
                )
                .await;
            return;
        }
    };

    let credential = match kimi_oauth::credential_from(&tokens, &identity, proxy) {
        Ok(credential) => credential,
        Err(_) => {
            let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
            let _ = bot
                .send_kb(
                    chat,
                    "⚠️ Аккаунт Kimi не прошёл проверку и не был подключён. Доступ не передан и выплата не завершена.",
                    Some(&kimi_ready_kb(None)),
                )
                .await;
            return;
        }
    };

    // Last generation check before anything durable is written. SQLite and the roster are not one
    // transaction, so this only narrows the unavoidable cross-store window — it does not close it.
    if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Kimi) {
        return;
    }

    let profile_id = format!("kimi-{}", &new_profile_suffix());
    match kimi_roster::publish(
        &roster.dir,
        &roster.keyring,
        &roster.active_key_id,
        &profile_id,
        &credential,
    ) {
        Ok(published) => {
            let _ = bot
                .send(
                    chat,
                    if published.replaced_existing {
                        "✅ Аккаунт Kimi подключён (обновлён существующий профиль этой подписки)."
                    } else {
                        "✅ Аккаунт Kimi подключён."
                    },
                )
                .await;
            complete_seller_job_after_handoff(
                bot,
                store,
                cfg,
                chat,
                Some(expected_job),
                HandoffKind::Kimi,
            )
            .await;
        }
        Err(kimi_roster::PublishError::Duplicate) => {
            let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
            let _ = bot
                .send_kb(
                    chat,
                    "⚠️ Этот аккаунт Kimi уже подключён к пулу. Доступ не передан и выплата не завершена — нужен новый аккаунт.",
                    Some(&kimi_ready_kb(None)),
                )
                .await;
        }
        Err(kimi_roster::PublishError::Storage) => {
            let _ = store.set_want_for_seller_job(chat, &expected_job, "km_ready");
            let _ = bot
                .send_kb(
                    chat,
                    "⚠️ Не удалось сохранить доступ. Доступ не передан и выплата не завершена; администратор уведомлён.",
                    Some(&kimi_ready_kb(None)),
                )
                .await;
            notify_admins(
                bot,
                cfg,
                store,
                "⚠️ KIMI publication failed closed. Проверь права AUTH_BOT_KIMI_DIR, profiles.json и совпадение credential keyring; секреты не логировались.",
                None,
            )
            .await;
        }
    }
}

/// Unix seconds for token expiry arithmetic.
fn kimi_now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// CSPRNG suffix for a fresh profile id. Never derived from the subject: a profile id is published
/// in the roster and must not leak account identity.
fn new_profile_suffix() -> String {
    let mut random = [0u8; 8];
    if getrandom::fill(&mut random).is_err() {
        return "0".repeat(16);
    }
    random.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Put the seller on the GLM "account ready" step and show the button that arms the key intake.
/// Mirrors `prepare_kimi_account`: the GLM branch needs no keyring to reach this step — it is
/// required only once a key is actually sealed. Entering this step resets the platform selection
/// to the international default: the choice lives in the seller job context, and a finished deal
/// must never leak its region into the next one.
async fn prepare_glm_account(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    let expected_job = job.job_ref();
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Glm) {
        return;
    }
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let effective_proxy = proxy.unwrap_or(&user.hproxy);
    let effective_order = if proxy.is_some() {
        proxy_order_id
    } else {
        user.hproxy_order
    };
    if effective_proxy.is_empty() {
        // Without an egress the seller must not open the account yet: registration and key
        // validation have to come from the same IP.
        if !store
            .set_handoff_state_for_seller_job(chat, &expected_job, "glm_proxy", "", effective_order)
            .unwrap_or(false)
        {
            return;
        }
        let _ = bot.send(chat, GLM_PROXY_PROMPT).await;
        return;
    }
    // A proxy that only passed the shape check would strand the seller on glm_ready: key
    // validation would run against an egress that can never work, with no way to fix it in
    // place. Canonicalise before pinning, exactly like the KIMI branch does.
    let replaceable_proxy = job_accepts_seller_proxy(store, &expected_job, effective_order);
    let effective_proxy = match glm_credential::normalize_proxy_url(effective_proxy) {
        Ok(proxy) => proxy,
        Err(_) => {
            elog::error("authbot", format!("[glm-proxy] chat={} canonicalisation rejected proxy: {}", chat,
                proxy_input_fingerprint(effective_proxy)));
            let (retry_proxy, retry_order) = if replaceable_proxy {
                ("", 0)
            } else {
                (effective_proxy, effective_order)
            };
            if !store
                .set_handoff_state_for_seller_job(
                    chat,
                    &expected_job,
                    "glm_proxy",
                    retry_proxy,
                    retry_order,
                )
                .unwrap_or(false)
            {
                return;
            }
            let seller_message = if replaceable_proxy {
                "❌ Не удалось разобрать этот прокси. Приём ключа не начат — пришли прокси заново в указанном формате."
            } else {
                "⚠️ Закреплённый за этой позицией прокси имеет неверный формат. Приём ключа не начат; администратор уведомлён."
            };
            let _ = bot.send(chat, seller_message).await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ GLM proxy не прошёл локальную проверку формата для {}. Сетевых запросов не выполнялось; секреты прокси не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };
    // A fresh deal re-chooses the platform: the international default applies until the seller
    // explicitly picks bigmodel.cn on the ready card.
    let _ = store.set_hregion(chat, "");
    if !store
        .set_handoff_state_for_seller_job(
            chat,
            &expected_job,
            "glm_ready",
            &effective_proxy,
            effective_order,
        )
        .unwrap_or(false)
    {
        return;
    }
    let _ = bot
        .send_kb(
            chat,
            GLM_ACCOUNT_SETUP,
            Some(&glm_ready_kb(
                current_handoff_back(store, cfg, chat).as_ref(),
            )),
        )
        .await;
}

/// Return the proxy only for the explicit GLM readiness state. Callback buttons can be old or
/// forwarded, so neither the button itself nor a stored proxy alone authorizes a state transition.
fn glm_ready_handoff(store: &Store, chat: i64) -> Option<(String, i64)> {
    let user = store.get_user(chat).ok().flatten()?;
    if user.want != "glm_ready" || user.hproxy.is_empty() {
        return None;
    }
    Some((user.hproxy, user.hproxy_order))
}

async fn continue_glm_handoff(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, chat: i64) {
    let Some((proxy, proxy_order_id)) = glm_ready_handoff(store, chat) else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    };
    start_glm_handoff(bot, store, cfg, chat, &proxy, proxy_order_id).await;
}

/// Arm the GLM key intake for the seller's current deal.
///
/// Unlike KIMI there is no device flow to start: confirming readiness simply moves the deal to
/// `glm_wait`, where the seller sends the console-issued API key as one text message. The key
/// is the only credential artifact — the seller never sends a password, 2FA or cookie.
async fn start_glm_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: &str,
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
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Glm) {
        let _ = bot
            .send(
                chat,
                "Текущая сделка не является GLM-сделкой. Открой актуальную карточку через /start.",
            )
            .await;
        return;
    }
    if cfg.glm_roster.is_none() {
        // Without a keyring nothing can be sealed, so the seller must not be sent through a
        // flow whose result we would have to throw away.
        let _ = store.set_want_for_seller_job(chat, &expected_job, "glm_ready");
        let _ = bot
            .send_kb(
                chat,
                "⚠️ Подключение GLM сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз этой же кнопкой после исправления.",
                Some(&glm_ready_kb(None)),
            )
            .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ GLM handoff недоступен: не настроен AEAD keyring (AUTH_BOT_GLM_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    }
    if proxy.is_empty() {
        // The readiness gate already refuses to arm the intake without an egress; fail closed
        // rather than validating a key from a different IP than the account was opened on.
        if store
            .set_handoff_state_for_seller_job(chat, &expected_job, "glm_proxy", "", proxy_order_id)
            .unwrap_or(false)
        {
            let _ = bot.send(chat, GLM_PROXY_PROMPT).await;
        }
        return;
    }
    let _ = store.set_want_for_seller_job(chat, &expected_job, "glm_wait");
    let _ = bot
        .send_kb(
            chat,
            GLM_KEY_PROMPT,
            handoff_back_kb(store, cfg, chat).as_ref(),
        )
        .await;
}

/// Callback `glm:region:int|cn` с карточки `glm_ready`. Кнопка ничего не авторизует сама по себе:
/// выбор принимается только внутри активной GLM-сделки ровно на шаге подтверждения аккаунта и
/// переживает рестарт в `users.hregion` до самого `credential_from`.
async fn select_glm_region(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    region: &str,
) {
    let region = match region {
        "int" | "cn" => region,
        _ => return,
    };
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже не относится к активной сделке. Отправь /start.",
            )
            .await;
        return;
    };
    if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Glm) {
        return;
    }
    let want = store
        .get_user(chat)
        .ok()
        .flatten()
        .map(|user| user.want)
        .unwrap_or_default();
    if want != "glm_ready" {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    }
    if store.set_hregion(chat, region).is_err() {
        return;
    }
    let label = if region == "cn" {
        "open.bigmodel.cn (Китай)"
    } else {
        "api.z.ai (международная)"
    };
    let _ = bot
        .send(
            chat,
            &format!(
                "🌐 Площадка аккаунта: <b>{label}</b>. Аккаунт и API-ключ обязаны быть созданы именно на ней."
            ),
        )
        .await;
    send_handoff_step_card(bot, store, cfg, chat, "glm_ready", false).await;
}

/// Canonical base URL выбранной площадки. Пустое или неизвестное значение — международная
/// площадка: default int по `docs/engine/GLM_PROVIDER.md` §7.
fn glm_base_url(region: &str) -> &'static str {
    match region {
        "cn" => glm_credential::GLM_BASE_URL_CHINA,
        _ => glm_credential::GLM_BASE_URL_INTERNATIONAL,
    }
}

/// Declared tier продукта оффера. Классификация обязана подтвердить GLM-провайдера, иначе голое
/// слово тарифа (Lite/Pro/Max — у Claude тоже есть Max) не имеет права стать GLM-планом.
fn glm_declared_plan(product: &str) -> Option<glm_credential::GlmPlan> {
    if handoff_kind(product) != HandoffKind::Glm {
        return None;
    }
    let lowered = product.to_lowercase();
    for (word, plan) in [
        ("lite", glm_credential::GlmPlan::Lite),
        ("pro", glm_credential::GlmPlan::Pro),
        ("max", glm_credential::GlmPlan::Max),
    ] {
        if lowered.contains(word) {
            return Some(plan);
        }
    }
    None
}

/// Форма введённого ключа до любого сетевого вызова: одна непустая строка без пробельных
/// символов. Всё остальное решает провайдер — локальных предположений о формате ключа нет.
fn glm_key_text(text: &str) -> Option<&str> {
    let key = text.trim();
    if key.is_empty() || key.len() > 512 || key.chars().any(char::is_whitespace) {
        return None;
    }
    Some(key)
}

/// Единственное, что журнал может узнать о ключе: его длину. Сам ключ не печатается никогда.
fn glm_key_fingerprint(key: &str) -> String {
    format!("key_len={}", key.len())
}

/// Вернуть сделку на подтверждение аккаунта после неудачной передачи ключа: ни конверта, ни
/// строки roster, ни завершения выплаты. Подсказки статические — ключ в них не подставляется.
async fn glm_back_to_ready(
    bot: &Bot,
    store: &Arc<Store>,
    chat: i64,
    expected_job: &SellerJobRef,
    message: &str,
) {
    let _ = store.set_want_for_seller_job(chat, expected_job, "glm_ready");
    let _ = bot.send_kb(chat, message, Some(&glm_ready_kb(None))).await;
}

/// Приём API-ключа на шаге `glm_wait`. Вся цепочка идёт через egress продавца: аккаунт открыт с
/// этого IP, и проверка с другого адреса — ровно то, что триггерит risk-контроль провайдера.
/// Ключ — секрет уровня Claude setup-token: не логируется, не возвращается эхом в чат и не
/// сохраняется в SQLite в открытом виде; валидация живёт только в памяти.
async fn handle_glm_key_message(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    text: &str,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Glm) {
        return;
    }
    let expected_job = job.job_ref();
    let Some(key) = glm_key_text(text) else {
        let _ = bot
            .send_kb(
                chat,
                GLM_KEY_MALFORMED,
                handoff_back_kb(store, cfg, chat).as_ref(),
            )
            .await;
        return;
    };
    let key = zeroize::Zeroizing::new(key.to_string());
    let Some(roster) = cfg.glm_roster.clone() else {
        glm_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            "⚠️ Подключение GLM сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз после исправления.",
        )
        .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ GLM handoff недоступен: не настроен AEAD keyring (AUTH_BOT_GLM_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    };
    let Some(plan) = glm_declared_plan(&job.product) else {
        glm_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            "⚠️ Продукт оффера не распознан как GLM Coding Plan. Доступ не передан; администратор уведомлён.",
        )
        .await;
        notify_admins(
            bot,
            cfg,
            store,
            &format!(
                "⚠️ GLM-сделка {} имеет нераспознанный declared plan; запрос к провайдеру не выполнялся.",
                seller_job_label(&job),
            ),
            None,
        )
        .await;
        return;
    };
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let proxy = user.hproxy.clone();
    if proxy.is_empty() {
        if store
            .set_handoff_state_for_seller_job(
                chat,
                &expected_job,
                "glm_proxy",
                "",
                user.hproxy_order,
            )
            .unwrap_or(false)
        {
            let _ = bot.send(chat, GLM_PROXY_PROMPT).await;
        }
        return;
    }
    let base_url = glm_base_url(&user.hregion);

    // 1. Бесплатный read-only quota probe с bounded retry: он не расходует квоту, поэтому
    // transport-сбой безопасно повторить. Отказ провайдера — финальный вердикт по ключу.
    let mut attempt = 0u32;
    let snapshot = loop {
        if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Glm) {
            return;
        }
        match glm_key::probe_quota(base_url, key.as_str(), &proxy).await {
            Ok(glm_key::QuotaProbe::Valid(snapshot)) => break snapshot,
            Ok(glm_key::QuotaProbe::Invalid) => {
                elog::error("authbot", format!("[glm-key] chat={} provider rejected key: {}", chat,
                    glm_key_fingerprint(key.as_str())));
                glm_back_to_ready(bot, store, chat, &expected_job, GLM_KEY_REJECTED).await;
                return;
            }
            Err(_) => {
                attempt += 1;
                if attempt >= 3 {
                    elog::error("authbot", format!("[glm-key] chat={} quota probe transport failed after {attempt} attempts", chat));
                    glm_back_to_ready(bot, store, chat, &expected_job, GLM_VALIDATION_TRANSPORT)
                        .await;
                    return;
                }
                tokio::time::sleep(glm_key::probe_retry_backoff(attempt - 1)).await;
            }
        }
    };

    // 2. Declared план обязан совпасть с наблюдаемым окном квоты: машиночитаемого /me у GLM
    // нет, поэтому corroboration — единственная проверка «продавец купил тот тариф».
    match glm_key::corroborate_plan(&snapshot, plan) {
        glm_key::PlanVerdict::Confirmed(_) => {}
        glm_key::PlanVerdict::PlanMismatch { .. } => {
            elog::error("authbot", format!("[glm-key] chat={} declared plan contradicts the observed quota window", chat));
            glm_back_to_ready(bot, store, chat, &expected_job, GLM_PLAN_MISMATCH).await;
            return;
        }
        glm_key::PlanVerdict::UnsupportedPlanShape => {
            elog::error("authbot", format!("[glm-key] chat={} quota shape is not an individual credits plan", chat));
            glm_back_to_ready(bot, store, chat, &expected_job, GLM_PLAN_SHAPE).await;
            return;
        }
    }

    // 3. Одна минимальная платная generation. После ambiguous transport она НИКОГДА не
    // повторяется автоматически: вызов мог уже списать квоту.
    let verdict = tokio::time::timeout(
        glm_key::GENERATION_DEADLINE,
        glm_key::run_admission_generation(base_url, key.as_str(), &proxy),
    )
    .await;
    match verdict {
        Ok(Ok(glm_key::KeyVerdict::Valid)) => {}
        Ok(Ok(glm_key::KeyVerdict::Invalid(reason))) => {
            elog::error("authbot", format!("[glm-key] chat={} admission refused key: class={reason:?} {}", chat,
                glm_key_fingerprint(key.as_str())));
            glm_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                glm_invalid_key_guidance(reason),
            )
            .await;
            return;
        }
        Ok(Ok(glm_key::KeyVerdict::QuotaExhausted)) => {
            glm_back_to_ready(bot, store, chat, &expected_job, GLM_QUOTA_EXHAUSTED).await;
            return;
        }
        Ok(Ok(glm_key::KeyVerdict::UnsupportedPlanShape)) => {
            glm_back_to_ready(bot, store, chat, &expected_job, GLM_PLAN_SHAPE).await;
            return;
        }
        Ok(Err(_)) | Err(_) => {
            elog::error("authbot", format!("[glm-key] chat={} admission generation transport ambiguous; paid call not replayed", chat));
            glm_back_to_ready(bot, store, chat, &expected_job, GLM_VALIDATION_TRANSPORT).await;
            return;
        }
    }

    // Последний generation guard перед любой долговременной записью. SQLite и roster — не одна
    // транзакция, поэтому это лишь сужает неизбежное cross-store окно, а не закрывает его.
    if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Glm) {
        return;
    }
    let credential = match glm_key::credential_from(key.as_str(), plan, base_url, &proxy) {
        Ok(credential) => credential,
        Err(_) => {
            glm_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Ключ не прошёл внутреннюю проверку формата. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ GLM credential_from отклонил уже валидированный материал для {}; секреты не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };

    let profile_id = format!("glm-{}", &new_profile_suffix());
    match glm_roster::publish(
        &roster.dir,
        &roster.keyring,
        &roster.active_key_id,
        &profile_id,
        &credential,
    ) {
        Ok(published) => {
            let _ = bot
                .send(
                    chat,
                    if published.replaced_existing {
                        "✅ Аккаунт GLM подключён (обновлён существующий профиль этой подписки)."
                    } else {
                        "✅ Аккаунт GLM подключён."
                    },
                )
                .await;
            complete_seller_job_after_handoff(
                bot,
                store,
                cfg,
                chat,
                Some(expected_job),
                HandoffKind::Glm,
            )
            .await;
        }
        Err(glm_roster::PublishError::Duplicate) => {
            glm_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Такой идентификатор профиля уже занят другим ключом. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ GLM publication hit a profile-id collision для {}; секреты не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
        }
        Err(glm_roster::PublishError::Storage) => {
            glm_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Не удалось сохранить доступ. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                "⚠️ GLM publication failed closed. Проверь права AUTH_BOT_GLM_DIR, profiles.json и совпадение credential keyring; секреты не логировались.",
                None,
            )
            .await;
        }
    }
}

/// Put the seller on the Tripo3D "account ready" step and show the button that arms the key
/// intake. Mirrors `prepare_glm_account`: the Tripo3D branch needs no keyring to reach this
/// step — it is required only once a key is actually sealed. Entering this step resets the
/// platform selection to the international default: the choice lives in the seller job
/// context, and a finished deal must never leak its region into the next one.
async fn prepare_tripo3d_account(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    let expected_job = job.job_ref();
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Tripo3d) {
        return;
    }
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let effective_proxy = proxy.unwrap_or(&user.hproxy);
    let effective_order = if proxy.is_some() {
        proxy_order_id
    } else {
        user.hproxy_order
    };
    if effective_proxy.is_empty() {
        // Without an egress the seller must not open the account yet: registration and key
        // validation have to come from the same IP.
        if !store
            .set_handoff_state_for_seller_job(chat, &expected_job, "t3_proxy", "", effective_order)
            .unwrap_or(false)
        {
            return;
        }
        let _ = bot.send(chat, TRIPO3D_PROXY_PROMPT).await;
        return;
    }
    // A proxy that only passed the shape check would strand the seller on t3_ready: key
    // validation would run against an egress that can never work, with no way to fix it in
    // place. Canonicalise before pinning, exactly like the GLM branch does.
    let replaceable_proxy = job_accepts_seller_proxy(store, &expected_job, effective_order);
    let effective_proxy = match tripo3d_credential::normalize_proxy_url(effective_proxy) {
        Ok(proxy) => proxy,
        Err(_) => {
            elog::error("authbot", format!("[tripo3d-proxy] chat={} canonicalisation rejected proxy: {}", chat,
                proxy_input_fingerprint(effective_proxy)));
            let (retry_proxy, retry_order) = if replaceable_proxy {
                ("", 0)
            } else {
                (effective_proxy, effective_order)
            };
            if !store
                .set_handoff_state_for_seller_job(
                    chat,
                    &expected_job,
                    "t3_proxy",
                    retry_proxy,
                    retry_order,
                )
                .unwrap_or(false)
            {
                return;
            }
            let seller_message = if replaceable_proxy {
                "❌ Не удалось разобрать этот прокси. Приём ключа не начат — пришли прокси заново в указанном формате."
            } else {
                "⚠️ Закреплённый за этой позицией прокси имеет неверный формат. Приём ключа не начат; администратор уведомлён."
            };
            let _ = bot.send(chat, seller_message).await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ Tripo3D proxy не прошёл локальную проверку формата для {}. Сетевых запросов не выполнялось; секреты прокси не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };
    // A fresh deal re-chooses the platform: the international default applies until the seller
    // explicitly picks tripo3d.com on the ready card.
    let _ = store.set_hregion(chat, "");
    if !store
        .set_handoff_state_for_seller_job(
            chat,
            &expected_job,
            "t3_ready",
            &effective_proxy,
            effective_order,
        )
        .unwrap_or(false)
    {
        return;
    }
    let _ = bot
        .send_kb(
            chat,
            TRIPO3D_ACCOUNT_SETUP,
            Some(&tripo3d_ready_kb(
                current_handoff_back(store, cfg, chat).as_ref(),
            )),
        )
        .await;
}

/// Return the proxy only for the explicit Tripo3D readiness state. Callback buttons can be old
/// or forwarded, so neither the button itself nor a stored proxy alone authorizes a state
/// transition.
fn tripo3d_ready_handoff(store: &Store, chat: i64) -> Option<(String, i64)> {
    let user = store.get_user(chat).ok().flatten()?;
    if user.want != "t3_ready" || user.hproxy.is_empty() {
        return None;
    }
    Some((user.hproxy, user.hproxy_order))
}

async fn continue_tripo3d_handoff(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, chat: i64) {
    let Some((proxy, proxy_order_id)) = tripo3d_ready_handoff(store, chat) else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    };
    start_tripo3d_handoff(bot, store, cfg, chat, &proxy, proxy_order_id).await;
}

/// Arm the Tripo3D key intake for the seller's current deal.
///
/// Like GLM there is no device flow to start: confirming readiness simply moves the deal to
/// `t3_wait`, where the seller sends the console-issued API key as one text message. The key
/// is the only credential artifact — the seller never sends a password, 2FA or cookie.
async fn start_tripo3d_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: &str,
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
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Tripo3d) {
        let _ = bot
            .send(
                chat,
                "Текущая сделка не является Tripo3D-сделкой. Открой актуальную карточку через /start.",
            )
            .await;
        return;
    }
    if cfg.tripo3d_roster.is_none() {
        // Without a keyring nothing can be sealed, so the seller must not be sent through a
        // flow whose result we would have to throw away.
        let _ = store.set_want_for_seller_job(chat, &expected_job, "t3_ready");
        let _ = bot
            .send_kb(
                chat,
                "⚠️ Подключение Tripo3D сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз этой же кнопкой после исправления.",
                Some(&tripo3d_ready_kb(None)),
            )
            .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ Tripo3D handoff недоступен: не настроен AEAD keyring (AUTH_BOT_TRIPO3D_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    }
    if proxy.is_empty() {
        // The readiness gate already refuses to arm the intake without an egress; fail closed
        // rather than validating a key from a different IP than the account was opened on.
        if store
            .set_handoff_state_for_seller_job(chat, &expected_job, "t3_proxy", "", proxy_order_id)
            .unwrap_or(false)
        {
            let _ = bot.send(chat, TRIPO3D_PROXY_PROMPT).await;
        }
        return;
    }
    let _ = store.set_want_for_seller_job(chat, &expected_job, "t3_wait");
    let _ = bot
        .send_kb(
            chat,
            TRIPO3D_KEY_PROMPT,
            handoff_back_kb(store, cfg, chat).as_ref(),
        )
        .await;
}

/// Callback `t3:region:global|cn` с карточки `t3_ready`. Кнопка ничего не авторизует сама по
/// себе: выбор принимается только внутри активной Tripo3D-сделки ровно на шаге подтверждения
/// аккаунта и переживает рестарт в `users.hregion` до самого `credential_from`.
async fn select_tripo3d_region(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    region: &str,
) {
    let region = match region {
        "global" | "cn" => region,
        _ => return,
    };
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже не относится к активной сделке. Отправь /start.",
            )
            .await;
        return;
    };
    if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Tripo3d) {
        return;
    }
    let want = store
        .get_user(chat)
        .ok()
        .flatten()
        .map(|user| user.want)
        .unwrap_or_default();
    if want != "t3_ready" {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    }
    if store.set_hregion(chat, region).is_err() {
        return;
    }
    let label = if region == "cn" {
        "api.tripo3d.com (Китай)"
    } else {
        "api.tripo3d.ai (международная)"
    };
    let _ = bot
        .send(
            chat,
            &format!(
                "🌐 Площадка аккаунта: <b>{label}</b>. Аккаунт и API-ключ обязаны быть созданы именно на ней."
            ),
        )
        .await;
    send_handoff_step_card(bot, store, cfg, chat, "t3_ready", false).await;
}

/// Canonical base URL выбранной площадки. Пустое или неизвестное значение — международная
/// площадка: default global по `docs/engine/TRIPO3D_PROVIDER.md` §7.
fn tripo3d_base_url(region: &str) -> &'static str {
    match region {
        "cn" => tripo3d_credential::TRIPO3D_BASE_URL_CHINA,
        _ => tripo3d_credential::TRIPO3D_BASE_URL_GLOBAL,
    }
}

/// Declared top-up cohort продукта оффера. Классификация обязана подтвердить
/// Tripo3D-провайдера, иначе голая сумма пополнения («$50») не имеет права стать когортой.
/// Когорта — это само имя продукта в канонической форме (`normalize_cohort`): именно оно
/// попадает в `cohort` конверта и дальше в calibration-схему 0049.
fn tripo3d_declared_cohort(product: &str) -> Option<String> {
    if handoff_kind(product) != HandoffKind::Tripo3d {
        return None;
    }
    tripo3d_credential::normalize_cohort(product).ok()
}

/// Форма введённого ключа до любого сетевого вызова: одна непустая строка без пробельных
/// символов. Всё остальное решает провайдер — локальных предположений о формате ключа нет
/// сверх задокументированного префикса (его проверяет `preflight_key_rejection`).
fn tripo3d_key_text(text: &str) -> Option<&str> {
    let key = text.trim();
    if key.is_empty() || key.len() > 512 || key.chars().any(char::is_whitespace) {
        return None;
    }
    Some(key)
}

/// Единственное, что журнал может узнать о ключе: его длину. Сам ключ не печатается никогда.
fn tripo3d_key_fingerprint(key: &str) -> String {
    format!("key_len={}", key.len())
}

/// Вернуть сделку на подтверждение аккаунта после неудачной передачи ключа: ни конверта, ни
/// строки roster, ни завершения выплаты. Подсказки статические — ключ в них не подставляется.
async fn tripo3d_back_to_ready(
    bot: &Bot,
    store: &Arc<Store>,
    chat: i64,
    expected_job: &SellerJobRef,
    message: &str,
) {
    let _ = store.set_want_for_seller_job(chat, expected_job, "t3_ready");
    let _ = bot.send_kb(chat, message, Some(&tripo3d_ready_kb(None))).await;
}

/// Приём API-ключа на шаге `t3_wait`. Вся цепочка идёт через egress продавца: аккаунт открыт с
/// этого IP, и проверка с другого адреса — ровно то, что триггерит risk-контроль провайдера.
/// Ключ — секрет уровня Claude setup-token: не логируется, не возвращается эхом в чат и не
/// сохраняется в SQLite в открытом виде; валидация живёт только в памяти.
///
/// Платной admission-задачи здесь намеренно нет: самая дешёвая платная задача Tripo3D
/// (5 кредитов = $0.05) превышает штатный бюджет admission micro-smoke $0.0001, а бесплатная
/// zero-cost задача не доказана (`docs/engine/TRIPO3D_PROVIDER.md` §7 — открытый вопрос
/// бюджета, fail closed). Валидация заканчивается на бесплатном probe баланса.
async fn handle_tripo3d_key_message(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    text: &str,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Tripo3d) {
        return;
    }
    let expected_job = job.job_ref();
    let Some(key) = tripo3d_key_text(text) else {
        let _ = bot
            .send_kb(
                chat,
                TRIPO3D_KEY_MALFORMED,
                handoff_back_kb(store, cfg, chat).as_ref(),
            )
            .await;
        return;
    };
    let key = zeroize::Zeroizing::new(key.to_string());
    // Задокументированная путаница Client ID решается локально, до любого сетевого вызова.
    if let Some(reason) = tripo3d_key::preflight_key_rejection(key.as_str()) {
        elog::error("authbot", format!("[tripo3d-key] chat={} key refused by local preflight: class={reason:?} {}", chat,
            tripo3d_key_fingerprint(key.as_str())));
        tripo3d_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            tripo3d_invalid_key_guidance(reason),
        )
        .await;
        return;
    }
    let Some(roster) = cfg.tripo3d_roster.clone() else {
        tripo3d_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            "⚠️ Подключение Tripo3D сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз после исправления.",
        )
        .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ Tripo3D handoff недоступен: не настроен AEAD keyring (AUTH_BOT_TRIPO3D_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    };
    let Some(cohort) = tripo3d_declared_cohort(&job.product) else {
        tripo3d_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            "⚠️ Продукт оффера не распознан как Tripo3D API. Доступ не передан; администратор уведомлён.",
        )
        .await;
        notify_admins(
            bot,
            cfg,
            store,
            &format!(
                "⚠️ Tripo3D-сделка {} имеет нераспознанную declared cohort; запрос к провайдеру не выполнялся.",
                seller_job_label(&job),
            ),
            None,
        )
        .await;
        return;
    };
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let proxy = user.hproxy.clone();
    if proxy.is_empty() {
        if store
            .set_handoff_state_for_seller_job(
                chat,
                &expected_job,
                "t3_proxy",
                "",
                user.hproxy_order,
            )
            .unwrap_or(false)
        {
            let _ = bot.send(chat, TRIPO3D_PROXY_PROMPT).await;
        }
        return;
    }
    let base_url = tripo3d_base_url(&user.hregion);

    // 1. Бесплатный read-only balance probe с bounded retry: он не расходует кредиты, поэтому
    // transport-сбой безопасно повторить. Отказ провайдера — финальный вердикт по ключу.
    let mut attempt = 0u32;
    let snapshot = loop {
        if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Tripo3d) {
            return;
        }
        match tripo3d_key::probe_balance(base_url, key.as_str(), &proxy).await {
            Ok(tripo3d_key::BalanceProbe::Valid(snapshot)) => break snapshot,
            Ok(tripo3d_key::BalanceProbe::Invalid) => {
                elog::error("authbot", format!("[tripo3d-key] chat={} provider rejected key: {}", chat,
                    tripo3d_key_fingerprint(key.as_str())));
                tripo3d_back_to_ready(bot, store, chat, &expected_job, TRIPO3D_KEY_REJECTED).await;
                return;
            }
            Err(_) => {
                attempt += 1;
                if attempt >= 3 {
                    elog::error("authbot", format!("[tripo3d-key] chat={} balance probe transport failed after {attempt} attempts", chat));
                    tripo3d_back_to_ready(bot, store, chat, &expected_job, TRIPO3D_VALIDATION_TRANSPORT)
                        .await;
                    return;
                }
                tokio::time::sleep(tripo3d_key::probe_retry_backoff(attempt - 1)).await;
            }
        }
    };

    // 2. Declared когорта обязана подтвердиться наблюдаемым балансом. Пока единица
    // balance/frozen не доказана (manifest §5.2), corroboration проверяет только читаемость
    // счётчиков — сравнение суммы невозможно и не подделывается. Нечитаемый ответ — отказ.
    match tripo3d_key::corroborate_cohort(&snapshot, &cohort) {
        tripo3d_key::CohortVerdict::Consistent => {}
        tripo3d_key::CohortVerdict::Unreadable => {
            elog::error("authbot", format!("[tripo3d-key] chat={} balance snapshot cannot corroborate the declared cohort", chat));
            tripo3d_back_to_ready(bot, store, chat, &expected_job, TRIPO3D_BALANCE_UNREADABLE).await;
            return;
        }
    }

    // Платной admission-задачи здесь нет намеренно (см. документацию функции): публикация
    // опирается на бесплатный probe и corroboration, а вопрос бюджета платного допуска
    // записан в манифесте §7 как открытый.

    // Последний generation guard перед любой долговременной записью. SQLite и roster — не одна
    // транзакция, поэтому это лишь сужает неизбежное cross-store окно, а не закрывает его.
    if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Tripo3d) {
        return;
    }
    let credential = match tripo3d_key::credential_from(key.as_str(), &cohort, base_url, &proxy) {
        Ok(credential) => credential,
        Err(_) => {
            tripo3d_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Ключ не прошёл внутреннюю проверку формата. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ Tripo3D credential_from отклонил уже валидированный материал для {}; секреты не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };

    let profile_id = format!("tripo3d-{}", &new_profile_suffix());
    match tripo3d_roster::publish(
        &roster.dir,
        &roster.keyring,
        &roster.active_key_id,
        &profile_id,
        &credential,
    ) {
        Ok(published) => {
            let _ = bot
                .send(
                    chat,
                    if published.replaced_existing {
                        "✅ Аккаунт Tripo3D подключён (обновлён существующий профиль этой подписки)."
                    } else {
                        "✅ Аккаунт Tripo3D подключён."
                    },
                )
                .await;
            complete_seller_job_after_handoff(
                bot,
                store,
                cfg,
                chat,
                Some(expected_job),
                HandoffKind::Tripo3d,
            )
            .await;
        }
        Err(tripo3d_roster::PublishError::Duplicate) => {
            tripo3d_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Такой идентификатор профиля уже занят другим ключом. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ Tripo3D publication hit a profile-id collision для {}; секреты не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
        }
        Err(tripo3d_roster::PublishError::Storage) => {
            tripo3d_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Не удалось сохранить доступ. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                "⚠️ Tripo3D publication failed closed. Проверь права AUTH_BOT_TRIPO3D_DIR, profiles.json и совпадение credential keyring; секреты не логировались.",
                None,
            )
            .await;
        }
    }
}

/// Put the seller on the Suno "account ready" step and show the button that arms the cookie
/// intake. Mirrors `prepare_glm_account`: the Suno branch needs no keyring to reach this step —
/// it is required only once a session is actually sealed. One platform (suno.com), so there is
/// no region selection to reset.
async fn prepare_suno_account(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: Option<&str>,
    proxy_order_id: i64,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    let expected_job = job.job_ref();
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Suno) {
        return;
    }
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let effective_proxy = proxy.unwrap_or(&user.hproxy);
    let effective_order = if proxy.is_some() {
        proxy_order_id
    } else {
        user.hproxy_order
    };
    if effective_proxy.is_empty() {
        // Without an egress the seller must not open the account yet: registration and session
        // validation have to come from the same IP.
        if !store
            .set_handoff_state_for_seller_job(chat, &expected_job, "su_proxy", "", effective_order)
            .unwrap_or(false)
        {
            return;
        }
        let _ = bot.send(chat, SUNO_PROXY_PROMPT).await;
        return;
    }
    // A proxy that only passed the shape check would strand the seller on su_ready: session
    // validation would run against an egress that can never work, with no way to fix it in
    // place. Canonicalise before pinning, exactly like the GLM branch does.
    let replaceable_proxy = job_accepts_seller_proxy(store, &expected_job, effective_order);
    let effective_proxy = match suno_credential::normalize_proxy_url(effective_proxy) {
        Ok(proxy) => proxy,
        Err(_) => {
            elog::error("authbot", format!("[suno-proxy] chat={} canonicalisation rejected proxy: {}", chat,
                proxy_input_fingerprint(effective_proxy)));
            let (retry_proxy, retry_order) = if replaceable_proxy {
                ("", 0)
            } else {
                (effective_proxy, effective_order)
            };
            if !store
                .set_handoff_state_for_seller_job(
                    chat,
                    &expected_job,
                    "su_proxy",
                    retry_proxy,
                    retry_order,
                )
                .unwrap_or(false)
            {
                return;
            }
            let seller_message = if replaceable_proxy {
                "❌ Не удалось разобрать этот прокси. Приём cookie не начат — пришли прокси заново в указанном формате."
            } else {
                "⚠️ Закреплённый за этой позицией прокси имеет неверный формат. Приём cookie не начат; администратор уведомлён."
            };
            let _ = bot.send(chat, seller_message).await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ Suno proxy не прошёл локальную проверку формата для {}. Сетевых запросов не выполнялось; секреты прокси не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
            return;
        }
    };
    if !store
        .set_handoff_state_for_seller_job(
            chat,
            &expected_job,
            "su_ready",
            &effective_proxy,
            effective_order,
        )
        .unwrap_or(false)
    {
        return;
    }
    let _ = bot
        .send_kb(
            chat,
            SUNO_ACCOUNT_SETUP,
            Some(&suno_ready_kb(
                current_handoff_back(store, cfg, chat).as_ref(),
            )),
        )
        .await;
}

/// Return the proxy only for the explicit Suno readiness state. Callback buttons can be old
/// or forwarded, so neither the button itself nor a stored proxy alone authorizes a state
/// transition.
fn suno_ready_handoff(store: &Store, chat: i64) -> Option<(String, i64)> {
    let user = store.get_user(chat).ok().flatten()?;
    if user.want != "su_ready" || user.hproxy.is_empty() {
        return None;
    }
    Some((user.hproxy, user.hproxy_order))
}

async fn continue_suno_handoff(bot: &Bot, store: &Arc<Store>, cfg: &Arc<Config>, chat: i64) {
    let Some((proxy, proxy_order_id)) = suno_ready_handoff(store, chat) else {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже неактивна. Открой актуальное сообщение бота или отправь /start.",
            )
            .await;
        return;
    };
    start_suno_handoff(bot, store, cfg, chat, &proxy, proxy_order_id).await;
}

/// Arm the Suno cookie intake for the seller's current deal.
///
/// Confirming readiness simply moves the deal to `su_wait`, where the seller sends the session
/// cookie as one text message. The cookie is the only credential artifact — the seller never
/// sends a password, 2FA or card data (manifest §2 records this sanctioned artifact).
async fn start_suno_handoff(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    proxy: &str,
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
    if !seller_job_matches_handoff(&job, &expected_job, HandoffKind::Suno) {
        let _ = bot
            .send(
                chat,
                "Текущая сделка не является Suno-сделкой. Открой актуальную карточку через /start.",
            )
            .await;
        return;
    }
    if cfg.suno_roster.is_none() {
        // Without a keyring nothing can be sealed, so the seller must not be sent through a
        // flow whose result we would have to throw away.
        let _ = store.set_want_for_seller_job(chat, &expected_job, "su_ready");
        let _ = bot
            .send_kb(
                chat,
                "⚠️ Подключение Suno сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз этой же кнопкой после исправления.",
                Some(&suno_ready_kb(None)),
            )
            .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ Suno handoff недоступен: не настроен AEAD keyring (AUTH_BOT_SUNO_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    }
    if proxy.is_empty() {
        // The readiness gate already refuses to arm the intake without an egress; fail closed
        // rather than validating a session from a different IP than the account was opened on.
        if store
            .set_handoff_state_for_seller_job(chat, &expected_job, "su_proxy", "", proxy_order_id)
            .unwrap_or(false)
        {
            let _ = bot.send(chat, SUNO_PROXY_PROMPT).await;
        }
        return;
    }
    let _ = store.set_want_for_seller_job(chat, &expected_job, "su_wait");
    let _ = bot
        .send_kb(
            chat,
            SUNO_COOKIE_PROMPT,
            handoff_back_kb(store, cfg, chat).as_ref(),
        )
        .await;
}

/// Declared план продукта оффера. Классификация обязана подтвердить Suno-провайдера, иначе
/// голое слово тарифа (Pro/Premier) не имеет права стать Suno-планом.
fn suno_declared_plan(product: &str) -> Option<suno_credential::SunoPlan> {
    if handoff_kind(product) != HandoffKind::Suno {
        return None;
    }
    let lowered = product.to_lowercase();
    // «premier» не содержит «pro», порядок проверки безопасен.
    for (word, plan) in [
        ("pro", suno_credential::SunoPlan::Pro),
        ("premier", suno_credential::SunoPlan::Premier),
    ] {
        if lowered.contains(word) {
            return Some(plan);
        }
    }
    None
}

/// Единственное, что журнал может узнать о cookie: её длину. Сама cookie не печатается никогда.
fn suno_cookie_fingerprint(cookie: &str) -> String {
    format!("cookie_len={}", cookie.len())
}

/// Вернуть сделку на подтверждение аккаунта после неудачной передачи cookie: ни конверта, ни
/// строки roster, ни завершения выплаты. Подсказки статические — cookie в них не подставляется.
async fn suno_back_to_ready(
    bot: &Bot,
    store: &Arc<Store>,
    chat: i64,
    expected_job: &SellerJobRef,
    message: &str,
) {
    let _ = store.set_want_for_seller_job(chat, expected_job, "su_ready");
    let _ = bot.send_kb(chat, message, Some(&suno_ready_kb(None))).await;
}

/// Приём cookie сессии на шаге `su_wait`. Вся цепочка идёт через egress продавца: аккаунт открыт
/// с этого IP, и проверка с другого адреса — ровно то, что триггерит risk-контроль провайдера.
/// Cookie — секрет уровня Claude setup-token: не логируется, не возвращается эхом в чат и не
/// сохраняется в SQLite в открытом виде; валидация живёт только в памяти.
///
/// Платной admission-песни здесь намеренно нет: одна песня стоит 5 кредитов = $0.02 производных,
/// что превышает штатный бюджет admission micro-smoke $0.0001
/// (`docs/engine/SUNO_PROVIDER.md` §7 — открытый вопрос бюджета, fail closed). Валидация
/// заканчивается на бесплатных probe (discovery → mint → billing).
async fn handle_suno_cookie_message(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    text: &str,
) {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return;
    };
    if !seller_job_matches_handoff(&job, &job.reference, HandoffKind::Suno) {
        return;
    }
    let expected_job = job.job_ref();
    let Some(cookie) = suno_session::cookie_text(text) else {
        let _ = bot
            .send_kb(
                chat,
                SUNO_COOKIE_MALFORMED,
                handoff_back_kb(store, cfg, chat).as_ref(),
            )
            .await;
        return;
    };
    let cookie = zeroize::Zeroizing::new(cookie.to_string());
    let Some(roster) = cfg.suno_roster.clone() else {
        suno_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            "⚠️ Подключение Suno сейчас временно недоступно. Доступ не передан; администратор уведомлён. Попробуй ещё раз после исправления.",
        )
        .await;
        notify_admins(
            bot,
            cfg,
            store,
            "⚠️ Suno handoff недоступен: не настроен AEAD keyring (AUTH_BOT_SUNO_CREDENTIAL_KEYS / _ACTIVE_KID).",
            None,
        )
        .await;
        return;
    };
    let Some(plan) = suno_declared_plan(&job.product) else {
        suno_back_to_ready(
            bot,
            store,
            chat,
            &expected_job,
            "⚠️ Продукт оффера не распознан как план Suno. Доступ не передан; администратор уведомлён.",
        )
        .await;
        notify_admins(
            bot,
            cfg,
            store,
            &format!(
                "⚠️ Suno-сделка {} имеет нераспознанный declared plan; запрос к провайдеру не выполнялся.",
                seller_job_label(&job),
            ),
            None,
        )
        .await;
        return;
    };
    let user = store.get_user(chat).ok().flatten().unwrap_or_default();
    let proxy = user.hproxy.clone();
    if proxy.is_empty() {
        if store
            .set_handoff_state_for_seller_job(
                chat,
                &expected_job,
                "su_proxy",
                "",
                user.hproxy_order,
            )
            .unwrap_or(false)
        {
            let _ = bot.send(chat, SUNO_PROXY_PROMPT).await;
        }
        return;
    }

    // Все три шага бесплатны и идемпотентны (mint JWT — keep-alive сессии), поэтому
    // transport-сбой каждого безопасно повторить с bounded backoff. Отказ 401/403 на любом
    // шаге — финальный вердикт по сессии. Generation guard перепроверяется перед каждым
    // сетевым вызовом: отмена/шаг назад обязаны остановить цепочку, а не опубликоваться в
    // сделку, которая уже ушла дальше.
    macro_rules! free_probe {
        ($attempt:ident, $call:expr) => {{
            loop {
                if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Suno)
                {
                    return;
                }
                match $call.await {
                    Ok(ok) => break ok,
                    Err(_) => {
                        $attempt += 1;
                        if $attempt >= 3 {
                            elog::error("authbot", format!("[suno-session] chat={} validation probe transport failed after {} attempts", chat, $attempt));
                            suno_back_to_ready(bot, store, chat, &expected_job, SUNO_VALIDATION_TRANSPORT)
                                .await;
                            return;
                        }
                        tokio::time::sleep(suno_session::probe_retry_backoff($attempt - 1)).await;
                    }
                }
            }
        }};
    }

    // 1. Clerk session discovery: без неё нет dedup-идентичности, и sealing невозможен.
    let mut attempt = 0u32;
    let session_id = match free_probe!(attempt, suno_session::discover_session(cookie.as_str(), &proxy)) {
        suno_session::SessionProbe::Active { session_id } => session_id,
        suno_session::SessionProbe::Invalid => {
            elog::error("authbot", format!("[suno-session] chat={} session rejected at discovery: {}", chat,
                suno_cookie_fingerprint(cookie.as_str())));
            suno_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                suno_invalid_session_guidance(suno_session::InvalidKeyReason::Auth),
            )
            .await;
            return;
        }
    };

    // 2. Mint короткоживущего JWT по обнаруженной сессии. JWT не персистится.
    let mut attempt = 0u32;
    let jwt = match free_probe!(attempt, suno_session::mint_jwt(cookie.as_str(), &session_id, &proxy)) {
        suno_session::JwtMint::Minted { jwt } => zeroize::Zeroizing::new(jwt),
        suno_session::JwtMint::Invalid => {
            elog::error("authbot", format!("[suno-session] chat={} session rejected at JWT mint", chat));
            suno_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                suno_invalid_session_guidance(suno_session::InvalidKeyReason::Auth),
            )
            .await;
            return;
        }
    };

    // 3. Бесплатный probe квоты подписки: declared план обязан совпасть с наблюдаемым
    // месячным лимитом. Машиночитаемого /me у Suno нет, поэтому corroboration — единственная
    // проверка «продавец активировал тот план».
    let mut attempt = 0u32;
    let snapshot = match free_probe!(attempt, suno_session::probe_billing(jwt.as_str(), cookie.as_str(), &proxy)) {
        suno_session::BillingProbe::Valid(snapshot) => snapshot,
        suno_session::BillingProbe::Invalid => {
            elog::error("authbot", format!("[suno-session] chat={} session rejected at billing probe", chat));
            suno_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                suno_invalid_session_guidance(suno_session::InvalidKeyReason::Auth),
            )
            .await;
            return;
        }
    };
    match suno_session::corroborate_plan(&snapshot, plan) {
        suno_session::PlanVerdict::Confirmed(_) => {}
        suno_session::PlanVerdict::PlanMismatch { .. } => {
            elog::error("authbot", format!("[suno-session] chat={} declared plan contradicts the observed monthly limit", chat));
            suno_back_to_ready(bot, store, chat, &expected_job, SUNO_PLAN_MISMATCH).await;
            return;
        }
        suno_session::PlanVerdict::Unreadable => {
            elog::error("authbot", format!("[suno-session] chat={} billing snapshot cannot corroborate the declared plan", chat));
            suno_back_to_ready(bot, store, chat, &expected_job, SUNO_BILLING_UNREADABLE).await;
            return;
        }
    }

    // Платной admission-песни здесь нет намеренно (см. документацию функции): публикация
    // опирается на бесплатные probe и corroboration, а вопрос бюджета платного допуска
    // записан в манифесте §7 как открытый.

    // Последний generation guard перед любой долговременной записью. SQLite и roster — не одна
    // транзакция, поэтому это лишь сужает неизбежное cross-store окно, а не закрывает его.
    if !seller_handoff_is_current(store, chat, Some(&expected_job), HandoffKind::Suno) {
        return;
    }
    let credential =
        match suno_session::credential_from(cookie.as_str(), &session_id, plan, &proxy) {
            Ok(credential) => credential,
            Err(_) => {
                suno_back_to_ready(
                    bot,
                    store,
                    chat,
                    &expected_job,
                    "⚠️ Сессия не прошла внутреннюю проверку формата. Доступ не передан и выплата не завершена; администратор уведомлён.",
                )
                .await;
                notify_admins(
                    bot,
                    cfg,
                    store,
                    &format!(
                        "⚠️ Suno credential_from отклонил уже валидированный материал для {}; секреты не логировались.",
                        seller_job_label(&job),
                    ),
                    None,
                )
                .await;
                return;
            }
        };

    let profile_id = format!("suno-{}", &new_profile_suffix());
    match suno_roster::publish(
        &roster.dir,
        &roster.keyring,
        &roster.active_key_id,
        &profile_id,
        &credential,
    ) {
        Ok(published) => {
            let _ = bot
                .send(
                    chat,
                    if published.replaced_existing {
                        "✅ Аккаунт Suno подключён (обновлён существующий профиль этой подписки)."
                    } else {
                        "✅ Аккаунт Suno подключён."
                    },
                )
                .await;
            complete_seller_job_after_handoff(
                bot,
                store,
                cfg,
                chat,
                Some(expected_job),
                HandoffKind::Suno,
            )
            .await;
        }
        Err(suno_roster::PublishError::Duplicate) => {
            suno_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Такой идентификатор профиля уже занят другой сессией. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                &format!(
                    "⚠️ Suno publication hit a profile-id collision для {}; секреты не логировались.",
                    seller_job_label(&job),
                ),
                None,
            )
            .await;
        }
        Err(suno_roster::PublishError::Storage) => {
            suno_back_to_ready(
                bot,
                store,
                chat,
                &expected_job,
                "⚠️ Не удалось сохранить доступ. Доступ не передан и выплата не завершена; администратор уведомлён.",
            )
            .await;
            notify_admins(
                bot,
                cfg,
                store,
                "⚠️ Suno publication failed closed. Проверь права AUTH_BOT_SUNO_DIR, profiles.json и совпадение credential keyring; секреты не логировались.",
                None,
            )
            .await;
        }
    }
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

/// Начать Gemini handoff после закрепления постоянного прокси. Один Antigravity consent сам по
/// себе является допуском: подтверждает identity, тариф, проект и реальную генерацию. Telegram не
/// получает ни один authorization code или callback URL.
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
                Some(&gemini_ready_kb(current_handoff_back(store, cfg, chat).as_ref())),
            )
            .await;
        notify_admins(
            bot,
            cfg,
            store,
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
            // Ссылка авторизации идёт обычной гиперссылкой, а не URL-кнопкой: кнопка Telegram
            // открывает встроенный браузер клиента, то есть чужой профиль и чужой egress, — ровно
            // то, что здесь запрещено. Гиперссылку продавец копирует в подготовленный
            // антидетект-профиль.
            let _ = bot
                .send(
                    chat,
                    &format!(
                        "🔗 <b>Этап 3 из 3 — подтверди доступ Antigravity</b>\n\n1️⃣ Не закрывая подготовленный антидетект-профиль и не меняя прокси, открой в нём официальную ссылку: <a href=\"{}\">авторизация Google для Antigravity</a>. <b>Не открывай её в Telegram, обычном браузере или на телефоне.</b>\n\n2️⃣ Войди именно в новый Google-аккаунт и подтверди доступ.",
                        esc(&links.authorize_url)
                    ),
                )
                .await;
            let _ = bot
                .send_url_button(
                    chat,
                    "3️⃣ После согласия Google перенаправит на <code>localhost:51121</code>, и страница может не открыться — это нормально. Скопируй весь адрес из адресной строки, нажми кнопку ниже и вставь его в защищённую форму. Не отправляй адрес сообщением в Telegram.\n\n4️⃣ Подписка и выплата завершатся после реальной тестовой генерации на этом аккаунте.",
                    "Подтвердить подключение",
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
                        .send_kb(
                            chat,
                            error.public_message(),
                            Some(&gemini_ready_kb(
                                current_handoff_back(store, cfg, chat).as_ref(),
                            )),
                        )
                        .await;
                }
            }
        }
    }
}

const GEMINI_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси, а закреплённого за этой позицией нет — поэтому <code>повторить</code> здесь не сработает, переиспользовать нечего. Пришли прокси как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

const KIMI_STEP_PROXY_RETRY: &str = "🤔 Не разобрал прокси. Пришли его как <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code> одним сообщением.";

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
        HandoffKind::Kimi => ("km_proxy", "km_ready"),
        HandoffKind::Glm => ("glm_proxy", "glm_ready"),
        HandoffKind::Tripo3d => ("t3_proxy", "t3_ready"),
        HandoffKind::Suno => ("su_proxy", "su_ready"),
    }
}

/// Куда вернётся продавец, нажав «назад», и что при этом произойдёт.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HandoffStepBack {
    /// Шаг, на который вернётся продавец.
    pub(crate) target: &'static str,
    /// Целевой шаг — ввод прокси: `hproxy` очищается, а `hproxy_order` обязан пережить откат.
    pub(crate) clears_proxy: bool,
    /// На текущем шаге уже выдана одноразовая ссылка или код: нужен явный confirm и teardown.
    pub(crate) invalidates_link: bool,
}

/// Ровно один шаг назад внутри ветки передачи доступа.
///
/// Единственный источник истины и для кнопки, и для мутации: `None` здесь означает и что кнопка
/// не рисуется, и что callback вместе со словом «назад» отказывают. Решение принимается только по
/// этим четырём входам, а состояние читается заново на каждом вызове — истории шагов нет и она не
/// нужна, потому что ветка это линия.
///
/// * `proxy_replaceable` — из [`job_accepts_seller_proxy`]. У закреплённого прокси покупателя и у
///   живого IPRoyal-лиза шага «ввод прокси» в истории продавца просто нет, поэтому назад некуда.
/// * `pinned_egress_known` — важен только для `gm_wait`, где прокси лежит не в `users.hproxy`
///   (его стирает `start_gemini_oauth`), а внутри запечатанной PKCE-транзакции. Остальные шаги
///   держат прокси в `users` и передают `true`.
pub(crate) fn handoff_step_back(
    kind: HandoffKind,
    want: &str,
    proxy_replaceable: bool,
    pinned_egress_known: bool,
) -> Option<HandoffStepBack> {
    let to_proxy_step = |target: &'static str, invalidates_link: bool| {
        proxy_replaceable.then_some(HandoffStepBack {
            target,
            clears_proxy: true,
            invalidates_link,
        })
    };
    match (kind, want) {
        (HandoffKind::Claude, "ho_email") => to_proxy_step("ho_proxy", false),
        (HandoffKind::Claude, "ho_code") => Some(HandoffStepBack {
            target: "ho_email",
            clears_proxy: false,
            invalidates_link: true,
        }),
        (HandoffKind::Codex, "cx_email") => to_proxy_step("cx_proxy", false),
        (HandoffKind::Codex, "cx_wait") => Some(HandoffStepBack {
            target: "cx_email",
            clears_proxy: false,
            invalidates_link: true,
        }),
        (HandoffKind::Kimi, "km_ready") => to_proxy_step("km_proxy", false),
        // Mirrors the Gemini edge: stepping back from an issued device code must invalidate it,
        // and without a recoverable egress it degrades to the proxy step rather than landing on
        // km_ready with an empty hproxy, which kimi_ready_handoff rejects.
        (HandoffKind::Kimi, "km_wait") => {
            if pinned_egress_known {
                Some(HandoffStepBack {
                    target: "km_ready",
                    clears_proxy: false,
                    invalidates_link: true,
                })
            } else if proxy_replaceable {
                Some(HandoffStepBack {
                    target: "km_proxy",
                    clears_proxy: true,
                    invalidates_link: true,
                })
            } else {
                None
            }
        }
        (HandoffKind::Glm, "glm_ready") => to_proxy_step("glm_proxy", false),
        // Mirrors the KIMI edge: stepping back from the key intake must cancel the pending key
        // submission, and without a recoverable egress it degrades to the proxy step rather
        // than landing on glm_ready with an empty hproxy, which glm_ready_handoff rejects.
        (HandoffKind::Glm, "glm_wait") => {
            if pinned_egress_known {
                Some(HandoffStepBack {
                    target: "glm_ready",
                    clears_proxy: false,
                    invalidates_link: true,
                })
            } else if proxy_replaceable {
                Some(HandoffStepBack {
                    target: "glm_proxy",
                    clears_proxy: true,
                    invalidates_link: true,
                })
            } else {
                None
            }
        }
        (HandoffKind::Tripo3d, "t3_ready") => to_proxy_step("t3_proxy", false),
        // Mirrors the GLM edge: stepping back from the key intake must cancel the pending key
        // submission, and without a recoverable egress it degrades to the proxy step rather
        // than landing on t3_ready with an empty hproxy, which tripo3d_ready_handoff rejects.
        (HandoffKind::Tripo3d, "t3_wait") => {
            if pinned_egress_known {
                Some(HandoffStepBack {
                    target: "t3_ready",
                    clears_proxy: false,
                    invalidates_link: true,
                })
            } else if proxy_replaceable {
                Some(HandoffStepBack {
                    target: "t3_proxy",
                    clears_proxy: true,
                    invalidates_link: true,
                })
            } else {
                None
            }
        }
        (HandoffKind::Suno, "su_ready") => to_proxy_step("su_proxy", false),
        // Mirrors the GLM edge: stepping back from the cookie intake must cancel the pending
        // submission, and without a recoverable egress it degrades to the proxy step rather
        // than landing on su_ready with an empty hproxy, which suno_ready_handoff rejects.
        (HandoffKind::Suno, "su_wait") => {
            if pinned_egress_known {
                Some(HandoffStepBack {
                    target: "su_ready",
                    clears_proxy: false,
                    invalidates_link: true,
                })
            } else if proxy_replaceable {
                Some(HandoffStepBack {
                    target: "su_proxy",
                    clears_proxy: true,
                    invalidates_link: true,
                })
            } else {
                None
            }
        }
        (HandoffKind::Gemini, "gm_ready") => to_proxy_step("gm_gproxy", false),
        // Единственное двухисходное ребро. Без восстановленного egress шаг назад привёл бы на
        // `gm_ready` с пустым `hproxy`, который `gemini_ready_handoff` отвергает — это тупик,
        // поэтому деградируем до ввода прокси, и только когда продавцу вообще можно его менять.
        (HandoffKind::Gemini, "gm_wait") => {
            if pinned_egress_known {
                Some(HandoffStepBack {
                    target: "gm_ready",
                    clears_proxy: false,
                    invalidates_link: true,
                })
            } else {
                to_proxy_step("gm_gproxy", true)
            }
        }
        // Первые шаги веток, legacy-состояния Gemini, регистрация и любая пара «не тот kind ×
        // не тот шаг» предшественника не имеют.
        _ => None,
    }
}

/// Имя шага на проводе callback-кнопки.
///
/// Совпадает с `want`, но проходит через явный whitelist: callback data приходит от пользователя,
/// и произвольная строка не должна попадать в резолвер даже теоретически. Функция работает в обе
/// стороны — и для отрисовки кнопки, и для разбора нажатия.
fn back_step_wire(want: &str) -> Option<&'static str> {
    match want {
        "ho_email" => Some("ho_email"),
        "ho_code" => Some("ho_code"),
        "cx_email" => Some("cx_email"),
        "cx_wait" => Some("cx_wait"),
        "gm_ready" => Some("gm_ready"),
        "gm_wait" => Some("gm_wait"),
        "km_ready" => Some("km_ready"),
        "km_wait" => Some("km_wait"),
        "glm_ready" => Some("glm_ready"),
        "glm_wait" => Some("glm_wait"),
        "t3_ready" => Some("t3_ready"),
        "t3_wait" => Some("t3_wait"),
        "su_ready" => Some("su_ready"),
        "su_wait" => Some("su_wait"),
        _ => None,
    }
}

/// Продавец просит шаг назад словом, а не кнопкой. Отдельно от `повторить`: то ребро означает
/// «тот же прокси, новое поколение, остаться на шаге» и обязано продолжать работать как раньше.
fn is_handoff_back(input: &str) -> bool {
    matches!(
        input.trim().to_lowercase().as_str(),
        "назад" | "back" | "/back"
    )
}

/// Строка клавиатуры под кнопку «назад». Подпись называет последствие, а не механизм.
fn handoff_back_row(step: &HandoffStepBack, from_wire: &str) -> Vec<(String, String)> {
    let label = if step.clears_proxy {
        "↩️ Изменить прокси"
    } else if step.target == "gm_ready" {
        "↩️ Назад: новая ссылка"
    } else if step.target == "glm_ready" || step.target == "t3_ready" || step.target == "su_ready" {
        "↩️ Назад: подтверждение аккаунта"
    } else {
        "↩️ Назад: другой email"
    };
    // Одноразовую ссылку молча не гасим: сначала явное подтверждение.
    let action = if step.invalidates_link { "ask" } else { "go" };
    vec![(label.into(), format!("hoback:{from_wire}:{action}"))]
}

/// Чем сейчас является «шаг назад» для этого продавца.
enum HandoffBack {
    /// Откат возможен: работа, ребро и egress, который надо восстановить на целевом шаге.
    Ready {
        job: SellerJob,
        step: HandoffStepBack,
        egress: Option<(String, i64)>,
    },
    /// Callback уже забрал одноразовый код — откатывать поздно, надо дождаться результата.
    Busy,
    /// Откатывать некуда: первый шаг ветки, закреплённый прокси или неподходящая фаза работы.
    Nowhere,
}

/// Единственный путь чтения состояния: им пользуются и отрисовка кнопки, и callback, и слово
/// «назад», и `/cancel`. Ровно поэтому UI и мутация не могут разойтись.
fn resolve_handoff_back(store: &Store, cfg: &Config, chat: i64) -> HandoffBack {
    let Some(job) = store.active_seller_job(chat).ok().flatten() else {
        return HandoffBack::Nowhere;
    };
    let kind = handoff_kind(&job.product);
    if !seller_job_matches_handoff(&job, &job.reference, kind) {
        return HandoffBack::Nowhere;
    }
    let Some(user) = store.get_user(chat).ok().flatten() else {
        return HandoffBack::Nowhere;
    };
    if kind == HandoffKind::Gemini
        && user.want == "gm_wait"
        && store.gemini_oauth_in_flight(chat).unwrap_or(false)
    {
        return HandoffBack::Busy;
    }
    let proxy_replaceable = job_accepts_seller_proxy(store, &job.reference, user.hproxy_order);
    // Egress восстанавливаем только там, где `users.hproxy` его уже не хранит.
    let egress = if kind == HandoffKind::Gemini && user.want == "gm_wait" {
        cfg.gemini_oauth
            .as_ref()
            .and_then(|oauth| gemini_oauth::pending_egress(store, oauth, chat))
            .or_else(|| pinned_job_egress(store, &job.reference))
    } else {
        None
    };
    let pinned_egress_known = user.want != "gm_wait" || egress.is_some();
    match handoff_step_back(kind, &user.want, proxy_replaceable, pinned_egress_known) {
        Some(step) => HandoffBack::Ready { job, step, egress },
        None => HandoffBack::Nowhere,
    }
}

/// Закреплённый egress работы как источник последней надежды для `gm_wait`: PKCE-транзакция могла
/// истечь, но прокси покупателя никуда не делся, и спрашивать его у продавца нельзя.
fn pinned_job_egress(store: &Store, expected: &SellerJobRef) -> Option<(String, i64)> {
    match expected.kind.as_str() {
        "offer" => store
            .get_offer(expected.offer_id)
            .ok()
            .flatten()
            .filter(|offer| !offer.buyer_proxy.is_empty())
            .map(|offer| (offer.buyer_proxy, 0)),
        "batch" => store
            .batch_items(expected.batch_id)
            .ok()?
            .into_iter()
            .find(|item| item.item_no == expected.item_no && !item.proxy.is_empty())
            .map(|item| (item.proxy, 0)),
        _ => None,
    }
}

/// Разрешённый прямо сейчас шаг назад, без остальных подробностей резолвера.
fn current_handoff_back(store: &Store, cfg: &Config, chat: i64) -> Option<HandoffStepBack> {
    match resolve_handoff_back(store, cfg, chat) {
        HandoffBack::Ready { step, .. } => Some(step),
        _ => None,
    }
}

/// Клавиатура «назад» для текущего шага — ровно тогда, когда резолвер разрешает шаг назад.
fn handoff_back_kb(store: &Store, cfg: &Config, chat: i64) -> Option<Keyboard> {
    let step = current_handoff_back(store, cfg, chat)?;
    let want = store.get_user(chat).ok().flatten()?.want;
    Some(vec![handoff_back_row(&step, back_step_wire(&want)?)])
}

/// Текст подтверждения для шага, на котором уже выдана одноразовая ссылка или код.
///
/// Продавец обязан узнать не только что старая ссылка умрёт, но и что уже подтверждённый в
/// браузере доступ может всё равно засчитаться: этой гонки не избежать, её можно только назвать.
fn handoff_back_confirm_text(want: &str) -> &'static str {
    match want {
        "ho_code" => "⚠️ Вернуться к вводу email?\n\nВыданная ссылка авторизации перестанет работать, и бот выдаст новую. Если ты <b>уже подтвердил доступ</b> в браузере, аккаунт может всё равно засчитаться — тогда напиши администратору, чтобы он сверил результат.",
        "km_wait" => "⚠️ Вернуться к подтверждению аккаунта?\n\nВыданный код устройства Kimi перестанет работать, и бот выдаст новый. Если ты <b>уже подтвердил вход</b>, аккаунт может всё равно засчитаться — тогда напиши администратору.",
        "glm_wait" => "⚠️ Вернуться к подтверждению аккаунта?\n\nОжидание API-ключа будет сброшено: чтобы передать ключ, нажми «Аккаунт готов — продолжить» ещё раз. Если ты <b>уже прислал ключ</b> и бот его проверяет, аккаунт может всё равно засчитаться — тогда напиши администратору.",
        "t3_wait" => "⚠️ Вернуться к подтверждению аккаунта?\n\nОжидание API-ключа будет сброшено: чтобы передать ключ, нажми «Аккаунт готов — продолжить» ещё раз. Если ты <b>уже прислал ключ</b> и бот его проверяет, аккаунт может всё равно засчитаться — тогда напиши администратору.",
        "su_wait" => "⚠️ Вернуться к подтверждению аккаунта?\n\nОжидание cookie будет сброшено: чтобы передать cookie, нажми «Аккаунт готов — продолжить» ещё раз. Если ты <b>уже прислал cookie</b> и бот её проверяет, аккаунт может всё равно засчитаться — тогда напиши администратору.",
        "cx_wait" => "⚠️ Вернуться к вводу email?\n\nВыданный одноразовый код ChatGPT перестанет работать, и бот выдаст новый. Если ты <b>уже подтвердил вход</b>, аккаунт может всё равно засчитаться — тогда напиши администратору.",
        _ => "⚠️ Вернуться на шаг назад?\n\nВыданная ссылка авторизации перестанет работать, и бот выдаст новую с тем же прокси. Если ты <b>уже подтвердил доступ</b> в браузере, аккаунт может всё равно засчитаться — тогда напиши администратору.",
    }
}

/// Единая точка входа для кнопки, слова «назад» и `/cancel`.
///
/// `confirmed` приходит только из явного подтверждения; для рёбер, гасящих одноразовую ссылку,
/// без него показывается подтверждение, а состояние не двигается.
async fn offer_handoff_back(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    confirmed: bool,
) {
    match resolve_handoff_back(store, cfg, chat) {
        HandoffBack::Busy => {
            let _ = bot
                .send(
                    chat,
                    "⏳ Бот уже проверяет твою авторизацию — вернуться назад сейчас нельзя. Дождись результата: он придёт сюда сам.",
                )
                .await;
        }
        HandoffBack::Nowhere => {
            let _ = bot
                .send(
                    chat,
                    "Назад отсюда некуда: это первый шаг сделки либо прокси закреплён за позицией и заменить его нельзя.",
                )
                .await;
            let want = store
                .get_user(chat)
                .ok()
                .flatten()
                .map(|user| user.want)
                .unwrap_or_default();
            send_handoff_step_card(bot, store, cfg, chat, &want, false).await;
        }
        HandoffBack::Ready { job, step, egress } => {
            let Some(user) = store.get_user(chat).ok().flatten() else {
                return;
            };
            if step.invalidates_link && !confirmed {
                let Some(wire) = back_step_wire(&user.want) else {
                    return;
                };
                let keyboard = vec![vec![(
                    "⚠️ Да, вернуться на шаг назад".to_string(),
                    format!("hoback:{wire}:go"),
                )]];
                let _ = bot
                    .send_kb(chat, handoff_back_confirm_text(&user.want), Some(&keyboard))
                    .await;
                return;
            }
            apply_handoff_back(bot, store, cfg, chat, &job, &step, &user, egress).await;
        }
    }
}

/// Выполнить ровно один шаг назад: сначала атомарная запись, затем teardown.
///
/// Порядок принципиален. Guarded `UPDATE` — единственная точка сериализации: всё, снесённое до
/// него, конкурентный обработчик с ещё актуальным поколением может законно пересоздать, а после
/// него любой путь завершения уже пишет устаревшим токеном и проваливается fail-closed. Teardown
/// поэтому — освобождение ресурсов, а не защита инварианта.
#[allow(clippy::too_many_arguments)]
async fn apply_handoff_back(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    job: &SellerJob,
    step: &HandoffStepBack,
    user: &crate::db::UserRow,
    egress: Option<(String, i64)>,
) {
    // `hproxy_order` — единственная ручка на оплаченный IPRoyal lease, поэтому переносим текущий,
    // а не пишем ноль. На шаге ввода прокси стирается только сам прокси.
    let proxy = if step.clears_proxy {
        Some((String::new(), user.hproxy_order))
    } else {
        egress
    };
    let rewound = store
        .rewind_handoff_step(
            chat,
            &job.reference,
            &user.want,
            step.target,
            proxy
                .as_ref()
                .map(|(proxy, order)| (proxy.as_str(), *order)),
        )
        .unwrap_or(None);
    if rewound.is_none() {
        let _ = bot
            .send(
                chat,
                "Эта кнопка уже устарела — состояние сделки изменилось. Ниже актуальный шаг.",
            )
            .await;
        let want = store
            .get_user(chat)
            .ok()
            .flatten()
            .map(|user| user.want)
            .unwrap_or_default();
        send_handoff_step_card(bot, store, cfg, chat, &want, false).await;
        return;
    }
    // Дочерние процессы вызываем безусловно: без активной capability это no-op, зато утёкший от
    // прошлой ошибки процесс не переживёт откат.
    setup_token::kill(chat);
    crate::codex_login::cancel(chat);
    send_handoff_step_card(bot, store, cfg, chat, step.target, true).await;
}

/// Переиздать карточку шага, на который вернулся продавец.
async fn send_handoff_step_card(
    bot: &Bot,
    store: &Arc<Store>,
    cfg: &Arc<Config>,
    chat: i64,
    step: &str,
    rewound: bool,
) {
    // Префикс «вернулись» врать не имеет права: тем же путём карточка переиздаётся и при отказе,
    // когда состояние никуда не двигалось.
    let moved = |text: &str| {
        if rewound {
            format!("↩️ {text}\n\n")
        } else {
            String::new()
        }
    };
    let back = current_handoff_back(store, cfg, chat);
    let back_kb = back.as_ref().and_then(|back| {
        let want = store.get_user(chat).ok().flatten()?.want;
        Some(vec![handoff_back_row(back, back_step_wire(&want)?)])
    });
    match step {
        "ho_proxy" | "cx_proxy" | "gm_gproxy" | "km_proxy" | "glm_proxy" | "t3_proxy" | "su_proxy" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "{}{}",
                        moved("Вернулись к вводу прокси. Прошлый прокси сброшен — следующий заменит его."),
                        proxy_prompt(step)
                    ),
                    back_kb.as_ref(),
                )
                .await;
        }
        "ho_email" | "cx_email" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "↩️ Вернулись к вводу email.\n\n{}",
                        account_setup_prompt(step)
                    ),
                    back_kb.as_ref(),
                )
                .await;
        }
        "gm_ready" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "{}{GEMINI_ACCOUNT_SETUP}",
                        moved("Вернулись на подтверждение аккаунта. Прокси сохранён.")
                    ),
                    Some(&gemini_ready_kb(back.as_ref())),
                )
                .await;
        }
        "km_ready" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "{}{KIMI_ACCOUNT_SETUP}",
                        moved("Вернулись на подтверждение аккаунта. Прокси сохранён.")
                    ),
                    Some(&kimi_ready_kb(back.as_ref())),
                )
                .await;
        }
        "glm_ready" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "{}{GLM_ACCOUNT_SETUP}",
                        moved("Вернулись на подтверждение аккаунта. Прокси сохранён.")
                    ),
                    Some(&glm_ready_kb(back.as_ref())),
                )
                .await;
        }
        "t3_ready" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "{}{TRIPO3D_ACCOUNT_SETUP}",
                        moved("Вернулись на подтверждение аккаунта. Прокси сохранён.")
                    ),
                    Some(&tripo3d_ready_kb(back.as_ref())),
                )
                .await;
        }
        "su_ready" => {
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "{}{SUNO_ACCOUNT_SETUP}",
                        moved("Вернулись на подтверждение аккаунта. Прокси сохранён.")
                    ),
                    Some(&suno_ready_kb(back.as_ref())),
                )
                .await;
        }
        _ => {
            let _ = bot
                .send(chat, "Открой актуальную карточку сделки через /jobs.")
                .await;
        }
    }
}

/// A seller-proxy job may replace its proxy after a failed OAuth transaction. Buyer-proxy jobs
/// and legacy jobs backed by an issued IPRoyal order remain pinned to their assigned egress.
///
/// New bot-issued handoffs persist `hproxy_order` for every provider. The durable
/// `offers.proxy_issued` flag remains the fail-closed authority for legacy rows created before that
/// propagation existed, and an unreadable offer is never treated as replaceable.
pub(crate) fn job_accepts_seller_proxy(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy_order_id: i64,
) -> bool {
    match expected.kind.as_str() {
        "batch" => store
            .get_batch(expected.batch_id)
            .ok()
            .flatten()
            .is_some_and(|batch| batch.proxy_source == PROXY_SOURCE_SELLER),
        "offer" => store
            .get_offer(expected.offer_id)
            .ok()
            .flatten()
            .is_some_and(|offer| match offer.proxy_source.as_str() {
                PROXY_SOURCE_SELLER => true,
                PROXY_SOURCE_LEGACY => {
                    current_proxy_order_id == 0
                        && !store.offer_proxy_issued(expected.offer_id).unwrap_or(true)
                }
                _ => false,
            }),
        _ => false,
    }
}

/// Historical name kept for the Gemini retry path in `gemini_oauth`.
pub(crate) fn gemini_job_accepts_proxy_input(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy_order_id: i64,
) -> bool {
    job_accepts_seller_proxy(store, expected, current_proxy_order_id)
}

#[derive(Debug, PartialEq, Eq)]
enum GeminiProxyRetry {
    /// URL продавца плюс признак того, что в нём распознаны логин и пароль.
    SellerSupplied(String, bool),
    Retained(String, i64),
    Fixed(String, i64),
    Invalid,
}

fn select_gemini_proxy_retry(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy: &str,
    current_proxy_order_id: i64,
    input: &str,
) -> GeminiProxyRetry {
    if job_accepts_seller_proxy(store, expected, current_proxy_order_id) {
        if is_gemini_proxy_retry(input) && !current_proxy.is_empty() {
            return GeminiProxyRetry::Retained(current_proxy.to_string(), current_proxy_order_id);
        }
        let parsed = parse_proxy_input(input);
        return if parsed.url.is_empty() {
            GeminiProxyRetry::Invalid
        } else {
            GeminiProxyRetry::SellerSupplied(parsed.url, parsed.credentials)
        };
    }
    if current_proxy.is_empty() {
        GeminiProxyRetry::Invalid
    } else {
        GeminiProxyRetry::Fixed(current_proxy.to_string(), current_proxy_order_id)
    }
}

fn is_gemini_proxy_retry(input: &str) -> bool {
    matches!(
        input.trim().to_lowercase().as_str(),
        "повторить" | "повтори" | "retry"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum KimiProxyInput {
    /// URL продавца плюс признак того, что в нём распознаны логин и пароль.
    SellerSupplied(String, bool),
    /// Закреплённый прокси покупателя/IPRoyal: сообщение продавца его заменить не может.
    Fixed(String, i64),
    /// Ввод не похож на прокси, а закреплённого egress подставить нечего.
    Invalid,
}

/// Решение шага `km_proxy`. Зеркалит Gemini-шаг без слова «повторить»: одноразовый device-код
/// живёт на более позднем шаге, поэтому здесь сообщение продавца — это всегда ввод egress, а не
/// перезапуск авторизации. Закреплённый buyer/IPRoyal прокси сообщение заменить не может никогда.
fn select_kimi_proxy_input(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy: &str,
    current_proxy_order_id: i64,
    input: &str,
) -> KimiProxyInput {
    if job_accepts_seller_proxy(store, expected, current_proxy_order_id) {
        let parsed = parse_proxy_input(input);
        return if parsed.url.is_empty() {
            KimiProxyInput::Invalid
        } else {
            KimiProxyInput::SellerSupplied(parsed.url, parsed.credentials)
        };
    }
    if current_proxy.is_empty() {
        KimiProxyInput::Invalid
    } else {
        KimiProxyInput::Fixed(current_proxy.to_string(), current_proxy_order_id)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GlmProxyInput {
    /// URL продавца плюс признак того, что в нём распознаны логин и пароль.
    SellerSupplied(String, bool),
    /// Закреплённый прокси покупателя/IPRoyal: сообщение продавца его заменить не может.
    Fixed(String, i64),
    /// Ввод не похож на прокси, а закреплённого egress подставить нечего.
    Invalid,
}

/// Решение шага `glm_proxy`. Зеркалит KIMI-шаг: приём ключа живёт на более позднем шаге, поэтому
/// здесь сообщение продавца — это всегда ввод egress. Закреплённый buyer/IPRoyal прокси
/// сообщение продавца заменить не может никогда.
fn select_glm_proxy_input(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy: &str,
    current_proxy_order_id: i64,
    input: &str,
) -> GlmProxyInput {
    if job_accepts_seller_proxy(store, expected, current_proxy_order_id) {
        let parsed = parse_proxy_input(input);
        return if parsed.url.is_empty() {
            GlmProxyInput::Invalid
        } else {
            GlmProxyInput::SellerSupplied(parsed.url, parsed.credentials)
        };
    }
    if current_proxy.is_empty() {
        GlmProxyInput::Invalid
    } else {
        GlmProxyInput::Fixed(current_proxy.to_string(), current_proxy_order_id)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Tripo3dProxyInput {
    /// URL продавца плюс признак того, что в нём распознаны логин и пароль.
    SellerSupplied(String, bool),
    /// Закреплённый прокси покупателя/IPRoyal: сообщение продавца его заменить не может.
    Fixed(String, i64),
    /// Ввод не похож на прокси, а закреплённого egress подставить нечего.
    Invalid,
}

/// Решение шага `t3_proxy`. Зеркалит GLM-шаг: приём ключа живёт на более позднем шаге, поэтому
/// здесь сообщение продавца — это всегда ввод egress. Закреплённый buyer/IPRoyal прокси
/// сообщение продавца заменить не может никогда.
fn select_tripo3d_proxy_input(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy: &str,
    current_proxy_order_id: i64,
    input: &str,
) -> Tripo3dProxyInput {
    if job_accepts_seller_proxy(store, expected, current_proxy_order_id) {
        let parsed = parse_proxy_input(input);
        return if parsed.url.is_empty() {
            Tripo3dProxyInput::Invalid
        } else {
            Tripo3dProxyInput::SellerSupplied(parsed.url, parsed.credentials)
        };
    }
    if current_proxy.is_empty() {
        Tripo3dProxyInput::Invalid
    } else {
        Tripo3dProxyInput::Fixed(current_proxy.to_string(), current_proxy_order_id)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SunoProxyInput {
    /// URL продавца плюс признак того, что в нём распознаны логин и пароль.
    SellerSupplied(String, bool),
    /// Закреплённый прокси покупателя/IPRoyal: сообщение продавца его заменить не может.
    Fixed(String, i64),
    /// Ввод не похож на прокси, а закреплённого egress подставить нечего.
    Invalid,
}

/// Решение шага `su_proxy`. Зеркалит GLM-шаг: приём cookie живёт на более позднем шаге,
/// поэтому здесь сообщение продавца — это всегда ввод egress. Закреплённый buyer/IPRoyal
/// прокси сообщение продавца заменить не может никогда.
fn select_suno_proxy_input(
    store: &Store,
    expected: &SellerJobRef,
    current_proxy: &str,
    current_proxy_order_id: i64,
    input: &str,
) -> SunoProxyInput {
    if job_accepts_seller_proxy(store, expected, current_proxy_order_id) {
        let parsed = parse_proxy_input(input);
        return if parsed.url.is_empty() {
            SunoProxyInput::Invalid
        } else {
            SunoProxyInput::SellerSupplied(parsed.url, parsed.credentials)
        };
    }
    if current_proxy.is_empty() {
        SunoProxyInput::Invalid
    } else {
        SunoProxyInput::Fixed(current_proxy.to_string(), current_proxy_order_id)
    }
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
                store,
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
        let setup =
            if next_step == "gm_ready" || next_step == "km_ready" || next_step == "glm_ready" || next_step == "t3_ready" || next_step == "su_ready" {
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
            prepare_gemini_account(bot, store, cfg, seller_chat, None, 0).await;
        } else if next_step == "km_ready" {
            prepare_kimi_account(bot, store, cfg, seller_chat, None, 0).await;
        } else if next_step == "glm_ready" {
            prepare_glm_account(bot, store, cfg, seller_chat, None, 0).await;
        } else if next_step == "t3_ready" {
            prepare_tripo3d_account(bot, store, cfg, seller_chat, None, 0).await;
        } else if next_step == "su_ready" {
            prepare_suno_account(bot, store, cfg, seller_chat, None, 0).await;
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
                    proxy_prompt(proxy_step)
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
            store,
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
            store,
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
            store,
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
                store,
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
            store,
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
            store,
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
            store,
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
            store,
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
            store,
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
    let proxy_order_id = store
        .get_user(chat)
        .ok()
        .flatten()
        .map(|user| user.hproxy_order)
        .unwrap_or(0);
    let (bin, dir, em, px) = (
        cfg.codex_bin.clone(),
        cfg.codex_homes_dir.clone(),
        email.trim().to_string(),
        proxy.to_string(),
    );
    let _ = bot.send(chat, "⏳ Готовлю авторизацию ChatGPT…").await;
    let started = tokio::task::spawn_blocking({
        let (em, px, bin, dir) = (em.clone(), px.clone(), bin.clone(), dir.clone());
        move || crate::codex_login::start(chat, &em, &px, proxy_order_id, &bin, &dir)
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
            .set_want_for_seller_job(chat, expected, "cx_wait")
            .unwrap_or(false)
    }) {
        crate::codex_login::cancel(chat);
        notify_admins(
            bot,
            cfg,
            store,
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
            Ok(crate::codex_login::Outcome::Authorized {
                label,
                has_proxy,
                profile_id,
                proxy_order_id,
                issued_at,
                canonical_ip,
            }) => {
                let binding = match (proxy_order_id, canonical_ip) {
                    (0, _) => Ok(()),
                    (_, Some(allocation_ip)) => store2
                        .upsert_proxy_binding_allocation(
                            "codex",
                            &profile_id,
                            proxy_order_id,
                            &allocation_ip.to_string(),
                            issued_at,
                            ProxyAuthorityStatus::Local,
                        )
                        .map(|_| ()),
                    (_, None) => Err(anyhow::anyhow!(
                        "managed Codex proxy host is not a literal allocation IP"
                    )),
                };
                if binding.is_err() {
                    notify_admins(
                        &bot2,
                        &cfg2,
                        &store2,
                        "⚠️ Codex опубликован в roster, но lifecycle binding не записан. Сделка оставлена незавершённой; публикацию не откатывать, требуется reconciliation.",
                        None,
                    )
                    .await;
                    return;
                }
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
                    notify_admins(&bot2, &cfg2, &store2, &format!(
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
                    px.order_id,
                )
                .unwrap_or(false)
            {
                notify_admins(
                    bot,
                    cfg,
                    store,
                    &format!(
                        "⚠️ Прокси IPRoyal для оффера #{} выпущен (заказ #{}), но активная работа продавца уже изменилась. Прокси не отправлен; нужна ручная проверка.",
                        oid, px.order_id
                    ),
                    None,
                )
                .await;
                return;
            }
            let next_prompt = if gemini || next_step == "km_ready" || next_step == "glm_ready" || next_step == "t3_ready" || next_step == "su_ready" {
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
                prepare_gemini_account(bot, store, cfg, seller_chat, None, px.order_id).await;
            } else if next_step == "km_ready" {
                prepare_kimi_account(bot, store, cfg, seller_chat, None, 0).await;
            } else if next_step == "glm_ready" {
                prepare_glm_account(bot, store, cfg, seller_chat, None, 0).await;
            } else if next_step == "t3_ready" {
                prepare_tripo3d_account(bot, store, cfg, seller_chat, None, 0).await;
            } else if next_step == "su_ready" {
                prepare_suno_account(bot, store, cfg, seller_chat, None, 0).await;
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
                store,
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
            store,
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
    let setup = if next_step == "gm_ready" || next_step == "km_ready" || next_step == "glm_ready" || next_step == "t3_ready" || next_step == "su_ready" {
        ""
    } else {
        account_setup_prompt(next_step)
    };
    let _ = bot.send(seller_chat, &format!(
        "📦 <b>Оффер #{} · {}</b>\n\n💸 <b>Оплата отправлена!</b> tx: <code>{}</code>\n\n✅ <b>Прокси покупателя для аккаунта:</b>\n<code>{}</code>\n\n{}",
        oid, esc(&offer.product), esc(hash), esc(&offer.buyer_proxy), setup
    )).await;
    if next_step == "gm_ready" {
        prepare_gemini_account(bot, store, cfg, seller_chat, None, 0).await;
    } else if next_step == "km_ready" {
        prepare_kimi_account(bot, store, cfg, seller_chat, None, 0).await;
    } else if next_step == "glm_ready" {
        prepare_glm_account(bot, store, cfg, seller_chat, None, 0).await;
    } else if next_step == "t3_ready" {
        prepare_tripo3d_account(bot, store, cfg, seller_chat, None, 0).await;
    } else if next_step == "su_ready" {
        prepare_suno_account(bot, store, cfg, seller_chat, None, 0).await;
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
        proxy_prompt(proxy_step)
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
    if data == "kimi:ready" {
        continue_kimi_handoff(bot, store, cfg, chat).await;
        return;
    }

    if data == "glm:ready" {
        continue_glm_handoff(bot, store, cfg, chat).await;
        return;
    }

    // Platform selection for the GLM key intake: int (api.z.ai, default) or cn (bigmodel.cn).
    if let Some(region) = data.strip_prefix("glm:region:") {
        select_glm_region(bot, store, cfg, chat, region).await;
        return;
    }

    if data == "t3:ready" {
        continue_tripo3d_handoff(bot, store, cfg, chat).await;
        return;
    }

    // Platform selection for the Tripo3D key intake: global (api.tripo3d.ai, default) or cn
    // (api.tripo3d.com).
    if let Some(region) = data.strip_prefix("t3:region:") {
        select_tripo3d_region(bot, store, cfg, chat, region).await;
        return;
    }

    if data == "su:ready" {
        continue_suno_handoff(bot, store, cfg, chat).await;
        return;
    }

    if data == "gemini:ready" {
        continue_gemini_handoff(bot, store, cfg, chat).await;
        return;
    }

    if data == "gemini:verified" {
        let Some(oauth) = cfg.gemini_oauth.as_ref() else {
            let _ = bot
                .send(
                    chat,
                    "Подключение Gemini временно недоступно: администратор уведомлён.",
                )
                .await;
            return;
        };
        gemini_oauth::finish_parked_verification(bot, store, cfg, oauth, chat).await;
        return;
    }

    // Шаг назад продавца. Кнопка ничего не авторизует сама по себе: и разрешение, и цель заново
    // выводятся из состояния, а исходный шаг с провода лишь отсеивает устаревшую кнопку.
    if let Some(rest) = data.strip_prefix("hoback:") {
        let mut parts = rest.splitn(2, ':');
        let from = parts.next().unwrap_or_default();
        let action = parts.next().unwrap_or_default();
        let current = store
            .get_user(chat)
            .ok()
            .flatten()
            .map(|user| user.want)
            .unwrap_or_default();
        if back_step_wire(from).is_none() || back_step_wire(&current) != back_step_wire(from) {
            let _ = bot
                .send(
                    chat,
                    "Эта кнопка уже устарела — шаг сделки изменился. Открой актуальную карточку через /jobs.",
                )
                .await;
            return;
        }
        match action {
            "ask" => offer_handoff_back(bot, store, cfg, chat, false).await,
            "go" => offer_handoff_back(bot, store, cfg, chat, true).await,
            _ => {}
        }
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
            store,
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
            if flow.mode == "batch" {
                if let Some(paused) = store.paused_batch_for_seller(seller_chat).ok().flatten() {
                    let _ = bot
                        .send(
                            chat,
                            &format!(
                                "⏸ У продавца уже есть batch #{} на паузе. Ему можно создать одиночный оффер, но новый batch — только после продолжения или удаления старого.",
                                paused.id
                            ),
                        )
                        .await;
                    return;
                }
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

    // Одиночные оферы удаляет только администратор. Callback подтверждения привязан к exact
    // generation, поэтому устаревшая кнопка не может остановить повторно запущенную авторизацию.
    if let Some(rest) = data.strip_prefix("odel:") {
        if !admin {
            return;
        }
        let mut parts = rest.splitn(2, ':');
        let offer_id = parts
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let action = parts.next().unwrap_or_default();
        let Some(job) = store
            .active_seller_jobs()
            .unwrap_or_default()
            .into_iter()
            .find(|job| job.reference.kind == "offer" && job.reference.offer_id == offer_id)
        else {
            let _ = bot
                .send(chat, "Оффер уже не находится в активных /jobs.")
                .await;
            return;
        };
        if action == "ask" {
            if job.phase == "paying" {
                let _ = bot
                    .send(
                        chat,
                        "Этот оффер сейчас в неопределённой фазе выплаты. Сначала проверь транзакцию и разблокируй её через payment review.",
                    )
                    .await;
                return;
            }
            if !matches!(job.phase.as_str(), "accepted" | "processing") {
                let _ = bot
                    .send(
                        chat,
                        "Оффер уже изменился; старая кнопка удаления неактивна.",
                    )
                    .await;
                return;
            }
            let paid_warning = if job.phase == "processing" {
                "\n\n⚠️ <b>Выплата уже отмечена отправленной.</b> Авторизация будет остановлена, но факт выплаты останется в аудите."
            } else {
                ""
            };
            let confirm = vec![vec![(
                "⚠️ Да, удалить из очереди".into(),
                format!("odel:{}:{}", offer_id, job.reference.token),
            )]];
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "🗑 <b>Удалить оффер #{}?</b>\n\nОн исчезнет из активных /jobs, текущая авторизация будет остановлена, а продавец освободится. Данные оффера и исходная фаза сохранятся в архиве. Автоматически восстановить оффер после подтверждения нельзя.{}",
                        offer_id, paid_warning
                    ),
                    Some(&confirm),
                )
                .await;
            return;
        }
        if action != job.reference.token {
            let _ = bot
                .send(
                    chat,
                    "Оффер уже изменился; подтверждение удаления устарело.",
                )
                .await;
            return;
        }
        let Some(archived_phase) = store
            .archive_offer(offer_id, job.seller_chat, &job.reference.token, chat)
            .ok()
            .flatten()
        else {
            let _ = bot
                .send(
                    chat,
                    "Не удалось удалить оффер: его состояние уже изменилось или выплата требует проверки.",
                )
                .await;
            return;
        };
        setup_token::kill(job.seller_chat);
        crate::codex_login::cancel(job.seller_chat);
        let payment_note = if archived_phase == "processing" {
            " Отметка о выполненной выплате сохранена в аудите."
        } else {
            ""
        };
        let _ = bot
            .send(
                chat,
                &format!(
                    "🗑 Оффер #{} удалён из активных /jobs. Продавец освобождён.{}",
                    offer_id, payment_note
                ),
            )
            .await;
        if chat != job.seller_chat {
            let _ = bot
                .send(
                    job.seller_chat,
                    &format!(
                        "🗑 Администратор удалил оффер #{} из рабочей очереди. Продолжать регистрацию или авторизацию по нему больше не нужно.",
                        offer_id
                    ),
                )
                .await;
        }
        return;
    }

    // Продавец и админ могут немедленно освободить продавца от текущего batch. Завершённые
    // позиции остаются завершёнными; текущая попытка инвалидируется и при resume начнётся заново.
    if let Some(rest) = data.strip_prefix("batchpause:") {
        let mut parts = rest.split(':');
        let batch_id = parts
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let action = parts.next().unwrap_or_default();
        let expected_item = parts
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        if !admin && batch.seller_chat != chat {
            return;
        }
        if action == "ask" {
            if batch.status != "processing" || batch.current_item <= 0 {
                let _ = bot
                    .send(
                        chat,
                        "Batch уже не выполняется; старая кнопка паузы неактивна.",
                    )
                    .await;
                return;
            }
            let confirm = vec![vec![(
                "⏸ Да, поставить на паузу".into(),
                format!("batchpause:{}:confirm:{}", batch.id, batch.current_item),
            )]];
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "⏸ <b>Поставить batch #{} на паузу?</b>\n\n✅ Уже выполненные позиции сохранятся.\n🔄 Текущая позиция <b>{}/{}</b> будет сброшена и при продолжении начнётся заново.\n📦 После паузы продавцу можно выдать одиночный оффер.",
                        batch.id, batch.current_item, batch.quantity
                    ),
                    Some(&confirm),
                )
                .await;
            return;
        }
        if action == "confirm" {
            if batch.status != "processing" || batch.current_item != expected_item {
                let _ = bot
                    .send(chat, "Batch уже изменился; подтверждение паузы устарело.")
                    .await;
                return;
            }
            let Some(paused_item) = store
                .pause_batch(batch.id, batch.seller_chat)
                .ok()
                .flatten()
            else {
                let _ = bot
                    .send(
                        chat,
                        "Не удалось поставить batch на паузу: его состояние изменилось.",
                    )
                    .await;
                return;
            };
            setup_token::kill(batch.seller_chat);
            crate::codex_login::cancel(batch.seller_chat);
            let message = format!(
                "⏸ <b>Batch #{} поставлен на паузу.</b>\nВыполнено: <b>{}</b> из <b>{}</b>. Позиция <b>{}/{}</b> начнётся заново после продолжения. Теперь продавцу можно выдать одиночный оффер.",
                batch.id,
                paused_item - 1,
                batch.quantity,
                paused_item,
                batch.quantity
            );
            let _ = bot.send(chat, &message).await;
            if chat != batch.seller_chat {
                let _ = bot.send(batch.seller_chat, &message).await;
            } else {
                notify_admins(bot, cfg, store, &message, None).await;
            }
            return;
        }
        return;
    }

    // Resume не вытесняет одиночную работу: если продавец занят, batch остаётся paused.
    if let Some(rest) = data.strip_prefix("batchresume:") {
        let batch_id = rest.parse::<i64>().unwrap_or(0);
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        if !admin && batch.seller_chat != chat {
            return;
        }
        if let Some(job) = store.active_seller_job(batch.seller_chat).ok().flatten() {
            let _ = bot
                .send(
                    chat,
                    &format!(
                        "⛔ Batch #{} пока нельзя продолжить: продавец занят.\n\n<b>{}</b>",
                        batch.id,
                        seller_job_label(&job)
                    ),
                )
                .await;
            return;
        }
        let Some(item_no) = store
            .resume_paused_batch(batch.id, batch.seller_chat)
            .ok()
            .flatten()
        else {
            let _ = bot
                .send(
                    chat,
                    "Не удалось продолжить batch: он уже изменился или продавец занят.",
                )
                .await;
            return;
        };
        let message = format!(
            "▶️ <b>Batch #{} продолжен.</b> Возвращаемся к позиции <b>{}/{}</b>.",
            batch.id, item_no, batch.quantity
        );
        let _ = bot.send(chat, &message).await;
        if chat != batch.seller_chat {
            let _ = bot.send(batch.seller_chat, &message).await;
        } else {
            notify_admins(bot, cfg, store, &message, None).await;
        }
        start_batch_item(bot, store, cfg, batch.id, item_no, None).await;
        return;
    }

    // Удаление доступно только администратору и является audit-safe soft delete. Paying batch
    // намеренно нельзя удалить, пока не проверена неопределённая blockchain-транзакция.
    if let Some(rest) = data.strip_prefix("batchdelete:") {
        if !admin {
            return;
        }
        let mut parts = rest.splitn(3, ':');
        let batch_id = parts
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let expected_status = parts.next().unwrap_or_default();
        let action = parts.next().unwrap_or_default();
        let Some(batch) = store.get_batch(batch_id).ok().flatten() else {
            return;
        };
        if expected_status == "ask" && action.is_empty() {
            if batch.status == "paying" {
                let _ = bot
                    .send(
                        chat,
                        "Этот batch сейчас в неопределённой фазе выплаты. Сначала проверь транзакцию и разблокируй её через payment review.",
                    )
                    .await;
                return;
            }
            let confirm = vec![vec![(
                "⚠️ Да, удалить из очереди".into(),
                format!("batchdelete:{}:{}:confirm", batch.id, batch.status),
            )]];
            let _ = bot
                .send_kb(
                    chat,
                    &format!(
                        "🗑 <b>Удалить batch #{}?</b>\n\nОн исчезнет из рабочей очереди, незавершённая позиция будет остановлена. Данные выплаты и прогресса сохранятся в архиве для аудита. Автоматически восстановить batch после подтверждения нельзя.",
                        batch.id
                    ),
                    Some(&confirm),
                )
                .await;
            return;
        }
        if action == "confirm" {
            if batch.status != expected_status {
                let _ = bot
                    .send(
                        chat,
                        "Batch уже изменился; подтверждение удаления устарело.",
                    )
                    .await;
                return;
            }
            let Some(released_job) = store.archive_batch(batch.id).ok().flatten() else {
                let _ = bot
                    .send(
                        chat,
                        "Не удалось удалить batch: его состояние уже изменилось.",
                    )
                    .await;
                return;
            };
            if released_job {
                setup_token::kill(batch.seller_chat);
                crate::codex_login::cancel(batch.seller_chat);
            }
            let _ = bot
                .send(
                    chat,
                    &format!(
                        "🗑 Batch #{} удалён из рабочей очереди. Финансовая история сохранена в архиве.",
                        batch.id
                    ),
                )
                .await;
            let _ = bot
                .send(
                    batch.seller_chat,
                    &format!(
                        "🗑 Администратор удалил batch #{} из рабочей очереди. Выполнять оставшиеся позиции больше не нужно.",
                        batch.id
                    ),
                )
                .await;
            return;
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
                    store,
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
                    store,
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
                notify_batch_payment_ready(bot, cfg, store, &batch, &rec).await;
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
            notify_batch_payment_ready(bot, cfg, store, &updated, &seller).await;
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
                        store,
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
                    store,
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
                            store,
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
                        store,
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
                        store,
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
                        store,
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
mod tests;
