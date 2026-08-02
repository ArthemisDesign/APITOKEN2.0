//! Синтетические ошибки самого router'а. Ошибки, пришедшие из плоскостей,
//! проксируются байт-в-байт и сюда не попадают. Форма ответа повторяет нативную
//! ошибку соответствующего провайдера: harness-клиенты разбирают ошибки по
//! провайдер-специфичному конверту (Claude Code иногда восстанавливается по
//! тексту ошибки, поэтому оборачивать чужой формат нельзя — см.
//! docs/engine/UNIFIED_ROUTER.md, «Совместимость с harness-агентами»).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Плоскость, которой принадлежит путь запроса. Определяет форму синтетической
/// ошибки router'а.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Anthropic,
    OpenAi,
    Gemini,
}

impl Lane {
    /// Плоскость по префиксу пути публичного контракта. `None` — путь вне
    /// контракта; его 404 шейпится в нейтральном OpenAI-совместимом конверте.
    pub fn from_path(path: &str) -> Option<Lane> {
        if path == "/balance" || path.starts_with("/v1/messages") {
            Some(Lane::Anthropic)
        } else if path.starts_with("/v1/responses") || path == "/v1/chat/completions" {
            Some(Lane::OpenAi)
        } else if path.starts_with("/v1beta/") {
            Some(Lane::Gemini)
        } else if path == "/v1/models" || path.starts_with("/v1/models/") {
            // Единый каталог — собственная поверхность router'а; её ошибки
            // OpenAI-совместимы (формат самого каталога).
            Some(Lane::OpenAi)
        } else {
            None
        }
    }
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    (status, axum::Json(body)).into_response()
}

/// 502: плоскость недостижима до заголовков ответа. Native lane и ambiguous
/// universal outcomes возвращают её сразу; только доказанный ConnectionRefused
/// может разрешить следующую явную fallback-модель (инвариант 2).
pub fn upstream_unavailable(lane: Lane, detail: &str) -> Response {
    match lane {
        Lane::Anthropic => json_response(
            StatusCode::BAD_GATEWAY,
            json!({"type": "error", "error": {"type": "api_error", "message": detail}}),
        ),
        Lane::OpenAi => json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error": {"message": detail, "type": "server_error", "code": "bad_gateway"}}),
        ),
        Lane::Gemini => json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error": {"code": 502, "message": detail, "status": "UNAVAILABLE"}}),
        ),
    }
}

/// 404 для пути вне публичного контракта — той же формой, что отвечает
/// OpenAI-плоскость на неподдерживаемый endpoint.
pub fn unsupported_endpoint() -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        json!({"error": {"message": "The requested endpoint is not supported.",
            "type": "invalid_request_error", "code": "unsupported_endpoint"}}),
    )
}

/// 405 в форме плоскости, выбранной по пути (harness видит привычный конверт).
pub fn method_not_allowed(path: &str) -> Response {
    let body = match Lane::from_path(path) {
        Some(Lane::Anthropic) => json!({"type": "error", "error": {"type": "not_found_error",
            "message": "Method not allowed for this endpoint."}}),
        Some(Lane::Gemini) => json!({"error": {"code": 405,
            "message": "Method not allowed for this endpoint.", "status": "NOT_FOUND"}}),
        Some(Lane::OpenAi) | None => json!({"error": {"message": "Method not allowed for this endpoint.",
            "type": "invalid_request_error", "code": "method_not_allowed"}}),
    };
    json_response(StatusCode::METHOD_NOT_ALLOWED, body)
}

/// 400 universal chat-пути: тело не JSON, превышает лимит или не содержит
/// валидный `model`. Конверт зеркалит 400-е OpenAI-плоскости на этом пути
/// (`invalid_request_error`, `code: null`), чтобы router-local отказ был
/// неотличим от отказа адаптера плоскости.
pub fn invalid_chat_request(message: &str, param: Option<&str>) -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        json!({"error": {"message": message, "type": "invalid_request_error",
            "param": param, "code": serde_json::Value::Null}}),
    )
}

/// 404 неизвестной модели — зеркалит контракт OpenAI-плоскости
/// (`model_not_found`), потому что каталог OpenAI-совместим.
pub fn model_not_found(id: &str) -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        json!({"error": {"message": format!("The model `{id}` does not exist or you do not have access to it."),
            "type": "invalid_request_error", "param": "model", "code": "model_not_found"}}),
    )
}

/// 401: клиентский ключ отклонён плоскостью. Плоскости делят один billing
/// authority, поэтому 401 любой из них однозначно означает невалидный ключ;
/// ответ плоскости отдаётся клиенту как есть (см. proxy::catalog_fetch).
pub fn catalog_unavailable() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": {"message": "The model catalog is temporarily unavailable.",
            "type": "server_error", "code": "catalog_unavailable"}}),
    )
}

/// 503 key-scoped pricing authority unavailable. A catalog without an authoritative current-key
/// overlay would invite clients to treat absent rates as zero or reuse another account's prices.
pub fn pricing_unavailable() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": {"message": "Personalized model pricing is temporarily unavailable.",
            "type": "server_error", "code": "pricing_unavailable"}}),
    )
}

/// 503 account-policy authority unavailable before execution.
pub fn policy_unavailable() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": {"message": "Account routing policy is temporarily unavailable.",
            "type": "server_error", "code": "policy_unavailable"}}),
    )
}

/// 403 no candidate in the logical chain is admitted by the account policy.
pub fn policy_restricted() -> Response {
    json_response(
        StatusCode::FORBIDDEN,
        json!({"error": {"message": "No requested model is allowed by the account routing policy.",
            "type": "permission_error", "code": "policy_restricted"}}),
    )
}

// ---------- Anthropic-конверт: universal messages dispatch (этап 5.1) ----------
//
// Путь `/v1/messages` говорит на Messages, поэтому синтетические ошибки его
// dispatch'а — в конверте нативной Anthropic-плоскости
// (`{"type":"error","error":{"type":...,"message":...}}`, без param/code — их
// в этом конверте нет): Claude Code восстанавливается по тексту ошибки, чужой
// формат оборачивать нельзя (см. «Совместимость с harness-агентами»). Зеркало
// OpenAI-конверта chat/responses dispatch'ей выше.

/// 400 universal messages-пути: тело не JSON, превышает лимит или не содержит
/// валидный `model`. Имя параметра — в тексте (в конверте поля param нет).
pub fn invalid_messages_request(message: &str) -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        json!({"type": "error", "error": {"type": "invalid_request_error", "message": message}}),
    )
}

/// 404 неизвестной модели на messages-пути — форма нативной Anthropic-плоскости.
pub fn messages_model_not_found(id: &str) -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        json!({"type": "error", "error": {"type": "not_found_error",
            "message": format!("The model `{id}` does not exist or you do not have access to it.")}}),
    )
}

/// 503 messages-пути: каталог недоступен, alias разрешить нельзя.
pub fn messages_catalog_unavailable() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"type": "error", "error": {"type": "api_error",
            "message": "The model catalog is temporarily unavailable."}}),
    )
}

/// 503 policy preflight failure in the Anthropic Messages envelope.
pub fn messages_policy_unavailable() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"type": "error", "error": {"type": "api_error",
            "message": "Account routing policy is temporarily unavailable."}}),
    )
}

/// 403 strict policy removed every candidate, before any provider attempt.
pub fn messages_policy_restricted() -> Response {
    json_response(
        StatusCode::FORBIDDEN,
        json!({"type": "error", "error": {"type": "permission_error",
            "message": "No requested model is allowed by the account routing policy."}}),
    )
}

/// Единый 401 messages-пути: ключ отклонён плоскостью при опросе каталога.
pub fn messages_auth_rejected() -> Response {
    json_response(
        StatusCode::UNAUTHORIZED,
        json!({"type": "error", "error": {"type": "authentication_error",
            "message": "Invalid or missing API key."}}),
    )
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::StatusCode;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(Body::from(response.into_body()), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn upstream_unavailable_matches_lane_envelope() {
        let response = upstream_unavailable(Lane::Anthropic, "x");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let json = body_json(response).await;
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "api_error");

        let response = upstream_unavailable(Lane::OpenAi, "x");
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "server_error");
        assert_eq!(json["error"]["code"], "bad_gateway");

        let response = upstream_unavailable(Lane::Gemini, "x");
        let json = body_json(response).await;
        assert_eq!(json["error"]["code"], 502);
        assert_eq!(json["error"]["status"], "UNAVAILABLE");
    }

    #[tokio::test]
    async fn unsupported_endpoint_is_openai_shaped_404() {
        let response = unsupported_endpoint();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["code"], "unsupported_endpoint");
    }

    #[tokio::test]
    async fn method_not_allowed_follows_lane_of_path() {
        let response = method_not_allowed("/v1/messages");
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(body_json(response).await["error"]["type"], "not_found_error");

        let response = method_not_allowed("/v1beta/models");
        assert_eq!(body_json(response).await["error"]["status"], "NOT_FOUND");

        let response = method_not_allowed("/v1/responses");
        assert_eq!(body_json(response).await["error"]["code"], "method_not_allowed");
    }

    #[tokio::test]
    async fn invalid_chat_request_is_openai_shaped_400() {
        let response = invalid_chat_request("Missing or invalid required parameter: model.", Some("model"));
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["param"], "model");
        assert!(json["error"]["code"].is_null());

        let response = invalid_chat_request("Invalid JSON in request body.", None);
        let json = body_json(response).await;
        assert!(json["error"]["param"].is_null());
    }

    #[tokio::test]
    async fn model_not_found_mirrors_openai_contract() {
        let response = model_not_found("gpt-9");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = body_json(response).await;
        assert_eq!(json["error"]["code"], "model_not_found");
        assert_eq!(json["error"]["param"], "model");
        assert!(json["error"]["message"].as_str().unwrap().contains("gpt-9"));
    }

    // ---------- universal messages dispatch: Anthropic-конверт (этап 5.1) ----------

    #[tokio::test]
    async fn messages_dispatch_errors_are_anthropic_shaped() {
        let response = invalid_messages_request("Missing or invalid required parameter: model.");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = body_json(response).await;
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("model"));
        // В Anthropic-конверте нет param/code полей OpenAI-формы.
        assert!(json["error"].get("param").is_none());
        assert!(json["error"].get("code").is_none());

        let response = messages_model_not_found("gpt-9");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = body_json(response).await;
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "not_found_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("gpt-9"));

        let response = messages_catalog_unavailable();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "api_error");

        let response = messages_auth_rejected();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "authentication_error");
    }
}
