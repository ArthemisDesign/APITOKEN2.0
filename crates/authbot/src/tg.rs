//! Тонкий async-клиент Telegram Bot API (reqwest) + нужные типы. Только то, что использует бот.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct Bot {
    token: String,
    api_root: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
    pub edited_message: Option<Message>,
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Message {
    pub chat: Chat,
    pub from: Option<User>,
    pub text: Option<String>,
    #[serde(default)]
    pub message_id: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Chat {
    pub id: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct User {
    pub id: i64,
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub message: Option<Message>,
    pub data: Option<String>,
}

/// Инлайн-клавиатура: строки из пар (подпись, callback_data).
pub type Keyboard = Vec<Vec<(String, String)>>;

impl Bot {
    pub fn new(token: &str) -> Self {
        Self::with_api_root(token, "https://api.telegram.org")
    }

    fn with_api_root(token: &str, api_root: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .expect("reqwest client");
        Bot {
            token: token.to_string(),
            api_root: api_root.trim_end_matches('/').to_string(),
            http,
        }
    }

    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_root, self.token, method)
    }

    /// Убрать bot-токен из строк ошибок (иначе он утекает в journalctl через URL).
    fn redact(&self, s: &str) -> String {
        s.replace(&self.token, "***")
    }

    async fn call(&self, method: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(method))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("{}", self.redact(&e.to_string())))?;
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| anyhow!("{}", self.redact(&e.to_string())))?;
        if v.get("ok").and_then(|b| b.as_bool()) != Some(true) {
            return Err(anyhow!(
                "{}: {}",
                method,
                v.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("error")
            ));
        }
        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    pub async fn get_me(&self) -> Result<String> {
        let r = self.call("getMe", serde_json::json!({})).await?;
        Ok(r.get("username")
            .and_then(|u| u.as_str())
            .unwrap_or("?")
            .to_string())
    }

    pub async fn get_updates(&self, offset: Option<i64>, timeout: u64) -> Result<Vec<Update>> {
        let mut body = serde_json::json!({ "timeout": timeout });
        if let Some(o) = offset {
            body["offset"] = serde_json::json!(o);
        }
        let r = self.call("getUpdates", body).await?;
        Ok(serde_json::from_value(r)?)
    }

    fn markup(kb: &Keyboard) -> serde_json::Value {
        let rows: Vec<Vec<serde_json::Value>> = kb
            .iter()
            .map(|row| {
                row.iter()
                    .map(|(t, d)| serde_json::json!({"text": t, "callback_data": d}))
                    .collect()
            })
            .collect();
        serde_json::json!({ "inline_keyboard": rows })
    }

    pub async fn send(&self, chat: i64, text: &str) -> Result<()> {
        self.send_kb(chat, text, None).await
    }

    pub async fn send_kb(&self, chat: i64, text: &str, kb: Option<&Keyboard>) -> Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat, "text": text,
            "parse_mode": "HTML", "disable_web_page_preview": true,
        });
        if let Some(k) = kb {
            body["reply_markup"] = Self::markup(k);
        }
        self.call("sendMessage", body).await?;
        Ok(())
    }

    /// Send one HTTPS authorization button. URL buttons are kept separate from callback-data
    /// keyboards so an OAuth URL can never be reflected back through `on_callback` or logs.
    pub async fn send_url_button(
        &self,
        chat: i64,
        text: &str,
        label: &str,
        url: &str,
    ) -> Result<()> {
        let parsed = reqwest::Url::parse(url)?;
        if parsed.scheme() != "https" {
            return Err(anyhow!("authorization button requires HTTPS"));
        }
        let body = serde_json::json!({
            "chat_id": chat,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
            "reply_markup": {
                "inline_keyboard": [[{"text": label, "url": parsed.as_str()}]]
            }
        });
        self.call("sendMessage", body).await?;
        Ok(())
    }

    /// Сообщение с ЗАКРЕПлённой нижней клавиатурой (reply keyboard). Кнопки шлют свой текст.
    /// resize + is_persistent: клавиатура компактная и не исчезает после нажатия.
    pub async fn send_reply_kb(&self, chat: i64, text: &str, rows: &[Vec<&str>]) -> Result<()> {
        let kb: Vec<Vec<serde_json::Value>> = rows
            .iter()
            .map(|r| r.iter().map(|b| serde_json::json!({ "text": b })).collect())
            .collect();
        let body = serde_json::json!({
            "chat_id": chat, "text": text,
            "parse_mode": "HTML", "disable_web_page_preview": true,
            "reply_markup": { "keyboard": kb, "resize_keyboard": true, "is_persistent": true },
        });
        self.call("sendMessage", body).await?;
        Ok(())
    }

    pub async fn answer_callback(&self, cb_id: &str, text: Option<&str>) -> Result<()> {
        let mut body = serde_json::json!({ "callback_query_id": cb_id });
        if let Some(t) = text {
            body["text"] = serde_json::json!(t);
        }
        // Ошибку "query is too old" гасим — не критично.
        let _ = self.call("answerCallbackQuery", body).await;
        Ok(())
    }

    pub async fn delete_webhook(&self) -> Result<()> {
        let _ = self.call("deleteWebhook", serde_json::json!({})).await;
        Ok(())
    }
}
