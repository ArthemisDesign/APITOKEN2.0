//! Google-shaped, default-off Gemini Batch and Files HTTP producer.

use super::{api, GeminiBatchBlobIdentity, GeminiBatchPublicFacade};
use crate::proxy::{self, Authz};
use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;

const BODY_LIMIT: usize = 20 * 1024 * 1024;
const FILE_CHUNK_PAGE_SIZE: i64 = registry::MAX_BATCH_FILE_CHUNK_PAGE_SIZE;
const JSONL_LINE_LIMIT: usize = BODY_LIMIT;
const MAX_BATCH_ITEMS: usize = 100_000;
const SCHEMA_VERSION: i32 = 1;
const UPLOAD_TTL: i64 = 48 * 60 * 60;
const JOB_TTL: i64 = 48 * 60 * 60;
const RESULT_TTL: i64 = 42 * 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Create(String),
    List,
    Get(String),
    Cancel(String),
    Delete(String),
    Files,
    File(String),
    Download(String),
    Upload,
}
fn id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
pub fn parse(m: &Method, p: &str) -> Option<Route> {
    if let Some(v) = p.strip_prefix("/v1beta/models/") {
        let (a, b) = v.split_once(':')?;
        return (m == Method::POST && b == "batchGenerateContent" && id(a))
            .then(|| Route::Create(a.into()));
    }
    if p == "/v1beta/batches" && m == Method::GET {
        return Some(Route::List);
    }
    if let Some(v) = p.strip_prefix("/v1beta/batches/") {
        if let Some(v) = v.strip_suffix(":cancel") {
            return (m == Method::POST && id(v)).then(|| Route::Cancel(v.into()));
        }
        if id(v) {
            return match *m {
                Method::GET => Some(Route::Get(v.into())),
                Method::DELETE => Some(Route::Delete(v.into())),
                _ => None,
            };
        }
    }
    if p == "/v1beta/files" && matches!(*m, Method::GET | Method::POST) {
        return Some(Route::Files);
    }
    if let Some(v) = p.strip_prefix("/v1beta/files/") {
        if let Some(v) = v.strip_suffix(":download") {
            return (m == Method::GET && id(v)).then(|| Route::Download(v.into()));
        }
        if id(v) {
            return match *m {
                Method::GET => Some(Route::File(v.into())),
                Method::DELETE => Some(Route::File(v.into())),
                _ => None,
            };
        }
    }
    (p == "/upload/v1beta/files" && m == Method::POST).then_some(Route::Upload)
}
pub fn cors() -> Response {
    let mut r = StatusCode::NO_CONTENT.into_response();
    let h = r.headers_mut();
    h.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    h.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
    );
    h.insert("access-control-allow-headers",HeaderValue::from_static("Authorization, Content-Length, Content-Type, Range, X-Goog-Api-Key, Idempotency-Key, X-Apitoken-Idempotency-Key, X-Goog-Upload-Protocol, X-Goog-Upload-Command, X-Goog-Upload-Offset, X-Goog-Upload-File-Name, X-Goog-Upload-Header-Content-Type, X-Goog-Upload-Header-Content-Length"));
    h.insert("access-control-expose-headers",HeaderValue::from_static("X-Goog-Upload-URL, X-Goog-Upload-Status, X-Goog-Upload-Size-Received, Content-Length, Content-Range, Accept-Ranges, ETag"));
    r
}

struct MeteredAuth {
    account: String,
    raw_key: String,
    key_id: String,
    mult_bp: i64,
    available: i64,
}
async fn auth(
    app: &AppState,
    headers: &HeaderMap,
    peer: &SocketAddr,
) -> Result<MeteredAuth, Response> {
    match proxy::authorize(app, headers, peer).await {
        ref metered @ Authz::Metered {
            account_id: ref account_id,
            key: ref key,
            key_id: ref key_id,
            available_nano,
            ..
        } => {
            let mult = metered.mult_for(registry::PROVIDER_GOOGLE);
            Ok(MeteredAuth {
                account: account_id.clone(),
                raw_key: key.clone(),
                key_id: key_id.clone(),
                mult_bp: mult,
                available: available_nano,
            })
        }
        Authz::Unavailable => Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "Billing authority is unavailable.",
        )),
        _ => Err(error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHENTICATED",
            "API key is invalid.",
        )),
    }
}
fn error(code: StatusCode, status: &str, message: impl Into<String>) -> Response {
    let message = message.into();
    (
        code,
        axum::Json(json!({"error":{"code":code.as_u16(),"status":status,"message":message}})),
    )
        .into_response()
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs() as i64)
        .unwrap_or(1)
}
fn fresh(prefix: &str) -> String {
    format!("{prefix}-{}", crate::fresh_request_id())
}
fn ts(v: i64) -> String {
    format!("{v}")
}
fn hex_digest(v: &[u8; 32]) -> String {
    v.iter().map(|b| format!("{b:02x}")).collect()
}
fn digest(v: &[u8]) -> [u8; 32] {
    Sha256::digest(v).into()
}
fn idem(headers: &HeaderMap, account: &str) -> Option<[u8; 32]> {
    headers
        .get("idempotency-key")
        .or_else(|| headers.get("x-apitoken-idempotency-key"))
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .map(|v| digest(format!("gemini-batch-idem\0{account}\0{v}").as_bytes()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UploadCommand {
    Start,
    Query,
    Upload { finalize: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UploadHeaders {
    command: UploadCommand,
    offset: Option<i64>,
    content_length: Option<i64>,
    declared_size: Option<i64>,
    mime_type: Option<String>,
    display_name: Option<String>,
}

fn one_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<Option<&'a str>, &'static str> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err("Upload header must appear exactly once.");
    }
    value
        .to_str()
        .map(Some)
        .map_err(|_| "Upload header is not valid ASCII.")
}

fn parse_nonnegative(value: &str) -> Option<i64> {
    (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        .then(|| value.parse::<i64>().ok())
        .flatten()
}

fn parse_upload_headers(headers: &HeaderMap) -> Result<UploadHeaders, &'static str> {
    if one_header(headers, "x-goog-upload-protocol")? != Some("resumable") {
        return Err("x-goog-upload-protocol must be resumable.");
    }
    let command = match one_header(headers, "x-goog-upload-command")? {
        Some("start") => UploadCommand::Start,
        Some("query") => UploadCommand::Query,
        Some("upload") => UploadCommand::Upload { finalize: false },
        Some("upload, finalize") | Some("upload,finalize") | Some("finalize") => {
            UploadCommand::Upload { finalize: true }
        }
        _ => return Err("x-goog-upload-command is invalid."),
    };
    let number = |name, message| -> Result<Option<i64>, &'static str> {
        one_header(headers, name)?
            .map(|value| parse_nonnegative(value).ok_or(message))
            .transpose()
    };
    let offset = number("x-goog-upload-offset", "x-goog-upload-offset is invalid.")?;
    let content_length = number("content-length", "content-length is invalid.")?;
    let declared_size = number(
        "x-goog-upload-header-content-length",
        "x-goog-upload-header-content-length is invalid.",
    )?;
    let mime_type = one_header(headers, "x-goog-upload-header-content-type")?
        .or(one_header(headers, "content-type")?)
        .map(str::to_owned);
    let display_name = one_header(headers, "x-goog-upload-file-name")?.map(str::to_owned);
    match command {
        UploadCommand::Start if offset.is_some() => {
            return Err("A start command must not include x-goog-upload-offset.")
        }
        UploadCommand::Start if declared_size.is_none() => {
            return Err("x-goog-upload-header-content-length is required.")
        }
        UploadCommand::Query if offset.is_some() || declared_size.is_some() => {
            return Err("A query command must not include upload offset or declared size.")
        }
        UploadCommand::Upload { .. } if offset.is_none() => {
            return Err("x-goog-upload-offset is required.")
        }
        _ => {}
    }
    Ok(UploadHeaders {
        command,
        offset,
        content_length,
        declared_size,
        mime_type,
        display_name,
    })
}

fn upload_file_id(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(key, value)| (key == "upload_id" && id(value)).then(|| value.to_owned()))
}
fn upload_url(id: &str) -> String {
    format!("/upload/v1beta/files?upload_id={id}")
}
fn set_upload_response_headers(
    response: &mut Response,
    id: &str,
    status: &'static str,
    offset: i64,
) {
    let headers = response.headers_mut();
    headers.insert("x-goog-upload-status", HeaderValue::from_static(status));
    if let Ok(value) = HeaderValue::from_str(&offset.to_string()) {
        headers.insert("x-goog-upload-size-received", value);
    }
    if let Ok(value) = HeaderValue::from_str(&upload_url(id)) {
        headers.insert("x-goog-upload-url", value);
    }
}

fn file_id_from_name(name: &str) -> Option<&str> {
    let file_id = name.strip_prefix("files/")?;
    id(file_id).then_some(file_id)
}

#[derive(Default)]
struct JsonlParser {
    pending: Vec<u8>,
    consumed: usize,
    item_count: usize,
    line_number: usize,
    #[cfg(test)]
    peak_pending_bytes: usize,
}

#[allow(dead_code)]
async fn file_requests(
    facade: &GeminiBatchPublicFacade,
    account_id: &str,
    file_id: &str,
) -> Result<Vec<Value>, Response> {
    let file = match facade
        .authority()
        .file_get(account_id.to_owned(), file_id.to_owned())
        .await
    {
        Ok(Some(file))
            if file.state == "active"
                && file.storage_kind == "chunked"
                && (0..=registry::MAX_BATCH_FILE_BYTES).contains(&file.size_bytes) =>
        {
            file
        }
        Ok(_) => {
            return Err(error(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "Input file was not found.",
            ))
        }
        Err(_) => {
            return Err(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                "File authority is unavailable.",
            ))
        }
    };
    if file.mime_type != "application/jsonl"
        && file.mime_type != "application/json"
        && file.mime_type != "application/x-ndjson"
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "inputConfig.fileName must reference a JSONL file.",
        ));
    }
    let mut parser = JsonlParser::default();
    let mut after = None;
    let mut expected_chunk = 0i64;
    let mut total = 0i64;
    loop {
        let page = facade
            .authority()
            .file_chunks(
                account_id.to_owned(),
                file_id.to_owned(),
                after,
                FILE_CHUNK_PAGE_SIZE,
            )
            .await
            .map_err(|_| {
                error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    "Input file authority is unavailable.",
                )
            })?;
        if page.chunks.is_empty() && page.next_chunk_index.is_some() {
            return Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATA_LOSS",
                "Input file chunk page is invalid.",
            ));
        }
        for chunk in page.chunks {
            if chunk.chunk_index != expected_chunk {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file chunks are incomplete.",
                ));
            }
            if chunk.plaintext_len < 0 {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file chunk length is invalid.",
                ));
            }
            total = total.checked_add(chunk.plaintext_len).ok_or_else(|| {
                error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file is too large.",
                )
            })?;
            if total > file.size_bytes || total > registry::MAX_BATCH_FILE_BYTES {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file size is invalid.",
                ));
            }
            expected_chunk = expected_chunk.checked_add(1).ok_or_else(|| {
                error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file chunk index overflowed.",
                )
            })?;
            let plaintext = facade
                .keys()
                .decrypt_file_chunk(
                    &super::GeminiBatchFileChunkIdentity {
                        account_id,
                        file_id,
                        chunk_index: chunk.chunk_index,
                        schema_version: SCHEMA_VERSION,
                    },
                    &chunk,
                )
                .map_err(|_| {
                    error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATA_LOSS",
                        "Input file authentication failed.",
                    )
                })?;
            if i64::try_from(plaintext.len()).ok() != Some(chunk.plaintext_len) {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file chunk length is invalid.",
                ));
            }
            parser
                .push(&plaintext)
                .map_err(|message| error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", message))?;
        }
        match page.next_chunk_index {
            Some(next) if next == expected_chunk - 1 && after != Some(next) => after = Some(next),
            Some(_) => {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file chunk cursor is invalid.",
                ))
            }
            None => break,
        }
    }
    if total != file.size_bytes {
        return Err(error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATA_LOSS",
            "Input file is incomplete.",
        ));
    }
    parser
        .finish()
        .map_err(|message| error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", message))
}

impl JsonlParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>, &'static str> {
        self.pending.extend_from_slice(bytes);
        #[cfg(test)]
        {
            self.peak_pending_bytes = self.peak_pending_bytes.max(self.pending.len());
        }
        let mut entries = Vec::new();
        while let Some(relative) = self.pending[self.consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let end = self.consumed + relative + 1;
            if end - self.consumed > JSONL_LINE_LIMIT {
                return Err("A JSONL line is too large.");
            }
            if let Some(entry) = self.parse_line(self.consumed, end)? {
                entries.push(entry);
            }
            self.consumed = end;
        }
        if self.pending.len().saturating_sub(self.consumed) > JSONL_LINE_LIMIT {
            return Err("A JSONL line is too large.");
        }
        if self.consumed > 0 {
            self.pending.drain(..self.consumed);
            self.consumed = 0;
        }
        Ok(entries)
    }

    fn finish(&mut self) -> Result<Vec<Value>, &'static str> {
        let mut entries = Vec::new();
        if !self.pending.is_empty() {
            let end = self.pending.len();
            if let Some(entry) = self.parse_line(0, end)? {
                entries.push(entry);
            }
            self.pending.clear();
        }
        if self.item_count == 0 {
            return Err("Batch must contain requests.");
        }
        Ok(entries)
    }

    fn parse_line(&mut self, start: usize, end: usize) -> Result<Option<Value>, &'static str> {
        self.line_number = self
            .line_number
            .checked_add(1)
            .ok_or("JSONL line count overflowed.")?;
        let line = self.pending[start..end]
            .strip_suffix(b"\n")
            .unwrap_or(&self.pending[start..end]);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Ok(None);
        }
        let entry: Value =
            serde_json::from_slice(line).map_err(|_| "A JSONL line is not valid JSON.")?;
        let object = entry
            .as_object()
            .ok_or("Each nonempty JSONL line must be an object.")?;
        let valid_key = object
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.is_empty() && key.len() <= 512);
        if !valid_key || !object.get("request").is_some_and(Value::is_object) {
            return Err(
                "Each nonempty JSONL line must contain bounded string key and object request.",
            );
        }
        if self.item_count >= MAX_BATCH_ITEMS {
            return Err("Batch contains too many requests.");
        }
        self.item_count += 1;
        Ok(Some(entry))
    }
}

pub async fn dispatch(
    app: AppState,
    peer: SocketAddr,
    route: Route,
    request: axum::extract::Request,
) -> Response {
    let Some(facade) = app.gemini_batch.as_ref().cloned() else {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Requested entity was not found.",
        );
    };
    let headers = request.headers().clone();
    let auth = match auth(&app, &headers, &peer).await {
        Ok(v) => v,
        Err(r) => return r,
    }; // auth-before-body
    match route {
        Route::Create(model) => create(facade, auth, model, headers, request.into_body()).await,
        Route::List => list(facade, auth, request.uri().query()).await,
        Route::Get(id) => get(facade, auth, id).await,
        Route::Cancel(id) => cancel(facade, auth, id).await,
        Route::Delete(id) => delete(facade, auth, id).await,
        Route::Files if request.method() == Method::GET => {
            file_list(facade, auth, request.uri().query()).await
        }
        Route::Files => file_metadata_create(facade, auth, headers, request.into_body()).await,
        Route::File(id) if request.method() == Method::DELETE => {
            file_delete(facade, auth, id).await
        }
        Route::File(id) => file_get(facade, auth, id).await,
        Route::Download(id) => file_download(facade, auth, id, headers).await,
        Route::Upload => {
            let query = request.uri().query().map(str::to_owned);
            file_upload(facade, auth, query.as_deref(), headers, request.into_body()).await
        }
    }
}

async fn body(body: Body, limit: usize) -> Result<Vec<u8>, Response> {
    to_bytes(body, limit)
        .await
        .map(|v| v.to_vec())
        .map_err(|_| {
            error(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                "Request body is invalid or too large.",
            )
        })
}
async fn create(
    f: Arc<GeminiBatchPublicFacade>,
    a: MeteredAuth,
    model_id: String,
    headers: HeaderMap,
    b: Body,
) -> Response {
    if a.mult_bp > 0 && a.available <= 0 {
        return error(
            StatusCode::PAYMENT_REQUIRED,
            "RESOURCE_EXHAUSTED",
            "Insufficient balance.",
        );
    }
    let bytes = match body(b, BODY_LIMIT).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let mut root: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                "Request body is not valid JSON.",
            )
        }
    };
    let Some(batch) = root.get_mut("batch").and_then(Value::as_object_mut) else {
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "batch is required.",
        );
    };
    if batch.contains_key("webhookConfig") {
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "webhookConfig is unsupported.",
        );
    }
    let display = batch
        .get("displayName")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 512)
        .unwrap_or("Gemini Batch")
        .to_owned();
    let priority = batch.get("priority").and_then(Value::as_i64).unwrap_or(0);
    let Some(input) = batch.get_mut("inputConfig").and_then(Value::as_object_mut) else {
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "batch.inputConfig is required.",
        );
    };
    let inline_requests = input.remove("requests");
    let file_name = input.remove("fileName");
    let (inline, input_kind, input_file_id) = match (inline_requests, file_name) {
        (Some(requests), None) => {
            let Some(requests) = requests.as_array().cloned() else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "inputConfig.requests must be an array.",
                );
            };
            if requests.is_empty() {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Batch must contain requests.",
                );
            }
            if requests.len() > MAX_BATCH_ITEMS {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Batch contains too many requests.",
                );
            }
            (Some(requests), registry::GeminiBatchInputKind::Inline, None)
        }
        (None, Some(file_name)) => {
            let Some(file_id) = file_name.as_str().and_then(file_id_from_name) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "inputConfig.fileName must be a valid files/{id} name.",
                );
            };
            let file_id = file_id.to_owned();
            let file = match f
                .authority()
                .file_get(a.account.clone(), file_id.clone())
                .await
            {
                Ok(Some(file))
                    if file.state == "active"
                        && file.storage_kind == "chunked"
                        && (0..=registry::MAX_BATCH_FILE_BYTES).contains(&file.size_bytes)
                        && matches!(
                            file.mime_type.as_str(),
                            "application/jsonl" | "application/json" | "application/x-ndjson"
                        ) =>
                {
                    file
                }
                Ok(_) => {
                    return error(
                        StatusCode::NOT_FOUND,
                        "NOT_FOUND",
                        "Input file was not found.",
                    )
                }
                Err(_) => {
                    return error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "UNAVAILABLE",
                        "File authority is unavailable.",
                    )
                }
            };
            let _ = file;
            (None, registry::GeminiBatchInputKind::File, Some(file_id))
        }
        (Some(_), Some(_)) => {
            return error(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                "inputConfig must contain exactly one of requests or fileName.",
            )
        }
        (None, None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                "inputConfig must contain requests or fileName.",
            )
        }
    };
    let Some(model) = f.gateway().config().model(&model_id).cloned() else {
        return error(StatusCode::NOT_FOUND, "NOT_FOUND", "Model was not found.");
    };
    if model.is_image_generation() {
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "Image-output models are unsupported in batch.",
        );
    }
    let mut created = now();
    let mut job = fresh("batch");
    let mut admission = fresh("batch-admission");
    let mut resume_at = 0i64;
    let mut committed_replay: Option<(String, [u8; 32])> = None;
    let begin = registry::GeminiBatchAdmissionBegin {
        admission_id: admission.clone(),
        job_id: job.clone(),
        account_id: a.account.clone(),
        creator_key_id: a.key_id.clone(),
        public_model: model_id.clone(),
        display_name: display.clone(),
        idempotency_digest: idem(&headers, &a.account),
        priority,
        input_kind,
        input_file_id: input_file_id.clone(),
        schema_version: SCHEMA_VERSION,
        encryption_policy_version: 1,
        create_ts: created,
        deadline_ts: created + JOB_TTL,
        expires_ts: created + JOB_TTL,
    };
    match f.ingest().begin(begin).await {
        Ok(registry::GeminiBatchAdmissionBeginOutcome::Replay {
            job_id,
            canonical_request_digest,
        }) => {
            committed_replay = Some((job_id, canonical_request_digest));
        }
        Ok(registry::GeminiBatchAdmissionBeginOutcome::Started {
            admission_id,
            job_id,
            create_ts,
            deadline_ts: _,
            expires_ts: _,
            next_item_index,
        }) => {
            admission = admission_id;
            job = job_id;
            created = create_ts;
            debug_assert_eq!(next_item_index, 0);
            resume_at = 0;
        }
        Err(e) if registry::is_gemini_batch_idempotency_conflict(&e) => {
            return error(
                StatusCode::CONFLICT,
                "ABORTED",
                "Idempotency key conflicts with another request.",
            )
        }
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                "Batch ingest authority is unavailable.",
            )
        }
    }
    let mut next = resume_at;
    let mut source_index = 0i64;
    let mut request_digests = Sha256::new();
    request_digests.update(b"apitoken:gemini-batch-request-digests:v1\0");
    let prices = metering::gemini_prices_at(&model.id, created).unwrap_or(model.prices);
    let (family, _) = metering::gemini_matched_tariff_at(&model.id, created)
        .unwrap_or(("google/gemini/unknown", prices));
    let mut stage_entries = |page_start: i64,
                             entries: Vec<Value>|
     -> Result<Vec<registry::GeminiBatchAdmissionItem>, Response> {
        let mut page = Vec::with_capacity(entries.len());
        for (entry_offset, entry) in entries.into_iter().enumerate() {
            let file_input = input_kind == registry::GeminiBatchInputKind::File;
            let Some(object) = entry.as_object() else {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Each batch item must be an object.",
                ));
            };
            let (mut req, client_key, metadata) = (
                if file_input {
                    object
                        .get("request")
                        .cloned()
                        .expect("validated JSONL request")
                } else {
                    object.get("request").cloned().unwrap_or(entry.clone())
                },
                object.get("key").and_then(Value::as_str).map(str::to_owned),
                (!file_input)
                    .then(|| object.get("metadata").cloned())
                    .flatten(),
            );
            if !req.is_object() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Each request must be an object.",
                ));
            }
            let mut refs = Vec::new();
            if let Some(contents) = req.get_mut("contents").and_then(Value::as_array_mut) {
                for content in contents {
                    if let Some(parts) = content.get_mut("parts").and_then(Value::as_array_mut) {
                        for part in parts {
                            if let Some(fd) =
                                part.get_mut("fileData").and_then(Value::as_object_mut)
                            {
                                if let Some(uri) = fd
                                    .get("fileUri")
                                    .and_then(Value::as_str)
                                    .and_then(file_id_from_name)
                                {
                                    refs.push(uri.to_owned());
                                    fd.insert(
                                        "fileUri".into(),
                                        Value::String(format!("files/{uri}")),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            api::canonicalize_native_request(&mut req);
            if let Err(msg) = api::validate_batch_generate_request(&req, &model) {
                return Err(error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", msg));
            }
            let (_, output, _, grounding) = api::batch_generation_controls(&req, &model);
            let plain = serde_json::to_vec(&req).expect("JSON serialization");
            let _ = (output, grounding, prices);
            let idx = page_start
                + i64::try_from(entry_offset).map_err(|_| {
                    error(
                        StatusCode::BAD_REQUEST,
                        "INVALID_ARGUMENT",
                        "Batch contains too many requests.",
                    )
                })?;
            let blob = f
                .keys()
                .encrypt_blob(
                    &GeminiBatchBlobIdentity {
                        account_id: &a.account,
                        job_id: &job,
                        item_index: idx,
                        kind: "request",
                        schema_version: SCHEMA_VERSION,
                    },
                    &plain,
                    created + RESULT_TTL,
                )
                .map_err(|_| {
                    error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "UNAVAILABLE",
                        "Batch encryption is unavailable.",
                    )
                })?;
            let meta = metadata
                .and_then(|v| serde_json::to_vec(&v).ok())
                .and_then(|v| {
                    f.keys()
                        .encrypt_blob(
                            &GeminiBatchBlobIdentity {
                                account_id: &a.account,
                                job_id: &job,
                                item_index: idx,
                                kind: "metadata",
                                schema_version: SCHEMA_VERSION,
                            },
                            &v,
                            created + RESULT_TTL,
                        )
                        .ok()
                });
            let request_digest = digest(&plain);
            request_digests.update(request_digest);
            page.push(registry::GeminiBatchAdmissionItem {
                requested_output_tokens: i64::try_from(output).unwrap_or(i64::MAX),
                item: registry::GeminiBatchCreateItem {
                    item_index: idx,
                    request_id: fresh("batch-item"),
                    logical_request_id: fresh("logical"),
                    execution_group_id: fresh("exec"),
                    client_key,
                    request_digest,
                    input_file_id: None,
                    referenced_file_ids: refs,
                    hold_nano: 0,
                    payable_multiplier_bp: a.mult_bp,
                    priced_ts: created,
                    tariff_family: family.to_owned(),
                    tariff_version: 1,
                    tariff_schedule_id: format!("{family}/v1"),
                    request_blob: blob,
                    metadata_blob: meta,
                },
            });
        }
        Ok(page)
    };
    let result: Result<(), Response> = async {
        if let Some(entries) = inline {
            for chunk in entries.chunks(registry::MAX_BATCH_ADMISSION_PAGE_SIZE) {
                let page = stage_entries(source_index, chunk.to_vec())?;
                source_index += chunk.len() as i64;
                if committed_replay.is_none() && !page.is_empty() {
                    next = f
                        .ingest()
                        .append(admission.clone(), next, page)
                        .await
                        .map_err(|_| {
                            error(
                                StatusCode::SERVICE_UNAVAILABLE,
                                "UNAVAILABLE",
                                "Batch ingest authority is unavailable.",
                            )
                        })?;
                }
            }
        } else {
            let file_id = input_file_id.as_deref().expect("file input");
            let file = f
                .authority()
                .file_get(a.account.clone(), file_id.to_owned())
                .await
                .map_err(|_| {
                    error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "UNAVAILABLE",
                        "Input file authority is unavailable.",
                    )
                })?
                .ok_or_else(|| {
                    error(
                        StatusCode::NOT_FOUND,
                        "NOT_FOUND",
                        "Input file was not found.",
                    )
                })?;
            let mut parser = JsonlParser::default();
            let mut after = None;
            let mut expected_chunk = 0i64;
            let mut total_bytes = 0i64;
            let mut pending = Vec::new();
            loop {
                let page = f
                    .authority()
                    .file_chunks(
                        a.account.clone(),
                        file_id.to_owned(),
                        after,
                        FILE_CHUNK_PAGE_SIZE,
                    )
                    .await
                    .map_err(|_| {
                        error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "UNAVAILABLE",
                            "Input file authority is unavailable.",
                        )
                    })?;
                if page.chunks.is_empty() && page.next_chunk_index.is_some() {
                    return Err(error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATA_LOSS",
                        "Input file chunk page is invalid.",
                    ));
                }
                for chunk in page.chunks {
                    if chunk.chunk_index != expected_chunk {
                        return Err(error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "DATA_LOSS",
                            "Input file chunks are incomplete.",
                        ));
                    }
                    total_bytes =
                        total_bytes
                            .checked_add(chunk.plaintext_len)
                            .ok_or_else(|| {
                                error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "DATA_LOSS",
                                    "Input file is too large.",
                                )
                            })?;
                    if total_bytes > file.size_bytes {
                        return Err(error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "DATA_LOSS",
                            "Input file size is invalid.",
                        ));
                    }
                    let plaintext = f
                        .keys()
                        .decrypt_file_chunk(
                            &super::GeminiBatchFileChunkIdentity {
                                account_id: &a.account,
                                file_id,
                                chunk_index: chunk.chunk_index,
                                schema_version: SCHEMA_VERSION,
                            },
                            &chunk,
                        )
                        .map_err(|_| {
                            error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "DATA_LOSS",
                                "Input file authentication failed.",
                            )
                        })?;
                    expected_chunk += 1;
                    pending.extend(
                        parser
                            .push(&plaintext)
                            .map_err(|m| error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", m))?,
                    );
                    while pending.len() >= registry::MAX_BATCH_ADMISSION_PAGE_SIZE {
                        let tail = pending.split_off(registry::MAX_BATCH_ADMISSION_PAGE_SIZE);
                        let entry_count = pending.len() as i64;
                        let page =
                            stage_entries(source_index, std::mem::replace(&mut pending, tail))?;
                        source_index += entry_count;
                        if committed_replay.is_none() && !page.is_empty() {
                            next = f
                                .ingest()
                                .append(admission.clone(), next, page)
                                .await
                                .map_err(|_| {
                                    error(
                                        StatusCode::SERVICE_UNAVAILABLE,
                                        "UNAVAILABLE",
                                        "Batch ingest authority is unavailable.",
                                    )
                                })?;
                        }
                    }
                }
                match page.next_chunk_index {
                    Some(cursor) if cursor == expected_chunk - 1 && after != Some(cursor) => {
                        after = Some(cursor)
                    }
                    Some(_) => {
                        return Err(error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "DATA_LOSS",
                            "Input file chunk cursor is invalid.",
                        ))
                    }
                    None => break,
                }
            }
            if total_bytes != file.size_bytes {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Input file is incomplete.",
                ));
            }
            pending.extend(
                parser
                    .finish()
                    .map_err(|m| error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", m))?,
            );
            if !pending.is_empty() {
                let entry_count = pending.len() as i64;
                let page = stage_entries(source_index, pending)?;
                source_index += entry_count;
                if committed_replay.is_none() && !page.is_empty() {
                    next = f
                        .ingest()
                        .append(admission.clone(), next, page)
                        .await
                        .map_err(|_| {
                            error(
                                StatusCode::SERVICE_UNAVAILABLE,
                                "UNAVAILABLE",
                                "Batch ingest authority is unavailable.",
                            )
                        })?;
                }
            }
        }
        Ok(())
    }
    .await;
    if let Err(response) = result {
        let _ = f.ingest().abort(admission).await;
        return response;
    }
    if next == 0 {
        let _ = f.ingest().abort(admission).await;
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "Batch must contain requests.",
        );
    }
    let item_digest: [u8; 32] = request_digests.finalize().into();
    let canonical=digest(&serde_json::to_vec(&json!({"model":model_id,"displayName":display,"priority":priority,"inputKind":if input_kind==registry::GeminiBatchInputKind::Inline{"inline"}else{"file"},"itemCount":next,"itemDigest":hex_digest(&item_digest)})).unwrap());
    if let Some((replay_job, stored_digest)) = committed_replay {
        return if stored_digest == canonical {
            operation_pending(&replay_job)
        } else {
            error(
                StatusCode::CONFLICT,
                "ABORTED",
                "Idempotency key conflicts with another request.",
            )
        };
    }
    match f
        .ingest()
        .publish(admission, next, canonical, a.raw_key)
        .await
    {
        Ok(registry::GeminiBatchCreateOutcome::Created { .. }) => operation_pending(&job),
        Ok(registry::GeminiBatchCreateOutcome::Replay { job_id }) => operation_pending(&job_id),
        Ok(registry::GeminiBatchCreateOutcome::RejectedFunds) => error(
            StatusCode::PAYMENT_REQUIRED,
            "RESOURCE_EXHAUSTED",
            "Insufficient balance.",
        ),
        Ok(registry::GeminiBatchCreateOutcome::RejectedLimit) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "RESOURCE_EXHAUSTED",
            "Batch limit exceeded.",
        ),
        Err(e) if registry::is_gemini_batch_idempotency_conflict(&e) => error(
            StatusCode::CONFLICT,
            "ABORTED",
            "Idempotency key conflicts with another request.",
        ),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "Batch ingest authority is unavailable.",
        ),
    }
}

fn operation_pending(id: &str) -> Response {
    (
        StatusCode::OK,
        axum::Json(json!({"name":format!("batches/{id}"),"done":false})),
    )
        .into_response()
}
fn decrypt_terminal_value(
    facade: &GeminiBatchPublicFacade,
    account_id: &str,
    job_id: &str,
    item_index: i64,
    kind: &'static str,
    blob: &registry::GeminiBatchEncryptedBlob,
) -> Result<Value, ()> {
    let plaintext = facade
        .keys()
        .decrypt_blob(
            &GeminiBatchBlobIdentity {
                account_id,
                job_id,
                item_index,
                kind,
                schema_version: SCHEMA_VERSION,
            },
            blob,
        )
        .map_err(|_| ())?;
    serde_json::from_slice(&plaintext).map_err(|_| ())
}

fn project_terminal_payload(kind: &str, payload: Value) -> Result<Value, &'static str> {
    if kind == "result" {
        if !payload.is_object() {
            return Err("Terminal Batch response is malformed.");
        }
        return Ok(json!({"response":payload}));
    }
    let status = payload.get("error").cloned().unwrap_or(payload);
    let valid_status = status.as_object().is_some_and(|status| {
        status.get("code").is_some_and(Value::is_number)
            && status.get("message").is_some_and(Value::is_string)
    });
    if !valid_status {
        return Err("Terminal Batch error is malformed.");
    }
    Ok(json!({"error":status}))
}

async fn terminal_item_value(
    facade: &GeminiBatchPublicFacade,
    account_id: &str,
    job_id: &str,
    item: &registry::GeminiBatchItem,
) -> Result<Value, Response> {
    let kind = if item.state == registry::GeminiBatchItemState::Succeeded {
        "result"
    } else {
        "error"
    };
    let blob = facade
        .authority()
        .blob_get(
            account_id.to_owned(),
            job_id.to_owned(),
            item.item_index,
            kind.to_owned(),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                "Batch authority is unavailable.",
            )
        })?;
    if blob.is_none() && item.state == registry::GeminiBatchItemState::Canceled {
        let status = if item.terminal_class == Some(registry::GeminiBatchTerminalClass::Expired) {
            json!({"code":408,"status":"DEADLINE_EXCEEDED","message":"Batch item expired before dispatch."})
        } else {
            json!({"code":499,"status":"CANCELLED","message":"Batch item was cancelled before dispatch."})
        };
        let mut projected = json!({"error":status});
        if let Some(client_key) = item.client_key.as_ref() {
            projected["metadata"] = json!({"key":client_key});
        }
        return Ok(projected);
    }
    let blob = blob.ok_or_else(|| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATA_LOSS",
            "Terminal Batch output is missing.",
        )
    })?;
    let payload = decrypt_terminal_value(facade, account_id, job_id, item.item_index, kind, &blob)
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATA_LOSS",
                "Terminal Batch output failed authentication.",
            )
        })?;
    let mut projected = project_terminal_payload(kind, payload)
        .map_err(|message| error(StatusCode::INTERNAL_SERVER_ERROR, "DATA_LOSS", message))?;
    let mut metadata = None;
    if let Some(metadata_blob) = facade
        .authority()
        .blob_get(
            account_id.to_owned(),
            job_id.to_owned(),
            item.item_index,
            "metadata".to_owned(),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                "Batch authority is unavailable.",
            )
        })?
    {
        metadata = Some(
            decrypt_terminal_value(
                facade,
                account_id,
                job_id,
                item.item_index,
                "metadata",
                &metadata_blob,
            )
            .map_err(|_| {
                error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Batch item metadata failed authentication.",
                )
            })?,
        );
    }
    if let Some(client_key) = item.client_key.as_ref() {
        let metadata_value = metadata.get_or_insert_with(|| json!({}));
        let Some(object) = metadata_value.as_object_mut() else {
            return Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATA_LOSS",
                "Batch item correlation metadata is malformed.",
            ));
        };
        if object
            .get("key")
            .is_some_and(|key| key.as_str() != Some(client_key))
        {
            return Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATA_LOSS",
                "Batch item correlation metadata conflicts with its durable key.",
            ));
        }
        object.insert("key".to_owned(), Value::String(client_key.clone()));
    }
    if let Some(metadata) = metadata {
        projected["metadata"] = metadata;
    }
    Ok(projected)
}

async fn job_value(
    facade: &GeminiBatchPublicFacade,
    account_id: &str,
    d: &registry::GeminiBatchJobDetail,
) -> Result<Value, Response> {
    let j = &d.job;
    let done = j.completed_ts.is_some();
    let mut value = json!({"name":format!("batches/{}",j.job_id),"done":done,"metadata":{"name":format!("batches/{}",j.job_id),"displayName":j.display_name,"model":format!("models/{}",j.public_model),"state":format!("BATCH_STATE_{:?}",j.state).to_uppercase(),"priority":j.priority.to_string(),"createTime":ts(j.create_ts),"updateTime":ts(j.update_ts),"endTime":j.completed_ts.map(ts),"batchStats":{"requestCount":j.stats.request_count.to_string(),"successfulRequestCount":j.stats.successful_request_count.to_string(),"failedRequestCount":j.stats.failed_request_count.to_string(),"pendingRequestCount":j.stats.pending_request_count.to_string()}}});
    if done && j.state == registry::GeminiBatchJobState::Expired {
        // Retain a constant-size tombstone; result blobs are intentionally unreadable after expiry.
    } else if done && j.input_kind == registry::GeminiBatchInputKind::File {
        let Some(file_id) = j.output_file_id.as_ref() else {
            return Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATA_LOSS",
                "Terminal Batch output file is missing.",
            ));
        };
        value["metadata"]["output"] = json!({"responsesFile":format!("files/{file_id}")});
        value["response"] = json!({"responsesFile":format!("files/{file_id}")});
    } else if done {
        let mut responses = Vec::with_capacity(d.items.len());
        for item in &d.items {
            if !item.state.is_terminal() {
                return Err(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATA_LOSS",
                    "Terminal Batch contains a nonterminal item.",
                ));
            }
            responses.push(terminal_item_value(facade, account_id, &j.job_id, item).await?);
        }
        value["response"] = json!({"inlinedResponses":responses});
    }
    Ok(value)
}
async fn get(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, id: String) -> Response {
    let account_id = a.account;
    match f.authority().get(account_id.clone(), id).await {
        Ok(Some(v)) => match job_value(&f, &account_id, &v).await {
            Ok(value) => (StatusCode::OK, axum::Json(value)).into_response(),
            Err(response) => response,
        },
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Requested entity was not found.",
        ),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "Batch authority is unavailable.",
        ),
    }
}
async fn list(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, q: Option<&str>) -> Response {
    let mut size = 20i64;
    let mut token = None;
    for pair in q.unwrap_or("").split('&').filter(|v| !v.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == "pageSize" {
            size = v.parse::<i64>().unwrap_or(20)
        } else if k == "pageToken" {
            if let Ok(raw) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(v.as_bytes()) {
                if let Ok(s) = String::from_utf8(raw) {
                    if let Some((t, id)) = s.split_once(':') {
                        token =
                            t.parse::<i64>()
                                .ok()
                                .map(|create_ts| registry::GeminiBatchPageCursor {
                                    create_ts,
                                    job_id: id.into(),
                                })
                    }
                }
            }
        }
    }
    match f.authority().list(a.account, token, size).await {
        Ok(p) => {
            let next = p.next_cursor.map(|c| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(format!("{}:{}", c.create_ts, c.job_id))
            });
            (StatusCode::OK,axum::Json(json!({"operations":p.jobs.into_iter().map(|j|json!({"name":format!("batches/{}",j.job_id),"done":j.completed_ts.is_some()})).collect::<Vec<_>>(),"nextPageToken":next}))).into_response()
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "Batch authority is unavailable.",
        ),
    }
}
async fn cancel(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, id: String) -> Response {
    match f.authority().cancel(a.account, id).await {
        Ok(Some(_)) => (StatusCode::OK, axum::Json(json!({}))).into_response(),
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Requested entity was not found.",
        ),
        Err(authority_error) => {
            elog::error(
                "gemini-batch",
                format!("Gemini Batch cancel authority failed: {authority_error:#}"),
            );
            error(
                StatusCode::FAILED_DEPENDENCY,
                "FAILED_PRECONDITION",
                "Batch cannot be canceled.",
            )
        }
    }
}
async fn delete(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, id: String) -> Response {
    match f.authority().delete(a.account, id).await {
        Ok(true) => (StatusCode::OK, axum::Json(json!({}))).into_response(),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Requested entity was not found.",
        ),
        Err(_) => error(
            StatusCode::FAILED_DEPENDENCY,
            "FAILED_PRECONDITION",
            "Batch is not terminal.",
        ),
    }
}
fn file_value(v: &registry::GeminiBatchFile) -> Value {
    json!({"name":format!("files/{}",v.file_id),"displayName":v.display_name,"mimeType":v.mime_type,"sizeBytes":v.size_bytes.to_string(),"createTime":ts(v.create_ts),"updateTime":ts(v.update_ts),"expirationTime":ts(v.expiration_ts),"state":v.state.to_uppercase()})
}
async fn file_get(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, id: String) -> Response {
    match f.authority().file_get(a.account, id).await {
        Ok(Some(v)) => (StatusCode::OK, axum::Json(file_value(&v))).into_response(),
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Requested entity was not found.",
        ),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "File authority unavailable.",
        ),
    }
}
async fn file_list(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, q: Option<&str>) -> Response {
    let limit = q
        .unwrap_or("")
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == "pageSize")
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .unwrap_or(20);
    match f.authority().file_list(a.account, limit).await {
        Ok(v) => (
            StatusCode::OK,
            axum::Json(json!({"files":v.iter().map(file_value).collect::<Vec<_>>()})),
        )
            .into_response(),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "File authority unavailable.",
        ),
    }
}
async fn file_delete(f: Arc<GeminiBatchPublicFacade>, a: MeteredAuth, id: String) -> Response {
    match f.authority().file_delete(a.account, id).await {
        Ok(true) => (StatusCode::OK, axum::Json(json!({}))).into_response(),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Requested entity was not found.",
        ),
        Err(_) => error(
            StatusCode::FAILED_DEPENDENCY,
            "FAILED_PRECONDITION",
            "File is referenced by a live batch.",
        ),
    }
}
async fn file_metadata_create(
    f: Arc<GeminiBatchPublicFacade>,
    a: MeteredAuth,
    h: HeaderMap,
    b: Body,
) -> Response {
    let raw = match body(b, 1024 * 1024).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let v: Value = serde_json::from_slice(&raw).unwrap_or(json!({}));
    let file = v.get("file").unwrap_or(&v);
    let name = file
        .get("displayName")
        .and_then(Value::as_str)
        .or_else(|| {
            h.get("x-goog-upload-file-name")
                .and_then(|v| v.to_str().ok())
        })
        .unwrap_or("upload");
    let mime = file
        .get("mimeType")
        .and_then(Value::as_str)
        .or_else(|| {
            h.get("x-goog-upload-header-content-type")
                .and_then(|v| v.to_str().ok())
        })
        .unwrap_or("application/octet-stream");
    let size = file
        .get("sizeBytes")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0);
    create_file(f, a, name, mime, size, None).await
}
async fn file_upload(
    f: Arc<GeminiBatchPublicFacade>,
    a: MeteredAuth,
    query: Option<&str>,
    h: HeaderMap,
    b: Body,
) -> Response {
    let upload = match parse_upload_headers(&h) {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", message),
    };
    match upload.command {
        UploadCommand::Query => {
            let Some(file_id) = upload_file_id(query) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Upload session is invalid.",
                );
            };
            match f
                .authority()
                .file_progress(a.account, file_id.clone())
                .await
            {
                Ok(Some(progress)) => {
                    let mut response = StatusCode::OK.into_response();
                    set_upload_response_headers(
                        &mut response,
                        &file_id,
                        if progress.active { "final" } else { "active" },
                        progress.received_bytes,
                    );
                    response
                }
                _ => error(
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    "Upload session was not found.",
                ),
            }
        }
        UploadCommand::Start => {
            let Some(size) = upload
                .declared_size
                .filter(|value| *value <= registry::MAX_BATCH_FILE_BYTES)
            else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Upload size is invalid.",
                );
            };
            let file_id = fresh("file");
            let created = now();
            let create = registry::GeminiBatchFileCreate {
                file_id: file_id.clone(),
                account_id: a.account,
                display_name: upload
                    .display_name
                    .unwrap_or_else(|| "upload".to_owned())
                    .chars()
                    .take(512)
                    .collect(),
                mime_type: upload
                    .mime_type
                    .unwrap_or_else(|| "application/octet-stream".to_owned())
                    .chars()
                    .take(255)
                    .collect(),
                size_bytes: size,
                // Resumable start does not yet know the whole plaintext digest. Registry completion
                // binds the trusted streaming digest to this declared placeholder only when zero;
                // nonzero create digests remain exact replay identities.
                sha256_digest: [0; 32],
                source_kind: "client_upload".to_owned(),
                create_ts: created,
                expiration_ts: created + UPLOAD_TTL,
            };
            match f.authority().file_create(create).await {
                Ok(registry::GeminiBatchFileCreateOutcome::Created) => {
                    let mut response = StatusCode::OK.into_response();
                    set_upload_response_headers(&mut response, &file_id, "active", 0);
                    response
                }
                Ok(_) => error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "RESOURCE_EXHAUSTED",
                    "File quota exceeded.",
                ),
                Err(_) => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    "File authority unavailable.",
                ),
            }
        }
        UploadCommand::Upload { finalize } => {
            let Some(file_id) = upload_file_id(query) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Upload session is invalid.",
                );
            };
            let file = match f
                .authority()
                .file_get(a.account.clone(), file_id.clone())
                .await
            {
                Ok(Some(file)) if file.state == "processing" && file.storage_kind == "chunked" => {
                    file
                }
                _ => {
                    return error(
                        StatusCode::NOT_FOUND,
                        "NOT_FOUND",
                        "Upload session was not found.",
                    )
                }
            };
            let raw = match body(b, registry::MAX_BATCH_FILE_CHUNK_BYTES as usize).await {
                Ok(value) => value,
                Err(response) => return response,
            };
            if upload
                .content_length
                .is_some_and(|value| value != raw.len() as i64)
            {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "content-length does not match upload body.",
                );
            }
            let offset = upload.offset.unwrap_or(0);
            let progress = match f
                .authority()
                .file_progress(a.account.clone(), file_id.clone())
                .await
            {
                Ok(Some(progress)) => progress,
                _ => {
                    return error(
                        StatusCode::NOT_FOUND,
                        "NOT_FOUND",
                        "Upload session was not found.",
                    )
                }
            };
            let offset = upload.offset.unwrap_or(0);
            if offset != progress.received_bytes
                || progress.received_bytes.saturating_add(raw.len() as i64) > progress.size_bytes
            {
                let mut response = error(
                    StatusCode::CONFLICT,
                    "ABORTED",
                    "Upload offset does not match persisted data.",
                );
                set_upload_response_headers(
                    &mut response,
                    &file_id,
                    "active",
                    progress.received_bytes,
                );
                return response;
            }
            let mut current = progress;
            if !raw.is_empty() {
                let chunk = match f.keys().encrypt_file_chunk(
                    &super::GeminiBatchFileChunkIdentity {
                        account_id: &a.account,
                        file_id: &file_id,
                        chunk_index: progress.next_chunk_index,
                        schema_version: SCHEMA_VERSION,
                    },
                    &raw,
                    now(),
                ) {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        return error(
                            StatusCode::BAD_REQUEST,
                            "INVALID_ARGUMENT",
                            "Upload chunk is invalid.",
                        )
                    }
                };
                match f
                    .authority()
                    .file_append(a.account.clone(), file_id.clone(), offset, chunk)
                    .await
                {
                    Ok(registry::GeminiBatchFileAppendOutcome::Appended(next)) => current = next,
                    Ok(registry::GeminiBatchFileAppendOutcome::OffsetConflict(actual)) => {
                        let mut response = error(
                            StatusCode::CONFLICT,
                            "ABORTED",
                            "Upload offset does not match persisted data.",
                        );
                        set_upload_response_headers(
                            &mut response,
                            &file_id,
                            "active",
                            actual.received_bytes,
                        );
                        return response;
                    }
                    _ => {
                        return error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "UNAVAILABLE",
                            "Upload persistence failed.",
                        )
                    }
                }
            }
            let new_offset = current.received_bytes;
            if !finalize {
                let mut response = StatusCode::OK.into_response();
                set_upload_response_headers(&mut response, &file_id, "active", new_offset);
                return response;
            }
            if new_offset != file.size_bytes {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "Finalize requires the declared upload size.",
                );
            }
            let mut whole = Sha256::new();
            let mut all_chunks = Vec::with_capacity(current.chunk_count as usize);
            let mut after = None;
            loop {
                let page = match f
                    .authority()
                    .file_chunks(
                        a.account.clone(),
                        file_id.clone(),
                        after,
                        registry::MAX_BATCH_FILE_CHUNK_PAGE_SIZE,
                    )
                    .await
                {
                    Ok(page) => page,
                    Err(_) => {
                        return error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "UNAVAILABLE",
                            "Upload manifest unavailable.",
                        )
                    }
                };
                for chunk in &page.chunks {
                    let plain = match f.keys().decrypt_file_chunk(
                        &super::GeminiBatchFileChunkIdentity {
                            account_id: &a.account,
                            file_id: &file_id,
                            chunk_index: chunk.chunk_index,
                            schema_version: SCHEMA_VERSION,
                        },
                        chunk,
                    ) {
                        Ok(plain) => plain,
                        Err(_) => {
                            return error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "DATA_LOSS",
                                "Upload authentication failed.",
                            )
                        }
                    };
                    whole.update(&*plain);
                }
                all_chunks.extend(page.chunks);
                match page.next_chunk_index {
                    Some(index) => after = Some(index),
                    None => break,
                }
            }
            let completion = registry::GeminiBatchFileCompletion {
                completed_ts: now(),
                whole_file_sha256_digest: whole.finalize().into(),
                chunk_manifest_digest: match super::gemini_batch_chunk_manifest_digest(&all_chunks)
                {
                    Ok(value) => value,
                    Err(_) => {
                        return error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "DATA_LOSS",
                            "Upload manifest is invalid.",
                        )
                    }
                },
            };
            if !matches!(
                f.authority()
                    .file_complete(a.account.clone(), file_id.clone(), completion)
                    .await,
                Ok(true)
            ) {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    "Upload finalize failed.",
                );
            }
            match f.authority().file_get(a.account, file_id.clone()).await {
                Ok(Some(file)) => {
                    let mut response = (
                        StatusCode::OK,
                        axum::Json(json!({"file":file_value(&file)})),
                    )
                        .into_response();
                    set_upload_response_headers(&mut response, &file_id, "final", new_offset);
                    response
                }
                _ => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    "File authority unavailable.",
                ),
            }
        }
    }
}
async fn create_file(
    f: Arc<GeminiBatchPublicFacade>,
    a: MeteredAuth,
    name: &str,
    mime: &str,
    size: i64,
    data: Option<Vec<u8>>,
) -> Response {
    let created = now();
    let id = fresh("file");
    let sha = data.as_ref().map(|v| digest(v)).unwrap_or([0; 32]);
    let create = registry::GeminiBatchFileCreate {
        file_id: id.clone(),
        account_id: a.account.clone(),
        display_name: name.chars().take(512).collect(),
        mime_type: mime.chars().take(255).collect(),
        size_bytes: size,
        sha256_digest: sha,
        source_kind: "client_upload".into(),
        create_ts: created,
        expiration_ts: created + UPLOAD_TTL,
    };
    match f.authority().file_create(create).await {
        Ok(registry::GeminiBatchFileCreateOutcome::Created)
        | Ok(registry::GeminiBatchFileCreateOutcome::Replay) => {}
        Ok(_) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "RESOURCE_EXHAUSTED",
                "File quota exceeded.",
            )
        }
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                "File authority unavailable.",
            )
        }
    };
    if let Some(data) = data {
        let mut enc = match f.keys().file_encryptor(&a.account, &id, SCHEMA_VERSION) {
            Ok(v) => v,
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    "File encryption unavailable.",
                )
            }
        };
        let mut upload_offset = 0i64;
        for chunk in data.chunks(registry::MAX_BATCH_FILE_CHUNK_BYTES as usize) {
            let c = match enc.push_chunk(chunk, created) {
                Ok(v) => v,
                Err(_) => {
                    return error(
                        StatusCode::BAD_REQUEST,
                        "INVALID_ARGUMENT",
                        "File chunk invalid.",
                    )
                }
            };
            if !matches!(
                f.authority()
                    .file_append(a.account.clone(), id.clone(), upload_offset, c)
                    .await,
                Ok(registry::GeminiBatchFileAppendOutcome::Appended(_))
            ) {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    "File persistence failed.",
                );
            }
            upload_offset = upload_offset.saturating_add(chunk.len() as i64);
        }
        let completion = match enc.finish(created) {
            Ok(v) => v,
            Err(_) => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "File completion failed.",
                )
            }
        };
        if !matches!(
            f.authority()
                .file_complete(a.account.clone(), id.clone(), completion)
                .await,
            Ok(true)
        ) {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                "File completion failed.",
            );
        }
    }
    match f.authority().file_get(a.account, id).await {
        Ok(Some(v)) => (StatusCode::OK, axum::Json(json!({"file":file_value(&v)}))).into_response(),
        _ => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "File authority unavailable.",
        ),
    }
}
const MAX_DOWNLOAD_RANGE_BYTES: i64 = 32 * 1024 * 1024;
fn parse_download_range(headers: &HeaderMap, size: i64) -> Result<Option<(i64, i64)>, Response> {
    let mut values = headers.get_all("range").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(range_error(size));
    }
    let Ok(value) = value.to_str() else {
        return Err(range_error(size));
    };
    let Some(spec) = value.strip_prefix("bytes=") else {
        return Err(range_error(size));
    };
    if spec.contains(',') {
        return Err(range_error(size));
    }
    let Some((start, end)) = spec.split_once('-') else {
        return Err(range_error(size));
    };
    let (Ok(start), Ok(end)) = (start.parse::<i64>(), end.parse::<i64>()) else {
        return Err(range_error(size));
    };
    if start < 0
        || end < start
        || start >= size
        || end >= size
        || end - start + 1 > MAX_DOWNLOAD_RANGE_BYTES
    {
        return Err(range_error(size));
    }
    Ok(Some((start, end)))
}
fn range_error(size: i64) -> Response {
    let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
        r.headers_mut().insert("content-range", v);
    }
    r
}
async fn file_download(
    f: Arc<GeminiBatchPublicFacade>,
    a: MeteredAuth,
    id: String,
    headers: HeaderMap,
) -> Response {
    let file = match f.authority().file_get(a.account.clone(), id.clone()).await {
        Ok(Some(file)) if file.state == "active" => file,
        _ => {
            return error(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "Requested entity was not found.",
            )
        }
    };
    let requested = match parse_download_range(&headers, file.size_bytes) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (start, end, status) = requested
        .map(|(start, end)| (start, end, StatusCode::PARTIAL_CONTENT))
        .unwrap_or((0, file.size_bytes.saturating_sub(1), StatusCode::OK));
    if file.size_bytes == 0 {
        let mut response = Response::new(Body::empty());
        response
            .headers_mut()
            .insert("accept-ranges", HeaderValue::from_static("bytes"));
        response
            .headers_mut()
            .insert("content-length", HeaderValue::from_static("0"));
        return response;
    }
    let account = a.account.clone();
    let file_id = id.clone();
    let keys = f.keys().clone();
    let authority = f.authority().clone();
    let stream = futures_util::stream::try_unfold(
        (authority, keys, account, file_id, None, 0i64, start, end),
        |(authority, keys, account, file_id, after, mut pos, start, end)| async move {
            if pos > end {
                return Ok::<_, std::io::Error>(None);
            }
            let page = authority
                .file_chunks(account.clone(), file_id.clone(), after, 4)
                .await
                .map_err(std::io::Error::other)?;
            if page.chunks.is_empty() {
                return Ok(None);
            }
            let mut emitted = Vec::new();
            let mut last = after;
            for chunk in page.chunks {
                let plain = keys
                    .decrypt_file_chunk(
                        &super::GeminiBatchFileChunkIdentity {
                            account_id: &account,
                            file_id: &file_id,
                            chunk_index: chunk.chunk_index,
                            schema_version: SCHEMA_VERSION,
                        },
                        &chunk,
                    )
                    .map_err(std::io::Error::other)?;
                let chunk_start = pos;
                let chunk_end = pos + plain.len() as i64 - 1;
                if chunk_end >= start && chunk_start <= end {
                    let from = (start - chunk_start).max(0) as usize;
                    let to = (end - chunk_start).min(plain.len() as i64 - 1) as usize + 1;
                    emitted.extend_from_slice(&plain[from..to]);
                }
                pos = chunk_end + 1;
                last = Some(chunk.chunk_index);
                if pos > end {
                    break;
                }
            }
            Ok(Some((
                bytes::Bytes::from(emitted),
                (authority, keys, account, file_id, last, pos, start, end),
            )))
        },
    );
    let mut response = Response::builder()
        .status(status)
        .body(Body::from_stream(stream))
        .unwrap();
    let h = response.headers_mut();
    h.insert(
        "content-type",
        file.mime_type
            .parse()
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert("accept-ranges", HeaderValue::from_static("bytes"));
    if let Ok(v) = HeaderValue::from_str(&(end - start + 1).to_string()) {
        h.insert("content-length", v);
    }
    if status == StatusCode::PARTIAL_CONTENT {
        if let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{end}/{}", file.size_bytes)) {
            h.insert("content-range", v);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::super::GeminiBatchDataKeyring;
    use super::*;
    #[test]
    fn closed() {
        assert!(parse(
            &Method::POST,
            "/v1beta/models/gemini-2.5-flash:batchGenerateContent"
        )
        .is_some());
        assert_eq!(
            parse(&Method::GET, "/v1beta/files/x:download"),
            Some(Route::Download("x".into()))
        );
        assert!(parse(&Method::GET, "/v1beta/batches/a/b").is_none());
    }

    #[test]
    fn terminal_payload_projection_preserves_exact_response_and_google_status() {
        let response = json!({
            "candidates":[{"content":{"role":"model","parts":[{"text":"six"}]},"finishReason":"STOP"}],
            "usageMetadata":{"promptTokenCount":9,"candidatesTokenCount":1,"totalTokenCount":10},
            "modelVersion":"gemini-2.5-flash"
        });
        assert_eq!(
            project_terminal_payload("result", response.clone()).unwrap(),
            json!({"response":response})
        );

        let status = json!({"code":429,"message":"quota exhausted","status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA"}]});
        assert_eq!(
            project_terminal_payload("error", json!({"error":status.clone()})).unwrap(),
            json!({"error":status})
        );
        assert!(project_terminal_payload("result", json!([1, 2])).is_err());
        assert!(project_terminal_payload("error", json!({"error":{"code":500}})).is_err());
    }

    #[test]
    fn terminal_blob_decryption_is_identity_bound_and_fail_closed() {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x33u8; 32]);
        let keyring = GeminiBatchDataKeyring::parse(&format!("test;test:{encoded}")).unwrap();
        let identity = GeminiBatchBlobIdentity {
            account_id: "account-a",
            job_id: "job-a",
            item_index: 0,
            kind: "result",
            schema_version: SCHEMA_VERSION,
        };
        let exact = json!({"candidates":[{"finishReason":"STOP"}]});
        let blob = keyring
            .encrypt_blob(&identity, &serde_json::to_vec(&exact).unwrap(), now() + 60)
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&keyring.decrypt_blob(&identity, &blob).unwrap())
                .unwrap(),
            exact
        );
        let wrong = GeminiBatchBlobIdentity {
            account_id: "account-b",
            ..identity
        };
        assert!(keyring.decrypt_blob(&wrong, &blob).is_err());
        let mut tampered = blob;
        tampered.ciphertext[0] ^= 1;
        assert!(keyring.decrypt_blob(&identity, &tampered).is_err());
    }

    #[test]
    fn range_parser_accepts_one_bounded_closed_range_and_rejects_others() {
        let mut headers = HeaderMap::new();
        headers.insert("range", HeaderValue::from_static("bytes=2-5"));
        assert_eq!(parse_download_range(&headers, 10).unwrap(), Some((2, 5)));
        headers.insert("range", HeaderValue::from_static("bytes=5-2"));
        assert_eq!(
            parse_download_range(&headers, 10).unwrap_err().status(),
            StatusCode::RANGE_NOT_SATISFIABLE
        );
        headers.insert("range", HeaderValue::from_static("bytes=0-1,4-5"));
        assert_eq!(
            parse_download_range(&headers, 10).unwrap_err().status(),
            StatusCode::RANGE_NOT_SATISFIABLE
        );
        headers.insert("range", HeaderValue::from_static("items=0-1"));
        let response = parse_download_range(&headers, 10).unwrap_err();
        assert_eq!(response.headers()["content-range"], "bytes */10");
    }

    #[test]
    fn file_name_requires_canonical_resource_name() {
        assert_eq!(file_id_from_name("files/file-1"), Some("file-1"));
        assert_eq!(file_id_from_name("file-1"), None);
        assert_eq!(file_id_from_name("files/a/b"), None);
        assert_eq!(file_id_from_name("files/"), None);
    }

    #[test]
    fn jsonl_parser_is_incremental_across_chunks_and_accepts_crlf() {
        let mut parser = JsonlParser::default();
        let mut entries = parser.push(b"{\"key\":\"a\",\"requ").unwrap();
        entries.extend(
            parser
                .push(b"est\":{\"contents\":[]}}\r\n\n{\"key\":\"b\",\"request\":{}}")
                .unwrap(),
        );
        entries.extend(parser.finish().unwrap());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["key"], "a");
        assert_eq!(entries[1]["request"], json!({}));
    }

    #[test]
    fn jsonl_parser_requires_key_and_request_on_every_nonempty_line() {
        let mut parser = JsonlParser::default();
        assert!(parser.push(b"{\"request\":{}}\n").is_err());

        let mut parser = JsonlParser::default();
        assert!(parser.push(b"{\"key\":\"a\",\"request\":1}\n").is_err());

        let mut parser = JsonlParser::default();
        assert!(parser.push(b"[]\n").is_err());
    }

    #[test]
    fn jsonl_parser_rejects_invalid_json_empty_input_and_oversized_line() {
        let mut parser = JsonlParser::default();
        assert_eq!(
            parser.push(b"not-json\n"),
            Err("A JSONL line is not valid JSON.")
        );

        let mut parser = JsonlParser::default();
        parser.push(b" \r\n\t\n").unwrap();
        assert!(parser.finish().is_err());

        let mut parser = JsonlParser::default();
        parser.pending.resize(JSONL_LINE_LIMIT, b'x');
        assert_eq!(parser.push(b"x"), Err("A JSONL line is too large."));
    }

    #[test]
    fn jsonl_parser_requires_nonempty_bounded_client_key() {
        let mut parser = JsonlParser::default();
        assert!(parser.push(b"{\"key\":\"\",\"request\":{}}\n").is_err());

        let key = "x".repeat(513);
        let line = format!("{{\"key\":\"{key}\",\"request\":{{}}}}\n");
        let mut parser = JsonlParser::default();
        assert!(parser.push(line.as_bytes()).is_err());
    }

    #[test]
    fn jsonl_parser_accepts_exactly_100k_and_rejects_100001() {
        let mut parser = JsonlParser::default();
        for index in 0..MAX_BATCH_ITEMS {
            let line = format!("{{\"key\":\"{index}\",\"request\":{{}}}}\n");
            let entries = parser.push(line.as_bytes()).unwrap();
            assert_eq!(entries.len(), 1);
        }
        assert!(parser.finish().unwrap().is_empty());
        assert_eq!(parser.item_count, MAX_BATCH_ITEMS);
        assert_eq!(
            parser.push(b"{\"key\":\"overflow\",\"request\":{}}\n"),
            Err("Batch contains too many requests.")
        );
    }

    #[test]
    fn stage5_synthetic_near_2gb_jsonl_is_streamed_in_order() {
        const FEED_CHUNK_BYTES: usize = 8 * 1024;
        const PHYSICAL_LINE_BYTES: usize = 96 * 1024;
        let logical_bytes = std::env::var("GEMINI_BATCH_STAGE5_LOGICAL_JSONL_BYTES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2_147_483_000);
        assert!(logical_bytes > 1_900_000_000);

        let mut parser = JsonlParser::default();
        let mut generated = 0u64;
        let mut sequence = 0usize;
        let mut line = Vec::with_capacity(PHYSICAL_LINE_BYTES + 256);
        let mut observed = 0usize;
        while generated < logical_bytes {
            line.clear();
            line.extend_from_slice(format!("{{\"key\":\"item-{sequence:08}\",\"request\":{{\"contents\":[],\"syntheticPadding\":\"").as_bytes());
            line.resize(line.len() + PHYSICAL_LINE_BYTES, b'x');
            line.extend_from_slice(b"\"}}\n");
            for chunk in line.chunks(FEED_CHUNK_BYTES) {
                for entry in parser.push(chunk).unwrap() {
                    assert_eq!(entry["key"], format!("item-{observed:08}"));
                    observed += 1;
                }
            }
            generated = generated.saturating_add(line.len() as u64);
            sequence += 1;
            if sequence >= MAX_BATCH_ITEMS {
                break;
            }
        }

        assert!(generated >= logical_bytes);
        assert!(sequence > 10_000);
        assert!(parser.peak_pending_bytes <= PHYSICAL_LINE_BYTES + 256);
        let trailing = parser.finish().unwrap();
        assert!(trailing.is_empty());
        assert_eq!(observed, sequence);
        assert_eq!(parser.item_count, sequence);
    }
}
