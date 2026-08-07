//! OpenAI-compatible Images API backed exclusively by the sealed ChatGPT OAuth pool.

use super::api::ApiError;
use super::billing::{begin_admission, AdmissionError, OpenAiImageAdmission};
use super::openai_image_snapshot::OpenAiImageOperation;
use super::{
    new_id, CodexGateway, CodexImageError, CodexImageResult, ImageBackground, ImageEditRequest,
    ImageGenerationRequest, ImageQuality, ImageReference, ImageSize, ImageTurnId,
};
use crate::proxy::TerminalErrorReason;
use crate::state::AppState;
use axum::body::to_bytes;
use axum::extract::{ConnectInfo, FromRequest, Multipart, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::STANDARD as STANDARD_BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

pub const IMAGE_GENERATION_BODY_LIMIT: usize = 256 * 1024;
const MAX_TEXT_FIELD_BYTES: usize = 128 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationBody {
    model: String,
    prompt: String,
    #[serde(default)]
    n: Option<u64>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    output_format: Option<String>,
    #[serde(default)]
    response_format: Option<String>,
}

#[derive(Default)]
struct EditForm {
    model: Option<String>,
    prompt: Option<String>,
    images: Vec<Vec<u8>>,
    n: Option<String>,
    background: Option<String>,
    quality: Option<String>,
    size: Option<String>,
    output_format: Option<String>,
    response_format: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ParsedImageUsage {
    metered: metering::OpenAiImageUsage,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

pub async fn generations(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let raw = match to_bytes(body, IMAGE_GENERATION_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => {
            return ApiError::invalid(
                "Request body exceeds the 256 KiB image generation limit.",
                None::<String>,
            )
            .into_response()
        }
    };
    let body: GenerationBody = match serde_json::from_slice(&raw) {
        Ok(body) => body,
        Err(_) => {
            return ApiError::invalid(
                "Invalid image generation JSON or unsupported field.",
                None::<String>,
            )
            .into_response()
        }
    };
    let model = match validate_model(&body.model) {
        Ok(model) => model,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = validate_controls(
        body.n,
        body.background.as_deref(),
        body.quality.as_deref(),
        body.size.as_deref(),
        body.output_format.as_deref(),
        body.response_format.as_deref(),
    ) {
        return error.into_response();
    }
    let image_request = match ImageGenerationRequest::new(body.prompt).map(|request| {
        request.with_controls(ImageBackground::Opaque, ImageQuality::Low, ImageSize::Auto)
    }) {
        Ok(request) => request,
        Err(error) => return validation_error(error).into_response(),
    };
    execute_generation(app, gateway, pending, model, image_request).await
}

pub async fn edits(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let request = axum::http::Request::from_parts(parts, body);
    let mut multipart = match Multipart::from_request(request, &app).await {
        Ok(multipart) => multipart,
        Err(_) => {
            return ApiError::invalid(
                "Invalid multipart image edit body or body exceeds the 17 MiB limit.",
                None::<String>,
            )
            .into_response()
        }
    };
    let form = match parse_edit_form(&mut multipart).await {
        Ok(form) => form,
        Err(error) => return error.into_response(),
    };
    let model_value = match form.model.as_deref() {
        Some(model) => model,
        None => {
            return ApiError::invalid("Missing required field: model.", "model".to_owned())
                .into_response()
        }
    };
    let model = match validate_model(model_value) {
        Ok(model) => model,
        Err(error) => return error.into_response(),
    };
    let n = match parse_optional_n(form.n.as_deref()) {
        Ok(n) => n,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = validate_controls(
        n,
        form.background.as_deref(),
        form.quality.as_deref(),
        form.size.as_deref(),
        form.output_format.as_deref(),
        form.response_format.as_deref(),
    ) {
        return error.into_response();
    }
    let prompt = match form.prompt {
        Some(prompt) => prompt,
        None => {
            return ApiError::invalid("Missing required field: prompt.", "prompt".to_owned())
                .into_response()
        }
    };
    if form.images.is_empty() {
        return ApiError::invalid("Missing required PNG field: image.", "image".to_owned())
            .into_response();
    }
    let mut references = Vec::with_capacity(form.images.len());
    for image in form.images {
        match ImageReference::new(image) {
            Ok(reference) => references.push(reference),
            Err(error) => return validation_error(error).into_response(),
        }
    }
    let Some(operation) = OpenAiImageOperation::edit(references.len()) else {
        return ApiError::invalid("Too many image fields.", "image".to_owned()).into_response();
    };
    let image_request = match ImageEditRequest::new(prompt, references).map(|request| {
        request.with_controls(ImageBackground::Opaque, ImageQuality::Low, ImageSize::Auto)
    }) {
        Ok(request) => request,
        Err(error) => return validation_error(error).into_response(),
    };
    execute_edit(app, gateway, pending, model, image_request, operation).await
}

async fn execute_generation(
    app: AppState,
    gateway: std::sync::Arc<CodexGateway>,
    pending: super::billing::PendingCodexAdmission,
    model: String,
    request: ImageGenerationRequest,
) -> Response {
    let home = match preflight_home(&gateway).await {
        Ok(home) => home,
        Err(error) => return transport_error(error, None),
    };
    let mut admission = match pending
        .reserve_image(&app, &model, OpenAiImageOperation::Generation)
        .await
    {
        Ok(admission) => admission,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let turn = match ImageTurnId::new(new_id("image_turn")) {
        Ok(turn) => turn,
        Err(error) => return validation_error(error).into_response(),
    };
    if let Err(error) = admission.mark_delivering().await {
        elog::error("codex-image", "codex image delivery marker failed");
        return ApiError::from(error).into_response();
    }
    let result = match gateway.generate_image_on_home(&home, &turn, &request).await {
        Ok(result) => result,
        Err(error) => return transport_error(error, Some(admission)),
    };
    completed_image(admission, &model, result, OpenAiImageOperation::Generation).await
}

async fn execute_edit(
    app: AppState,
    gateway: std::sync::Arc<CodexGateway>,
    pending: super::billing::PendingCodexAdmission,
    model: String,
    request: ImageEditRequest,
    operation: OpenAiImageOperation,
) -> Response {
    let home = match preflight_home(&gateway).await {
        Ok(home) => home,
        Err(error) => return transport_error(error, None),
    };
    let mut admission = match pending.reserve_image(&app, &model, operation).await {
        Ok(admission) => admission,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let turn = match ImageTurnId::new(new_id("image_turn")) {
        Ok(turn) => turn,
        Err(error) => return validation_error(error).into_response(),
    };
    if let Err(error) = admission.mark_delivering().await {
        elog::error("codex-image", "codex image delivery marker failed");
        return ApiError::from(error).into_response();
    }
    let result = match gateway.edit_image_on_home(&home, &turn, &request).await {
        Ok(result) => result,
        Err(error) => return transport_error(error, Some(admission)),
    };
    completed_image(admission, &model, result, operation).await
}

async fn preflight_home(gateway: &CodexGateway) -> Result<String, CodexImageError> {
    let home = gateway.select_image_canary_home().await?;
    gateway.preflight_image_home(&home).await?;
    Ok(home)
}

async fn completed_image(
    admission: OpenAiImageAdmission,
    model: &str,
    result: CodexImageResult,
    operation: OpenAiImageOperation,
) -> Response {
    if !valid_result_controls(&result) {
        elog::error("codex-image", "image output contract violation: unsupported controls");
        admission.retain_full_hold();
        return executed_error(
            StatusCode::BAD_GATEWAY,
            "The image provider returned unsupported output controls.",
            "image_output_contract",
        );
    }
    let usage = match result.usage().and_then(|value| parse_usage(value).ok()) {
        Some(usage)
            if !matches!(operation, OpenAiImageOperation::Edit { .. })
                || usage.metered.total_image_input_tokens > 0 =>
        {
            usage
        }
        _ => {
            elog::error("codex-image", "image terminal usage missing or unparseable");
            admission.retain_full_hold();
            return executed_error(
                StatusCode::BAD_GATEWAY,
                "The image provider returned incomplete terminal usage.",
                "image_usage_contract",
            );
        }
    };
    let request_id = admission
        .request_id()
        .map(str::to_owned)
        .unwrap_or_else(|| new_id("img"));
    let body = json!({
        "created": result.created(),
        "data": [{"b64_json": STANDARD_BASE64.encode(result.png())}],
        "usage": public_usage(usage)
    });
    admission.settle(model, &usage.metered);
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn validate_model(value: &str) -> Result<String, ApiError> {
    let model = value.strip_prefix("openai/").unwrap_or(value);
    metering::openai_image_tariff(model)
        .map(|_| model.to_owned())
        .map_err(|_| {
            ApiError::not_found(
                format!("The model '{value}' does not exist."),
                "model".to_owned(),
            )
        })
}

fn validate_controls(
    n: Option<u64>,
    background: Option<&str>,
    quality: Option<&str>,
    size: Option<&str>,
    output_format: Option<&str>,
    response_format: Option<&str>,
) -> Result<(), ApiError> {
    if n.unwrap_or(1) != 1 {
        return Err(ApiError::invalid(
            "Only n=1 is supported by this image pool.",
            "n".to_owned(),
        ));
    }
    for (actual, expected, field) in [
        (background, "opaque", "background"),
        (quality, "low", "quality"),
        (size, "auto", "size"),
        (output_format, "png", "output_format"),
        (response_format, "b64_json", "response_format"),
    ] {
        if actual.is_some_and(|value| value != expected) {
            return Err(ApiError::invalid(
                format!(
                    "Only {field}={expected} is supported by the verified image pool contract."
                ),
                field.to_owned(),
            ));
        }
    }
    Ok(())
}

async fn parse_edit_form(multipart: &mut Multipart) -> Result<EditForm, ApiError> {
    let mut form = EditForm::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::invalid("Invalid multipart image edit body.", None::<String>))?
    {
        let name = field.name().map(str::to_owned).ok_or_else(|| {
            ApiError::invalid("Multipart fields must have names.", None::<String>)
        })?;
        match name.as_str() {
            "image" => {
                if form.images.len() >= 5 {
                    return Err(ApiError::invalid(
                        "At most five image fields are supported.",
                        "image".to_owned(),
                    ));
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| ApiError::invalid("Invalid image field.", "image".to_owned()))?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err(ApiError::invalid(
                        "The PNG image exceeds 16 MiB.",
                        "image".to_owned(),
                    ));
                }
                form.images.push(bytes.to_vec());
            }
            "model" | "prompt" | "n" | "background" | "quality" | "size" | "output_format"
            | "response_format" => {
                let bytes = field.bytes().await.map_err(|_| {
                    ApiError::invalid("Invalid multipart text field.", name.clone())
                })?;
                if bytes.len() > MAX_TEXT_FIELD_BYTES {
                    return Err(ApiError::invalid(
                        format!("Field {name} is too large."),
                        name,
                    ));
                }
                let value = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    ApiError::invalid("Multipart text fields must be UTF-8.", name.clone())
                })?;
                insert_text_field(&mut form, &name, value)?;
            }
            "mask" | "image[]" => {
                return Err(ApiError::invalid(
                    format!("Field {name} is not supported by the verified image pool contract."),
                    name,
                ));
            }
            _ => {
                return Err(ApiError::invalid(
                    format!("Unsupported image edit field: {name}."),
                    name,
                ));
            }
        }
    }
    Ok(form)
}

fn insert_text_field(form: &mut EditForm, name: &str, value: String) -> Result<(), ApiError> {
    let target = match name {
        "model" => &mut form.model,
        "prompt" => &mut form.prompt,
        "n" => &mut form.n,
        "background" => &mut form.background,
        "quality" => &mut form.quality,
        "size" => &mut form.size,
        "output_format" => &mut form.output_format,
        "response_format" => &mut form.response_format,
        _ => unreachable!("field name validated by caller"),
    };
    if target.replace(value).is_some() {
        return Err(ApiError::invalid(
            format!("Duplicate field: {name}."),
            name.to_owned(),
        ));
    }
    Ok(())
}

fn parse_optional_n(value: Option<&str>) -> Result<Option<u64>, ApiError> {
    value
        .map(|value| value.parse::<u64>().map(Some))
        .unwrap_or(Ok(None))
        .map_err(|_| ApiError::invalid("Field n must be a positive integer.", "n".to_owned()))
}

fn valid_result_controls(result: &CodexImageResult) -> bool {
    let dimensions = format!("{}x{}", result.width(), result.height());
    result.background() == "opaque"
        && result.quality() == "low"
        && (result.size() == "auto" || result.size() == dimensions)
        && result.output_format().is_none_or(|format| format == "png")
}

fn parse_usage(value: &Value) -> Result<ParsedImageUsage, ()> {
    let object = value.as_object().ok_or(())?;
    let input_tokens = number(object.get("input_tokens"))?;
    let output_tokens = number(object.get("output_tokens"))?;
    let total_tokens = number(object.get("total_tokens"))?;
    let input = object
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .ok_or(())?;
    let output = object
        .get("output_tokens_details")
        .and_then(Value::as_object)
        .ok_or(())?;
    let text_tokens = number(input.get("text_tokens"))?;
    let image_input_tokens = number(input.get("image_tokens"))?;
    let image_output_tokens = number(output.get("image_tokens"))?;
    let cached = optional_number(input.get("cached_tokens"))?.unwrap_or(0);
    if cached != 0
        || text_tokens.checked_add(image_input_tokens) != Some(input_tokens)
        || image_output_tokens != output_tokens
        || input_tokens.checked_add(output_tokens) != Some(total_tokens)
        || output_tokens == 0
    {
        return Err(());
    }
    Ok(ParsedImageUsage {
        metered: metering::OpenAiImageUsage {
            total_text_input_tokens: text_tokens,
            cached_text_input_tokens: 0,
            total_image_input_tokens: image_input_tokens,
            cached_image_input_tokens: 0,
            image_output_tokens,
        },
        input_tokens,
        output_tokens,
        total_tokens,
    })
}

fn number(value: Option<&Value>) -> Result<u64, ()> {
    value.and_then(Value::as_u64).ok_or(())
}

fn optional_number(value: Option<&Value>) -> Result<Option<u64>, ()> {
    value.map(|value| value.as_u64().ok_or(())).transpose()
}

fn public_usage(usage: ParsedImageUsage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "input_tokens_details": {
            "text_tokens": usage.metered.total_text_input_tokens,
            "image_tokens": usage.metered.total_image_input_tokens
        },
        "output_tokens": usage.output_tokens,
        "output_tokens_details": {"image_tokens": usage.metered.image_output_tokens},
        "total_tokens": usage.total_tokens
    })
}

fn validation_error(error: CodexImageError) -> ApiError {
    ApiError::invalid(error.to_string(), None::<String>)
}

fn transport_error(error: CodexImageError, admission: Option<OpenAiImageAdmission>) -> Response {
    if admission.is_none() {
        return match error {
            CodexImageError::Validation(_) => validation_error(error).into_response(),
            other => {
                elog::warn("codex-image", format!("codex image preflight failed: {other}"));
                ApiError::from(AdmissionError::Unavailable).into_response()
            }
        };
    }
    if let Some(admission) = admission {
        admission.retain_full_hold();
    }
    let (status, message, reason) = match error {
        CodexImageError::BadRequest(_) => (
            StatusCode::BAD_REQUEST,
            "The image provider rejected the request.",
            "image_upstream_rejected",
        ),
        CodexImageError::UsageLimit(_) => (
            StatusCode::TOO_MANY_REQUESTS,
            "The image pool is temporarily rate limited.",
            "image_upstream_usage_limit",
        ),
        other => {
            elog::error(
                "codex-image",
                format!("image upstream outcome ambiguous: {other}"),
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "The image request outcome could not be finalized safely.",
                "image_upstream_ambiguous",
            )
        }
    };
    executed_error(status, message, reason)
}

fn executed_error(status: StatusCode, message: &str, reason: &'static str) -> Response {
    let body = json!({
        "error": {
            "message": message,
            "type": if status == StatusCode::TOO_MANY_REQUESTS { "rate_limit_error" } else { "server_error" },
            "param": null,
            "code": if status == StatusCode::TOO_MANY_REQUESTS { "rate_limit_exceeded" } else { "service_unavailable" }
        }
    });
    let mut response = (status, axum::Json(body)).into_response();
    response
        .extensions_mut()
        .insert(TerminalErrorReason(reason));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_are_exact_and_fail_closed() {
        assert!(validate_controls(
            Some(1),
            Some("opaque"),
            Some("low"),
            Some("auto"),
            Some("png"),
            Some("b64_json")
        )
        .is_ok());
        assert!(validate_controls(Some(2), None, None, None, None, None).is_err());
        assert!(validate_controls(None, Some("transparent"), None, None, None, None).is_err());
        assert!(validate_controls(None, None, Some("high"), None, None, None).is_err());
        assert!(validate_controls(None, None, None, Some("1024x1024"), None, None).is_err());
        assert!(validate_controls(None, None, None, None, Some("webp"), None).is_err());
    }

    #[test]
    fn usage_requires_exact_modality_sums_and_rejects_opaque_cache() {
        let usage = json!({
            "input_tokens": 17,
            "input_tokens_details": {"text_tokens": 7, "image_tokens": 10},
            "output_tokens": 20,
            "output_tokens_details": {"image_tokens": 20},
            "total_tokens": 37
        });
        let parsed = parse_usage(&usage).unwrap();
        assert_eq!(parsed.metered.total_text_input_tokens, 7);
        assert_eq!(parsed.metered.total_image_input_tokens, 10);
        assert_eq!(parsed.metered.image_output_tokens, 20);

        let mut cached = usage.clone();
        cached["input_tokens_details"]["cached_tokens"] = Value::from(1);
        assert!(parse_usage(&cached).is_err());
        let mut mismatch = usage;
        mismatch["total_tokens"] = Value::from(38);
        assert!(parse_usage(&mismatch).is_err());
    }

    #[test]
    fn only_reviewed_aliases_are_accepted() {
        assert_eq!(validate_model("gpt-image-2").unwrap(), "gpt-image-2");
        assert_eq!(validate_model("openai/gpt-image-2").unwrap(), "gpt-image-2");
        assert_eq!(
            validate_model("gpt-image-2-2026-04-21").unwrap(),
            "gpt-image-2-2026-04-21"
        );
        assert!(validate_model("gpt-image-1").is_err());
    }
}
