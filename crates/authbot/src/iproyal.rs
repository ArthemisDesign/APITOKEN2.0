//! Клиент IPRoyal reseller API — авто-выпуск UK ISP прокси (30 дней) для передачи продавцу.
//! База `https://apid.iproyal.com/v1/reseller`, авторизация заголовком `X-Access-Token`.
//! Каталог (продукт/план/локация) подтягиваем ДИНАМИЧЕСКИ по именам — устойчиво к смене ID.
//! Город выбираем СЛУЧАЙНО из доступных (in-stock) британских городов.
//!
//! Прокси с кредами приходят в `GET /orders/{id}` → `proxy_data.proxies[]`
//! (`{ip, username, password}`), порты — в `proxy_data.ports` (`socks5` и `http|https`).
//! Используем SOCKS5-порт: HTTP-порт IPRoyal ISP на практике нестабилен (reset), а SOCKS5
//! работает и движок его нативно поддерживает. Строка — `socks5h://user:pass@ip:socks5_port`.
//! Эндпоинт `/orders/proxies` — тупиковый (422). Подтверждено живыми заказами 2026-07-15.

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BASE: &str = "https://apid.iproyal.com/v1/reseller";

pub struct Iproyal {
    key: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct IssuedProxy {
    pub host: String,
    pub port: u32, // HTTP(CONNECT)-порт: claude CLI умеет только HTTP-прокси, движок тоже
    pub user: String,
    pub pass: String,
    pub city: String, // человекочитаемо, напр. "London (England)"
    pub order_id: i64,
}

impl IssuedProxy {
    /// `http://user:pass@host:port` — формат для реестра/движка/claude CLI (HTTP CONNECT).
    pub fn url(&self) -> String {
        format!("http://{}:{}@{}:{}", self.user, self.pass, self.host, self.port)
    }
    /// `host:port:user:pass` — компактный формат для человека (HTTP-порт).
    pub fn compact(&self) -> String {
        format!("{}:{}:{}:{}", self.host, self.port, self.user, self.pass)
    }
}

impl Iproyal {
    pub fn new(key: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Iproyal { key: key.to_string(), http }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let r = self
            .http
            .get(format!("{BASE}{path}"))
            .header("X-Access-Token", &self.key)
            .header("accept", "application/json")
            .send()
            .await?;
        let code = r.status();
        let v: Value = r.json().await?;
        if !code.is_success() {
            return Err(anyhow!("IPRoyal GET {path} -> {code}"));
        }
        Ok(v)
    }

    /// Список ISP-заказов: (ip, expire_date, order_id). Пагинация (`page` обязателен). Для маппинга
    /// прокси подписки → срок + заказ (чтобы ПРОДЛИТЬ тот же прокси). Fingerprint-free (IPRoyal API).
    pub async fn list_isp_orders(&self) -> Result<Vec<(String, String, i64)>> {
        let mut out = Vec::new();
        for page in 1..=50 {
            let v = self.get(&format!("/orders?product_id=9&page={page}&per_page=100")).await?;
            let data = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
            if data.is_empty() { break; }
            for o in &data {
                let exp = o.get("expire_date").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let oid = o.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
                if let Some(ps) = o.get("proxy_data").and_then(|d| d.get("proxies")).and_then(|d| d.as_array()) {
                    for p in ps {
                        if let Some(ip) = p.get("ip").and_then(|x| x.as_str()) {
                            out.push((ip.to_string(), exp.clone(), oid));
                        }
                    }
                }
            }
            let last = v.get("meta").and_then(|m| m.get("last_page")).and_then(|x| x.as_i64()).unwrap_or(page);
            if page >= last { break; }
        }
        Ok(out)
    }

    /// Продлить заказ (`POST /orders/{id}/extend`) на 30 дней — ТОТ ЖЕ IP (egress не меняется →
    /// ноль аномалии для Anthropic). Тратит баланс IPRoyal. Возвращает новый expire_date.
    pub async fn extend_order(&self, order_id: i64) -> Result<String> {
        let body = serde_json::json!({ "product_plan_id": 22 }); // 22 = «30 Days» (ISP Dedicated)
        let r = self.http.post(format!("{BASE}/orders/{order_id}/extend"))
            .header("X-Access-Token", &self.key)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .json(&body).send().await?;
        let code = r.status();
        let v: Value = r.json().await?;
        if !code.is_success() {
            return Err(anyhow!("IPRoyal extend {order_id} -> {code}: {}", short(&v)));
        }
        Ok(v.get("expire_date").and_then(|x| x.as_str()).unwrap_or("").to_string())
    }

    /// Выпуск ОДНОГО UK ISP прокси на 30 дней. Город — случайный из доступных.
    pub async fn issue_uk_isp_30d(&self) -> Result<IssuedProxy> {
        // 1) каталог: ISP-продукт, план «30 Days», локация United Kingdom + in-stock города
        let products = self.get("/products").await?;
        let arr = products
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow!("products: нет data"))?;
        let prod = arr
            .iter()
            .find(|p| {
                p.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.to_lowercase().contains("isp"))
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow!("ISP-продукт не найден в каталоге"))?;
        let product_id = prod
            .get("id")
            .and_then(|x| x.as_i64())
            .ok_or_else(|| anyhow!("нет product id"))?;
        let plan_id = prod
            .get("plans")
            .and_then(|p| p.as_array())
            .and_then(|ps| ps.iter().find(|pl| pl.get("name").and_then(|n| n.as_str()) == Some("30 Days")))
            .and_then(|pl| pl.get("id"))
            .and_then(|x| x.as_i64())
            .ok_or_else(|| anyhow!("план «30 Days» не найден"))?;
        let uk = prod
            .get("locations")
            .and_then(|l| l.as_array())
            .and_then(|ls| ls.iter().find(|l| l.get("name").and_then(|n| n.as_str()) == Some("United Kingdom")))
            .ok_or_else(|| anyhow!("локация United Kingdom не найдена"))?;

        // in-stock города (child_locations); если нет — уровень страны
        let mut cities: Vec<(i64, String)> = uk
            .get("child_locations")
            .and_then(|c| c.as_array())
            .map(|cs| {
                cs.iter()
                    .filter(|c| c.get("out_of_stock").and_then(|b| b.as_bool()) == Some(false))
                    .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        if cities.is_empty() {
            let id = uk.get("id").and_then(|x| x.as_i64()).ok_or_else(|| anyhow!("нет UK location id"))?;
            cities.push((id, "United Kingdom".into()));
        }
        // случайный город (без rand-крейта: наносекунды как источник энтропии)
        let idx = (nanos() as usize) % cities.len();
        let (location_id, city) = cities[idx].clone();

        // 2) POST /orders — покупка 1 прокси на 30 дней (без авто-продления)
        let body = serde_json::json!({
            "product_id": product_id,
            "product_plan_id": plan_id,
            "product_location_id": location_id,
            "quantity": 1,
            "auto_extend": false
        });
        let r = self
            .http
            .post(format!("{BASE}/orders"))
            .header("X-Access-Token", &self.key)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .json(&body)
            .send()
            .await?;
        let code = r.status();
        let ov: Value = r.json().await?;
        if !code.is_success() {
            return Err(anyhow!("IPRoyal POST /orders -> {code}: {}", short(&ov)));
        }
        let order_id = ov
            .get("id")
            .and_then(|x| x.as_i64())
            .or_else(|| ov.get("data").and_then(|d| d.get("id")).and_then(|x| x.as_i64()))
            .ok_or_else(|| anyhow!("order id не найден в ответе: {}", short(&ov)))?;

        // 3) забрать прокси с кредами (провижн не мгновенный) — до ~10 попыток
        let mut last = Value::Null;
        for _ in 0..10 {
            // Прокси с кредами — в самом заказе: GET /orders/{id} → proxy_data.proxies[].
            let list = self.get(&format!("/orders/{order_id}")).await.unwrap_or(Value::Null);
            last = list.clone();
            if let Some(px) = extract_proxy(&list, order_id, &city) {
                return Ok(px);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Err(anyhow!(
            "заказ {order_id} создан, но прокси не удалось распарсить. Сырой ответ: {}",
            short(&last)
        ))
    }
}

fn nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as u64).unwrap_or(1)
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 300 { format!("{}…", &s[..300]) } else { s }
}

/// Первый прокси-объект из ответа (гибко: data[]/proxies[]/proxy_data.proxies[]).
fn first_proxy(v: &Value) -> Option<Value> {
    let direct = [
        v.get("proxies").and_then(|d| d.as_array()),
        v.get("data").and_then(|d| d.as_array()),
        v.get("proxy_data").and_then(|d| d.get("proxies")).and_then(|d| d.as_array()),
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
        if let Some(n) = proxy.get(k).and_then(|x| x.as_u64()) {
            return Some(n as u32);
        }
    }
    let port_srcs = [
        proxy.get("ports"),
        order.get("proxy_data").and_then(|d| d.get("ports")),
        order.get("data").and_then(|d| d.get("proxy_data")).and_then(|d| d.get("ports")),
        order.get("ports"),
    ];
    for src in port_srcs.into_iter().flatten() {
        for key in ["http|https", "http", "https", "http_port"] {
            if let Some(n) = src.get(key).and_then(|x| x.as_u64()) {
                return Some(n as u32);
            }
        }
    }
    None
}

/// Собрать IssuedProxy из ответа; None пока прокси не провизился/схема иная.
fn extract_proxy(list: &Value, order_id: i64, city: &str) -> Option<IssuedProxy> {
    let p = first_proxy(list)?;
    let host = sfield(&p, &["ip", "host", "address", "proxy_address", "ip_address"])?;
    let user = sfield(&p, &["username", "login", "user", "proxy_username"])?;
    let pass = sfield(&p, &["password", "pass", "proxy_password"])?;
    let port = http_port(&p, list)?;
    if host.is_empty() {
        return None;
    }
    Some(IssuedProxy { host, port, user, pass, city: city.to_string(), order_id })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
