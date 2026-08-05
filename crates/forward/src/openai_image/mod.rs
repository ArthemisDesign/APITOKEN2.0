//! Strict, dormant OpenAI Images transport for the reviewed GPT Image 2 wire contract.
//!
//! The module owns request validation, serialized single-attempt upstream dispatch, bounded response
//! collection, full bounded PNG decoding, strict terminal usage reconciliation, and privacy-safe
//! evidence. It is deliberately not wired to routes or runtime configuration here.

use std::fmt;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as STANDARD_BASE64;
use base64::Engine as _;
use futures_util::StreamExt;
use metering::{
    openai_image_cost_nanodollars, openai_image_tariff, OpenAiImageUsage, GPT_IMAGE_2_ALIAS,
    GPT_IMAGE_2_SNAPSHOT,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio::time::Instant;

const PRODUCTION_BASE_URL: &str = "https://api.apiyi.com";
const GENERATIONS_PATH: &str = "/v1/images/generations";
const EDITS_PATH: &str = "/v1/images/edits";
const DEFAULT_MAX_RESPONSE_BODY: usize = 16 * 1024 * 1024;
const MAX_RESPONSE_BODY: usize = 16 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 128 * 1024;
const MAX_PROMPT_CHARS: usize = 32_000;
const MAX_REFERENCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_MASK_BYTES_EXCLUSIVE: usize = 4 * 1024 * 1024;
const MAX_EDIT_MEDIA_BYTES: usize = 16 * 1024 * 1024;
const MAX_REQUEST_ID_CHARS: usize = 128;
const MAX_DECODED_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_PNG_DECODE_BYTES: usize = 12 * 1024 * 1024;
// Fixed plausibility window: Unix timestamps from 2020-01-01 through 2100-01-01, inclusive.
const MIN_CREATED_UNIX_SECONDS: u64 = 1_577_836_800;
const MAX_CREATED_UNIX_SECONDS: u64 = 4_102_444_800;

/// Configuration for the ApiYi-hosted OpenAI Images transport.
#[derive(Clone)]
pub struct ApiYiImageConfig {
    base_url: String,
    api_key: Arc<str>,
    connect_timeout: Duration,
    turn_timeout: Duration,
    max_response_body: usize,
}

impl ApiYiImageConfig {
    pub fn production(
        api_key: impl Into<String>,
        connect_timeout: Duration,
        turn_timeout: Duration,
        max_response_body: usize,
    ) -> Result<Self, ImageTransportError> {
        Self::validated(
            PRODUCTION_BASE_URL,
            api_key.into(),
            connect_timeout,
            turn_timeout,
            max_response_body,
            false,
        )
    }

    /// Build production configuration with the fixed 16 MiB response ceiling.
    pub fn production_default(
        api_key: impl Into<String>,
        connect_timeout: Duration,
        turn_timeout: Duration,
    ) -> Result<Self, ImageTransportError> {
        Self::production(
            api_key,
            connect_timeout,
            turn_timeout,
            DEFAULT_MAX_RESPONSE_BODY,
        )
    }

    fn validated(
        base_url: &str,
        api_key: String,
        connect_timeout: Duration,
        turn_timeout: Duration,
        max_response_body: usize,
        allow_loopback: bool,
    ) -> Result<Self, ImageTransportError> {
        validate_api_key(&api_key)?;
        if connect_timeout.is_zero() || connect_timeout > Duration::from_secs(30) {
            return Err(validation("connect timeout must be within 1ns..=30s"));
        }
        if turn_timeout.is_zero() || turn_timeout > Duration::from_secs(600) {
            return Err(validation("turn timeout must be within 1ns..=600s"));
        }
        if max_response_body == 0 || max_response_body > MAX_RESPONSE_BODY {
            return Err(validation("response body limit must be within 1..=16 MiB"));
        }
        if allow_loopback {
            if !is_literal_loopback_origin(base_url) {
                return Err(validation(
                    "test origin must be a literal loopback HTTP origin",
                ));
            }
        } else if base_url != PRODUCTION_BASE_URL {
            return Err(validation(
                "production origin is not the pinned ApiYi origin",
            ));
        }
        Ok(Self {
            base_url: base_url.to_owned(),
            api_key: Arc::from(api_key),
            connect_timeout,
            turn_timeout,
            max_response_body,
        })
    }

    #[cfg(test)]
    fn loopback(
        base_url: &str,
        api_key: impl Into<String>,
        connect_timeout: Duration,
        turn_timeout: Duration,
        max_response_body: usize,
    ) -> Result<Self, ImageTransportError> {
        Self::validated(
            base_url,
            api_key.into(),
            connect_timeout,
            turn_timeout,
            max_response_body,
            true,
        )
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub fn turn_timeout(&self) -> Duration {
        self.turn_timeout
    }

    pub fn max_response_body(&self) -> usize {
        self.max_response_body
    }
}

impl fmt::Debug for ApiYiImageConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiYiImageConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"REDACTED")
            .field("connect_timeout", &self.connect_timeout)
            .field("turn_timeout", &self.turn_timeout)
            .field("max_response_body", &self.max_response_body)
            .finish()
    }
}

/// A validated generation request. Unsupported controls cannot be represented.
#[derive(Clone)]
pub struct GenerationRequest {
    model: String,
    prompt: String,
}

impl GenerationRequest {
    pub fn new(
        model: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Result<Self, ImageTransportError> {
        let model = model.into();
        let prompt = prompt.into();
        validate_model(&model)?;
        validate_prompt(&prompt)?;
        Ok(Self { model, prompt })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }
}

impl fmt::Debug for GenerationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerationRequest")
            .field("model", &self.model)
            .field("prompt", &"REDACTED")
            .field("prompt_chars", &self.prompt.chars().count())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PngMetadata {
    width: u32,
    height: u32,
    color_type: png::ColorType,
}

/// One fully decoded and validated PNG edit reference.
#[derive(Clone)]
pub struct ReferenceImage {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

impl ReferenceImage {
    pub fn new(bytes: Vec<u8>) -> Result<Self, ImageTransportError> {
        let metadata = decode_png_metadata(&bytes).map_err(validation)?;
        Ok(Self {
            bytes,
            width: metadata.width,
            height: metadata.height,
        })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl fmt::Debug for ReferenceImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReferenceImage")
            .field("bytes", &"REDACTED")
            .field("len", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// Optional fully decoded PNG edit mask with an explicit alpha channel.
#[derive(Clone)]
pub struct ImageMask {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

impl ImageMask {
    pub fn new(bytes: Vec<u8>) -> Result<Self, ImageTransportError> {
        if bytes.len() >= MAX_MASK_BYTES_EXCLUSIVE {
            return Err(validation("mask PNG must be smaller than 4 MiB"));
        }
        let metadata = decode_png_metadata(&bytes).map_err(validation)?;
        if !matches!(
            metadata.color_type,
            png::ColorType::Rgba | png::ColorType::GrayscaleAlpha
        ) {
            return Err(validation("mask PNG must have an explicit alpha channel"));
        }
        Ok(Self {
            bytes,
            width: metadata.width,
            height: metadata.height,
        })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl fmt::Debug for ImageMask {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageMask")
            .field("bytes", &"REDACTED")
            .field("len", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// A validated edit request. It always contains 1..=16 PNG references.
#[derive(Clone)]
pub struct EditRequest {
    model: String,
    prompt: String,
    references: Vec<ReferenceImage>,
    mask: Option<ImageMask>,
}

impl EditRequest {
    pub fn new(
        model: impl Into<String>,
        prompt: impl Into<String>,
        references: Vec<ReferenceImage>,
        mask: Option<ImageMask>,
    ) -> Result<Self, ImageTransportError> {
        let model = model.into();
        let prompt = prompt.into();
        validate_model(&model)?;
        validate_prompt(&prompt)?;
        validate_edit_media(&references, mask.as_ref())?;
        Ok(Self {
            model,
            prompt,
            references,
            mask,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn references(&self) -> &[ReferenceImage] {
        &self.references
    }

    pub fn mask(&self) -> Option<&ImageMask> {
        self.mask.as_ref()
    }
}

impl fmt::Debug for EditRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let media_bytes = self
            .references
            .iter()
            .map(ReferenceImage::len)
            .sum::<usize>()
            + self.mask.as_ref().map_or(0, ImageMask::len);
        formatter
            .debug_struct("EditRequest")
            .field("model", &self.model)
            .field("prompt", &"REDACTED")
            .field("prompt_chars", &self.prompt.chars().count())
            .field("reference_count", &self.references.len())
            .field("has_mask", &self.mask.is_some())
            .field("media_bytes", &media_bytes)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheAccounting {
    NotReportedChargedFresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageUsageEvidence {
    input_tokens: u64,
    text_input_tokens: u64,
    image_input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    output_details: ImageOutputTokenDetails,
    metered: OpenAiImageUsage,
    cache_accounting: CacheAccounting,
}

impl ImageUsageEvidence {
    pub fn input_tokens(&self) -> u64 {
        self.input_tokens
    }
    pub fn text_input_tokens(&self) -> u64 {
        self.text_input_tokens
    }
    pub fn image_input_tokens(&self) -> u64 {
        self.image_input_tokens
    }
    pub fn output_tokens(&self) -> u64 {
        self.output_tokens
    }
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }
    pub fn output_details(&self) -> ImageOutputTokenDetails {
        self.output_details
    }
    pub fn metered(&self) -> OpenAiImageUsage {
        self.metered
    }
    pub fn cache_accounting(&self) -> CacheAccounting {
        self.cache_accounting
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageOutputTokenDetails {
    text_tokens: u64,
    image_tokens: u64,
}

impl ImageOutputTokenDetails {
    pub fn text_tokens(&self) -> u64 {
        self.text_tokens
    }
    pub fn image_tokens(&self) -> u64 {
        self.image_tokens
    }
}

pub struct ImageResult {
    image: Vec<u8>,
    usage: ImageUsageEvidence,
    requested_model_id: String,
    canonical_model_id: &'static str,
    tariff_schedule_id: String,
    schedule_effective_from: i64,
    cost_nanodollars: i128,
    image_sha256: String,
    request_id: Option<String>,
}

impl ImageResult {
    pub fn image(&self) -> &[u8] {
        &self.image
    }
    pub fn into_image(self) -> Vec<u8> {
        self.image
    }
    pub fn usage(&self) -> &ImageUsageEvidence {
        &self.usage
    }
    pub fn requested_model_id(&self) -> &str {
        &self.requested_model_id
    }
    pub fn canonical_model_id(&self) -> &str {
        self.canonical_model_id
    }
    pub fn tariff_schedule_id(&self) -> &str {
        &self.tariff_schedule_id
    }
    pub fn schedule_effective_from(&self) -> i64 {
        self.schedule_effective_from
    }
    pub fn cost_nanodollars(&self) -> i128 {
        self.cost_nanodollars
    }
    pub fn image_sha256(&self) -> &str {
        &self.image_sha256
    }
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

impl fmt::Debug for ImageResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageResult")
            .field("image", &"REDACTED")
            .field("image_len", &self.image.len())
            .field("usage", &self.usage)
            .field("requested_model_id", &self.requested_model_id)
            .field("canonical_model_id", &self.canonical_model_id)
            .field("tariff_schedule_id", &self.tariff_schedule_id)
            .field("schedule_effective_from", &self.schedule_effective_from)
            .field("cost_nanodollars", &self.cost_nanodollars)
            .field("image_sha256", &self.image_sha256)
            .field("request_id", &self.request_id)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    message: &'static str,
}

impl ValidationError {
    pub fn message(&self) -> &'static str {
        self.message
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseContext {
    status: u16,
    request_id: Option<String>,
}

impl ResponseContext {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidResponseError {
    message: &'static str,
    context: ResponseContext,
}

impl InvalidResponseError {
    pub fn message(&self) -> &'static str {
        self.message
    }

    pub fn status(&self) -> u16 {
        self.context.status
    }

    pub fn request_id(&self) -> Option<&str> {
        self.context.request_id.as_deref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamErrorCode {
    ModerationBlocked,
    InvalidImageFile,
    InvalidValue,
    RateLimitExceeded,
    InsufficientBalance,
    ServerError,
    Unknown,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UpstreamError {
    context: ResponseContext,
    code: UpstreamErrorCode,
}

impl UpstreamError {
    pub fn context(&self) -> &ResponseContext {
        &self.context
    }

    pub fn status(&self) -> u16 {
        self.context.status
    }

    pub fn request_id(&self) -> Option<&str> {
        self.context.request_id.as_deref()
    }

    pub fn code(&self) -> UpstreamErrorCode {
        self.code
    }
}

impl fmt::Debug for UpstreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpstreamError")
            .field("context", &self.context)
            .field("code", &self.code)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ImageTransportError {
    Validation(ValidationError),
    OutcomeUnknown(Option<ResponseContext>),
    Timeout(Option<ResponseContext>),
    InvalidResponse(InvalidResponseError),
    Upstream(UpstreamError),
    UnparseableUpstream(ResponseContext),
}

impl ImageTransportError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::OutcomeUnknown(Some(context))
            | Self::Timeout(Some(context))
            | Self::UnparseableUpstream(context) => Some(context.status),
            Self::InvalidResponse(error) => Some(error.status()),
            Self::Upstream(error) => Some(error.status()),
            Self::Validation(_) | Self::OutcomeUnknown(None) | Self::Timeout(None) => None,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::OutcomeUnknown(Some(context))
            | Self::Timeout(Some(context))
            | Self::UnparseableUpstream(context) => context.request_id(),
            Self::InvalidResponse(error) => error.request_id(),
            Self::Upstream(error) => error.request_id(),
            Self::Validation(_) | Self::OutcomeUnknown(None) | Self::Timeout(None) => None,
        }
    }
}

impl fmt::Debug for ImageTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => formatter.debug_tuple("Validation").field(error).finish(),
            Self::OutcomeUnknown(context) => formatter
                .debug_tuple("OutcomeUnknown")
                .field(context)
                .finish(),
            Self::Timeout(context) => formatter.debug_tuple("Timeout").field(context).finish(),
            Self::InvalidResponse(error) => formatter
                .debug_tuple("InvalidResponse")
                .field(error)
                .finish(),
            Self::Upstream(error) => formatter.debug_tuple("Upstream").field(error).finish(),
            Self::UnparseableUpstream(context) => formatter
                .debug_tuple("UnparseableUpstream")
                .field(context)
                .finish(),
        }
    }
}

impl fmt::Display for ImageTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => {
                write!(
                    formatter,
                    "image request validation failed: {}",
                    error.message
                )
            }
            Self::OutcomeUnknown(_) => formatter.write_str("image upstream outcome is unknown"),
            Self::Timeout(_) => formatter.write_str("image upstream turn timed out"),
            Self::InvalidResponse(_) => {
                formatter.write_str("image upstream returned an invalid response")
            }
            Self::Upstream(error) => write!(
                formatter,
                "image upstream returned a sanitized HTTP {} error",
                error.status()
            ),
            Self::UnparseableUpstream(context) => write!(
                formatter,
                "image upstream returned an unparseable HTTP {} error",
                context.status()
            ),
        }
    }
}

impl std::error::Error for ImageTransportError {}

/// Strict serialized single-attempt gateway.
pub struct OpenAiImageGateway {
    config: ApiYiImageConfig,
    client: wreq::Client,
    single_turn: Semaphore,
}

impl fmt::Debug for OpenAiImageGateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAiImageGateway")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl OpenAiImageGateway {
    pub fn new(config: ApiYiImageConfig) -> Result<Self, ImageTransportError> {
        let client = wreq::Client::builder()
            .no_proxy()
            .redirect(wreq::redirect::Policy::none())
            .retry(wreq::retry::Policy::never())
            .connect_timeout(config.connect_timeout)
            .build()
            .map_err(|_| validation("failed to build image HTTP client"))?;
        Ok(Self {
            config,
            client,
            single_turn: Semaphore::new(1),
        })
    }

    pub async fn generate(
        &self,
        request: &GenerationRequest,
    ) -> Result<ImageResult, ImageTransportError> {
        let deadline = Instant::now() + self.config.turn_timeout;
        let _permit = tokio::time::timeout_at(deadline, self.single_turn.acquire())
            .await
            .map_err(|_| ImageTransportError::Timeout(None))?
            .map_err(|_| ImageTransportError::OutcomeUnknown(None))?;
        ensure_deadline(deadline, None)?;
        let validation_result =
            validate_model(&request.model).and_then(|()| validate_prompt(&request.prompt));
        ensure_deadline(deadline, None)?;
        validation_result?;
        let wire = GenerationWire {
            model: &request.model,
            prompt: &request.prompt,
            quality: "low",
            size: "1024x1024",
            n: 1,
            output_format: "png",
            background: "opaque",
            moderation: "auto",
            stream: false,
        };
        ensure_deadline(deadline, None)?;
        let body_result = serde_json::to_vec(&wire)
            .map_err(|_| validation("failed to encode generation request"));
        ensure_deadline(deadline, None)?;
        let body = body_result?;
        self.dispatch(
            GENERATIONS_PATH,
            &request.model,
            "application/json",
            body,
            deadline,
        )
        .await
    }

    pub async fn edit(&self, request: &EditRequest) -> Result<ImageResult, ImageTransportError> {
        let deadline = Instant::now() + self.config.turn_timeout;
        let _permit = tokio::time::timeout_at(deadline, self.single_turn.acquire())
            .await
            .map_err(|_| ImageTransportError::Timeout(None))?
            .map_err(|_| ImageTransportError::OutcomeUnknown(None))?;
        ensure_deadline(deadline, None)?;
        let validation_result = validate_edit_request(request);
        ensure_deadline(deadline, None)?;
        validation_result?;
        let boundary_result = multipart_boundary();
        ensure_deadline(deadline, None)?;
        let boundary = boundary_result?;
        ensure_deadline(deadline, None)?;
        let body_result = build_edit_multipart(request, &boundary);
        ensure_deadline(deadline, None)?;
        let body = body_result?;
        let content_type = format!("multipart/form-data; boundary={boundary}");
        self.dispatch(EDITS_PATH, &request.model, &content_type, body, deadline)
            .await
    }

    async fn dispatch(
        &self,
        path: &str,
        requested_model: &str,
        content_type: &str,
        body: Vec<u8>,
        deadline: Instant,
    ) -> Result<ImageResult, ImageTransportError> {
        ensure_deadline(deadline, None)?;
        let url = format!("{}{path}", self.config.base_url);
        let authorization = format!("Bearer {}", self.config.api_key);
        // This is intentionally the only `.send()` in the transport.
        let send = self
            .client
            .post(url)
            .header("authorization", authorization)
            .header("content-type", content_type)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .body(body)
            .send();
        let response = tokio::time::timeout_at(deadline, send)
            .await
            .map_err(|_| ImageTransportError::OutcomeUnknown(None))?
            .map_err(|_| ImageTransportError::OutcomeUnknown(None))?;

        let status = response.status().as_u16();
        let context = ResponseContext {
            status,
            request_id: sanitized_request_id(response.headers(), self.config.api_key.as_ref()),
        };
        ensure_deadline(deadline, Some(&context))?;
        inspect_content_length(response.headers(), self.config.max_response_body).map_err(
            |_| {
                classify_unparseable_or_invalid(
                    status,
                    context.clone(),
                    "invalid response Content-Length",
                )
            },
        )?;
        let bytes = tokio::time::timeout_at(
            deadline,
            read_bounded(response, self.config.max_response_body),
        )
        .await
        .map_err(|_| ImageTransportError::OutcomeUnknown(Some(context.clone())))?
        .map_err(|error| match error {
            ReadBoundedError::TooLarge => classify_unparseable_or_invalid(
                status,
                context.clone(),
                "response body exceeds the configured limit",
            ),
            ReadBoundedError::Network => ImageTransportError::OutcomeUnknown(Some(context.clone())),
        })?;
        ensure_post_dispatch_deadline(deadline, &context)?;
        if !(200..300).contains(&status) {
            return parse_upstream_error(&bytes, context, deadline);
        }
        parse_success(requested_model, &bytes, context, deadline)
    }
}

#[derive(Serialize)]
struct GenerationWire<'a> {
    model: &'a str,
    prompt: &'a str,
    quality: &'static str,
    size: &'static str,
    n: u8,
    output_format: &'static str,
    background: &'static str,
    moderation: &'static str,
    stream: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessEnvelope {
    data: Vec<SuccessData>,
    usage: ProviderUsage,
    created: u64,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    background: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    output_format: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    quality: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    size: Option<String>,
}

fn deserialize_optional_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessData {
    b64_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderUsage {
    input_tokens: u64,
    input_tokens_details: ProviderInputDetails,
    output_tokens: u64,
    total_tokens: u64,
    output_tokens_details: ProviderOutputDetails,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderInputDetails {
    text_tokens: u64,
    image_tokens: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderOutputDetails {
    text_tokens: u64,
    image_tokens: u64,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorObject,
}

#[derive(Deserialize)]
struct ErrorObject {
    #[serde(default)]
    code: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadBoundedError {
    TooLarge,
    Network,
}

async fn read_bounded(response: wreq::Response, limit: usize) -> Result<Vec<u8>, ReadBoundedError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ReadBoundedError::Network)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ReadBoundedError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn inspect_content_length(headers: &wreq::header::HeaderMap, limit: usize) -> Result<(), ()> {
    let Some(value) = headers.get("content-length") else {
        return Ok(());
    };
    let length = value
        .to_str()
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(())?;
    if length > limit as u64 {
        return Err(());
    }
    Ok(())
}

fn parse_success(
    requested_model: &str,
    body: &[u8],
    context: ResponseContext,
    deadline: Instant,
) -> Result<ImageResult, ImageTransportError> {
    ensure_deadline(deadline, Some(&context))?;
    let envelope_result: Result<SuccessEnvelope, _> = serde_json::from_slice(body);
    ensure_deadline(deadline, Some(&context))?;
    let envelope = envelope_result
        .map_err(|_| invalid_response("success body is not the expected JSON schema", &context))?;
    validate_success_metadata(&envelope, &context)?;
    if envelope.data.len() != 1 {
        return Err(invalid_response(
            "success must contain exactly one data item",
            &context,
        ));
    }
    let encoded = envelope.data[0].b64_json.as_bytes();
    ensure_deadline(deadline, Some(&context))?;
    let decoded_len = canonical_base64_decoded_len(encoded)
        .ok_or_else(|| invalid_response("b64_json is not canonical standard base64", &context))?;
    if decoded_len > MAX_DECODED_IMAGE_BYTES {
        return Err(invalid_response(
            "decoded image must not exceed 12 MiB",
            &context,
        ));
    }
    ensure_deadline(deadline, Some(&context))?;
    let image_result = STANDARD_BASE64.decode(encoded);
    ensure_deadline(deadline, Some(&context))?;
    let image = image_result
        .map_err(|_| invalid_response("b64_json is not canonical standard base64", &context))?;
    validate_output_png(&image, &context, deadline)?;
    let usage = reconcile_usage(envelope.usage, &context)?;
    ensure_deadline(deadline, Some(&context))?;
    let tariff = openai_image_tariff(requested_model)
        .map_err(|_| invalid_response("requested model has no pinned image tariff", &context))?;
    let cost_nanodollars = openai_image_cost_nanodollars(&usage.metered, &tariff.prices)
        .map_err(|_| invalid_response("image metering arithmetic failed", &context))?;
    let image_sha256 = format!("{:x}", Sha256::digest(&image));
    ensure_deadline(deadline, Some(&context))?;
    Ok(ImageResult {
        image,
        usage,
        requested_model_id: requested_model.to_owned(),
        canonical_model_id: tariff.canonical_model_id,
        tariff_schedule_id: tariff.tariff_schedule_id.as_str().to_owned(),
        schedule_effective_from: tariff.schedule_effective_from,
        cost_nanodollars,
        image_sha256,
        request_id: context.request_id,
    })
}

fn canonical_base64_decoded_len(encoded: &[u8]) -> Option<usize> {
    if encoded.is_empty() || encoded.len() % 4 != 0 {
        return None;
    }
    let padding = if encoded.ends_with(b"==") {
        2
    } else if encoded.ends_with(b"=") {
        1
    } else {
        0
    };
    let data_len = encoded.len().checked_sub(padding)?;
    if encoded[..data_len]
        .iter()
        .any(|byte| base64_sextet(*byte).is_none())
        || encoded[data_len..].iter().any(|byte| *byte != b'=')
        || encoded[..data_len].contains(&b'=')
    {
        return None;
    }
    match padding {
        0 => {}
        1 => {
            if data_len % 4 != 3 || base64_sextet(encoded[data_len - 1])? & 0b11 != 0 {
                return None;
            }
        }
        2 => {
            if data_len % 4 != 2 || base64_sextet(encoded[data_len - 1])? & 0b1111 != 0 {
                return None;
            }
        }
        _ => return None,
    }
    encoded
        .len()
        .checked_div(4)?
        .checked_mul(3)?
        .checked_sub(padding)
}

fn base64_sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn validate_success_metadata(
    envelope: &SuccessEnvelope,
    context: &ResponseContext,
) -> Result<(), ImageTransportError> {
    if !(MIN_CREATED_UNIX_SECONDS..=MAX_CREATED_UNIX_SECONDS).contains(&envelope.created) {
        return Err(invalid_response(
            "success created timestamp is implausible",
            context,
        ));
    }
    for (actual, expected, message) in [
        (
            envelope.background.as_deref(),
            "opaque",
            "success background is not opaque",
        ),
        (
            envelope.output_format.as_deref(),
            "png",
            "success output_format is not png",
        ),
        (
            envelope.quality.as_deref(),
            "low",
            "success quality is not low",
        ),
        (
            envelope.size.as_deref(),
            "1024x1024",
            "success size is not 1024x1024",
        ),
    ] {
        if actual.is_some_and(|actual| actual != expected) {
            return Err(invalid_response(message, context));
        }
    }
    Ok(())
}

fn reconcile_usage(
    usage: ProviderUsage,
    context: &ResponseContext,
) -> Result<ImageUsageEvidence, ImageTransportError> {
    let detail_input = usage
        .input_tokens_details
        .text_tokens
        .checked_add(usage.input_tokens_details.image_tokens)
        .ok_or_else(|| invalid_response("input token sum overflow", context))?;
    if detail_input != usage.input_tokens {
        return Err(invalid_response(
            "input token details do not reconcile",
            context,
        ));
    }
    let total = usage
        .input_tokens
        .checked_add(usage.output_tokens)
        .ok_or_else(|| invalid_response("total token sum overflow", context))?;
    if total != usage.total_tokens {
        return Err(invalid_response("total tokens do not reconcile", context));
    }
    let details = usage.output_tokens_details;
    if details.text_tokens != 0 || details.image_tokens != usage.output_tokens {
        return Err(invalid_response(
            "output token details do not reconcile",
            context,
        ));
    }
    let output_details = ImageOutputTokenDetails {
        text_tokens: details.text_tokens,
        image_tokens: details.image_tokens,
    };
    let metered = OpenAiImageUsage {
        total_text_input_tokens: usage.input_tokens_details.text_tokens,
        cached_text_input_tokens: 0,
        total_image_input_tokens: usage.input_tokens_details.image_tokens,
        cached_image_input_tokens: 0,
        image_output_tokens: usage.output_tokens,
    };
    Ok(ImageUsageEvidence {
        input_tokens: usage.input_tokens,
        text_input_tokens: usage.input_tokens_details.text_tokens,
        image_input_tokens: usage.input_tokens_details.image_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        output_details,
        metered,
        cache_accounting: CacheAccounting::NotReportedChargedFresh,
    })
}

fn decode_png_metadata(bytes: &[u8]) -> Result<PngMetadata, &'static str> {
    if bytes.is_empty() {
        return Err("PNG must be nonempty");
    }
    if bytes.len() > MAX_REFERENCE_BYTES {
        return Err("encoded PNG must not exceed 16 MiB");
    }
    let limits = png::Limits {
        bytes: MAX_PNG_DECODE_BYTES,
    };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|_| "invalid PNG")?;
    let info = reader.info();
    if info.animation_control.is_some() || info.frame_control.is_some() {
        return Err("animated PNG is not supported");
    }
    let metadata = PngMetadata {
        width: info.width,
        height: info.height,
        color_type: info.color_type,
    };
    let output_size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAX_PNG_DECODE_BYTES)
        .ok_or("decoded PNG must not exceed 12 MiB")?;
    let mut pixels = vec![0; output_size];
    reader.next_frame(&mut pixels).map_err(|_| "invalid PNG")?;
    reader.finish().map_err(|_| "invalid PNG")?;
    Ok(metadata)
}

fn validate_output_png(
    image: &[u8],
    context: &ResponseContext,
    deadline: Instant,
) -> Result<(), ImageTransportError> {
    ensure_deadline(deadline, Some(context))?;
    let metadata_result = decode_png_metadata(image);
    ensure_deadline(deadline, Some(context))?;
    let metadata = metadata_result.map_err(|message| invalid_response(message, context))?;
    if metadata.width != 1024 || metadata.height != 1024 {
        return Err(invalid_response(
            "decoded PNG dimensions are not 1024x1024",
            context,
        ));
    }
    Ok(())
}

fn parse_upstream_error(
    body: &[u8],
    context: ResponseContext,
    deadline: Instant,
) -> Result<ImageResult, ImageTransportError> {
    ensure_deadline(deadline, Some(&context))?;
    let envelope_result: Result<ErrorEnvelope, _> = serde_json::from_slice(body);
    ensure_deadline(deadline, Some(&context))?;
    let envelope =
        envelope_result.map_err(|_| ImageTransportError::UnparseableUpstream(context.clone()))?;
    let code = envelope
        .error
        .code
        .as_ref()
        .and_then(Value::as_str)
        .map(map_upstream_code)
        .unwrap_or(UpstreamErrorCode::Unknown);
    ensure_deadline(deadline, Some(&context))?;
    Err(ImageTransportError::Upstream(UpstreamError {
        context,
        code,
    }))
}

fn map_upstream_code(code: &str) -> UpstreamErrorCode {
    match code {
        "moderation_blocked" => UpstreamErrorCode::ModerationBlocked,
        "invalid_image_file" => UpstreamErrorCode::InvalidImageFile,
        "invalid_value" => UpstreamErrorCode::InvalidValue,
        "rate_limit_exceeded" => UpstreamErrorCode::RateLimitExceeded,
        "insufficient_balance" => UpstreamErrorCode::InsufficientBalance,
        "server_error" => UpstreamErrorCode::ServerError,
        _ => UpstreamErrorCode::Unknown,
    }
}

fn sanitized_request_id(headers: &wreq::header::HeaderMap, api_key: &str) -> Option<String> {
    let raw = headers
        .get("x-request-id")
        .or_else(|| headers.get("request-id"))?
        .to_str()
        .ok()?;
    if raw.contains(api_key) {
        return None;
    }
    let value: String = raw
        .chars()
        .take(MAX_REQUEST_ID_CHARS)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':') {
                character
            } else {
                '_'
            }
        })
        .collect();
    (!value.is_empty()).then_some(value)
}

fn validate_api_key(api_key: &str) -> Result<(), ImageTransportError> {
    if !(32..=256).contains(&api_key.len())
        || !api_key.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        || api_key.to_ascii_lowercase().starts_with("bearer ")
    {
        return Err(validation(
            "API key must be 32..=256 printable ASCII bytes without an auth prefix",
        ));
    }
    Ok(())
}

fn validate_model(model: &str) -> Result<(), ImageTransportError> {
    if model != GPT_IMAGE_2_ALIAS && model != GPT_IMAGE_2_SNAPSHOT {
        return Err(validation("unsupported image model"));
    }
    Ok(())
}

fn validate_prompt(prompt: &str) -> Result<(), ImageTransportError> {
    if prompt.is_empty()
        || prompt.len() > MAX_PROMPT_BYTES
        || prompt.chars().count() > MAX_PROMPT_CHARS
    {
        return Err(validation(
            "prompt must be nonempty, at most 32000 Unicode characters and at most 128 KiB",
        ));
    }
    Ok(())
}

fn validate_edit_media(
    references: &[ReferenceImage],
    mask: Option<&ImageMask>,
) -> Result<(), ImageTransportError> {
    if references.is_empty() || references.len() > 16 {
        return Err(validation("edit requires 1..=16 reference images"));
    }
    if let Some(mask) = mask {
        let first = &references[0];
        if mask.width != first.width || mask.height != first.height {
            return Err(validation(
                "mask dimensions must equal the first reference dimensions",
            ));
        }
    }
    let total = references
        .iter()
        .try_fold(0usize, |sum, image| sum.checked_add(image.bytes.len()))
        .and_then(|sum| mask.map_or(Some(sum), |mask| sum.checked_add(mask.bytes.len())))
        .ok_or_else(|| validation("edit media size overflow"))?;
    if total > MAX_EDIT_MEDIA_BYTES {
        return Err(validation("aggregate edit media must not exceed 16 MiB"));
    }
    Ok(())
}

fn validate_edit_request(request: &EditRequest) -> Result<(), ImageTransportError> {
    validate_model(&request.model)?;
    validate_prompt(&request.prompt)?;
    validate_edit_media(&request.references, request.mask.as_ref())
}

fn multipart_boundary() -> Result<String, ImageTransportError> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| {
        validation("operating-system randomness unavailable for multipart boundary")
    })?;
    let mut encoded = String::with_capacity(random.len() * 2);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(format!("apitoken-image-{encoded}"))
}

fn build_edit_multipart(
    request: &EditRequest,
    boundary: &str,
) -> Result<Vec<u8>, ImageTransportError> {
    let media_len = request
        .references
        .iter()
        .map(ReferenceImage::len)
        .sum::<usize>()
        + request.mask.as_ref().map_or(0, ImageMask::len);
    let mut body = Vec::with_capacity(media_len.saturating_add(4096));
    for (name, value) in [
        ("model", request.model.as_str()),
        ("prompt", request.prompt.as_str()),
        ("quality", "low"),
        ("size", "1024x1024"),
        ("n", "1"),
        ("output_format", "png"),
        ("background", "opaque"),
        ("moderation", "auto"),
        ("stream", "false"),
    ] {
        append_text_part(&mut body, boundary, name, value);
    }
    for (index, reference) in request.references.iter().enumerate() {
        let filename = format!("image-{:02}.png", index + 1);
        append_file_part(
            &mut body,
            boundary,
            "image[]",
            &filename,
            "image/png",
            &reference.bytes,
        );
    }
    if let Some(mask) = &request.mask {
        append_file_part(
            &mut body,
            boundary,
            "mask",
            "mask.png",
            "image/png",
            &mask.bytes,
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    if body.len() < media_len {
        return Err(validation("multipart size overflow"));
    }
    Ok(body)
}

fn append_text_part(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn append_file_part(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

fn ensure_deadline(
    deadline: Instant,
    context: Option<&ResponseContext>,
) -> Result<(), ImageTransportError> {
    if Instant::now() >= deadline {
        return Err(match context {
            Some(context) => ImageTransportError::OutcomeUnknown(Some(context.clone())),
            None => ImageTransportError::Timeout(None),
        });
    }
    Ok(())
}

fn ensure_post_dispatch_deadline(
    deadline: Instant,
    context: &ResponseContext,
) -> Result<(), ImageTransportError> {
    ensure_deadline(deadline, Some(context))
}

fn classify_unparseable_or_invalid(
    status: u16,
    context: ResponseContext,
    message: &'static str,
) -> ImageTransportError {
    if (200..300).contains(&status) {
        invalid_response(message, &context)
    } else {
        ImageTransportError::UnparseableUpstream(context)
    }
}

fn is_literal_loopback_origin(value: &str) -> bool {
    let Some(authority) = value.strip_prefix("http://") else {
        return false;
    };
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return false;
    }
    let port = if let Some(port) = authority.strip_prefix("127.0.0.1:") {
        port
    } else if let Some(port) = authority.strip_prefix("[::1]:") {
        port
    } else {
        return false;
    };
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

fn validation(message: &'static str) -> ImageTransportError {
    ImageTransportError::Validation(ValidationError { message })
}

fn invalid_response(message: &'static str, context: &ResponseContext) -> ImageTransportError {
    ImageTransportError::InvalidResponse(InvalidResponseError {
        message,
        context: context.clone(),
    })
}

#[cfg(test)]
mod tests;
