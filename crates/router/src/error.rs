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
        } else if path.starts_with("/v1/responses")
            || path.starts_with("/v1/images/")
            || path == "/v1/chat/completions"
        {
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

fn anthropic_error_response(
    status: StatusCode,
    kind: &str,
    message: impl Into<String>,
) -> Response {
    let request_id = crate::identity::fresh_error_request_id()
        .expect("operating-system CSPRNG unavailable for synthetic Anthropic request identity");
    let mut response = json_response(
        status,
        json!({
            "type": "error",
            "error": {"type": kind, "message": message.into()},
            "request_id": request_id.clone(),
        }),
    );
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("request-id"),
        request_id
            .parse()
            .expect("router-generated UUIDv4 is a valid header value"),
    );
    response
}

/// 502: плоскость недостижима до заголовков ответа. Native lane и ambiguous
/// universal outcomes возвращают её сразу; только доказанный ConnectionRefused
/// может разрешить следующую явную fallback-модель (инвариант 2).
pub fn upstream_unavailable(lane: Lane, detail: &str) -> Response {
    match lane {
        Lane::Anthropic => anthropic_error_response(StatusCode::BAD_GATEWAY, "api_error", detail),
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
    match Lane::from_path(path) {
        Some(Lane::Anthropic) => anthropic_error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "not_found_error",
            "Method not allowed for this endpoint.",
        ),
        Some(Lane::Gemini) => json_response(
            StatusCode::METHOD_NOT_ALLOWED,
            json!({"error": {"code": 405,
                "message": "Method not allowed for this endpoint.", "status": "NOT_FOUND"}}),
        ),
        Some(Lane::OpenAi) | None => json_response(
            StatusCode::METHOD_NOT_ALLOWED,
            json!({"error": {"message": "Method not allowed for this endpoint.",
                "type": "invalid_request_error", "code": "method_not_allowed"}}),
        ),
    }
}

/// 400 universal chat-пути: тело не JSON или не содержит валидный `model`.
/// Конверт зеркалит 400-е OpenAI-плоскости на этом пути
/// (`invalid_request_error`, `code: null`), чтобы router-local отказ был
/// неотличим от отказа адаптера плоскости. Oversized bodies use 413 below so the
/// exact-SHA canary can tell admission failure from a local schema rejection.
pub fn invalid_chat_request(message: &str, param: Option<&str>) -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        json!({"error": {"message": message, "type": "invalid_request_error",
            "param": param, "code": serde_json::Value::Null}}),
    )
}

/// 413: declared or materialized body exceeded the composed router request cap.
/// Status is Payload Too Large so admission evidence can distinguish this from a
/// 400 after a fully admitted body. The envelope stays OpenAI-shaped.
pub fn chat_payload_too_large(message: &str) -> Response {
    json_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        json!({"error": {"message": message, "type": "invalid_request_error",
            "param": serde_json::Value::Null, "code": "payload_too_large"}}),
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

/// 503 bodyless auth authority unavailable before request-body materialization.
pub fn auth_unavailable() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": {"message": "Authentication is temporarily unavailable.",
            "type": "server_error", "code": "authentication_unavailable"}}),
    )
}

/// 503 worst-case universal request-body budget exhausted. This is admission, not execution.
pub fn body_admission_overloaded() -> Response {
    json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": {"message": "The router request-body budget is temporarily exhausted.",
            "type": "server_error", "code": "router_overloaded"}}),
    )
}

/// 408 slow/incomplete universal request body. Idle and maximum read deadlines are admission
/// safety; no provider execution has started and clients may retry with a complete request.
pub fn body_read_timeout() -> Response {
    json_response(
        StatusCode::REQUEST_TIMEOUT,
        json!({"error": {"message": "The request body was not received in time.",
            "type": "invalid_request_error", "code": "request_timeout"}}),
    )
}

/// 415 compressed request bodies are forbidden on materializing text routes. The wire cap is
/// counted on uncompressed JSON; gzip/br must not become a decompression-bomb bypass.
pub fn unsupported_content_encoding() -> Response {
    json_response(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        json!({"error": {"message": "Request Content-Encoding is not supported.",
            "type": "invalid_request_error", "code": "unsupported_content_encoding"}}),
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

/// 400 universal messages-пути: тело не JSON или не содержит валидный `model`.
/// Имя параметра — в тексте (в конверте поля param нет). Oversized bodies use 413.
pub fn invalid_messages_request(message: &str) -> Response {
    anthropic_error_response(StatusCode::BAD_REQUEST, "invalid_request_error", message)
}

/// 413 messages-пути: тело превысило составленный request cap router'а.
pub fn messages_payload_too_large(message: &str) -> Response {
    anthropic_error_response(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large", message)
}

/// 404 неизвестной модели на messages-пути — форма нативной Anthropic-плоскости.
pub fn messages_model_not_found(id: &str) -> Response {
    anthropic_error_response(
        StatusCode::NOT_FOUND,
        "not_found_error",
        format!("The model `{id}` does not exist or you do not have access to it."),
    )
}

/// 503 messages-пути: каталог недоступен, alias разрешить нельзя.
pub fn messages_catalog_unavailable() -> Response {
    anthropic_error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "api_error",
        "The model catalog is temporarily unavailable.",
    )
}

/// 503 policy preflight failure in the Anthropic Messages envelope.
pub fn messages_policy_unavailable() -> Response {
    anthropic_error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "api_error",
        "Account routing policy is temporarily unavailable.",
    )
}

/// 403 strict policy removed every candidate, before any provider attempt.
pub fn messages_policy_restricted() -> Response {
    anthropic_error_response(
        StatusCode::FORBIDDEN,
        "permission_error",
        "No requested model is allowed by the account routing policy.",
    )
}

/// Единый 401 messages-пути: ключ отклонён плоскостью при опросе каталога.
pub fn messages_auth_rejected() -> Response {
    anthropic_error_response(
        StatusCode::UNAUTHORIZED,
        "authentication_error",
        "Invalid or missing API key.",
    )
}

/// 503 bodyless auth authority unavailable in the Anthropic Messages envelope.
pub fn messages_auth_unavailable() -> Response {
    anthropic_error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "api_error",
        "Authentication is temporarily unavailable.",
    )
}

/// 503 request-body admission overload in the Anthropic Messages envelope.
pub fn messages_body_admission_overloaded() -> Response {
    anthropic_error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "api_error",
        "The router request-body budget is temporarily exhausted.",
    )
}

/// 408 request-body deadline in the Anthropic Messages envelope.
pub fn messages_body_read_timeout() -> Response {
    anthropic_error_response(
        StatusCode::REQUEST_TIMEOUT,
        "invalid_request_error",
        "The request body was not received in time.",
    )
}

/// 415 compressed request bodies are forbidden on the Anthropic Messages envelope.
pub fn messages_unsupported_content_encoding() -> Response {
    anthropic_error_response(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "invalid_request_error",
        "Request Content-Encoding is not supported.",
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

    async fn assert_anthropic_request_identity(response: Response) -> serde_json::Value {
        let request_id = response
            .headers()
            .get("request-id")
            .and_then(|value| value.to_str().ok())
            .unwrap()
            .to_string();
        let json = body_json(response).await;
        assert_eq!(json["request_id"], request_id);
        json
    }

    #[tokio::test]
    async fn upstream_unavailable_matches_lane_envelope() {
        let response = upstream_unavailable(Lane::Anthropic, "x");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let json = assert_anthropic_request_identity(response).await;
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
        assert_eq!(
            body_json(response).await["error"]["type"],
            "not_found_error"
        );

        let response = method_not_allowed("/v1beta/models");
        assert_eq!(body_json(response).await["error"]["status"], "NOT_FOUND");

        let response = method_not_allowed("/v1/responses");
        assert_eq!(
            body_json(response).await["error"]["code"],
            "method_not_allowed"
        );
    }

    #[tokio::test]
    async fn invalid_chat_request_is_openai_shaped_400() {
        let response = invalid_chat_request(
            "Missing or invalid required parameter: model.",
            Some("model"),
        );
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
    async fn chat_payload_too_large_is_openai_shaped_413() {
        let response = chat_payload_too_large(&format!(
            "Request body exceeds the {} limit.",
            api_limits::current::ROUTER_REQUEST
        ));
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["code"], "payload_too_large");
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&api_limits::current::ROUTER_REQUEST.to_string()));
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

    #[tokio::test]
    async fn early_auth_and_body_overload_are_openai_shaped_503s() {
        let response = auth_unavailable();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body_json(response).await["error"]["code"],
            "authentication_unavailable"
        );

        let response = body_admission_overloaded();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body_json(response).await["error"]["code"],
            "router_overloaded"
        );

        let response = body_read_timeout();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(
            body_json(response).await["error"]["code"],
            "request_timeout"
        );
    }

    // ---------- universal messages dispatch: Anthropic-конверт (этап 5.1) ----------

    #[tokio::test]
    async fn messages_dispatch_errors_are_anthropic_shaped() {
        let response = invalid_messages_request("Missing or invalid required parameter: model.");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = assert_anthropic_request_identity(response).await;
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("model"));
        // В Anthropic-конверте нет param/code полей OpenAI-формы.
        assert!(json["error"].get("param").is_none());
        assert!(json["error"].get("code").is_none());

        let response = messages_model_not_found("gpt-9");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = assert_anthropic_request_identity(response).await;
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "not_found_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("gpt-9"));

        let response = messages_catalog_unavailable();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = assert_anthropic_request_identity(response).await;
        assert_eq!(json["error"]["type"], "api_error");

        let response = messages_auth_rejected();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = assert_anthropic_request_identity(response).await;
        assert_eq!(json["error"]["type"], "authentication_error");

        for response in [
            messages_auth_unavailable(),
            messages_body_admission_overloaded(),
        ] {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let json = assert_anthropic_request_identity(response).await;
            assert_eq!(json["type"], "error");
            assert_eq!(json["error"]["type"], "api_error");
        }
        let response = messages_body_read_timeout();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(
            assert_anthropic_request_identity(response).await["error"]["type"],
            "invalid_request_error"
        );

        let response = messages_payload_too_large("too large");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            assert_anthropic_request_identity(response).await["error"]["type"],
            "request_too_large"
        );
    }
}
