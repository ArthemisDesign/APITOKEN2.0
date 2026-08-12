use crate::codex_login;
use crate::db::{
    ExactRenewalEvent, ProxyBinding, ProxyLifecycleConflict, RenewalEventOutcome, RenewalRequest,
    RenewalRequestState, RenewalSelection, Store,
};
use crate::gemini_oauth;
use crate::iproyal::{ExtendOrderFailureClass, Iproyal, IspOrder};
use anyhow::{anyhow, bail, Result};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{
    sse::{Event, KeepAlive, Sse},
    IntoResponse, Response,
};
use axum::routing::{get, post};
use axum::{Json, Router};
use registry::authority::AuthorityConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_CODEX_RUNTIME_URL: &str = "http://127.0.0.1:8792/codex-subs";
pub const DEFAULT_GEMINI_RUNTIME_URL: &str = "http://127.0.0.1:8794/gemini-subs";
const RUNTIME_RESPONSE_LIMIT: usize = 1024 * 1024;
const RUNTIME_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
use tokio::sync::{broadcast, RwLock};
use zeroize::Zeroizing;

const SCHEMA_VERSION: u8 = 1;
const DEFAULT_BIND: &str = "127.0.0.1:8806";
const BODY_LIMIT: usize = 16 * 1024;
const MAX_CONTROL_KEY_BYTES: usize = 4096;
const MAX_RENEWAL_ITEMS: usize = 100;
const MAX_INVENTORY_ID_BYTES: usize = 160;
const GUARD_INTERVAL: Duration = Duration::from_secs(30 * 60);

pub fn parse_bind(raw: Option<&str>) -> Result<SocketAddr> {
    let value = raw.unwrap_or(DEFAULT_BIND);
    let bind = value
        .parse::<SocketAddr>()
        .map_err(|_| anyhow!("proxy admin bind must be a literal socket address"))?;
    if !bind.ip().is_loopback() || bind.port() == 0 {
        bail!("proxy admin bind must use a non-zero loopback port");
    }
    Ok(bind)
}

pub fn parse_runtime_url(
    raw: Option<&str>,
    default: &str,
    expected_path: &str,
) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw.unwrap_or(default))
        .map_err(|_| anyhow!("proxy admin runtime URL must be absolute HTTP"))?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != expected_path
    {
        bail!("proxy admin runtime URL is not an allowed loopback endpoint");
    }
    let host = url
        .host_str()
        .and_then(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .filter(std::net::IpAddr::is_loopback)
        .ok_or_else(|| anyhow!("proxy admin runtime URL must use a literal loopback IP"))?;
    if !host.is_loopback() || url.port().is_some_and(|port| port == 0) {
        bail!("proxy admin runtime URL must use a non-zero loopback port");
    }
    Ok(url)
}

pub(crate) struct ProxyAdminKey(Zeroizing<String>);

impl ProxyAdminKey {
    pub(crate) fn parse(mut bytes: Zeroizing<Vec<u8>>) -> Result<Self> {
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        if bytes.len() != 64
            || !bytes
                .iter()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            bail!(
                "proxy admin key file must contain exactly 64 lowercase hex bytes and optional LF"
            );
        }
        let value = String::from_utf8(std::mem::take(&mut *bytes))
            .map_err(|_| anyhow!("proxy admin key file is not valid ASCII"))?;
        Ok(Self(Zeroizing::new(value)))
    }
}

struct ControlKey(Zeroizing<String>);

impl ControlKey {
    fn new(value: String) -> Result<Self> {
        if value.is_empty() {
            bail!("proxy admin control key must not be empty");
        }
        if value.len() > MAX_CONTROL_KEY_BYTES {
            bail!("proxy admin control key exceeds the safety bound");
        }
        Ok(Self(Zeroizing::new(value)))
    }

    fn from_proxy_admin(value: ProxyAdminKey) -> Self {
        Self(value.0)
    }

    fn matches(&self, candidate: &[u8]) -> bool {
        constant_time_equal(self.0.as_bytes(), candidate)
    }
}

fn constant_time_equal(expected: &[u8], candidate: &[u8]) -> bool {
    let mut difference = expected.len() ^ candidate.len();
    for index in 0..MAX_CONTROL_KEY_BYTES {
        let left = expected.get(index).copied().unwrap_or(0);
        let right = candidate.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

pub struct Service {
    bind: SocketAddr,
    runtime: Arc<Runtime>,
}

impl Service {
    pub fn new(
        bind: SocketAddr,
        admin_key: ProxyAdminKey,
        runtime_control_key: String,
        store: Arc<Store>,
        iproyal: Option<Arc<Iproyal>>,
        authority: AuthorityConfig,
        fleet: String,
        codex: Option<codex_login::RosterConfig>,
        gemini: Option<gemini_oauth::Config>,
        codex_runtime_url: reqwest::Url,
        gemini_runtime_url: reqwest::Url,
    ) -> Result<Self> {
        if !bind.ip().is_loopback() || bind.port() == 0 {
            bail!("proxy admin bind must use a non-zero loopback port");
        }
        if !authority.is_postgres() {
            bail!("proxy admin authority must use PostgreSQL");
        }
        if fleet.is_empty() {
            bail!("proxy admin fleet must not be empty");
        }
        let runtime_client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(RUNTIME_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| anyhow!("proxy admin runtime client configuration failed"))?;
        let admin_key = ControlKey::from_proxy_admin(admin_key);
        let runtime_control_key = ControlKey::new(runtime_control_key)?;
        if admin_key.matches(runtime_control_key.0.as_bytes()) {
            bail!("proxy admin key must differ from the runtime control key");
        }
        Ok(Self {
            bind,
            runtime: Arc::new(Runtime {
                admin_key,
                runtime_control_key,
                store,
                iproyal,
                authority,
                fleet,
                codex,
                gemini,
                runtime_client,
                codex_runtime_url,
                gemini_runtime_url,
                provider_cache: RwLock::new(ProviderSnapshot::unavailable()),
                admin_changes: broadcast::channel(64).0,
                #[cfg(test)]
                test_local_projection: std::sync::RwLock::new(None),
            }),
        })
    }

    pub async fn run(self) -> Result<()> {
        let listener_runtime = self.runtime.clone();
        let actor_runtime = self.runtime;
        tokio::select! {
            _ = serve_listener(self.bind, listener_runtime) => {}
            _ = actor_loop(actor_runtime) => {}
        }
        Err(anyhow!("proxy admin runtime stopped"))
    }
}

struct Runtime {
    admin_key: ControlKey,
    runtime_control_key: ControlKey,
    store: Arc<Store>,
    iproyal: Option<Arc<Iproyal>>,
    authority: AuthorityConfig,
    fleet: String,
    codex: Option<codex_login::RosterConfig>,
    gemini: Option<gemini_oauth::Config>,
    runtime_client: reqwest::Client,
    codex_runtime_url: reqwest::Url,
    gemini_runtime_url: reqwest::Url,
    provider_cache: RwLock<ProviderSnapshot>,
    admin_changes: broadcast::Sender<AdminChange>,
    #[cfg(test)]
    test_local_projection: std::sync::RwLock<Option<LocalProjection>>,
}

fn router(runtime: Arc<Runtime>) -> Router {
    Router::new()
        .route("/proxy-admin/inventory", get(inventory_handler))
        .route("/proxy-admin/events", get(events_handler))
        .route("/proxy-admin/renew", post(renew_handler))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(runtime)
}

#[derive(Clone, Serialize)]
struct AdminChange {
    source: &'static str,
    resources: &'static [&'static str],
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    resync: bool,
}

impl AdminChange {
    fn inventory(reason: &'static str) -> Self {
        Self {
            source: "authbot",
            resources: &["/proxy-admin/inventory"],
            reason: Some(reason),
            resync: false,
        }
    }

    fn resync() -> Self {
        Self {
            source: "authbot",
            resources: &["/proxy-admin/inventory"],
            reason: None,
            resync: true,
        }
    }
}

async fn events_handler(State(runtime): State<Arc<Runtime>>, headers: HeaderMap) -> Response {
    if !authorized(&runtime, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
        );
    }
    let receiver = runtime.admin_changes.subscribe();
    let initial = futures_util::stream::once(async {
        Ok::<_, std::convert::Infallible>(admin_event(&AdminChange::resync(), "resync"))
    });
    let changes = futures_util::stream::unfold(receiver, |mut receiver| async move {
        let change = match receiver.recv().await {
            Ok(change) => change,
            Err(broadcast::error::RecvError::Lagged(_)) => AdminChange::resync(),
            Err(broadcast::error::RecvError::Closed) => return None,
        };
        let event = admin_event(&change, if change.resync { "resync" } else { "change" });
        Some((Ok::<_, std::convert::Infallible>(event), receiver))
    });
    use futures_util::StreamExt as _;
    Sse::new(initial.chain(changes))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(25))
                .text("heartbeat"),
        )
        .into_response()
}

fn admin_event(change: &AdminChange, event_type: &'static str) -> Event {
    Event::default()
        .event(event_type)
        .json_data(change)
        .expect("AdminChange always serializes")
}

async fn serve_listener(bind: SocketAddr, runtime: Arc<Runtime>) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| anyhow!("proxy admin listener failed"))?;
    axum::serve(listener, router(runtime))
        .await
        .map_err(|_| anyhow!("proxy admin listener failed"))
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

fn error_response(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (
        status,
        [(CACHE_CONTROL, "no-store")],
        Json(ErrorBody { code, message }),
    )
        .into_response()
}

fn json_response<T: Serialize>(status: StatusCode, value: T) -> Response {
    (status, [(CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

fn authorized(runtime: &Runtime, headers: &HeaderMap) -> bool {
    headers
        .get("x-proxy-admin-key")
        .map(|value| runtime.admin_key.matches(value.as_bytes()))
        .unwrap_or_else(|| runtime.admin_key.matches(&[]))
}

fn validated_actor(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("x-admin-actor")?;
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(*byte, b'@' | b'.' | b'_' | b':' | b'/' | b'+' | b'-')
        })
    {
        return None;
    }
    value.to_str().ok()
}

fn valid_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
        || !matches!(bytes[14], b'1'..=b'5')
        || !matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
    {
        return false;
    }
    bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}

fn valid_inventory_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INVENTORY_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Clone, PartialEq, Eq)]
struct ProviderSnapshot {
    inventory_ok: bool,
    orders: HashMap<i64, IspOrder>,
    balance_nano_usd: Option<String>,
    balance_observed_at: Option<i64>,
    cards_auto_extend: bool,
}

impl ProviderSnapshot {
    fn unavailable() -> Self {
        Self {
            inventory_ok: false,
            orders: HashMap::new(),
            balance_nano_usd: None,
            balance_observed_at: None,
            cards_auto_extend: false,
        }
    }

    fn auto_extend_enabled(&self) -> bool {
        self.cards_auto_extend || self.orders.values().any(|order| order.auto_extend)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Liveness {
    Live,
    Degraded,
    Dead,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BindingStatus {
    Bound,
    Unbound,
    Mismatch,
    Unknown,
}

#[derive(Serialize)]
struct ProviderView {
    provider: &'static str,
    balance_nano_usd: Option<String>,
    balance_observed_at: Option<i64>,
    auto_extend_enabled: bool,
}

#[derive(Serialize)]
struct InventoryItem {
    inventory_id: String,
    account_email: String,
    proxy_hint: String,
    order_hint: String,
    provider: String,
    subscription_plan: String,
    liveness: Liveness,
    subscription_expires_at: Option<i64>,
    proxy_expires_at: Option<i64>,
    binding_status: BindingStatus,
    renewable: bool,
    operator_renewable: bool,
    renew_block_code: Option<&'static str>,
}

#[derive(Serialize)]
struct InventoryResponse {
    schema_version: u8,
    observed_at: Option<i64>,
    providers: Vec<ProviderView>,
    items: Vec<InventoryItem>,
}

#[derive(Clone)]
struct SubscriptionProjection {
    provider: &'static str,
    binding_provider: &'static str,
    local_id: String,
    account_email: String,
    canonical_plan: String,
    issued_at: i64,
    expires_at: i64,
    liveness: Liveness,
    renewable_eligible: bool,
    canonical_ip: Option<std::net::IpAddr>,
    order_id: Option<i64>,
}

#[derive(Clone)]
struct SubscriptionSource {
    subscriptions: Vec<SubscriptionProjection>,
    source_ok: bool,
}

impl SubscriptionSource {
    fn unavailable() -> Self {
        Self {
            subscriptions: Vec::new(),
            source_ok: false,
        }
    }

    fn local_projection(&self) -> LocalProjection {
        LocalProjection {
            states: self
                .subscriptions
                .iter()
                .map(|subscription| {
                    (
                        subscription.local_id.clone(),
                        LocalState {
                            liveness: subscription.liveness,
                            renewable_eligible: subscription.renewable_eligible,
                            expires_at: subscription.expires_at,
                            canonical_ip: subscription.canonical_ip,
                            order_id: subscription.order_id,
                        },
                    )
                })
                .collect(),
            source_ok: self.source_ok,
        }
    }
}

#[derive(Clone, Copy)]
struct LocalState {
    liveness: Liveness,
    renewable_eligible: bool,
    expires_at: i64,
    canonical_ip: Option<std::net::IpAddr>,
    order_id: Option<i64>,
}

#[derive(Clone)]
struct LocalProjection {
    states: HashMap<String, LocalState>,
    source_ok: bool,
}

impl LocalProjection {
    fn unavailable() -> Self {
        Self {
            states: HashMap::new(),
            source_ok: false,
        }
    }

    fn state(&self, local_id: &str) -> Option<LocalState> {
        self.states.get(local_id).copied()
    }
}

async fn inventory_handler(State(runtime): State<Arc<Runtime>>, headers: HeaderMap) -> Response {
    if !authorized(&runtime, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
        );
    }
    json_response(StatusCode::OK, build_inventory(&runtime).await)
}

async fn build_inventory(runtime: &Arc<Runtime>) -> InventoryResponse {
    refresh_provider(runtime, false).await;
    let (claude, gpt, gemini) = tokio::join!(
        load_claude_projection(runtime.clone()),
        load_gpt_projection(runtime.clone()),
        load_gemini_projection(runtime.clone()),
    );
    let provider = runtime.provider_cache.read().await.clone();
    let mut subscriptions = Vec::new();
    subscriptions.extend(claude.subscriptions);
    subscriptions.extend(gpt.subscriptions);
    subscriptions.extend(gemini.subscriptions);
    reconcile_subscriptions(&runtime.store, &provider, &subscriptions);
    let bindings = runtime.store.list_proxy_bindings().unwrap_or_default();
    inventory_response(provider, bindings, subscriptions)
}

fn inventory_response(
    provider: ProviderSnapshot,
    bindings: Vec<ProxyBinding>,
    subscriptions: Vec<SubscriptionProjection>,
) -> InventoryResponse {
    let binding_by_local = bindings
        .iter()
        .map(|binding| {
            (
                (binding.provider.as_str(), binding.local_id.as_str()),
                binding,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut items = Vec::new();

    if provider.inventory_ok {
        for subscription in &subscriptions {
            let Some(ip) = subscription.canonical_ip else {
                continue;
            };
            if subscription.liveness == Liveness::Dead {
                continue;
            }
            let Some(binding) = binding_by_local
                .get(&(
                    subscription.binding_provider,
                    subscription.local_id.as_str(),
                ))
                .copied()
            else {
                continue;
            };
            if subscription
                .order_id
                .is_some_and(|order_id| order_id != binding.order_id)
                || binding.allocation_ip != Some(ip)
            {
                continue;
            }
            let Some(order) = provider.orders.get(&binding.order_id) else {
                continue;
            };
            if !order_contains_ip(order, ip) {
                continue;
            }
            items.push(project_subscription_item(subscription, binding, &provider));
        }
    }

    let auto_extend_enabled = provider.auto_extend_enabled();
    InventoryResponse {
        schema_version: SCHEMA_VERSION,
        observed_at: Some(unix_now()),
        providers: vec![ProviderView {
            provider: "iproyal",
            balance_nano_usd: provider.balance_nano_usd,
            balance_observed_at: provider.balance_observed_at,
            auto_extend_enabled,
        }],
        items,
    }
}

fn reconcile_subscriptions(
    store: &Store,
    provider: &ProviderSnapshot,
    subscriptions: &[SubscriptionProjection],
) {
    if !provider.inventory_ok {
        return;
    }
    let allocations = provider_allocations(provider);
    for subscription in subscriptions {
        let Some(ip) = subscription.canonical_ip else {
            continue;
        };
        let matched_order = match subscription.order_id {
            Some(order_id)
                if allocations
                    .iter()
                    .any(|candidate| *candidate == (order_id, ip)) =>
            {
                Some(order_id)
            }
            Some(_) => None,
            None => {
                let candidates = allocations
                    .iter()
                    .filter(|(_, candidate_ip)| *candidate_ip == ip)
                    .map(|(order_id, _)| *order_id)
                    .collect::<Vec<_>>();
                if candidates.len() == 1 {
                    candidates.first().copied()
                } else {
                    None
                }
            }
        };
        let Some(order_id) = matched_order else {
            continue;
        };
        let _ = store.upsert_proxy_binding_allocation(
            subscription.binding_provider,
            &subscription.local_id,
            order_id,
            &ip.to_string(),
            subscription.issued_at,
            crate::db::ProxyAuthorityStatus::Local,
        );
    }
}

fn renewable_guard(
    binding: &ProxyBinding,
    provider: &ProviderSnapshot,
    local: &LocalProjection,
    allow_inactive_subscription: bool,
) -> std::result::Result<i64, &'static str> {
    if !provider.inventory_ok || !local.source_ok {
        return Err("source_unavailable");
    }
    let Some(order) = provider.orders.get(&binding.order_id) else {
        return Err("binding_mismatch");
    };
    let Some(allocation_ip) = binding.allocation_ip else {
        return Err("binding_mismatch");
    };
    if !order_contains_ip(order, allocation_ip) {
        return Err("binding_mismatch");
    }
    let Some(state) = local.state(&binding.local_id) else {
        return Err("binding_mismatch");
    };
    if state.canonical_ip != Some(allocation_ip)
        || !state
            .order_id
            .is_none_or(|order_id| order_id == binding.order_id)
    {
        return Err("binding_mismatch");
    }
    if !matches!(binding.provider.as_str(), "claude" | "codex" | "gemini") {
        return Err("binding_mismatch");
    }
    if (!allow_inactive_subscription && state.expires_at <= unix_now())
        || !state.renewable_eligible
        || !matches!(state.liveness, Liveness::Live | Liveness::Degraded)
    {
        return Err("local_profile_inactive");
    }
    if !sanitized_order_active(order) {
        return Err("provider_order_inactive");
    }
    parse_expiry(&order.expire_date)
        .filter(|expiry| *expiry > unix_now())
        .ok_or("provider_order_invalid")
}

fn project_subscription_item(
    subscription: &SubscriptionProjection,
    binding: &ProxyBinding,
    provider: &ProviderSnapshot,
) -> InventoryItem {
    let ip = subscription
        .canonical_ip
        .expect("exact-bound subscription inventory has IP");
    let proxy_expires_at = provider
        .orders
        .get(&binding.order_id)
        .and_then(|order| parse_expiry(&order.expire_date));
    let local = LocalProjection {
        states: [(
            subscription.local_id.clone(),
            LocalState {
                liveness: subscription.liveness,
                renewable_eligible: subscription.renewable_eligible,
                expires_at: subscription.expires_at,
                canonical_ip: subscription.canonical_ip,
                order_id: subscription.order_id,
            },
        )]
        .into_iter()
        .collect(),
        source_ok: true,
    };
    let guard = renewable_guard(binding, provider, &local, false);
    let operator_guard = renewable_guard(binding, provider, &local, true);
    InventoryItem {
        inventory_id: binding.inventory_id.clone(),
        account_email: subscription.account_email.clone(),
        proxy_hint: proxy_hint(ip),
        order_hint: masked_order_hint(binding.order_id),
        provider: subscription.provider.to_string(),
        subscription_plan: canonical_plan(&subscription.canonical_plan),
        liveness: subscription.liveness,
        subscription_expires_at: Some(subscription.expires_at),
        proxy_expires_at,
        binding_status: BindingStatus::Bound,
        renewable: guard.is_ok(),
        operator_renewable: operator_guard.is_ok(),
        renew_block_code: guard.err(),
    }
}

fn provider_allocations(provider: &ProviderSnapshot) -> Vec<(i64, std::net::IpAddr)> {
    let mut allocations = provider
        .orders
        .values()
        .flat_map(|order| {
            order.ips.iter().filter_map(move |raw| {
                raw.parse::<std::net::IpAddr>()
                    .ok()
                    .map(|ip| (order.order_id, ip))
            })
        })
        .collect::<Vec<_>>();
    allocations.sort_unstable();
    allocations.dedup();
    allocations
}

fn order_contains_ip(order: &IspOrder, expected: std::net::IpAddr) -> bool {
    order
        .ips
        .iter()
        .filter_map(|raw| raw.parse::<std::net::IpAddr>().ok())
        .any(|ip| ip == expected)
}

fn proxy_hint(ip: std::net::IpAddr) -> String {
    let digest = format!("{:x}", Sha256::digest(ip.to_string().as_bytes()));
    format!("proxy-{}", &digest[..12])
}

fn canonical_plan(plan: &str) -> String {
    let plan = plan.trim();
    if plan.is_empty() {
        "unknown".to_string()
    } else {
        plan.to_string()
    }
}

fn validated_account_email(email: &str) -> Result<&str> {
    if email.is_empty() || email.len() > 254 || !email.is_ascii() {
        bail!("account email is outside the proxy admin safety bounds");
    }
    let Some((local, domain)) = email.split_once('@') else {
        bail!("account email is invalid for proxy admin projection");
    };
    if local.is_empty()
        || local.len() > 64
        || domain.is_empty()
        || domain.len() > 253
        || domain.contains('@')
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'!'
                        | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'/'
                        | b'='
                        | b'?'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'{'
                        | b'|'
                        | b'}'
                        | b'~'
                        | b'-'
                )
        })
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        bail!("account email is invalid for proxy admin projection");
    }
    Ok(email)
}

fn masked_order_hint(order_id: i64) -> String {
    let decimal = order_id.to_string();
    if decimal.len() <= 4 {
        return format!("order-{}d", decimal.len());
    }
    format!("••••{}", &decimal[decimal.len() - 4..])
}

fn sanitized_order_active(order: &IspOrder) -> bool {
    matches!(
        order.status.to_ascii_lowercase().as_str(),
        "active" | "confirmed" | "completed"
    )
}

async fn load_claude_projection(runtime: Arc<Runtime>) -> SubscriptionSource {
    let authority = runtime.authority.clone();
    let fleet = runtime.fleet.clone();
    match tokio::task::spawn_blocking(move || {
        let mut authority = authority.connect()?;
        let rows = authority.load_claude_lifecycle()?;
        rows.into_iter()
            .filter(|row| row.fleet == fleet)
            .map(|row| {
                validated_account_email(&row.email)?;
                let liveness = if row.status != "active" || !row.has_token {
                    Liveness::Dead
                } else {
                    match row.auth_state.as_str() {
                        "healthy" => Liveness::Live,
                        "suspect" => Liveness::Degraded,
                        "dead" => Liveness::Dead,
                        _ => Liveness::Unknown,
                    }
                };
                let renewable_eligible = matches!(liveness, Liveness::Live | Liveness::Degraded);
                Ok(SubscriptionProjection {
                    provider: "claude",
                    binding_provider: "claude",
                    local_id: opaque_claude_local_id(&row.email),
                    account_email: row.email,
                    canonical_plan: canonical_plan(&row.plan),
                    issued_at: row.added_ts.max(1),
                    expires_at: row.added_ts.max(1).saturating_add(30 * 86_400),
                    liveness,
                    renewable_eligible,
                    canonical_ip: literal_proxy_ip(&row.proxy),
                    order_id: None,
                })
            })
            .collect::<Result<Vec<_>>>()
    })
    .await
    {
        Ok(Ok(subscriptions)) => SubscriptionSource {
            subscriptions,
            source_ok: true,
        },
        Ok(Err(_)) | Err(_) => SubscriptionSource::unavailable(),
    }
}

async fn load_gpt_projection(runtime: Arc<Runtime>) -> SubscriptionSource {
    let Some(config) = runtime.codex.clone() else {
        return SubscriptionSource::unavailable();
    };
    let Ok(mut subscriptions) =
        read_roster_projection(move || config.lifecycle_profiles(), "gpt", "codex", false).await
    else {
        return SubscriptionSource::unavailable();
    };
    let response =
        fetch_runtime::<CodexRuntimeResponse>(&runtime, &runtime.codex_runtime_url).await;
    match response.and_then(|response| join_codex_runtime(&mut subscriptions, response)) {
        Ok(()) => SubscriptionSource {
            subscriptions,
            source_ok: true,
        },
        Err(_) => SubscriptionSource::unavailable(),
    }
}

async fn load_gemini_projection(runtime: Arc<Runtime>) -> SubscriptionSource {
    let Some(config) = runtime.gemini.clone() else {
        return SubscriptionSource::unavailable();
    };
    let Ok(mut subscriptions) = read_roster_projection(
        move || config.lifecycle_profiles(),
        "gemini",
        "gemini",
        true,
    )
    .await
    else {
        return SubscriptionSource::unavailable();
    };
    let response =
        fetch_runtime::<GeminiRuntimeResponse>(&runtime, &runtime.gemini_runtime_url).await;
    match response.and_then(|response| join_gemini_runtime(&mut subscriptions, response)) {
        Ok(()) => SubscriptionSource {
            subscriptions,
            source_ok: true,
        },
        Err(_) => SubscriptionSource::unavailable(),
    }
}

async fn load_binding_projection(runtime: Arc<Runtime>, binding: &ProxyBinding) -> LocalProjection {
    #[cfg(test)]
    if let Some(projection) = runtime.test_local_projection.read().unwrap().clone() {
        return projection;
    }
    match binding.provider.as_str() {
        "claude" => load_claude_projection(runtime).await.local_projection(),
        "codex" => load_gpt_projection(runtime).await.local_projection(),
        "gemini" => load_gemini_projection(runtime).await.local_projection(),
        _ => LocalProjection::unavailable(),
    }
}

async fn read_roster_projection<F, T>(
    read: F,
    provider: &'static str,
    binding_provider: &'static str,
    gemini_calendar_expiry: bool,
) -> Result<Vec<SubscriptionProjection>>
where
    F: FnOnce() -> Result<Vec<T>> + Send + 'static,
    T: RosterProjection + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        read()?
            .into_iter()
            .map(|profile| {
                let issued_at = profile.issued_at().max(1);
                let plan = canonical_plan(profile.plan());
                let account_email = validated_account_email(profile.account_email())?.to_string();
                let expires_at = if gemini_calendar_expiry && plan == "google_ai_pro" {
                    add_calendar_months_utc(issued_at, 18).unwrap_or(issued_at)
                } else {
                    issued_at.saturating_add(30 * 86_400)
                };
                Ok(SubscriptionProjection {
                    provider,
                    binding_provider,
                    local_id: profile.profile_id().to_string(),
                    account_email,
                    canonical_plan: plan,
                    issued_at,
                    expires_at,
                    liveness: Liveness::Unknown,
                    renewable_eligible: false,
                    canonical_ip: profile.canonical_ip(),
                    order_id: (profile.order_id() > 0).then_some(profile.order_id()),
                })
            })
            .collect()
    })
    .await
    .map_err(|_| anyhow!("proxy admin roster task failed"))?
}

#[derive(Deserialize)]
struct CodexRuntimeResponse {
    homes: Vec<CodexRuntimeHome>,
}

#[derive(Deserialize)]
struct CodexRuntimeHome {
    id: String,
    account_state: String,
}

#[derive(Deserialize)]
struct GeminiRuntimeResponse {
    profiles: Vec<GeminiRuntimeProfile>,
}

#[derive(Deserialize)]
struct GeminiRuntimeProfile {
    id: String,
    authenticated: bool,
    disabled: bool,
}

async fn fetch_runtime<T: serde::de::DeserializeOwned>(
    runtime: &Runtime,
    url: &reqwest::Url,
) -> Result<T> {
    let response = runtime
        .runtime_client
        .get(url.clone())
        .header("x-api-key", runtime.runtime_control_key.0.as_str())
        .send()
        .await
        .map_err(|_| anyhow!("proxy admin runtime request failed"))?;
    if !response.status().is_success() {
        bail!("proxy admin runtime returned a non-success status");
    }
    if response
        .content_length()
        .is_some_and(|length| length > RUNTIME_RESPONSE_LIMIT as u64)
    {
        bail!("proxy admin runtime response exceeds the safety bound");
    }
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("proxy admin runtime response read failed"))?
    {
        if bytes.len().saturating_add(chunk.len()) > RUNTIME_RESPONSE_LIMIT {
            bail!("proxy admin runtime response exceeds the safety bound");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("proxy admin runtime response is malformed"))
}

#[derive(Clone, Copy)]
struct RuntimeStatus {
    liveness: Liveness,
    renewable_eligible: bool,
}

fn join_codex_runtime(
    subscriptions: &mut [SubscriptionProjection],
    response: CodexRuntimeResponse,
) -> Result<()> {
    let mut states = HashMap::new();
    for home in response.homes {
        if home.id.is_empty() || home.account_state.is_empty() || states.contains_key(&home.id) {
            bail!("Codex runtime response is malformed");
        }
        let state = match home.account_state.as_str() {
            "healthy" => RuntimeStatus {
                liveness: Liveness::Live,
                renewable_eligible: true,
            },
            "suspect" => RuntimeStatus {
                liveness: Liveness::Degraded,
                renewable_eligible: true,
            },
            "dead" => RuntimeStatus {
                liveness: Liveness::Dead,
                renewable_eligible: false,
            },
            _ => bail!("Codex runtime response is malformed"),
        };
        states.insert(home.id, state);
    }
    apply_runtime_states(subscriptions, states)
}

fn join_gemini_runtime(
    subscriptions: &mut [SubscriptionProjection],
    response: GeminiRuntimeResponse,
) -> Result<()> {
    let mut states = HashMap::new();
    for profile in response.profiles {
        if profile.id.is_empty() || states.contains_key(&profile.id) {
            bail!("Gemini runtime response is malformed");
        }
        let state = if !profile.authenticated {
            RuntimeStatus {
                liveness: Liveness::Dead,
                renewable_eligible: false,
            }
        } else if profile.disabled {
            RuntimeStatus {
                liveness: Liveness::Degraded,
                renewable_eligible: false,
            }
        } else {
            RuntimeStatus {
                liveness: Liveness::Live,
                renewable_eligible: true,
            }
        };
        states.insert(profile.id, state);
    }
    apply_runtime_states(subscriptions, states)
}

fn apply_runtime_states(
    subscriptions: &mut [SubscriptionProjection],
    mut states: HashMap<String, RuntimeStatus>,
) -> Result<()> {
    for subscription in subscriptions {
        let state = states
            .remove(&subscription.local_id)
            .ok_or_else(|| anyhow!("runtime response is missing a roster profile"))?;
        subscription.liveness = state.liveness;
        subscription.renewable_eligible = state.renewable_eligible;
    }
    Ok(())
}

trait RosterProjection {
    fn profile_id(&self) -> &str;
    fn account_email(&self) -> &str;
    fn order_id(&self) -> i64;
    fn issued_at(&self) -> i64;
    fn plan(&self) -> &str;
    fn canonical_ip(&self) -> Option<std::net::IpAddr>;
}

impl RosterProjection for codex_login::LifecycleProfile {
    fn profile_id(&self) -> &str {
        &self.profile_id
    }
    fn account_email(&self) -> &str {
        &self.account_email
    }
    fn order_id(&self) -> i64 {
        self.order_id
    }
    fn issued_at(&self) -> i64 {
        self.issued_at
    }
    fn plan(&self) -> &str {
        &self.canonical_plan
    }
    fn canonical_ip(&self) -> Option<std::net::IpAddr> {
        self.canonical_ip
    }
}

impl RosterProjection for gemini_oauth::LifecycleProfile {
    fn profile_id(&self) -> &str {
        &self.profile_id
    }
    fn account_email(&self) -> &str {
        &self.account_email
    }
    fn order_id(&self) -> i64 {
        self.order_id
    }
    fn issued_at(&self) -> i64 {
        self.issued_at
    }
    fn plan(&self) -> &str {
        &self.canonical_plan
    }
    fn canonical_ip(&self) -> Option<std::net::IpAddr> {
        self.canonical_ip
    }
}

fn literal_proxy_ip(proxy: &str) -> Option<std::net::IpAddr> {
    reqwest::Url::parse(proxy)
        .ok()?
        .host_str()?
        .trim_matches(['[', ']'])
        .parse()
        .ok()
}

fn opaque_claude_local_id(email: &str) -> String {
    format!("claude_{:x}", Sha256::digest(email.as_bytes()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenewBody {
    idempotency_key: String,
    inventory_ids: Vec<String>,
    #[serde(default)]
    allow_inactive_subscription: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RenewItemStatus {
    Renewed,
    Failed,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RenewStatus {
    Succeeded,
    Partial,
    Failed,
    Uncertain,
}

#[derive(Serialize)]
struct RenewResult {
    inventory_id: String,
    status: RenewItemStatus,
    proxy_expires_at: Option<i64>,
    result_code: Option<&'static str>,
}

#[derive(Serialize)]
struct RenewResponse {
    schema_version: u8,
    idempotency_key: String,
    idempotent_replay: bool,
    status: RenewStatus,
    observed_at: Option<i64>,
    results: Vec<RenewResult>,
}

async fn renew_handler(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
    body: std::result::Result<Json<RenewBody>, JsonRejection>,
) -> Response {
    if !authorized(&runtime, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
        );
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "request body is invalid",
            )
        }
    };
    let Some(actor) = validated_actor(&headers) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_actor",
            "X-Admin-Actor is required and invalid",
        );
    };
    if !valid_uuid(&body.idempotency_key) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_idempotency_key",
            "idempotency_key must be a UUID",
        );
    }
    let canonical_ids = match validate_inventory_selection(&body.inventory_ids) {
        Ok(ids) => ids,
        Err(code) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                code,
                "inventory selection is invalid",
            )
        }
    };

    let existing = match runtime
        .store
        .get_renewal_request_by_key(&body.idempotency_key)
    {
        Ok(request) => request,
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "renewal_store_unavailable",
                "renewal request store is unavailable",
            )
        }
    };
    let (request, replay) =
        if let Some(request) = existing {
            if request.inventory_ids != canonical_ids {
                return error_response(
                    StatusCode::CONFLICT,
                    "idempotency_conflict",
                    "idempotency key belongs to another inventory selection",
                );
            }
            if request.state != RenewalRequestState::Pending {
                return replay_response(&runtime.store, request, true);
            }
            (request, true)
        } else {
            let mut selections = Vec::with_capacity(canonical_ids.len());
            for inventory_id in &canonical_ids {
                match runtime
                    .store
                    .get_proxy_binding_by_inventory_id(inventory_id)
                {
                    Ok(Some(binding)) => {
                        let Some(allocation_ip) = binding.allocation_ip else {
                            return error_response(
                                StatusCode::BAD_REQUEST,
                                "unknown_inventory_id",
                                "inventory selection contains an unknown id",
                            );
                        };
                        selections.push(RenewalSelection {
                            inventory_id: inventory_id.clone(),
                            order_id: binding.order_id,
                            allocation_ip,
                            allow_inactive_subscription: body.allow_inactive_subscription,
                        });
                    }
                    Ok(None) | Err(_) => {
                        return error_response(
                            StatusCode::BAD_REQUEST,
                            "unknown_inventory_id",
                            "inventory selection contains an unknown id",
                        )
                    }
                }
            }
            let request =
                match runtime.store.create_or_get_renewal_request_exact(
                    &body.idempotency_key,
                    &selections,
                    actor,
                ) {
                    Ok(request) => request,
                    Err(error)
                        if error.downcast_ref::<ProxyLifecycleConflict>().is_some_and(
                            |conflict| *conflict == ProxyLifecycleConflict::IdempotencyKeyReused,
                        ) =>
                    {
                        return error_response(
                            StatusCode::CONFLICT,
                            "idempotency_conflict",
                            "idempotency key belongs to another inventory selection",
                        )
                    }
                    Err(error)
                        if error.downcast_ref::<ProxyLifecycleConflict>().is_some_and(
                            |conflict| *conflict == ProxyLifecycleConflict::RenewalSelectionBusy,
                        ) =>
                    {
                        return error_response(
                            StatusCode::CONFLICT,
                            "renewal_selection_busy",
                            "inventory selection overlaps a queued or active renewal request",
                        )
                    }
                    Err(_) => {
                        return error_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "renewal_store_unavailable",
                            "renewal request could not be stored",
                        )
                    }
                };
            (request, false)
        };
    let claimed = match runtime.store.claim_renewal_request(request.id) {
        Ok(Some(claimed)) => claimed,
        Ok(None) => {
            return match runtime.store.get_renewal_request(request.id) {
                Ok(Some(current)) => replay_response(&runtime.store, current, true),
                Ok(None) | Err(_) => json_response(
                    StatusCode::OK,
                    uncertain_response(&request, true, "request_in_progress"),
                ),
            }
        }
        Err(error)
            if error
                .downcast_ref::<ProxyLifecycleConflict>()
                .is_some_and(|conflict| {
                    *conflict == ProxyLifecycleConflict::RenewalSelectionBusy
                }) =>
        {
            return error_response(
                StatusCode::CONFLICT,
                "renewal_selection_busy",
                "inventory selection overlaps a queued or active renewal request",
            )
        }
        Err(_) => {
            return json_response(
                StatusCode::OK,
                uncertain_response(&request, replay, "claim_uncertain"),
            )
        }
    };

    process_request(&runtime, claimed.clone()).await;
    let _ = runtime
        .admin_changes
        .send(AdminChange::inventory("renewal_finished"));
    match runtime.store.get_renewal_request(claimed.id) {
        Ok(Some(current)) => replay_response(&runtime.store, current, replay),
        Ok(None) | Err(_) => json_response(
            StatusCode::OK,
            uncertain_response(&claimed, replay, "result_unavailable"),
        ),
    }
}

fn validate_inventory_selection(
    values: &[String],
) -> std::result::Result<Vec<String>, &'static str> {
    if values.is_empty() || values.len() > MAX_RENEWAL_ITEMS {
        return Err("invalid_inventory_count");
    }
    if values.iter().any(|value| !valid_inventory_id(value)) {
        return Err("invalid_inventory_id");
    }
    let mut canonical = values.to_vec();
    canonical.sort();
    if canonical.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("duplicate_inventory_id");
    }
    Ok(canonical)
}

fn replay_response(store: &Store, request: RenewalRequest, replay: bool) -> Response {
    let events = match store.get_exact_renewal_events(request.id) {
        Ok(events) => events,
        Err(_) => {
            return json_response(
                StatusCode::OK,
                uncertain_response(&request, replay, "result_unavailable"),
            )
        }
    };
    json_response(StatusCode::OK, renewal_response(&request, &events, replay))
}

fn uncertain_response(
    request: &RenewalRequest,
    replay: bool,
    result_code: &'static str,
) -> RenewResponse {
    RenewResponse {
        schema_version: SCHEMA_VERSION,
        idempotency_key: request.idempotency_key.clone(),
        idempotent_replay: replay,
        status: RenewStatus::Uncertain,
        observed_at: Some(request.updated_at),
        results: request
            .inventory_ids
            .iter()
            .map(|inventory_id| RenewResult {
                inventory_id: inventory_id.clone(),
                status: RenewItemStatus::Uncertain,
                proxy_expires_at: None,
                result_code: Some(result_code),
            })
            .collect(),
    }
}

fn renewal_response(
    request: &RenewalRequest,
    events: &[ExactRenewalEvent],
    replay: bool,
) -> RenewResponse {
    let nonterminal = matches!(
        request.state,
        RenewalRequestState::Pending | RenewalRequestState::InProgress
    );
    let by_inventory = events
        .iter()
        .map(|event| (event.inventory_id.as_str(), event))
        .collect::<HashMap<_, _>>();
    let results = request
        .inventory_ids
        .iter()
        .map(
            |inventory_id| match by_inventory.get(inventory_id.as_str()).copied() {
                Some(event) => result_from_event(inventory_id.clone(), event),
                None => RenewResult {
                    inventory_id: inventory_id.clone(),
                    status: RenewItemStatus::Uncertain,
                    proxy_expires_at: None,
                    result_code: Some("operation_not_observed"),
                },
            },
        )
        .collect::<Vec<_>>();
    let renewed = results
        .iter()
        .filter(|result| result.status == RenewItemStatus::Renewed)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.status == RenewItemStatus::Failed)
        .count();
    let uncertain = results.len().saturating_sub(renewed + failed);
    let status =
        if nonterminal || request.state == RenewalRequestState::Indeterminate || uncertain > 0 {
            RenewStatus::Uncertain
        } else if renewed == results.len() {
            RenewStatus::Succeeded
        } else if failed == results.len() {
            RenewStatus::Failed
        } else {
            RenewStatus::Partial
        };
    RenewResponse {
        schema_version: SCHEMA_VERSION,
        idempotency_key: request.idempotency_key.clone(),
        idempotent_replay: replay,
        status,
        observed_at: Some(request.updated_at),
        results,
    }
}

fn result_from_event(inventory_id: String, exact: &ExactRenewalEvent) -> RenewResult {
    let event = &exact.event;
    let (status, result_code) = match event.outcome {
        RenewalEventOutcome::Renewed => (RenewItemStatus::Renewed, None),
        RenewalEventOutcome::Unchanged => {
            (RenewItemStatus::Failed, Some("provider_order_inactive"))
        }
        RenewalEventOutcome::NotFound => {
            (RenewItemStatus::Failed, Some("provider_order_not_found"))
        }
        RenewalEventOutcome::Rejected => (RenewItemStatus::Failed, Some("binding_mismatch")),
        RenewalEventOutcome::ProviderRejected => {
            (RenewItemStatus::Failed, Some("provider_renewal_rejected"))
        }
        RenewalEventOutcome::LocalProfileInactive => {
            (RenewItemStatus::Failed, Some("local_profile_inactive"))
        }
        RenewalEventOutcome::ProviderUnavailable => {
            (RenewItemStatus::Failed, Some("source_unavailable"))
        }
        RenewalEventOutcome::Indeterminate => (
            RenewItemStatus::Uncertain,
            Some("provider_result_uncertain"),
        ),
    };
    RenewResult {
        inventory_id,
        status,
        proxy_expires_at: event.new_expiry_at,
        result_code,
    }
}

async fn process_request(runtime: &Arc<Runtime>, request: RenewalRequest) {
    let selections = match runtime.store.get_renewal_selections(request.id) {
        Ok(selections) => selections,
        Err(_) => {
            let _ = runtime.store.indeterminate_renewal_request(request.id);
            return;
        }
    };
    let Some(client) = runtime.iproyal.clone() else {
        if !record_events(
            &runtime.store,
            request.id,
            &selections,
            RenewalEventOutcome::ProviderUnavailable,
            None,
        ) {
            let _ = runtime.store.indeterminate_renewal_request(request.id);
            return;
        }
        let _ = runtime.store.fail_renewal_request(request.id);
        return;
    };

    let inventory = match client.isp_inventory().await {
        Ok(inventory) => inventory,
        Err(_) => {
            if !record_events(
                &runtime.store,
                request.id,
                &selections,
                RenewalEventOutcome::ProviderUnavailable,
                None,
            ) {
                let _ = runtime.store.indeterminate_renewal_request(request.id);
                return;
            }
            let _ = runtime.store.fail_renewal_request(request.id);
            return;
        }
    };
    let provider = ProviderSnapshot {
        inventory_ok: true,
        orders: inventory
            .into_iter()
            .map(|order| (order.order_id, order))
            .collect(),
        balance_nano_usd: None,
        balance_observed_at: None,
        cards_auto_extend: false,
    };
    let mut groups = HashMap::<i64, Vec<RenewalSelection>>::new();
    for selection in selections {
        groups
            .entry(selection.order_id)
            .or_default()
            .push(selection);
    }

    let mut any_failed = false;
    for (order_id, group) in groups {
        let mut outcome = None;
        for selection in &group {
            let binding = match runtime
                .store
                .get_proxy_binding_by_inventory_id(&selection.inventory_id)
            {
                Ok(Some(binding))
                    if binding.order_id == order_id
                        && binding.allocation_ip == Some(selection.allocation_ip) =>
                {
                    binding
                }
                _ => {
                    outcome = Some(RenewalEventOutcome::Rejected);
                    break;
                }
            };
            let local = load_binding_projection(runtime.clone(), &binding).await;
            if let Err(code) = renewable_guard(
                &binding,
                &provider,
                &local,
                selection.allow_inactive_subscription,
            ) {
                outcome = Some(match code {
                    "source_unavailable" => RenewalEventOutcome::ProviderUnavailable,
                    "local_profile_inactive" => RenewalEventOutcome::LocalProfileInactive,
                    "provider_order_inactive" | "provider_order_invalid" => {
                        RenewalEventOutcome::Unchanged
                    }
                    _ => RenewalEventOutcome::Rejected,
                });
                break;
            }
        }
        if let Some(outcome) = outcome {
            any_failed = true;
            if !record_events(&runtime.store, request.id, &group, outcome, None) {
                let _ = runtime.store.indeterminate_renewal_request(request.id);
                return;
            }
            continue;
        }

        let ips = group
            .iter()
            .map(|selection| selection.allocation_ip.to_string())
            .collect::<Vec<_>>();
        let expiry = match client.extend_order_ips(order_id, &ips).await {
            Ok(expiry) => match parse_expiry(&expiry).filter(|expiry| *expiry > 0) {
                Some(expiry) => expiry,
                None => {
                    elog::warn(
                        "authbot",
                        "[proxy-renewal] outcome=uncertain phase=expiry_projection",
                    );
                    let _ = record_events(
                        &runtime.store,
                        request.id,
                        &group,
                        RenewalEventOutcome::Indeterminate,
                        None,
                    );
                    let _ = runtime.store.indeterminate_renewal_request(request.id);
                    return;
                }
            },
            Err(error) if error.class() == ExtendOrderFailureClass::Rejected => {
                elog::warn(
                    "authbot",
                    format!("[proxy-renewal] outcome=rejected phase={}", error.phase()),
                );
                any_failed = true;
                if !record_events(
                    &runtime.store,
                    request.id,
                    &group,
                    RenewalEventOutcome::ProviderRejected,
                    None,
                ) {
                    let _ = runtime.store.indeterminate_renewal_request(request.id);
                    return;
                }
                continue;
            }
            Err(error) => {
                elog::warn(
                    "authbot",
                    format!("[proxy-renewal] outcome=uncertain phase={}", error.phase()),
                );
                let _ = record_events(
                    &runtime.store,
                    request.id,
                    &group,
                    RenewalEventOutcome::Indeterminate,
                    None,
                );
                let _ = runtime.store.indeterminate_renewal_request(request.id);
                return;
            }
        };
        if !record_events(
            &runtime.store,
            request.id,
            &group,
            RenewalEventOutcome::Renewed,
            Some(expiry),
        ) {
            let _ = runtime.store.indeterminate_renewal_request(request.id);
            return;
        }
    }

    if any_failed {
        let _ = runtime.store.fail_renewal_request(request.id);
    } else {
        let _ = runtime.store.complete_renewal_request(request.id);
    }
}

fn record_events(
    store: &Store,
    request_id: i64,
    selections: &[RenewalSelection],
    outcome: RenewalEventOutcome,
    new_expiry_at: Option<i64>,
) -> bool {
    let observed_at = unix_now();
    selections.iter().all(|selection| {
        store
            .record_renewal_event_for_inventory(
                request_id,
                &selection.inventory_id,
                outcome,
                observed_at,
                new_expiry_at,
            )
            .is_ok()
    })
}

async fn actor_loop(runtime: Arc<Runtime>) -> Result<()> {
    refresh_provider(&runtime, true).await;
    let mut interval = tokio::time::interval(GUARD_INTERVAL);
    interval.tick().await;
    loop {
        interval.tick().await;
        refresh_provider(&runtime, true).await;
    }
}

async fn refresh_provider(runtime: &Arc<Runtime>, guard_auto_extend: bool) {
    let Some(client) = runtime.iproyal.clone() else {
        *runtime.provider_cache.write().await = ProviderSnapshot::unavailable();
        return;
    };
    let (inventory_result, balance_result, cards_result) = tokio::join!(
        client.isp_inventory(),
        client.balance(),
        client.cards_warning(),
    );
    let observed_at = unix_now();
    let (mut orders, mut inventory_ok) = match inventory_result {
        Ok(orders) => (orders, true),
        Err(_) => (Vec::new(), false),
    };
    if guard_auto_extend && inventory_ok {
        let guarded = orders
            .iter()
            .filter(|order| order.auto_extend)
            .map(|order| order.order_id)
            .collect::<Vec<_>>();
        for order_id in &guarded {
            if client.ensure_auto_extend_disabled(*order_id).await.is_err() {
                inventory_ok = false;
            }
        }
        if !guarded.is_empty() {
            match client.isp_inventory().await {
                Ok(refreshed) => orders = refreshed,
                Err(_) => inventory_ok = false,
            }
        }
    }
    let (balance_nano_usd, balance_observed_at) = match balance_result {
        Ok(balance) => (Some(balance), Some(observed_at)),
        Err(_) => (None, None),
    };
    let cards_auto_extend = cards_result.map(|cards| cards.warning).unwrap_or(false);
    let snapshot = ProviderSnapshot {
        inventory_ok,
        orders: orders
            .into_iter()
            .map(|order| (order.order_id, order))
            .collect(),
        balance_nano_usd,
        balance_observed_at,
        cards_auto_extend,
    };
    let changed = {
        let mut cached = runtime.provider_cache.write().await;
        let changed = *cached != snapshot;
        *cached = snapshot;
        changed
    };
    if changed {
        let _ = runtime
            .admin_changes
            .send(AdminChange::inventory("provider_inventory"));
    }
}

fn parse_expiry(value: &str) -> Option<i64> {
    let value = value.trim();
    let date_time = value.strip_suffix('Z').unwrap_or(value);
    let (date, time) = match date_time.split_once(['T', ' ']) {
        Some(parts) => parts,
        None => (date_time, "00:00:00"),
    };
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    let second_text = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let second = second_text.split('.').next()?.parse::<i64>().ok()?;
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146097 + day_of_era - 719468;
    days.checked_mul(86400)?
        .checked_add(hour * 3600 + minute * 60 + second)
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

fn add_calendar_months_utc(timestamp: i64, months: i64) -> Option<i64> {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let z = days.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);

    let total_month = year
        .checked_mul(12)?
        .checked_add(month - 1)?
        .checked_add(months)?;
    let target_year = total_month.div_euclid(12);
    let target_month = total_month.rem_euclid(12) + 1;
    let target_day = day.min(days_in_month(target_year, target_month));
    let adjusted_year = target_year - i64::from(target_month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = if target_month > 2 {
        target_month - 3
    } else {
        target_month + 9
    };
    let day_of_year = (153 * shifted_month + 2) / 5 + target_day - 1;
    let target_days = era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + day_of_year
        - 719_468;
    target_days.checked_mul(86_400)?.checked_add(seconds)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::routing::get;
    use serde_json::{json, Value};
    use std::collections::{BTreeSet, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const UUID_A: &str = "123e4567-e89b-12d3-a456-426614174000";
    const UUID_B: &str = "123e4567-e89b-42d3-b456-426614174001";
    const UUID_C: &str = "123e4567-e89b-42d3-b456-426614174002";

    fn runtime(iproyal: Option<Arc<Iproyal>>) -> Arc<Runtime> {
        Arc::new(Runtime {
            admin_key: ControlKey::new("test-admin-key".to_string()).unwrap(),
            runtime_control_key: ControlKey::new("test-runtime-control-key".to_string()).unwrap(),
            store: Arc::new(Store::open(":memory:").unwrap()),
            iproyal,
            authority: AuthorityConfig::Sqlite {
                path: ":memory:".to_string(),
            },
            fleet: "prod".to_string(),
            codex: None,
            gemini: None,
            runtime_client: reqwest::Client::builder().no_proxy().build().unwrap(),
            codex_runtime_url: reqwest::Url::parse(DEFAULT_CODEX_RUNTIME_URL).unwrap(),
            gemini_runtime_url: reqwest::Url::parse(DEFAULT_GEMINI_RUNTIME_URL).unwrap(),
            provider_cache: RwLock::new(ProviderSnapshot::unavailable()),
            admin_changes: broadcast::channel(16).0,
            test_local_projection: std::sync::RwLock::new(None),
        })
    }

    fn local_projection(states: &[(&str, Liveness, bool)]) -> LocalProjection {
        exact_local_projection(
            &states
                .iter()
                .map(|(id, liveness, renewable_eligible)| {
                    (
                        *id,
                        *liveness,
                        *renewable_eligible,
                        Some("203.0.113.9"),
                        Some(42),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    fn exact_local_projection(
        states: &[(&str, Liveness, bool, Option<&str>, Option<i64>)],
    ) -> LocalProjection {
        LocalProjection {
            states: states
                .iter()
                .map(
                    |(id, liveness, renewable_eligible, canonical_ip, order_id)| {
                        (
                            (*id).to_string(),
                            LocalState {
                                liveness: *liveness,
                                renewable_eligible: *renewable_eligible,
                                expires_at: i64::MAX,
                                canonical_ip: canonical_ip.map(|ip| ip.parse().unwrap()),
                                order_id: *order_id,
                            },
                        )
                    },
                )
                .collect(),
            source_ok: true,
        }
    }

    fn headers(key: Option<&str>, actor: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(key) = key {
            headers.insert("x-proxy-admin-key", key.parse().unwrap());
        }
        if let Some(actor) = actor {
            headers.insert("x-admin-actor", actor.parse().unwrap());
        }
        headers
    }

    fn accepted_json<T>(value: T) -> std::result::Result<Json<T>, JsonRejection> {
        Ok(Json(value))
    }

    async fn response_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn spawn_runtime_server(
        path: &'static str,
        body: Value,
        status: StatusCode,
    ) -> (reqwest::Url, tokio::task::JoinHandle<()>) {
        async fn respond(
            State((expected_path, body, status)): State<(&'static str, Value, StatusCode)>,
            headers: HeaderMap,
            request: axum::extract::Request,
        ) -> Response {
            assert_eq!(request.uri().path(), expected_path);
            assert_eq!(
                headers
                    .get("x-api-key")
                    .and_then(|value| value.to_str().ok()),
                Some("test-runtime-control-key")
            );
            (status, Json(body)).into_response()
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(path, get(respond))
            .with_state((path, body, status));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            reqwest::Url::parse(&format!("http://{address}{path}")).unwrap(),
            task,
        )
    }

    fn keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
    }

    fn binding(
        runtime: &Arc<Runtime>,
        provider: &str,
        local_id: &str,
        order_id: i64,
    ) -> ProxyBinding {
        binding_ip(runtime, provider, local_id, order_id, "203.0.113.9")
    }

    fn binding_ip(
        runtime: &Arc<Runtime>,
        provider: &str,
        local_id: &str,
        order_id: i64,
        ip: &str,
    ) -> ProxyBinding {
        runtime
            .store
            .upsert_proxy_binding_allocation(
                provider,
                local_id,
                order_id,
                ip,
                1_700_000_000,
                crate::db::ProxyAuthorityStatus::Local,
            )
            .unwrap()
    }

    fn subscription(
        provider: &'static str,
        local_id: &str,
        order_id: Option<i64>,
        ip: &str,
    ) -> SubscriptionProjection {
        SubscriptionProjection {
            provider,
            binding_provider: if provider == "gpt" { "codex" } else { provider },
            local_id: local_id.to_string(),
            account_email: "test@example.com".to_string(),
            canonical_plan: if provider == "gemini" {
                "google_ai_pro".to_string()
            } else {
                "chatgpt_plus".to_string()
            },
            issued_at: 1_700_000_000,
            expires_at: 4_102_444_800,
            liveness: Liveness::Live,
            renewable_eligible: true,
            canonical_ip: Some(ip.parse().unwrap()),
            order_id,
        }
    }

    fn complete_provider(order_id: i64) -> ProviderSnapshot {
        ProviderSnapshot {
            inventory_ok: true,
            orders: [(
                order_id,
                IspOrder {
                    order_id,
                    expire_date: "2099-09-10".to_string(),
                    status: "confirmed".to_string(),
                    auto_extend: false,
                    ips: vec!["203.0.113.9".to_string()],
                },
            )]
            .into_iter()
            .collect(),
            balance_nano_usd: Some("1234567890".to_string()),
            balance_observed_at: Some(1_700_000_000),
            cards_auto_extend: false,
        }
    }

    #[test]
    fn codex_binding_projects_public_gpt_row_and_passes_renewal_guard() {
        let runtime = runtime(None);
        let binding = binding(&runtime, "codex", "profile-a", 123456);
        let local = exact_local_projection(&[(
            "profile-a",
            Liveness::Live,
            true,
            Some("203.0.113.9"),
            Some(123456),
        )]);
        assert!(renewable_guard(&binding, &complete_provider(123456), &local, false).is_ok());
        let inventory = inventory_response(
            complete_provider(123456),
            vec![binding.clone()],
            vec![subscription(
                "gpt",
                "profile-a",
                Some(123456),
                "203.0.113.9",
            )],
        );
        let value = serde_json::to_value(inventory).unwrap();
        assert_eq!(
            keys(&value),
            ["schema_version", "observed_at", "providers", "items"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        assert_eq!(
            keys(&value["providers"][0]),
            [
                "provider",
                "balance_nano_usd",
                "balance_observed_at",
                "auto_extend_enabled",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        assert_eq!(
            keys(&value["items"][0]),
            [
                "inventory_id",
                "account_email",
                "proxy_hint",
                "order_hint",
                "provider",
                "subscription_plan",
                "liveness",
                "subscription_expires_at",
                "proxy_expires_at",
                "binding_status",
                "renewable",
                "operator_renewable",
                "renew_block_code",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        assert_eq!(value["items"][0]["provider"], "gpt");
        assert_eq!(value["items"][0]["account_email"], "test@example.com");
        assert_eq!(value["items"][0]["subscription_plan"], "chatgpt_plus");
        assert_eq!(
            value["items"][0]["subscription_expires_at"],
            4_102_444_800i64
        );
        assert!(value["items"][0]["proxy_hint"]
            .as_str()
            .unwrap()
            .starts_with("proxy-"));
        assert!(!value.to_string().contains("203.0.113.9"));
        assert_ne!(value["items"][0]["order_hint"], "123456");
        assert_no_forbidden_keys(&value);

        let request = RenewalRequest {
            id: 1,
            idempotency_key: UUID_A.to_string(),
            inventory_ids: vec![binding.inventory_id],
            order_ids: vec![123456],
            requested_by: "admin".into(),
            state: RenewalRequestState::Failed,
            created_at: 10,
            updated_at: 11,
        };
        let response = renewal_response(&request, &[], true);
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(
            keys(&value),
            [
                "schema_version",
                "idempotency_key",
                "idempotent_replay",
                "status",
                "observed_at",
                "results",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        assert_eq!(
            keys(&value["results"][0]),
            ["inventory_id", "status", "proxy_expires_at", "result_code",]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        assert_no_forbidden_keys(&value);
    }

    #[test]
    fn uuid_and_inventory_selection_validation_are_strict() {
        for valid in [
            "123e4567-e89b-12d3-a456-426614174000",
            "123e4567-e89b-22d3-8456-426614174000",
            "123e4567-e89b-32d3-9456-426614174000",
            "123e4567-e89b-42d3-b456-426614174000",
            "123e4567-e89b-52d3-A456-426614174000",
        ] {
            assert!(valid_uuid(valid), "{valid}");
        }
        for invalid in [
            "",
            "123e4567-e89b-02d3-a456-426614174000",
            "123e4567-e89b-62d3-a456-426614174000",
            "123e4567-e89b-12d3-c456-426614174000",
            "123e4567e89b12d3a456426614174000",
            "123e4567-e89b-12d3-a456-42661417400z",
        ] {
            assert!(!valid_uuid(invalid), "{invalid}");
        }
        assert_eq!(
            validate_inventory_selection(&[]),
            Err("invalid_inventory_count")
        );
        assert_eq!(
            validate_inventory_selection(&vec!["x".to_string(); 101]),
            Err("invalid_inventory_count")
        );
        assert_eq!(
            validate_inventory_selection(&["one".into(), "one".into()]),
            Err("duplicate_inventory_id")
        );
        assert_eq!(
            validate_inventory_selection(&["bad/id".into()]),
            Err("invalid_inventory_id")
        );
        assert!(serde_json::from_value::<RenewBody>(json!({
            "idempotency_key": UUID_A,
            "inventory_ids": ["inv-a"],
            "order_ids": [42]
        }))
        .is_err());
    }

    #[test]
    fn wire_enums_serialize_to_exact_values() {
        assert_eq!(
            serde_json::to_value([
                Liveness::Live,
                Liveness::Degraded,
                Liveness::Dead,
                Liveness::Unknown,
            ])
            .unwrap(),
            json!(["live", "degraded", "dead", "unknown"])
        );
        assert_eq!(
            serde_json::to_value([
                BindingStatus::Bound,
                BindingStatus::Unbound,
                BindingStatus::Mismatch,
                BindingStatus::Unknown,
            ])
            .unwrap(),
            json!(["bound", "unbound", "mismatch", "unknown"])
        );
        assert_eq!(
            serde_json::to_value([
                RenewItemStatus::Renewed,
                RenewItemStatus::Failed,
                RenewItemStatus::Uncertain,
            ])
            .unwrap(),
            json!(["renewed", "failed", "uncertain"])
        );
        assert_eq!(
            serde_json::to_value([
                RenewStatus::Succeeded,
                RenewStatus::Partial,
                RenewStatus::Failed,
                RenewStatus::Uncertain,
            ])
            .unwrap(),
            json!(["succeeded", "partial", "failed", "uncertain"])
        );
    }

    #[test]
    fn liveness_and_binding_projection_is_fail_closed() {
        let runtime = runtime(None);
        let bound = binding(&runtime, "claude", "claude-local", 42);
        let live = exact_local_projection(&[(
            "claude-local",
            Liveness::Live,
            true,
            Some("203.0.113.9"),
            None,
        )]);
        assert!(renewable_guard(&bound, &complete_provider(42), &live, false).is_ok());

        let absent = local_projection(&[]);
        assert_eq!(
            renewable_guard(&bound, &complete_provider(42), &absent, false),
            Err("binding_mismatch")
        );
        let missing_ip =
            exact_local_projection(&[("claude-local", Liveness::Live, true, None, None)]);
        assert_eq!(
            renewable_guard(&bound, &complete_provider(42), &missing_ip, false),
            Err("binding_mismatch")
        );

        let mut partial = complete_provider(42);
        partial.inventory_ok = false;
        assert_eq!(
            renewable_guard(&bound, &partial, &live, false),
            Err("source_unavailable")
        );

        let unknown_claude = local_projection(&[("claude-local", Liveness::Unknown, false)]);
        assert_eq!(
            renewable_guard(&bound, &complete_provider(42), &unknown_claude, false),
            Err("local_profile_inactive")
        );
        let mut expired = live.clone();
        expired.states.get_mut("claude-local").unwrap().expires_at = unix_now();
        assert_eq!(
            renewable_guard(&bound, &complete_provider(42), &expired, false),
            Err("local_profile_inactive")
        );
        assert!(renewable_guard(&bound, &complete_provider(42), &expired, true).is_ok());

        let mut wrong_ip = complete_provider(42);
        wrong_ip.orders.get_mut(&42).unwrap().ips = vec!["203.0.113.10".into()];
        assert_eq!(
            renewable_guard(&bound, &wrong_ip, &live, false),
            Err("binding_mismatch")
        );
    }

    #[test]
    fn inventory_reconciles_unique_ip_but_omits_external_and_unmatched() {
        let runtime = runtime(None);
        let provider = complete_provider(42);
        let unique = subscription("gpt", "managed", None, "203.0.113.9");
        let external = subscription("gemini", "external", None, "198.51.100.7");
        reconcile_subscriptions(
            &runtime.store,
            &provider,
            &[unique.clone(), external.clone()],
        );
        let bindings = runtime.store.list_proxy_bindings().unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].provider, "codex");
        assert_eq!(bindings[0].order_id, 42);
        assert_eq!(bindings[0].allocation_ip, unique.canonical_ip);

        let response = inventory_response(provider, bindings, vec![unique, external]);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].provider, "gpt");
        assert_eq!(response.items[0].binding_status, BindingStatus::Bound);
    }

    #[test]
    fn inventory_omits_ambiguous_unbound_and_mismatched_rows() {
        let runtime = runtime(None);
        let mut provider = complete_provider(42);
        provider.orders.insert(
            43,
            IspOrder {
                order_id: 43,
                expire_date: "2099-09-10".into(),
                status: "confirmed".into(),
                auto_extend: false,
                ips: vec!["203.0.113.9".into()],
            },
        );
        let ambiguous = subscription("gpt", "ambiguous", None, "203.0.113.9");
        reconcile_subscriptions(&runtime.store, &provider, &[ambiguous.clone()]);
        assert!(runtime.store.list_proxy_bindings().unwrap().is_empty());

        let mismatch = subscription("gpt", "mismatch", Some(43), "203.0.113.9");
        let mismatch_binding = binding(&runtime, "codex", "mismatch", 42);
        let response =
            inventory_response(provider, vec![mismatch_binding], vec![ambiguous, mismatch]);
        assert!(response.items.is_empty());
    }

    #[test]
    fn inventory_omits_dead_and_retains_exact_bound_degraded_rows() {
        let runtime = runtime(None);
        let mut provider = complete_provider(42);
        provider
            .orders
            .get_mut(&42)
            .unwrap()
            .ips
            .push("203.0.113.10".into());
        let dead_binding = binding(&runtime, "codex", "dead", 42);
        let degraded_binding = binding_ip(&runtime, "gemini", "degraded", 42, "203.0.113.10");
        let mut dead = subscription("gpt", "dead", Some(42), "203.0.113.9");
        dead.liveness = Liveness::Dead;
        dead.renewable_eligible = false;
        let mut degraded = subscription("gemini", "degraded", Some(42), "203.0.113.10");
        degraded.liveness = Liveness::Degraded;
        degraded.renewable_eligible = false;

        let response = inventory_response(
            provider,
            vec![dead_binding, degraded_binding],
            vec![dead, degraded],
        );
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].liveness, Liveness::Degraded);
        assert_eq!(response.items[0].binding_status, BindingStatus::Bound);
        assert!(!response.items[0].renewable);
        assert!(!response.items[0].operator_renewable);
        assert_eq!(
            response.items[0].renew_block_code,
            Some("local_profile_inactive")
        );
    }

    #[test]
    fn inventory_exposes_operator_renewal_when_only_subscription_expired() {
        let runtime = runtime(None);
        let binding = binding(&runtime, "codex", "expired", 42);
        let mut expired = subscription("gpt", "expired", Some(42), "203.0.113.9");
        expired.expires_at = unix_now();

        let response = inventory_response(complete_provider(42), vec![binding], vec![expired]);
        assert_eq!(response.items.len(), 1);
        assert!(!response.items[0].renewable);
        assert!(response.items[0].operator_renewable);
        assert_eq!(
            response.items[0].renew_block_code,
            Some("local_profile_inactive")
        );
    }

    #[test]
    fn inventory_retains_exact_bound_claude_unknown_but_provider_unavailable_has_no_rows() {
        let runtime = runtime(None);
        let binding = binding(&runtime, "claude", "unknown", 42);
        let mut unknown = subscription("claude", "unknown", Some(42), "203.0.113.9");
        unknown.liveness = Liveness::Unknown;
        unknown.renewable_eligible = false;

        let available = inventory_response(
            complete_provider(42),
            vec![binding.clone()],
            vec![unknown.clone()],
        );
        assert_eq!(available.items.len(), 1);
        assert_eq!(available.items[0].liveness, Liveness::Unknown);
        assert!(!available.items[0].renewable);
        assert!(!available.items[0].operator_renewable);

        let unavailable = inventory_response(
            ProviderSnapshot::unavailable(),
            vec![binding],
            vec![unknown],
        );
        assert!(unavailable.items.is_empty());
    }

    #[test]
    fn account_email_validation_accepts_only_the_exact_ascii_subset() {
        let max_local = format!("{}@example.com", "a".repeat(64));
        let max_label = format!("a@{}.example", "b".repeat(63));
        let max_total = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        for valid in [
            "owner@example.com",
            "OPS_100%+tag-name@sub-domain.example",
            "!#$%&'*+/=?^_`{|}~-@example.com",
            max_local.as_str(),
            max_label.as_str(),
            max_total.as_str(),
        ] {
            assert_eq!(validated_account_email(valid).unwrap(), valid);
        }

        let long_local = format!("{}@example.com", "a".repeat(65));
        let long_label = format!("a@{}.example", "b".repeat(64));
        let long_total = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(62)
        );
        for invalid in [
            "",
            "owner",
            "@example.com",
            "owner@",
            "owner@@example.com",
            ".owner@example.com",
            "owner.@example.com",
            "own..er@example.com",
            "owner name@example.com",
            "owner,tag@example.com",
            "owner@example..com",
            "owner@-example.com",
            "owner@example-.com",
            "owner@example_com",
            "owner@éxample.com",
            "öwner@example.com",
            " owner@example.com",
            "owner@example.com\n",
            long_local.as_str(),
            long_label.as_str(),
            long_total.as_str(),
        ] {
            assert!(
                validated_account_email(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn calendar_expiry_clamps_month_end() {
        let january_31 = parse_expiry("2025-01-31T12:00:00Z").unwrap();
        assert_eq!(
            add_calendar_months_utc(january_31, 1),
            parse_expiry("2025-02-28T12:00:00Z")
        );
        assert_eq!(
            add_calendar_months_utc(january_31, 18),
            parse_expiry("2026-07-31T12:00:00Z")
        );
    }

    #[test]
    fn parse_runtime_url_accepts_only_exact_literal_loopback_http() {
        assert_eq!(
            parse_runtime_url(None, DEFAULT_CODEX_RUNTIME_URL, "/codex-subs")
                .unwrap()
                .as_str(),
            DEFAULT_CODEX_RUNTIME_URL
        );
        assert!(parse_runtime_url(
            Some("http://[::1]:8794/gemini-subs"),
            DEFAULT_GEMINI_RUNTIME_URL,
            "/gemini-subs"
        )
        .is_ok());
        assert!(parse_runtime_url(
            Some("http://127.0.0.1/codex-subs"),
            DEFAULT_CODEX_RUNTIME_URL,
            "/codex-subs"
        )
        .is_ok());
        for invalid in [
            "codex-subs",
            "https://127.0.0.1:8792/codex-subs",
            "http://localhost:8792/codex-subs",
            "http://192.0.2.1:8792/codex-subs",
            "http://user@127.0.0.1:8792/codex-subs",
            "http://127.0.0.1:8792/codex-subs/",
            "http://127.0.0.1:8792/codex-subs?full=1",
            "http://127.0.0.1:8792/codex-subs#fragment",
            "http://127.0.0.1:0/codex-subs",
        ] {
            assert!(
                parse_runtime_url(Some(invalid), DEFAULT_CODEX_RUNTIME_URL, "/codex-subs").is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[tokio::test]
    async fn fetch_runtime_sends_key_and_joins_codex_by_opaque_id() {
        let (url, server) = spawn_runtime_server(
            "/codex-subs",
            json!({"homes": [{"id": "codex-a", "account_state": "suspect"}]}),
            StatusCode::OK,
        )
        .await;
        let runtime = runtime(None);
        let response = fetch_runtime::<CodexRuntimeResponse>(&runtime, &url)
            .await
            .unwrap();
        let mut subscriptions = vec![subscription("gpt", "codex-a", None, "203.0.113.9")];
        join_codex_runtime(&mut subscriptions, response).unwrap();
        assert_eq!(subscriptions[0].liveness, Liveness::Degraded);
        assert!(subscriptions[0].renewable_eligible);
        assert_eq!(
            subscriptions[0].canonical_ip,
            Some("203.0.113.9".parse().unwrap())
        );
        assert_eq!(subscriptions[0].order_id, None);
        server.abort();
    }

    #[tokio::test]
    async fn fetch_runtime_joins_gemini_and_marks_unauthenticated_dead() {
        let (url, server) = spawn_runtime_server(
            "/gemini-subs",
            json!({
                "profiles": [{
                    "id": "gemini-a",
                    "authenticated": false,
                    "disabled": false
                }]
            }),
            StatusCode::OK,
        )
        .await;
        let runtime = runtime(None);
        let response = fetch_runtime::<GeminiRuntimeResponse>(&runtime, &url)
            .await
            .unwrap();
        let mut subscriptions = vec![subscription("gemini", "gemini-a", None, "203.0.113.9")];
        join_gemini_runtime(&mut subscriptions, response).unwrap();
        assert_eq!(subscriptions[0].liveness, Liveness::Dead);
        assert!(!subscriptions[0].renewable_eligible);
        server.abort();
    }

    #[tokio::test]
    async fn fetch_runtime_duplicate_and_missing_ids_fail_join() {
        let runtime = runtime(None);
        let (duplicate_url, duplicate_server) = spawn_runtime_server(
            "/codex-subs",
            json!({
                "homes": [
                    {"id": "codex-a", "account_state": "healthy"},
                    {"id": "codex-a", "account_state": "dead"}
                ]
            }),
            StatusCode::OK,
        )
        .await;
        let duplicate = fetch_runtime::<CodexRuntimeResponse>(&runtime, &duplicate_url)
            .await
            .unwrap();
        let mut codex = vec![subscription("gpt", "codex-a", None, "203.0.113.9")];
        assert!(join_codex_runtime(&mut codex, duplicate).is_err());
        duplicate_server.abort();

        let (missing_url, missing_server) =
            spawn_runtime_server("/gemini-subs", json!({"profiles": []}), StatusCode::OK).await;
        let missing = fetch_runtime::<GeminiRuntimeResponse>(&runtime, &missing_url)
            .await
            .unwrap();
        let mut gemini = vec![subscription("gemini", "gemini-a", None, "203.0.113.9")];
        assert!(join_gemini_runtime(&mut gemini, missing).is_err());
        missing_server.abort();
    }

    #[tokio::test]
    async fn fetch_runtime_unavailable_endpoint_returns_error() {
        let (url, server) = spawn_runtime_server(
            "/codex-subs",
            json!({"error": "unavailable"}),
            StatusCode::SERVICE_UNAVAILABLE,
        )
        .await;
        let runtime = runtime(None);
        assert!(fetch_runtime::<CodexRuntimeResponse>(&runtime, &url)
            .await
            .is_err());
        server.abort();
    }

    #[test]
    fn runtime_payload_join_is_exact_and_fail_closed() {
        let mut codex = vec![subscription("gpt", "codex-a", None, "203.0.113.9")];
        join_codex_runtime(
            &mut codex,
            CodexRuntimeResponse {
                homes: vec![
                    CodexRuntimeHome {
                        id: "codex-a".into(),
                        account_state: "suspect".into(),
                    },
                    CodexRuntimeHome {
                        id: "extra".into(),
                        account_state: "healthy".into(),
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(codex[0].liveness, Liveness::Degraded);
        assert!(codex[0].renewable_eligible);

        let duplicate = CodexRuntimeResponse {
            homes: vec![
                CodexRuntimeHome {
                    id: "codex-a".into(),
                    account_state: "healthy".into(),
                },
                CodexRuntimeHome {
                    id: "codex-a".into(),
                    account_state: "dead".into(),
                },
            ],
        };
        assert!(join_codex_runtime(&mut codex, duplicate).is_err());
        for account_state in ["", "unknown", "Healthy"] {
            assert!(join_codex_runtime(
                &mut codex,
                CodexRuntimeResponse {
                    homes: vec![CodexRuntimeHome {
                        id: "codex-a".into(),
                        account_state: account_state.into(),
                    }],
                },
            )
            .is_err());
        }
        assert!(
            join_codex_runtime(&mut codex, CodexRuntimeResponse { homes: Vec::new() }).is_err()
        );

        let mut gemini = vec![subscription("gemini", "gemini-a", None, "203.0.113.9")];
        join_gemini_runtime(
            &mut gemini,
            GeminiRuntimeResponse {
                profiles: vec![GeminiRuntimeProfile {
                    id: "gemini-a".into(),
                    authenticated: true,
                    disabled: true,
                }],
            },
        )
        .unwrap();
        assert_eq!(gemini[0].liveness, Liveness::Degraded);
        assert!(!gemini[0].renewable_eligible);

        join_gemini_runtime(
            &mut gemini,
            GeminiRuntimeResponse {
                profiles: vec![GeminiRuntimeProfile {
                    id: "gemini-a".into(),
                    authenticated: false,
                    disabled: false,
                }],
            },
        )
        .unwrap();
        assert_eq!(gemini[0].liveness, Liveness::Dead);
        assert!(!gemini[0].renewable_eligible);

        let duplicate = GeminiRuntimeResponse {
            profiles: vec![
                GeminiRuntimeProfile {
                    id: "gemini-a".into(),
                    authenticated: true,
                    disabled: false,
                },
                GeminiRuntimeProfile {
                    id: "gemini-a".into(),
                    authenticated: false,
                    disabled: false,
                },
            ],
        };
        assert!(join_gemini_runtime(&mut gemini, duplicate).is_err());
        assert!(join_gemini_runtime(
            &mut gemini,
            GeminiRuntimeResponse {
                profiles: Vec::new()
            }
        )
        .is_err());
    }

    #[test]
    fn runtime_payload_requires_individual_fields_but_allows_top_level_extras() {
        assert!(serde_json::from_value::<CodexRuntimeResponse>(json!({
            "homes": [{"id": "a", "account_state": "healthy"}],
            "sanitized_extra": true
        }))
        .is_ok());
        assert!(serde_json::from_value::<CodexRuntimeResponse>(json!({
            "homes": [{"id": "a"}]
        }))
        .is_err());
        assert!(serde_json::from_value::<GeminiRuntimeResponse>(json!({
            "profiles": [{"id": "a", "authenticated": true}]
        }))
        .is_err());
    }

    #[test]
    fn parse_bind_and_authentication_guards_are_strict() {
        assert_eq!(parse_bind(None).unwrap(), "127.0.0.1:8806".parse().unwrap());
        assert_eq!(
            parse_bind(Some("[::1]:9900")).unwrap(),
            "[::1]:9900".parse().unwrap()
        );
        assert!(parse_bind(Some("localhost:8806")).is_err());
        assert!(parse_bind(Some("0.0.0.0:8806")).is_err());
        assert!(parse_bind(Some("127.0.0.1:0")).is_err());
        assert!(constant_time_equal(b"same", b"same"));
        assert!(!constant_time_equal(b"same", b"different"));
        assert_eq!(
            validated_actor(&headers(None, Some("ops@example.com/team+1"))),
            Some("ops@example.com/team+1")
        );
        assert_eq!(validated_actor(&headers(None, None)), None);
        assert_eq!(validated_actor(&headers(None, Some("bad actor"))), None);
    }

    #[test]
    fn service_rejects_reused_admin_and_runtime_keys() {
        let error = Service::new(
            "127.0.0.1:8806".parse().unwrap(),
            ProxyAdminKey::parse(Zeroizing::new(b"a".repeat(64))).unwrap(),
            "a".repeat(64),
            Arc::new(Store::open(":memory:").unwrap()),
            None,
            AuthorityConfig::Postgres {
                url: "postgresql://unused".into(),
            },
            "prod".into(),
            None,
            None,
            reqwest::Url::parse(DEFAULT_CODEX_RUNTIME_URL).unwrap(),
            reqwest::Url::parse(DEFAULT_GEMINI_RUNTIME_URL).unwrap(),
        )
        .err()
        .expect("matching keys must fail");
        assert!(error
            .to_string()
            .contains("must differ from the runtime control key"));
    }

    #[tokio::test]
    async fn handlers_require_key_and_post_requires_actor() {
        let runtime = runtime(None);
        assert_eq!(
            inventory_handler(State(runtime.clone()), HeaderMap::new())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("wrong"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec!["unknown".to_string()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            inventory_handler(
                State(runtime.clone()),
                headers(Some("test-runtime-control-key"), None),
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let mut legacy_header = HeaderMap::new();
        legacy_header.insert("x-api-key", "test-admin-key".parse().unwrap());
        assert_eq!(
            inventory_handler(State(runtime.clone()), legacy_header)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let response = renew_handler(
            State(runtime),
            headers(Some("test-admin-key"), None),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec!["unknown".to_string()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(response).await["code"], "invalid_actor");
    }

    #[tokio::test]
    async fn event_feed_requires_admin_key_and_starts_with_resync() {
        use futures_util::StreamExt as _;

        let runtime = runtime(None);
        assert_eq!(
            events_handler(State(runtime.clone()), HeaderMap::new())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let response = events_handler(State(runtime), headers(Some("test-admin-key"), None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );

        // Do not buffer an infinite SSE body: the eager resync is the reconnect contract.
        let mut body = response.into_body().into_data_stream();
        let first = tokio::time::timeout(Duration::from_secs(1), body.next())
            .await
            .expect("initial SSE frame timed out")
            .expect("SSE body ended before its initial frame")
            .expect("initial SSE frame failed");
        let first = std::str::from_utf8(&first).unwrap();
        assert!(first.contains("event: resync"), "{first}");
        assert!(first.contains("\"source\":\"authbot\""), "{first}");
        assert!(first.contains("\"resync\":true"), "{first}");
    }

    #[tokio::test]
    async fn get_without_provider_has_exact_safe_shape() {
        let response =
            inventory_handler(State(runtime(None)), headers(Some("test-admin-key"), None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["schema_version"], 1);
        assert!(value["observed_at"].as_i64().is_some());
        assert_eq!(value["providers"][0]["provider"], "iproyal");
        assert_eq!(value["providers"][0]["balance_nano_usd"], Value::Null);
        assert_eq!(value["providers"][0]["balance_observed_at"], Value::Null);
        assert_eq!(value["providers"][0]["auto_extend_enabled"], false);
        assert_eq!(value["items"], json!([]));
        assert_no_forbidden_keys(&value);
    }

    #[tokio::test]
    async fn renew_rejects_duplicate_and_unknown_inventory_ids() {
        let runtime = runtime(None);
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec!["same".into(), "same".into()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await["code"],
            "duplicate_inventory_id"
        );
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec!["unknown".into()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await["code"],
            "unknown_inventory_id"
        );
        assert!(runtime.store.list_renewal_requests().unwrap().is_empty());
    }

    #[tokio::test]
    async fn same_selection_replays_and_different_selection_conflicts() {
        let runtime = runtime(None);
        let first_binding = binding(&runtime, "codex", "first", 11);
        let second_binding = binding(&runtime, "gemini", "second", 12);
        let first = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![first_binding.inventory_id.clone()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_value = response_json(first).await;
        assert_eq!(first_value["status"], "failed");
        assert_eq!(first_value["idempotent_replay"], false);

        let replay = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("other-admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![first_binding.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let replay_value = response_json(replay).await;
        assert_eq!(replay_value["status"], first_value["status"]);
        assert_eq!(replay_value["results"], first_value["results"]);
        assert_eq!(replay_value["observed_at"], first_value["observed_at"]);
        assert_eq!(replay_value["idempotent_replay"], true);
        assert_eq!(
            runtime
                .store
                .get_renewal_request_by_key(UUID_A)
                .unwrap()
                .unwrap()
                .requested_by,
            "admin"
        );

        let conflict = renew_handler(
            State(runtime),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![second_binding.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(conflict).await["code"],
            "idempotency_conflict"
        );
    }

    #[tokio::test]
    async fn overlapping_queued_selection_is_409_without_insert_while_disjoint_can_proceed() {
        let runtime = runtime(None);
        let first = binding(&runtime, "codex", "first-active", 11);
        let second = binding(&runtime, "gemini", "second-active", 12);
        runtime
            .store
            .create_or_get_renewal_request(
                UUID_A,
                &[(first.inventory_id.clone(), first.order_id)],
                "admin",
            )
            .unwrap();

        let overlap = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_B.to_string(),
                inventory_ids: vec![first.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(overlap.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(overlap).await["code"],
            "renewal_selection_busy"
        );
        assert!(runtime
            .store
            .get_renewal_request_by_key(UUID_B)
            .unwrap()
            .is_none());

        let disjoint = renew_handler(
            State(runtime),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_C.to_string(),
                inventory_ids: vec![second.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        assert_eq!(disjoint.status(), StatusCode::OK);
        assert_eq!(response_json(disjoint).await["status"], "failed");
    }

    #[tokio::test]
    async fn pending_replay_resumes_once_but_in_progress_does_not_spend() {
        let (client, extend_calls, server) = mock_iproyal().await;
        let runtime = runtime(Some(client));
        let binding = binding(&runtime, "codex", "pending", 42);
        *runtime.test_local_projection.write().unwrap() =
            Some(local_projection(&[("pending", Liveness::Live, true)]));
        let selections = vec![(binding.inventory_id.clone(), 42)];
        let pending = runtime
            .store
            .create_or_get_renewal_request(UUID_A, &selections, "admin")
            .unwrap();
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: pending.inventory_ids,
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let value = response_json(response).await;
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["idempotent_replay"], true);
        assert_eq!(value["results"][0]["status"], "renewed");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);

        let in_progress = runtime
            .store
            .create_or_get_renewal_request(UUID_B, &selections, "admin")
            .unwrap();
        let in_progress = runtime
            .store
            .claim_renewal_request(in_progress.id)
            .unwrap()
            .unwrap();
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_B.to_string(),
                inventory_ids: in_progress.inventory_ids,
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let value = response_json(response).await;
        assert_eq!(value["status"], "uncertain");
        assert_eq!(value["idempotent_replay"], true);
        assert_eq!(value["results"][0]["status"], "uncertain");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn terminal_replay_works_without_provider() {
        let runtime = runtime(None);
        let binding = binding(&runtime, "codex", "terminal", 42);
        let request = runtime
            .store
            .create_or_get_renewal_request(UUID_A, &[(binding.inventory_id.clone(), 42)], "admin")
            .unwrap();
        let request = runtime
            .store
            .claim_renewal_request(request.id)
            .unwrap()
            .unwrap();
        runtime
            .store
            .record_renewal_event(
                request.id,
                42,
                RenewalEventOutcome::Renewed,
                100,
                Some(1_800_000_000),
            )
            .unwrap();
        runtime.store.complete_renewal_request(request.id).unwrap();
        let response = renew_handler(
            State(runtime),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![binding.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let value = response_json(response).await;
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["idempotent_replay"], true);
        assert_eq!(value["results"][0]["status"], "renewed");
        assert_eq!(value["results"][0]["proxy_expires_at"], 1_800_000_000i64);
    }

    #[test]
    fn partial_and_duplicate_order_results_map_by_inventory() {
        let request = RenewalRequest {
            id: 1,
            idempotency_key: UUID_A.to_string(),
            inventory_ids: vec!["inv-a".into(), "inv-b".into(), "inv-c".into()],
            order_ids: vec![1, 1, 3],
            requested_by: "admin".into(),
            state: RenewalRequestState::Indeterminate,
            created_at: 10,
            updated_at: 20,
        };
        let events = vec![
            ExactRenewalEvent {
                event: crate::db::RenewalEvent {
                    id: 1,
                    request_id: 1,
                    order_id: 1,
                    outcome: RenewalEventOutcome::Renewed,
                    observed_at: 11,
                    new_expiry_at: Some(30),
                },
                inventory_id: "inv-a".into(),
                allocation_ip: "203.0.113.1".parse().unwrap(),
            },
            ExactRenewalEvent {
                event: crate::db::RenewalEvent {
                    id: 2,
                    request_id: 1,
                    order_id: 1,
                    outcome: RenewalEventOutcome::Rejected,
                    observed_at: 12,
                    new_expiry_at: None,
                },
                inventory_id: "inv-b".into(),
                allocation_ip: "203.0.113.2".parse().unwrap(),
            },
        ];
        let response = renewal_response(&request, &events, true);
        assert_eq!(response.status, RenewStatus::Uncertain);
        assert_eq!(response.results[0].inventory_id, "inv-a");
        assert_eq!(response.results[0].status, RenewItemStatus::Renewed);
        assert_eq!(response.results[1].status, RenewItemStatus::Failed);
        assert_eq!(response.results[2].status, RenewItemStatus::Uncertain);

        let terminal = RenewalRequest {
            state: RenewalRequestState::Failed,
            inventory_ids: vec!["inv-a".into(), "inv-b".into()],
            order_ids: vec![1, 1],
            ..request
        };
        let partial = renewal_response(&terminal, &events, true);
        assert_eq!(partial.status, RenewStatus::Partial);
        assert_eq!(partial.results[0].status, RenewItemStatus::Renewed);
        assert_eq!(partial.results[1].status, RenewItemStatus::Failed);
    }

    #[test]
    fn explicit_provider_rejection_has_its_own_terminal_result_code() {
        let exact = ExactRenewalEvent {
            event: crate::db::RenewalEvent {
                id: 1,
                request_id: 1,
                order_id: 42,
                outcome: RenewalEventOutcome::ProviderRejected,
                observed_at: 10,
                new_expiry_at: None,
            },
            inventory_id: "inv-a".into(),
            allocation_ip: "203.0.113.9".parse().unwrap(),
        };
        let result = result_from_event("inv-a".into(), &exact);

        assert_eq!(result.status, RenewItemStatus::Failed);
        assert_eq!(result.result_code, Some("provider_renewal_rejected"));
    }

    #[tokio::test]
    async fn inactive_subscription_override_is_idempotent_and_renews_once() {
        let (client, extend_calls, server) = mock_iproyal().await;
        let runtime = runtime(Some(client));
        let binding = binding(&runtime, "codex", "expired", 42);
        let mut projection = local_projection(&[("expired", Liveness::Live, true)]);
        projection.states.get_mut("expired").unwrap().expires_at = unix_now();
        *runtime.test_local_projection.write().unwrap() = Some(projection);

        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![binding.inventory_id.clone()],
                allow_inactive_subscription: true,
            }),
        )
        .await;
        let value = response_json(response).await;
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["idempotent_replay"], false);
        assert_eq!(value["results"][0]["status"], "renewed");
        let request = runtime
            .store
            .get_renewal_request_by_key(UUID_A)
            .unwrap()
            .unwrap();
        assert_eq!(request.state, RenewalRequestState::Completed);
        let events = runtime.store.get_exact_renewal_events(request.id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.outcome, RenewalEventOutcome::Renewed);
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);

        let replay = renew_handler(
            State(runtime),
            headers(Some("test-admin-key"), Some("other-admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![binding.inventory_id],
                allow_inactive_subscription: true,
            }),
        )
        .await;
        let replay = response_json(replay).await;
        assert_eq!(replay["status"], "succeeded");
        assert_eq!(replay["idempotent_replay"], true);
        assert_eq!(replay["results"][0]["status"], "renewed");
        assert_eq!(replay["results"], value["results"]);
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn stale_roster_binding_fails_before_paid_extend() {
        let (client, extend_calls, server) = mock_iproyal().await;
        let runtime = runtime(Some(client));
        let binding = binding(&runtime, "codex", "stale", 42);

        *runtime.test_local_projection.write().unwrap() = Some(exact_local_projection(&[(
            "stale",
            Liveness::Live,
            true,
            Some("203.0.113.10"),
            Some(42),
        )]));
        let wrong_ip = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![binding.inventory_id.clone()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let wrong_ip = response_json(wrong_ip).await;
        assert_eq!(wrong_ip["status"], "failed");
        assert_eq!(wrong_ip["results"][0]["result_code"], "binding_mismatch");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 0);

        *runtime.test_local_projection.write().unwrap() = Some(exact_local_projection(&[(
            "stale",
            Liveness::Live,
            true,
            Some("203.0.113.9"),
            Some(43),
        )]));
        let wrong_order = renew_handler(
            State(runtime),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_B.to_string(),
                inventory_ids: vec![binding.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let wrong_order = response_json(wrong_order).await;
        assert_eq!(wrong_order["status"], "failed");
        assert_eq!(wrong_order["results"][0]["result_code"], "binding_mismatch");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 0);
        server.abort();
    }

    #[tokio::test]
    async fn claimant_renews_synchronously_once() {
        let (client, extend_calls, server) = mock_iproyal().await;
        let runtime = runtime(Some(client));
        let binding = binding(&runtime, "codex", "renew", 42);
        *runtime.test_local_projection.write().unwrap() =
            Some(local_projection(&[("renew", Liveness::Live, true)]));
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![binding.inventory_id],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let value = response_json(response).await;
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["idempotent_replay"], false);
        assert_eq!(value["results"][0]["status"], "renewed");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn multiple_allocations_same_order_use_one_selective_call_and_exact_events() {
        let (client, extend_calls, server) = mock_iproyal_multi_ip().await;
        let runtime = runtime(Some(client));
        let first = binding_ip(&runtime, "codex", "first-ip", 42, "203.0.113.9");
        let second = binding_ip(&runtime, "codex", "second-ip", 42, "203.0.113.10");
        *runtime.test_local_projection.write().unwrap() = Some(exact_local_projection(&[
            (
                "first-ip",
                Liveness::Live,
                true,
                Some("203.0.113.9"),
                Some(42),
            ),
            (
                "second-ip",
                Liveness::Live,
                true,
                Some("203.0.113.10"),
                Some(42),
            ),
        ]));
        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![first.inventory_id.clone(), second.inventory_id.clone()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let value = response_json(response).await;
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["results"][0]["status"], "renewed");
        assert_eq!(value["results"][1]["status"], "renewed");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);
        let request = runtime
            .store
            .get_renewal_request_by_key(UUID_A)
            .unwrap()
            .unwrap();
        let events = runtime.store.get_exact_renewal_events(request.id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .map(|event| event.inventory_id.as_str())
                .collect::<HashSet<_>>(),
            [first.inventory_id.as_str(), second.inventory_id.as_str()]
                .into_iter()
                .collect()
        );
        server.abort();
    }

    #[tokio::test]
    async fn one_selected_allocation_does_not_require_its_same_order_sibling() {
        let (client, extend_calls, server) = mock_iproyal_multi_ip().await;
        let runtime = runtime(Some(client));
        let first = binding_ip(&runtime, "codex", "first-ip", 42, "203.0.113.9");
        let second = binding_ip(&runtime, "codex", "second-ip", 42, "203.0.113.10");
        *runtime.test_local_projection.write().unwrap() = Some(exact_local_projection(&[
            (
                "first-ip",
                Liveness::Live,
                true,
                Some("203.0.113.9"),
                Some(42),
            ),
            (
                "second-ip",
                Liveness::Live,
                true,
                Some("203.0.113.10"),
                Some(42),
            ),
        ]));

        let response = renew_handler(
            State(runtime.clone()),
            headers(Some("test-admin-key"), Some("admin")),
            accepted_json(RenewBody {
                idempotency_key: UUID_A.to_string(),
                inventory_ids: vec![first.inventory_id.clone()],
                allow_inactive_subscription: false,
            }),
        )
        .await;
        let value = response_json(response).await;

        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["results"].as_array().unwrap().len(), 1);
        assert_eq!(value["results"][0]["inventory_id"], first.inventory_id);
        assert_ne!(value["results"][0]["inventory_id"], second.inventory_id);
        assert_eq!(value["results"][0]["status"], "renewed");
        assert_eq!(extend_calls.load(Ordering::SeqCst), 1);
        let request = runtime
            .store
            .get_renewal_request_by_key(UUID_A)
            .unwrap()
            .unwrap();
        let events = runtime.store.get_exact_renewal_events(request.id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].inventory_id, first.inventory_id);
        server.abort();
    }

    async fn mock_iproyal() -> (Arc<Iproyal>, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        mock_iproyal_with_order(false).await
    }

    async fn mock_iproyal_multi_ip() -> (Arc<Iproyal>, Arc<AtomicUsize>, tokio::task::JoinHandle<()>)
    {
        mock_iproyal_with_order(true).await
    }

    async fn mock_iproyal_with_order(
        multi_ip: bool,
    ) -> (Arc<Iproyal>, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let extend_calls = Arc::new(AtomicUsize::new(0));
        let task_calls = extend_calls.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                    if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let line = String::from_utf8_lossy(&bytes)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let before = if multi_ip {
                    r#"{"id":42,"expire_date":"2099-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"},{"ip":"203.0.113.10"}]},"auto_extend_settings":null}"#
                } else {
                    r#"{"id":42,"expire_date":"2099-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"}]},"auto_extend_settings":null}"#
                };
                let after = if multi_ip {
                    r#"{"id":42,"expire_date":"2099-09-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"},{"ip":"203.0.113.10"}]},"auto_extend_settings":null}"#
                } else {
                    r#"{"id":42,"expire_date":"2099-09-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"}]},"auto_extend_settings":null}"#
                };
                let inventory = if multi_ip {
                    r#"{"data":[{"id":42,"expire_date":"2099-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"},{"ip":"203.0.113.10"}]},"auto_extend_settings":null}],"meta":{"last_page":1}}"#
                } else {
                    r#"{"data":[{"id":42,"expire_date":"2099-08-10","status":"confirmed","proxy_data":{"proxies":[{"ip":"203.0.113.9"}]},"auto_extend_settings":null}],"meta":{"last_page":1}}"#
                };
                let body = if line.starts_with("GET /products ") {
                    r#"{"data":[{"id":7,"name":"ISP","plans":[{"id":9,"name":"30 Days"}],"locations":[]}]}"#
                } else if line.starts_with("GET /orders?product_id=7") {
                    inventory
                } else if line.starts_with("POST /orders/toggle-auto-extend ") {
                    r#"{}"#
                } else if line.starts_with("GET /orders/42 ") {
                    if task_calls.load(Ordering::SeqCst) == 0 {
                        before
                    } else {
                        after
                    }
                } else if line.starts_with("POST /orders/42/extend ") {
                    let body = String::from_utf8_lossy(&bytes);
                    let body = body.split("\r\n\r\n").nth(1).unwrap_or_default();
                    let value: Value = serde_json::from_str(body).unwrap();
                    let proxies = value["proxies"].as_array().unwrap();
                    assert!(!proxies.is_empty());
                    assert!(proxies.iter().all(|proxy| {
                        proxy == "203.0.113.9" || (multi_ip && proxy == "203.0.113.10")
                    }));
                    task_calls.fetch_add(1, Ordering::SeqCst);
                    r#"{}"#
                } else if line.starts_with("GET /balance ") {
                    r#""1.25""#
                } else if line.starts_with("GET /cards ") {
                    r#"{"data":[]}"#
                } else {
                    r#"{}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client =
            Arc::new(Iproyal::with_base_url("mock-key", &format!("http://{address}")).unwrap());
        (client, extend_calls, task)
    }

    #[test]
    fn expiry_parser_is_strict() {
        assert_eq!(parse_expiry("1970-01-01 00:00:01"), Some(1));
        assert_eq!(parse_expiry("2024-02-29T12:30:45Z"), Some(1_709_209_845));
        assert_eq!(parse_expiry("2023-02-29"), None);
        assert_eq!(parse_expiry("2026-13-01"), None);
        assert_eq!(parse_expiry("not-a-date"), None);
    }

    fn assert_no_forbidden_keys(value: &Value) {
        const FORBIDDEN: &[&str] = &[
            "credential",
            "credentials",
            "email",
            "full_identity",
            "ip",
            "password",
            "project",
            "proxy_host",
            "proxy_url",
            "secret",
            "subject",
            "token",
            "username",
        ];
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key != "account_email" {
                        assert!(!FORBIDDEN.contains(&key.as_str()), "forbidden key: {key}");
                    }
                    assert_no_forbidden_keys(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    assert_no_forbidden_keys(value);
                }
            }
            _ => {}
        }
    }
}
