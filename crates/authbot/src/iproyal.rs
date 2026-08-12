//! Клиент IPRoyal reseller API — авто-выпуск UK ISP прокси (30 дней) для передачи продавцу.
//! База `https://apid.iproyal.com/v1/reseller`, авторизация заголовком `X-Access-Token`.
//! Каталог (продукт/план/локация) подтягиваем ДИНАМИЧЕСКИ по именам — устойчиво к смене ID.
//! Город выбираем СЛУЧАЙНО из доступных (in-stock) британских городов.
//!
//! Прокси с кредами приходят в `GET /orders/{id}` → `proxy_data.proxies[]`
//! (`{ip, username, password}`), порты — в `proxy_data.ports` (`socks5` и `http|https`).
//! Используем HTTP CONNECT-порт: его принимает общий Claude/Codex/Gemini handoff без смены
//! формата. Строка — `http://user:pass@ip:http_port`.
//! Эндпоинт `/orders/proxies` — тупиковый (422). Подтверждено живыми заказами 2026-07-15.

use anyhow::{anyhow, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const BASE: &str = "https://apid.iproyal.com/v1/reseller";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 128;

pub struct Iproyal {
    key: Zeroizing<String>,
    http: reqwest::Client,
    base: String,
}

/// Безопасная проекция ISP-инвентаря: только данные жизненного цикла, без proxy credentials и
/// произвольных upstream-полей.
#[derive(Clone, PartialEq, Eq)]
pub struct IspOrder {
    pub order_id: i64,
    pub expire_date: String,
    pub status: String,
    pub auto_extend: bool,
    pub ips: Vec<String>,
}

impl std::fmt::Debug for IspOrder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IspOrder")
            .field("order_id", &self.order_id)
            .field("expire_date", &self.expire_date)
            .field("status", &self.status)
            .field("auto_extend", &self.auto_extend)
            .field("proxy_count", &self.ips.len())
            .finish()
    }
}

/// Безопасный агрегат способов оплаты. Реквизиты карт намеренно не парсятся и не возвращаются.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardsWarning {
    pub cards: usize,
    pub cards_with_auto_extend: usize,
    pub warning: bool,
}

#[derive(Debug, Deserialize)]
struct ProductsResponse {
    data: Vec<Product>,
}

#[derive(Debug, Deserialize)]
struct Product {
    id: i64,
    name: String,
    #[serde(default)]
    plans: Vec<ProductPlan>,
    #[serde(default)]
    locations: Vec<ProductLocation>,
}

#[derive(Debug, Deserialize)]
struct ProductPlan {
    id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProductLocation {
    id: i64,
    name: String,
    #[serde(default)]
    child_locations: Vec<ChildLocation>,
}

#[derive(Debug, Deserialize)]
struct ChildLocation {
    id: i64,
    name: String,
    out_of_stock: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OrdersResponse {
    data: Vec<Order>,
    #[serde(default)]
    meta: PageMeta,
}

#[derive(Debug, Deserialize)]
struct Order {
    id: i64,
    expire_date: String,
    status: String,
    proxy_data: ProxyData,
    #[serde(default, deserialize_with = "deserialize_auto_extend_setting")]
    auto_extend_settings: AutoExtendSetting,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ExactOrderResponse {
    Direct(Order),
    Wrapped { data: Order },
}

impl ExactOrderResponse {
    fn into_order(self) -> Order {
        match self {
            Self::Direct(order) | Self::Wrapped { data: order } => order,
        }
    }
}

#[derive(Debug, Default)]
struct AutoExtendSetting {
    present: bool,
    enabled: bool,
}

fn deserialize_auto_extend_setting<'de, D>(
    deserializer: D,
) -> std::result::Result<AutoExtendSetting, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(AutoExtendSetting {
        present: true,
        enabled: Option::<serde::de::IgnoredAny>::deserialize(deserializer)?.is_some(),
    })
}

#[derive(Debug, Deserialize)]
struct ProxyData {
    proxies: Vec<OrderProxy>,
}

#[derive(Debug, Deserialize)]
struct OrderProxy {
    ip: String,
}

#[derive(Debug, Default, Deserialize)]
struct PageMeta {
    last_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CardsResponse {
    data: Vec<CardState>,
}

#[derive(Debug, Deserialize)]
struct CardState {
    has_royal_auto_extend_enabled: bool,
}

#[derive(Serialize)]
struct ToggleAutoExtend {
    order_id: i64,
    is_enabled: bool,
}

#[derive(Serialize)]
struct ExtendOrder<'a> {
    product_plan_id: i64,
    proxies: &'a [String],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtendOrderFailureClass {
    Rejected,
    Uncertain,
}

#[derive(Debug)]
pub struct ExtendOrderFailure {
    class: ExtendOrderFailureClass,
    phase: &'static str,
}

impl ExtendOrderFailure {
    fn rejected(phase: &'static str) -> Self {
        Self {
            class: ExtendOrderFailureClass::Rejected,
            phase,
        }
    }

    fn uncertain(phase: &'static str) -> Self {
        Self {
            class: ExtendOrderFailureClass::Uncertain,
            phase,
        }
    }

    pub fn class(&self) -> ExtendOrderFailureClass {
        self.class
    }

    pub fn phase(&self) -> &'static str {
        self.phase
    }
}

impl std::fmt::Display for ExtendOrderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "IPRoyal extend {} during {}",
            match self.class {
                ExtendOrderFailureClass::Rejected => "rejected",
                ExtendOrderFailureClass::Uncertain => "uncertain",
            },
            self.phase
        )
    }
}

impl std::error::Error for ExtendOrderFailure {}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct IssuedProxy {
    pub host: String,
    pub port: u32, // HTTP(CONNECT)-порт: claude CLI умеет только HTTP-прокси, движок тоже
    pub user: String,
    pub pass: String,
    pub city: String, // человекочитаемо, напр. "London (England)"
    pub order_id: i64,
}

impl std::fmt::Debug for IssuedProxy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedProxy")
            .field("city", &self.city)
            .field("order_id", &self.order_id)
            .field("credentials", &"REDACTED")
            .finish()
    }
}

impl IssuedProxy {
    /// `http://user:pass@host:port` — формат для реестра/движка/claude CLI (HTTP CONNECT).
    pub fn url(&self) -> String {
        format!(
            "http://{}:{}@{}:{}",
            self.user, self.pass, self.host, self.port
        )
    }
    /// `host:port:user:pass` — компактный формат для человека (HTTP-порт).
    pub fn compact(&self) -> String {
        format!("{}:{}:{}:{}", self.host, self.port, self.user, self.pass)
    }
}

impl Iproyal {
    pub fn new(key: &str) -> Self {
        Self::with_base_url(key, BASE).expect("valid IPRoyal base URL")
    }

    /// Отдельный constructor для локального HTTP mock; production caller остаётся на [`Self::new`].
    pub fn with_base_url(key: &str, base: &str) -> Result<Self> {
        let base = base.trim_end_matches('/');
        let url = reqwest::Url::parse(base).map_err(|_| anyhow!("invalid IPRoyal base URL"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(anyhow!("invalid IPRoyal base URL"));
        }
        let http = reqwest::Client::builder()
            // The reseller credential must never traverse an ambient HTTP(S)_PROXY inherited by
            // the service. Subscription traffic gets its own explicit per-profile proxy later.
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .user_agent("apitoken-iproyal-lifecycle/1")
            .build()
            .map_err(|_| anyhow!("IPRoyal HTTP client initialization failed"))?;
        Ok(Iproyal {
            key: Zeroizing::new(key.to_string()),
            http,
            base: base.to_string(),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.send_json(self.http.get(self.url(path)?), "GET").await
    }

    async fn post<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.send_json(self.http.post(self.url(path)?).json(body), "POST")
            .await
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        operation: &'static str,
    ) -> Result<T> {
        let bytes = self.send_raw(request, operation).await?;
        serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("IPRoyal {operation} returned invalid JSON"))
    }

    async fn send_raw(
        &self,
        request: reqwest::RequestBuilder,
        operation: &'static str,
    ) -> Result<Vec<u8>> {
        let mut response = request
            .header("X-Access-Token", self.key.as_str())
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| anyhow!("IPRoyal {operation} transport failed"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("IPRoyal {operation} failed ({status})"));
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_RESPONSE_BYTES)
        {
            return Err(anyhow!("IPRoyal {operation} response is too large"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("IPRoyal {operation} response read failed"))?
        {
            if bytes.len().saturating_add(chunk.len()) as u64 > MAX_RESPONSE_BYTES {
                return Err(anyhow!("IPRoyal {operation} response is too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    fn url(&self, path: &str) -> Result<String> {
        if !path.starts_with('/') || path.contains(['\r', '\n']) {
            return Err(anyhow!("invalid IPRoyal request path"));
        }
        Ok(format!("{}{path}", self.base))
    }

    async fn isp_product(&self) -> Result<Product> {
        let products: ProductsResponse = self.get("/products").await?;
        products
            .data
            .into_iter()
            .find(|product| product.name.to_ascii_lowercase().contains("isp"))
            .filter(|product| product.id > 0)
            .ok_or_else(|| anyhow!("IPRoyal ISP product is unavailable"))
    }

    /// Typed ISP inventory without usernames, passwords, card details or arbitrary upstream data.
    pub async fn isp_inventory(&self) -> Result<Vec<IspOrder>> {
        let product_id = self.isp_product().await?.id;
        let mut out = Vec::new();
        for page in 1..=50u32 {
            let response: OrdersResponse = self
                .get(&format!(
                    "/orders?product_id={product_id}&page={page}&per_page=100"
                ))
                .await?;
            let last_page = response.meta.last_page.unwrap_or(page);
            if last_page > 50 {
                return Err(anyhow!("IPRoyal orders pagination exceeds safety bound"));
            }
            if response.data.is_empty() {
                break;
            }
            for order in response.data {
                if order.id <= 0 {
                    return Err(anyhow!("IPRoyal inventory contains invalid order id"));
                }
                out.push(sanitize_order(order)?);
            }
            if page >= last_page {
                break;
            }
            if page == 50 {
                return Err(anyhow!("IPRoyal orders pagination exceeds safety bound"));
            }
        }
        Ok(out)
    }

    /// Совместимый API для текущего lifecycle caller.
    pub async fn list_isp_orders(&self) -> Result<Vec<(String, String, i64)>> {
        Ok(self
            .isp_inventory()
            .await?
            .into_iter()
            .flat_map(|order| {
                order
                    .ips
                    .into_iter()
                    .map(move |ip| (ip, order.expire_date.clone(), order.order_id))
            })
            .collect())
    }

    /// Точный reseller balance в nano-USD decimal integer string, без float/rounding.
    pub async fn balance(&self) -> Result<String> {
        let bytes = self
            .send_raw(self.http.get(self.url("/balance")?), "GET")
            .await?;
        parse_balance_nano_usd(&bytes)
    }

    /// Агрегированное предупреждение о card-level auto-extend без card metadata.
    pub async fn cards_warning(&self) -> Result<CardsWarning> {
        let cards: CardsResponse = self.get("/cards").await?;
        let enabled = cards
            .data
            .iter()
            .filter(|card| card.has_royal_auto_extend_enabled)
            .count();
        Ok(CardsWarning {
            cards: cards.data.len(),
            cards_with_auto_extend: enabled,
            warning: enabled > 0,
        })
    }

    async fn exact_order(&self, order_id: i64) -> Result<Order> {
        let response: ExactOrderResponse = self.get(&format!("/orders/{order_id}")).await?;
        let order = response.into_order();
        if order.id != order_id {
            return Err(anyhow!("IPRoyal exact order id does not match"));
        }
        validate_order_structure(&order)?;
        Ok(order)
    }

    async fn disable_auto_extend_if_enabled(&self, order_id: i64, current: Order) -> Result<Order> {
        if order_id <= 0 {
            return Err(anyhow!("invalid IPRoyal order id"));
        }
        if !current.auto_extend_settings.present {
            return Err(anyhow!("IPRoyal order has no auto-extend state"));
        }
        if !current.auto_extend_settings.enabled {
            return Ok(current);
        }
        let _: Value = self
            .post(
                "/orders/toggle-auto-extend",
                &ToggleAutoExtend {
                    order_id,
                    is_enabled: false,
                },
            )
            .await?;
        let confirmed = self.exact_order(order_id).await?;
        if !confirmed.auto_extend_settings.present || confirmed.auto_extend_settings.enabled {
            return Err(anyhow!("IPRoyal auto-extend disable was not confirmed"));
        }
        Ok(confirmed)
    }

    /// Выключить включённый auto-extend заказа и подтвердить результат exact refetch. Уже
    /// выключенное состояние не мутируется: IPRoyal не документирует toggle как идемпотентный.
    pub async fn ensure_auto_extend_disabled(&self, order_id: i64) -> Result<()> {
        if order_id <= 0 {
            return Err(anyhow!("invalid IPRoyal order id"));
        }
        let current = self.exact_order(order_id).await?;
        self.disable_auto_extend_if_enabled(order_id, current)
            .await?;
        Ok(())
    }

    /// Совместимый API текущего lifecycle caller: выбирает все IP из exact order, а основной API
    /// повторно подтверждает полный набор непосредственно перед платным POST.
    pub async fn extend_order(
        &self,
        order_id: i64,
    ) -> std::result::Result<String, ExtendOrderFailure> {
        if order_id <= 0 {
            return Err(ExtendOrderFailure::rejected("input_validation"));
        }
        let order = self
            .exact_order(order_id)
            .await
            .map_err(|_| ExtendOrderFailure::rejected("order_preflight"))?;
        let ips = canonical_order_ips(&order)
            .map_err(|_| ExtendOrderFailure::rejected("order_preflight"))?;
        self.extend_order_ips(order_id, &ips).await
    }

    /// Ручное продление выбранных canonical IP заказа. IPRoyal принимает `proxies`; отсутствие или
    /// пустой список означало бы весь заказ, поэтому непустой операторский выбор передаётся явно.
    pub async fn extend_order_ips(
        &self,
        order_id: i64,
        ips: &[String],
    ) -> std::result::Result<String, ExtendOrderFailure> {
        if order_id <= 0 {
            return Err(ExtendOrderFailure::rejected("input_validation"));
        }
        if ips.is_empty() {
            return Err(ExtendOrderFailure::rejected("input_validation"));
        }
        let selected = canonicalize_unique_selection(ips)
            .map_err(|_| ExtendOrderFailure::rejected("input_validation"))?;
        let preflight = self
            .exact_order(order_id)
            .await
            .map_err(|_| ExtendOrderFailure::rejected("order_preflight"))?;
        validate_order_renewable(&preflight)
            .map_err(|_| ExtendOrderFailure::rejected("order_preflight"))?;
        prove_order_selection(&preflight, &selected)
            .map_err(|_| ExtendOrderFailure::rejected("order_preflight"))?;
        let product = self
            .isp_product()
            .await
            .map_err(|_| ExtendOrderFailure::rejected("catalog_preflight"))?;
        let plan_id = thirty_day_plan_id_typed(&product)
            .map_err(|_| ExtendOrderFailure::rejected("catalog_preflight"))?;
        let before = self
            .disable_auto_extend_if_enabled(order_id, preflight)
            .await
            .map_err(|_| ExtendOrderFailure::rejected("auto_extend_preflight"))?;
        validate_order_renewable(&before)
            .map_err(|_| ExtendOrderFailure::rejected("order_recheck"))?;
        prove_order_selection(&before, &selected)
            .map_err(|_| ExtendOrderFailure::rejected("order_recheck"))?;
        let before_expiry = parse_expiry(&before.expire_date)
            .map_err(|_| ExtendOrderFailure::rejected("order_recheck"))?;
        self.send_paid_extend(order_id, plan_id, &selected).await?;
        let after = self
            .exact_order(order_id)
            .await
            .map_err(|_| ExtendOrderFailure::uncertain("post_payment_confirmation"))?;
        validate_order_renewable(&after)
            .map_err(|_| ExtendOrderFailure::uncertain("post_payment_confirmation"))?;
        prove_order_selection(&after, &selected)
            .map_err(|_| ExtendOrderFailure::uncertain("post_payment_confirmation"))?;
        if !after.auto_extend_settings.present || after.auto_extend_settings.enabled {
            return Err(ExtendOrderFailure::uncertain("post_payment_confirmation"));
        }
        let after_expiry = parse_expiry(&after.expire_date)
            .map_err(|_| ExtendOrderFailure::uncertain("post_payment_confirmation"))?;
        if after_expiry <= before_expiry {
            return Err(ExtendOrderFailure::uncertain("post_payment_confirmation"));
        }
        Ok(after.expire_date)
    }

    async fn send_paid_extend(
        &self,
        order_id: i64,
        plan_id: i64,
        proxies: &[String],
    ) -> std::result::Result<(), ExtendOrderFailure> {
        let url = self
            .url(&format!("/orders/{order_id}/extend"))
            .map_err(|_| ExtendOrderFailure::rejected("paid_request_setup"))?;
        let response = self
            .http
            .post(url)
            .header("X-Access-Token", self.key.as_str())
            .header("accept", "application/json")
            .json(&ExtendOrder {
                product_plan_id: plan_id,
                proxies,
            })
            .send()
            .await
            .map_err(|_| ExtendOrderFailure::uncertain("paid_transport"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        if status.is_client_error() {
            return Err(ExtendOrderFailure::rejected("paid_response"));
        }
        Err(ExtendOrderFailure::uncertain("paid_response"))
    }

    /// Выпуск ОДНОГО UK ISP прокси на 30 дней. Город — случайный из доступных.
    pub async fn issue_uk_isp_30d(&self) -> Result<IssuedProxy> {
        // 1) каталог: ISP-продукт, план «30 Days», локация United Kingdom + in-stock города
        let product = self.isp_product().await?;
        let product_id = product.id;
        let plan_id = thirty_day_plan_id_typed(&product)?;
        let uk = product
            .locations
            .iter()
            .find(|location| location.name == "United Kingdom")
            .ok_or_else(|| anyhow!("IPRoyal UK location is unavailable"))?;

        // in-stock города (child_locations); если нет — уровень страны
        let mut cities: Vec<(i64, String)> = uk
            .child_locations
            .iter()
            .filter(|city| city.out_of_stock == Some(false))
            .filter(|city| city.id > 0 && validate_text("city", &city.name, MAX_TEXT_BYTES).is_ok())
            .map(|city| (city.id, city.name.clone()))
            .collect();
        if cities.is_empty() {
            if uk.id <= 0 {
                return Err(anyhow!("IPRoyal UK location is invalid"));
            }
            cities.push((uk.id, "United Kingdom".into()));
        }
        // Avoid a timestamp-derived fleet pattern. Location selection is independent for every
        // allocation and fails closed if the operating-system CSPRNG is unavailable.
        let idx = random_index(cities.len())?;
        let (location_id, city) = cities[idx].clone();

        // 2) POST /orders — покупка 1 прокси на 30 дней (без авто-продления)
        let body = serde_json::json!({
            "product_id": product_id,
            "product_plan_id": plan_id,
            "product_location_id": location_id,
            "quantity": 1,
            "auto_extend": false
        });
        let order: Value = self.post("/orders", &body).await?;
        let order_id = order
            .get("id")
            .and_then(Value::as_i64)
            .or_else(|| order.pointer("/data/id").and_then(Value::as_i64))
            .filter(|id| *id > 0)
            .ok_or_else(|| anyhow!("IPRoyal create order response has no valid order id"))?;

        // 3) забрать прокси с кредами (провижн не мгновенный) — до ~10 попыток
        for _ in 0..10 {
            // Прокси с кредами — в самом заказе: GET /orders/{id} → proxy_data.proxies[].
            if let Ok(list) = self.get::<Value>(&format!("/orders/{order_id}")).await {
                if let Some(px) = extract_proxy(&list, order_id, &city) {
                    return Ok(px);
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Err(anyhow!(
            "IPRoyal order was created but proxy provisioning did not complete"
        ))
    }
}

fn parse_balance_nano_usd(bytes: &[u8]) -> Result<String> {
    let raw = trim_json_whitespace(bytes);
    let decimal = if raw.len() >= 2 && raw.first() == Some(&b'"') && raw.last() == Some(&b'"') {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };
    if decimal.is_empty() || decimal.len() > 32 || !decimal.is_ascii() {
        return Err(anyhow!("IPRoyal balance response is invalid"));
    }

    let (whole, fraction) = match decimal.iter().position(|byte| *byte == b'.') {
        Some(dot) => (&decimal[..dot], Some(&decimal[dot + 1..])),
        None => (decimal, None),
    };
    if whole.is_empty()
        || (whole.len() > 1 && whole[0] == b'0')
        || !whole.iter().all(u8::is_ascii_digit)
        || fraction.is_some_and(|digits| {
            digits.is_empty() || digits.len() > 9 || !digits.iter().all(u8::is_ascii_digit)
        })
    {
        return Err(anyhow!("IPRoyal balance response is invalid"));
    }

    let whole = parse_decimal_u64(whole)?;
    let mut nano = whole
        .checked_mul(1_000_000_000)
        .ok_or_else(|| anyhow!("IPRoyal balance response is out of range"))?;
    if let Some(digits) = fraction {
        let fractional = parse_decimal_u64(digits)?;
        let scale = 10u64.pow((9 - digits.len()) as u32);
        nano = nano
            .checked_add(fractional * scale)
            .ok_or_else(|| anyhow!("IPRoyal balance response is out of range"))?;
    }
    Ok(nano.to_string())
}

fn trim_json_whitespace(mut bytes: &[u8]) -> &[u8] {
    const JSON_WHITESPACE: &[u8] = b" \t\r\n";
    while bytes
        .first()
        .is_some_and(|byte| JSON_WHITESPACE.contains(byte))
    {
        bytes = &bytes[1..];
    }
    while bytes
        .last()
        .is_some_and(|byte| JSON_WHITESPACE.contains(byte))
    {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn parse_decimal_u64(digits: &[u8]) -> Result<u64> {
    digits.iter().try_fold(0u64, |value, digit| {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit - b'0')))
            .ok_or_else(|| anyhow!("IPRoyal balance response is out of range"))
    })
}

fn thirty_day_plan_id_typed(product: &Product) -> Result<i64> {
    product
        .plans
        .iter()
        .find(|plan| plan.name == "30 Days")
        .map(|plan| plan.id)
        .filter(|id| *id > 0)
        .ok_or_else(|| anyhow!("IPRoyal 30-day plan is unavailable"))
}

fn validate_order_structure(order: &Order) -> Result<()> {
    validate_text("expiry", &order.expire_date, MAX_TEXT_BYTES)?;
    parse_expiry(&order.expire_date)?;
    validate_text("status", &order.status, 32)?;
    if !order.auto_extend_settings.present {
        return Err(anyhow!("IPRoyal inventory has no auto-extend state"));
    }
    for proxy in &order.proxy_data.proxies {
        canonical_ip(&proxy.ip)?;
    }
    Ok(())
}

fn validate_order_renewable(order: &Order) -> Result<()> {
    if !matches!(
        order.status.to_ascii_lowercase().as_str(),
        "active" | "confirmed" | "completed"
    ) {
        return Err(anyhow!("IPRoyal order is not active"));
    }
    Ok(())
}

fn canonical_order_ips(order: &Order) -> Result<Vec<String>> {
    order
        .proxy_data
        .proxies
        .iter()
        .map(|proxy| canonical_ip(&proxy.ip))
        .collect()
}

fn canonicalize_unique_selection(ips: &[String]) -> Result<Vec<String>> {
    let mut selected = Vec::with_capacity(ips.len());
    let mut seen = HashSet::with_capacity(ips.len());
    for ip in ips {
        let canonical = canonical_ip(ip)?;
        if !seen.insert(canonical.clone()) {
            return Err(anyhow!("duplicate IPRoyal proxy IP selection"));
        }
        selected.push(canonical);
    }
    Ok(selected)
}

fn prove_order_selection(order: &Order, selected: &[String]) -> Result<()> {
    let order_ips = canonical_order_ips(order)?;
    for ip in selected {
        if order_ips
            .iter()
            .filter(|candidate| *candidate == ip)
            .count()
            != 1
        {
            return Err(anyhow!(
                "IPRoyal exact order does not contain selected IP exactly once"
            ));
        }
    }
    Ok(())
}

fn sanitize_order(order: Order) -> Result<IspOrder> {
    validate_order_structure(&order)?;
    let ips = canonical_order_ips(&order)?;
    Ok(IspOrder {
        order_id: order.id,
        expire_date: order.expire_date,
        status: order.status,
        auto_extend: order.auto_extend_settings.enabled,
        ips,
    })
}

fn validate_text(label: &'static str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(anyhow!("IPRoyal {label} is invalid"));
    }
    Ok(())
}

fn canonical_ip(value: &str) -> Result<String> {
    validate_text("proxy IP", value, MAX_TEXT_BYTES)?;
    value
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.to_string())
        .map_err(|_| anyhow!("IPRoyal proxy IP is invalid"))
}

fn parse_expiry(value: &str) -> Result<i64> {
    let value = value.trim();
    let date_time = value.strip_suffix('Z').unwrap_or(value);
    let (date, time) = date_time
        .split_once(['T', ' '])
        .unwrap_or((date_time, "00:00:00"));
    let date = date
        .split('-')
        .map(str::parse::<i64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| anyhow!("IPRoyal expiry is invalid"))?;
    let mut time_parts = time.split(':');
    let hour = time_parts
        .next()
        .ok_or_else(|| anyhow!("IPRoyal expiry is invalid"))?;
    let minute = time_parts
        .next()
        .ok_or_else(|| anyhow!("IPRoyal expiry is invalid"))?;
    let second = time_parts
        .next()
        .ok_or_else(|| anyhow!("IPRoyal expiry is invalid"))?;
    if time_parts.next().is_some()
        || second.split_once('.').is_some_and(|(_, fraction)| {
            fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return Err(anyhow!("IPRoyal expiry is invalid"));
    }
    let time = [hour, minute, second.split('.').next().unwrap_or_default()]
        .into_iter()
        .map(str::parse::<i64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| anyhow!("IPRoyal expiry is invalid"))?;
    if date.len() != 3
        || time.len() != 3
        || !(1..=12).contains(&date[1])
        || date[2] < 1
        || date[2] > days_in_month(date[0], date[1])
        || !(0..=23).contains(&time[0])
        || !(0..=59).contains(&time[1])
        || !(0..=59).contains(&time[2])
    {
        return Err(anyhow!("IPRoyal expiry is invalid"));
    }
    let year = date[0] - i64::from(date[1] <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = if date[1] > 2 {
        date[1] - 3
    } else {
        date[1] + 9
    };
    let day_of_year = (153 * month + 2) / 5 + date[2] - 1;
    let days = era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year
        - 719468;
    days.checked_mul(86400)
        .and_then(|value| value.checked_add(time[0] * 3600 + time[1] * 60 + time[2]))
        .ok_or_else(|| anyhow!("IPRoyal expiry is out of range"))
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || year % 4 == 0 && year % 100 != 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn random_index(len: usize) -> Result<usize> {
    if len == 0 {
        return Err(anyhow!("IPRoyal location set is empty"));
    }
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| anyhow!("operating-system CSPRNG unavailable"))?;
    Ok((u64::from_le_bytes(random) % len as u64) as usize)
}

/// Первый прокси-объект из ответа (гибко: data[]/proxies[]/proxy_data.proxies[]).
fn first_proxy(v: &Value) -> Option<Value> {
    let direct = [
        v.get("proxies").and_then(|d| d.as_array()),
        v.get("data").and_then(|d| d.as_array()),
        v.get("proxy_data")
            .and_then(|d| d.get("proxies"))
            .and_then(|d| d.as_array()),
        v.as_array(),
    ];
    for c in direct.into_iter().flatten() {
        if let Some(f) = c.first() {
            return Some(f.clone());
        }
    }
    // data — объект заказа с вложенным proxy_data.proxies
    v.get("data")
        .and_then(|d| d.get("proxy_data"))
        .and_then(|d| d.get("proxies"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first().cloned())
}

fn sfield(p: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = p.get(*k).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// HTTP(CONNECT)-порт: из полей прокси, либо из объекта `ports` (прокси или уровень заказа).
/// IPRoyal кладёт его как `http|https` (ключ с трубой), иногда `http`/`https`/`http_port`.
fn http_port(proxy: &Value, order: &Value) -> Option<u32> {
    for k in ["http_port", "https_port", "http", "https", "http|https"] {
        if let Some(port) = proxy.get(k).and_then(Value::as_u64).and_then(valid_port) {
            return Some(port);
        }
    }
    let port_srcs = [
        proxy.get("ports"),
        order.get("proxy_data").and_then(|d| d.get("ports")),
        order
            .get("data")
            .and_then(|d| d.get("proxy_data"))
            .and_then(|d| d.get("ports")),
        order.get("ports"),
    ];
    for src in port_srcs.into_iter().flatten() {
        for key in ["http|https", "http", "https", "http_port"] {
            if let Some(port) = src.get(key).and_then(Value::as_u64).and_then(valid_port) {
                return Some(port);
            }
        }
    }
    None
}

fn valid_port(port: u64) -> Option<u32> {
    (1..=u16::MAX as u64).contains(&port).then_some(port as u32)
}

/// Собрать IssuedProxy из ответа; None пока прокси не провизился/схема иная.
fn extract_proxy(list: &Value, order_id: i64, city: &str) -> Option<IssuedProxy> {
    let p = first_proxy(list)?;
    let host = sfield(
        &p,
        &["ip", "host", "address", "proxy_address", "ip_address"],
    )?;
    let user = sfield(&p, &["username", "login", "user", "proxy_username"])?;
    let pass = sfield(&p, &["password", "pass", "proxy_password"])?;
    let port = http_port(&p, list)?;
    if host.is_empty() {
        return None;
    }
    Some(IssuedProxy {
        host,
        port,
        user,
        pass,
        city: city.to_string(),
        order_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn mock_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0; 16 * 1024];
                let read = stream.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..read]).into_owned();
                recorded.lock().unwrap().push(request);
                let reason = if status == 200 { "OK" } else { "Error" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}"), requests, task)
    }

    #[test]
    fn extract_flat_shape() {
        let v = serde_json::json!({
            "data": [{ "ip": "1.2.3.4", "http_port": 12323, "username": "u1", "password": "p1" }]
        });
        let px = extract_proxy(&v, 420, "London").expect("proxy");
        assert_eq!(px.url(), "http://u1:p1@1.2.3.4:12323");
        assert_eq!(px.compact(), "1.2.3.4:12323:u1:p1");
    }

    #[test]
    fn extract_ports_object_shape() {
        // HTTP-порт берётся из proxy_data.ports."http|https" на уровне заказа
        let v = serde_json::json!({
            "proxy_data": { "ports": { "socks5": 12324, "http|https": 12323 },
                            "proxies": [{ "host": "5.6.7.8", "login": "u2", "password": "p2" }] }
        });
        let px = extract_proxy(&v, 421, "Manchester").expect("proxy");
        assert_eq!(px.url(), "http://u2:p2@5.6.7.8:12323");
    }

    #[test]
    fn extract_real_order_shape() {
        // форма живого GET /orders/{id} (значения — фиктивные, IP из RFC5737 TEST-NET-3)
        let v = serde_json::json!({
            "id": 12345, "status": "confirmed",
            "proxy_data": {
                "ports": { "socks5": 12324, "http|https": 12323 },
                "proxies": [{ "username": "u_test", "password": "p_test", "ip": "203.0.113.7" }]
            }
        });
        let px = extract_proxy(&v, 12345, "London (England)").expect("proxy");
        assert_eq!(px.host, "203.0.113.7");
        assert_eq!(px.port, 12323);
        assert_eq!(px.url(), "http://u_test:p_test@203.0.113.7:12323");
    }

    #[test]
    fn none_when_not_provisioned() {
        let v = serde_json::json!({ "proxy_data": { "ports": {}, "proxies": [] } });
        assert!(extract_proxy(&v, 1, "London").is_none());
    }

    #[test]
    fn debug_output_never_contains_proxy_credentials() {
        let proxy = IssuedProxy {
            host: "203.0.113.7".into(),
            port: 12323,
            user: "secret-user".into(),
            pass: "secret-password".into(),
            city: "London".into(),
            order_id: 99,
        };
        let debug = format!("{proxy:?}");
        assert!(!debug.contains("secret-user"));
        assert!(!debug.contains("secret-password"));
        assert!(!debug.contains("203.0.113.7"));
    }

    #[test]
    fn catalogue_plan_and_expiry_are_strict() {
        let product: Product = serde_json::from_value(serde_json::json!({
            "id": 900,
            "name": "Dedicated ISP Proxies",
            "plans": [{"id": 321, "name": "30 Days"}]
        }))
        .unwrap();
        assert_eq!(thirty_day_plan_id_typed(&product).unwrap(), 321);
        assert_eq!(parse_expiry("1970-01-01 00:00:01").unwrap(), 1);
        assert!(parse_expiry("2023-02-29").is_err());
        assert!(parse_expiry("2026-08-29garbage").is_err());
    }

    #[test]
    fn invalid_proxy_ports_fail_closed() {
        for port in [0, 65_536, u64::MAX] {
            let value = serde_json::json!({
                "data": [{ "ip": "203.0.113.8", "http_port": port, "username": "u", "password": "p" }]
            });
            assert!(extract_proxy(&value, 1, "London").is_none());
        }
    }

    #[tokio::test]
    async fn inventory_retains_zero_ip_inactive_and_duplicate_canonical_occurrences() {
        let (base, requests, task) = mock_server(vec![
            (
                200,
                r#"{"data":[{"id":77,"name":"Dedicated ISP Proxies","plans":[],"locations":[]}]}"#,
            ),
            (
                200,
                r#"{"data":[{"id":42,"expire_date":"2026-09-01 00:00:00","status":"confirmed","proxy_data":{"proxies":[{"ip":"2001:0db8::1","username":"secret-user","password":"secret-pass"},{"ip":"2001:db8:0:0:0:0:0:1"}]},"auto_extend_settings":null,"note":"raw-secret"},{"id":43,"expire_date":"2026-07-01","status":"expired","proxy_data":{"proxies":[]},"auto_extend_settings":null}],"meta":{"last_page":1}}"#,
            ),
        ])
        .await;
        let client = Iproyal::with_base_url("reseller-secret", &base).unwrap();
        let inventory = client.isp_inventory().await.unwrap();
        task.await.unwrap();

        assert_eq!(
            inventory,
            vec![
                IspOrder {
                    order_id: 42,
                    expire_date: "2026-09-01 00:00:00".into(),
                    status: "confirmed".into(),
                    auto_extend: false,
                    ips: vec!["2001:db8::1".into(), "2001:db8::1".into()],
                },
                IspOrder {
                    order_id: 43,
                    expire_date: "2026-07-01".into(),
                    status: "expired".into(),
                    auto_extend: false,
                    ips: vec![],
                },
            ]
        );
        let debug = format!("{inventory:?}");
        assert!(!debug.contains("secret-user"));
        assert!(!debug.contains("secret-pass"));
        assert!(!debug.contains("raw-secret"));
        let requests = requests.lock().unwrap();
        assert!(requests[1].starts_with("GET /orders?product_id=77&page=1&per_page=100 HTTP/1.1"));
        assert!(requests
            .iter()
            .all(|request| request.contains("x-access-token: reseller-secret")));
    }

    #[tokio::test]
    async fn inventory_requires_explicit_auto_extend_state() {
        let (base, _, task) = mock_server(vec![
            (
                200,
                r#"{"data":[{"id":77,"name":"Dedicated ISP Proxies","plans":[],"locations":[]}]}"#,
            ),
            (
                200,
                r#"{"data":[{"id":42,"expire_date":"2026-09-01","status":"confirmed","proxy_data":{"proxies":[]}}],"meta":{"last_page":1}}"#,
            ),
        ])
        .await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        let error = client.isp_inventory().await.unwrap_err().to_string();
        task.await.unwrap();

        assert_eq!(error, "IPRoyal inventory has no auto-extend state");
    }

    #[tokio::test]
    async fn inventory_rejects_malformed_structural_fields() {
        for order in [
            r#"{"id":42,"expire_date":"2026-09-01","status":"confirmed","auto_extend_settings":null}"#,
            r#"{"id":42,"expire_date":"2026-09-01","status":"confirmed","proxy_data":{},"auto_extend_settings":null}"#,
            r#"{"id":42,"expire_date":"2026-09-01","status":"confirmed","proxy_data":{"proxies":[{}]},"auto_extend_settings":null}"#,
        ] {
            let body = Box::leak(
                format!(r#"{{"data":[{order}],"meta":{{"last_page":1}}}}"#).into_boxed_str(),
            );
            let (base, _, task) = mock_server(vec![
                (
                    200,
                    r#"{"data":[{"id":77,"name":"Dedicated ISP Proxies","plans":[],"locations":[]}] }"#,
                ),
                (200, body),
            ])
            .await;
            let client = Iproyal::with_base_url("key", &base).unwrap();
            assert!(client.isp_inventory().await.is_err(), "accepted {order}");
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn balance_number_and_string_are_exact_nano_usd() {
        let (base, _, task) = mock_server(vec![
            (200, "884.05"),
            (200, r#""0.123456789""#),
            (200, r#""7.1""#),
            (200, "18446744073.709551615"),
        ])
        .await;
        let client = Iproyal::with_base_url("key", &base).unwrap();

        assert_eq!(client.balance().await.unwrap(), "884050000000");
        assert_eq!(client.balance().await.unwrap(), "123456789");
        assert_eq!(client.balance().await.unwrap(), "7100000000");
        assert_eq!(client.balance().await.unwrap(), u64::MAX.to_string());
        task.await.unwrap();
    }

    #[tokio::test]
    async fn balance_rejects_noncanonical_or_unsafe_values() {
        for body in [
            r#""1.0000000001""#,
            "1e2",
            r#""1E2""#,
            "-1",
            r#""""#,
            "1.",
            ".1",
            "01",
            r#""1\\u002e5""#,
            r#""1.5"garbage"#,
            "null",
            "18446744074",
        ] {
            let (base, _, task) = mock_server(vec![(200, body)]).await;
            let client = Iproyal::with_base_url("key", &base).unwrap();
            assert!(client.balance().await.is_err(), "accepted {body}");
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn cards_return_only_safe_aggregates() {
        let (base, _, task) = mock_server(vec![(
            200,
            r#"{"data":[{"last_four_digits":"1234","has_royal_auto_extend_enabled":true},{"last_four_digits":"5678","has_royal_auto_extend_enabled":false}]}"#,
        )])
        .await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        let warning = client.cards_warning().await.unwrap();
        task.await.unwrap();

        assert_eq!(
            warning,
            CardsWarning {
                cards: 2,
                cards_with_auto_extend: 1,
                warning: true,
            }
        );
        assert!(!format!("{warning:?}").contains("1234"));
    }

    #[tokio::test]
    async fn standalone_auto_extend_guard_skips_already_disabled_order() {
        let (base, requests, task) = mock_server(vec![
            (
                200,
                r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[]},"auto_extend_settings":null}"#,
            ),
        ])
        .await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        assert_eq!(
            client
                .ensure_auto_extend_disabled(0)
                .await
                .unwrap_err()
                .to_string(),
            "invalid IPRoyal order id"
        );
        client.ensure_auto_extend_disabled(42).await.unwrap();
        task.await.unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /orders/42 HTTP/1.1"));
        assert!(requests.iter().all(|request| !request.starts_with("POST ")));
    }

    #[tokio::test]
    async fn standalone_auto_extend_guard_toggles_enabled_order_and_refetches() {
        let (base, requests, task) = mock_server(vec![
            (
                200,
                r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[]},"auto_extend_settings":{"is_enabled":true}}"#,
            ),
            (200, "{}"),
            (
                200,
                r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[]},"auto_extend_settings":null}"#,
            ),
        ])
        .await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        client.ensure_auto_extend_disabled(42).await.unwrap();
        task.await.unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /orders/42 HTTP/1.1"));
        assert!(requests[1].starts_with("POST /orders/toggle-auto-extend HTTP/1.1"));
        assert!(requests[1].contains(r#""order_id":42"#));
        assert!(requests[1].contains(r#""is_enabled":false"#));
        assert!(requests[2].starts_with("GET /orders/42 HTTP/1.1"));
    }

    #[tokio::test]
    async fn selective_extend_skips_disabled_toggle_and_posts_selected_ips() {
        let exact = r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"2001:db8::1"},{"ip":"203.0.113.9"}]},"auto_extend_settings":null}"#;
        let (base, requests, task) = mock_server(vec![
            (200, exact),
            (
                200,
                r#"{"data":[{"id":77,"name":"Dedicated ISP Proxies","plans":[{"id":321,"name":"30 Days"}],"locations":[]}]}"#,
            ),
            (200, r#"{"accepted":true}"#),
            (
                200,
                r#"{"id":42,"expire_date":"2026-09-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"2001:db8::1"},{"ip":"203.0.113.9"}]},"auto_extend_settings":null}"#,
            ),
        ])
        .await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        let expiry = client
            .extend_order_ips(42, &["2001:0db8:0:0:0:0:0:1".into()])
            .await
            .unwrap();
        task.await.unwrap();

        assert_eq!(expiry, "2026-09-10");
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /orders/42 HTTP/1.1"));
        assert!(requests[1].starts_with("GET /products HTTP/1.1"));
        assert!(requests[2].starts_with("POST /orders/42/extend HTTP/1.1"));
        assert!(requests
            .iter()
            .all(|request| { !request.starts_with("POST /orders/toggle-auto-extend ") }));
        let body = requests[2].split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            serde_json::json!({
                "product_plan_id": 321,
                "proxies": ["2001:db8::1"]
            })
        );
        assert!(requests[3].starts_with("GET /orders/42 HTTP/1.1"));
    }

    #[tokio::test]
    async fn selective_extend_rejects_empty_and_canonical_duplicates_without_post() {
        let (base, requests, task) = mock_server(vec![]).await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        let empty = client.extend_order_ips(42, &[]).await.unwrap_err();
        assert_eq!(empty.class(), ExtendOrderFailureClass::Rejected);
        assert_eq!(empty.phase(), "input_validation");
        let duplicate = client
            .extend_order_ips(42, &["2001:db8::1".into(), "2001:0db8:0:0:0:0:0:1".into()])
            .await
            .unwrap_err();
        assert_eq!(duplicate.class(), ExtendOrderFailureClass::Rejected);
        assert_eq!(duplicate.phase(), "input_validation");
        task.await.unwrap();
        assert!(requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn selective_extend_rejects_wrong_or_duplicate_occurrence_without_post() {
        for exact in [
            r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.10"}]},"auto_extend_settings":null}"#,
            r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"},{"ip":"203.0.113.9"}]},"auto_extend_settings":null}"#,
        ] {
            let (base, requests, task) = mock_server(vec![(200, exact)]).await;
            let client = Iproyal::with_base_url("key", &base).unwrap();
            let error = client
                .extend_order_ips(42, &["203.0.113.9".into()])
                .await
                .unwrap_err();
            assert_eq!(error.class(), ExtendOrderFailureClass::Rejected);
            assert_eq!(error.phase(), "order_preflight");
            task.await.unwrap();
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(requests[0].starts_with("GET /orders/42 HTTP/1.1"));
            assert!(!requests[0].starts_with("POST "));
        }
    }

    #[tokio::test]
    async fn selection_missing_from_exact_order_is_rejected_before_paid_post() {
        let exact = r#"{"id":42,"expire_date":"2026-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"},{"ip":"203.0.113.10"}]},"auto_extend_settings":null}"#;
        let (base, requests, task) = mock_server(vec![(200, exact)]).await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        let error = client
            .extend_order_ips(42, &["203.0.113.11".into()])
            .await
            .unwrap_err();
        task.await.unwrap();

        assert_eq!(error.class(), ExtendOrderFailureClass::Rejected);
        assert_eq!(error.phase(), "order_preflight");
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests.iter().all(|request| !request.starts_with("POST ")));
    }

    #[tokio::test]
    async fn paid_client_error_is_rejected_but_server_error_is_uncertain() {
        for (status, class) in [
            (422, ExtendOrderFailureClass::Rejected),
            (503, ExtendOrderFailureClass::Uncertain),
        ] {
            let (base, requests, task) =
                mock_server(vec![(status, r#"{"secret":"hidden"}"#)]).await;
            let client = Iproyal::with_base_url("key", &base).unwrap();
            let error = client
                .send_paid_extend(42, 321, &["203.0.113.9".into()])
                .await
                .unwrap_err();
            task.await.unwrap();

            assert_eq!(error.class(), class);
            assert_eq!(error.phase(), "paid_response");
            assert!(!error.to_string().contains("hidden"));
            let requests = requests.lock().unwrap();
            let body = requests[0].split("\r\n\r\n").nth(1).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(body).unwrap(),
                serde_json::json!({
                    "product_plan_id": 321,
                    "proxies": ["203.0.113.9"]
                })
            );
        }
    }

    #[tokio::test]
    async fn paid_transport_loss_is_uncertain() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 16 * 1024];
            let _ = stream.read(&mut bytes).await.unwrap();
        });
        let client = Iproyal::with_base_url("key", &format!("http://{address}")).unwrap();
        let error = client
            .send_paid_extend(42, 321, &["203.0.113.9".into()])
            .await
            .unwrap_err();
        task.await.unwrap();

        assert_eq!(error.class(), ExtendOrderFailureClass::Uncertain);
        assert_eq!(error.phase(), "paid_transport");
    }

    #[tokio::test]
    async fn upstream_error_body_is_never_exposed() {
        let secret = "upstream-secret-card-and-token";
        let leaked = Box::leak(format!(r#"{{"message":"{secret}"}}"#).into_boxed_str());
        let (base, _, task) = mock_server(vec![(500, leaked)]).await;
        let client = Iproyal::with_base_url("key", &base).unwrap();
        let error = client.balance().await.unwrap_err().to_string();
        task.await.unwrap();

        assert!(!error.contains(secret));
        assert!(error.len() <= 128);
        assert_eq!(error, "IPRoyal GET failed (500 Internal Server Error)");
    }
}
