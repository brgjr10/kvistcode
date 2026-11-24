//! Loopback-only Streamable HTTP MCP server (`POST /mcp`) for Kvistcode.
//!
//! Security model:
//! - Strictly opt-in (`Settings.local_mcp.enabled == false` by default).
//! - ALWAYS binds to `127.0.0.1:<port>` (NEVER `0.0.0.0` or LAN interfaces).
//! - Rejects DNS rebinding (`Host` must be loopback with our bound port) and
//!   cross-site browser requests (`Origin` / `Sec-Fetch-Site`).
//! - Requires `Authorization: Bearer <token>` on every request, compared in
//!   constant time against a 256-bit random token persisted in `data_dir` with
//!   `0600` permissions (never logged or included in diagnostics).

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use kvistcode_core::local_mcp::{
    client_config_snippet, constant_time_eq, endpoint_url_for_port, handle_jsonrpc,
    load_or_create_token, read_token, rotate_token as core_rotate_token, token_preview,
    LocalMcpSettings, MIN_LOCAL_MCP_PORT,
};
use kvistcode_core::Core;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{oneshot, watch};
use tracing::{info, warn};

const MAX_CONNECTIONS: usize = 32;
const MAX_BODY_BYTES: usize = 256 * 1024;
const HEADER_READ_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_secs(1)
} else {
    Duration::from_secs(10)
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMcpStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub url: Option<String>,
    pub token_preview: Option<String>,
    pub token: Option<String>,
    pub config_snippet: Option<String>,
    pub tools_enabled: Vec<String>,
    pub error: Option<String>,
}

struct Running {
    addr: SocketAddr,
    port: u16,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

pub struct LocalMcpServer {
    core: Core,
    data_dir: PathBuf,
    token: Mutex<Option<String>>,
    running: Mutex<Option<Running>>,
    apply_lock: tokio::sync::Mutex<()>,
    shutting_down: AtomicBool,
    error: Mutex<Option<String>>,
    active_conns: Arc<AtomicUsize>,
}

impl LocalMcpServer {
    pub fn new(core: Core, data_dir: PathBuf) -> Arc<Self> {
        let initial_token = read_token(&data_dir);
        Arc::new(LocalMcpServer {
            core,
            data_dir,
            token: Mutex::new(initial_token),
            running: Mutex::new(None),
            apply_lock: tokio::sync::Mutex::new(()),
            shutting_down: AtomicBool::new(false),
            error: Mutex::new(None),
            active_conns: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn ensure_token(&self) -> Result<String, String> {
        if let Some(tok) = self.token.lock().clone() {
            return Ok(tok);
        }
        match load_or_create_token(&self.data_dir) {
            Ok(tok) => {
                *self.token.lock() = Some(tok.clone());
                Ok(tok)
            }
            Err(e) => Err(format!("Couldn't prepare local MCP bearer token: {e:#}")),
        }
    }

    pub fn status(&self) -> LocalMcpStatus {
        let settings = self
            .core
            .settings()
            .map(|s| s.local_mcp)
            .unwrap_or_default();
        let listening = self.running.lock().as_ref().map(|r| r.addr);
        let running = listening.is_some();
        let port = listening.map_or(settings.port, |a| a.port());
        let url = listening.map(|a| endpoint_url_for_port(a.port()));

        let token = if settings.enabled || running {
            self.token
                .lock()
                .clone()
                .or_else(|| read_token(&self.data_dir))
        } else {
            None
        };
        let preview = token.as_deref().map(token_preview);
        let snippet_url = url
            .clone()
            .unwrap_or_else(|| endpoint_url_for_port(settings.port));
        let config_snippet = token
            .as_deref()
            .map(|t| client_config_snippet(&snippet_url, t));

        LocalMcpStatus {
            enabled: settings.enabled,
            running,
            port,
            url,
            token_preview: preview,
            token,
            config_snippet,
            tools_enabled: settings.enabled_tool_names(),
            error: self.error.lock().clone(),
        }
    }

    /// Generates and persists a fresh bearer token. Immediately invalidates the previous
    /// token for all future HTTP requests without restarting the listener.
    pub fn rotate_token(&self) -> Result<LocalMcpStatus, String> {
        let next = core_rotate_token(&self.data_dir)
            .map_err(|e| format!("Couldn't rotate local MCP token: {e:#}"))?;
        *self.token.lock() = Some(next);
        info!("rotated local MCP bearer token");
        Ok(self.status())
    }

    /// Starts, restarts, or stops the loopback HTTP server to match `settings`.
    pub async fn apply(self: &Arc<Self>, settings: &LocalMcpSettings) {
        let _serialized = self.apply_lock.lock().await;
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let unchanged = self
            .running
            .lock()
            .as_ref()
            .is_some_and(|r| r.port == settings.port);
        if settings.enabled && unchanged {
            // Ensure a token exists if somehow cleared.
            if let Err(e) = self.ensure_token() {
                *self.error.lock() = Some(e);
            }
            return;
        }
        let was_running = self.stop_and_wait().await;
        if !settings.enabled {
            *self.error.lock() = None;
            return;
        }
        if settings.port < MIN_LOCAL_MCP_PORT {
            *self.error.lock() = Some(format!(
                "The local MCP port must be between {MIN_LOCAL_MCP_PORT} and 65535."
            ));
            return;
        }
        if let Err(e) = self.ensure_token() {
            warn!("{e}");
            *self.error.lock() = Some(e);
            return;
        }
        match self.start_on_port(settings.port, was_running).await {
            Ok(addr) => {
                info!(%addr, "local MCP server listening");
                *self.error.lock() = None;
            }
            Err(msg) => {
                warn!("local MCP server failed to start: {msg}");
                *self.error.lock() = Some(msg);
            }
        }
    }

    /// Stops the server at app exit and keeps it stopped.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let _serialized = self.apply_lock.lock().await;
        self.stop_and_wait().await;
    }

    async fn stop_and_wait(&self) -> bool {
        let prev = self.running.lock().take();
        let Some(mut running) = prev else {
            return false;
        };
        if let Some(tx) = running.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = running.task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
        }
        info!(addr = %running.addr, "local MCP server stopped");
        true
    }

    async fn start_on_port(
        self: &Arc<Self>,
        port: u16,
        was_running: bool,
    ) -> Result<SocketAddr, String> {
        // Strictly loopback: NEVER bind 0.0.0.0 or any non-loopback address.
        let bind_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let attempts = if was_running { 10 } else { 1 };
        let mut last_err = None;
        let mut listener = None;
        for attempt in 0..attempts {
            match tokio::net::TcpListener::bind(bind_addr).await {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt + 1 < attempts {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
        let listener = listener.ok_or_else(|| {
            let e = last_err.expect("bind failed");
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!(
                    "Port {port} is already in use on 127.0.0.1. Pick another port in Local MCP settings."
                )
            } else {
                format!("Couldn't listen on 127.0.0.1:{port}: {e}")
            }
        })?;
        let actual = listener
            .local_addr()
            .map_err(|e| format!("Couldn't read bound local MCP address: {e}"))?;
        debug_assert!(actual.ip().is_loopback());

        let (tx, rx) = oneshot::channel();
        let state = RouteState {
            server: self.clone(),
            bound_port: actual.port(),
        };
        let router = Router::new()
            .route(
                "/mcp",
                post(handle_mcp_post)
                    .get(handle_mcp_get)
                    .delete(handle_mcp_delete),
            )
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(state);

        let conns = self.active_conns.clone();
        let task = tauri::async_runtime::spawn(serve(listener, router, rx, conns));
        *self.running.lock() = Some(Running {
            addr: actual,
            port: actual.port(),
            shutdown: Some(tx),
            task: Some(task),
        });
        Ok(actual)
    }

    fn verify_request(&self, bound_port: u16, headers: &HeaderMap) -> Result<(), HttpRejection> {
        // 1. Block cross-site browser requests (Fetch Metadata).
        if let Some(site) = headers
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
        {
            if !site.eq_ignore_ascii_case("none") && !site.eq_ignore_ascii_case("same-origin") {
                return Err(HttpRejection::forbidden(
                    "Cross-site requests aren't allowed.",
                ));
            }
        }

        // 2. Block DNS rebinding: Host must name loopback + our bound port.
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or("");
        let allowed_hosts = [
            format!("127.0.0.1:{bound_port}"),
            format!("localhost:{bound_port}"),
            format!("[::1]:{bound_port}"),
        ];
        if !allowed_hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            return Err(HttpRejection::misdirected("Unexpected Host header."));
        }

        // 3. Block cross-origin browser requests: if Origin is set, it must be our loopback origin.
        if let Some(origin) = headers.get(header::ORIGIN) {
            let allowed_origins = [
                format!("http://127.0.0.1:{bound_port}"),
                format!("http://localhost:{bound_port}"),
                format!("http://[::1]:{bound_port}"),
            ];
            let ok = origin
                .to_str()
                .ok()
                .map(str::trim)
                .is_some_and(|o| allowed_origins.iter().any(|a| a.eq_ignore_ascii_case(o)));
            if !ok {
                return Err(HttpRejection::forbidden(
                    "Cross-origin requests aren't allowed.",
                ));
            }
        }

        // 4. Require Authorization: Bearer <token> (constant-time compare).
        let supplied = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                let trimmed = v.trim();
                let (scheme, rest) = trimmed.split_once(' ')?;
                scheme.eq_ignore_ascii_case("bearer").then(|| rest.trim())
            })
            .unwrap_or("");
        let expected = self.token.lock().clone().unwrap_or_default();
        if expected.is_empty() || supplied.is_empty() || !constant_time_eq(supplied, &expected) {
            return Err(HttpRejection::unauthorized(
                "Missing or invalid bearer token.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct RouteState {
    server: Arc<LocalMcpServer>,
    bound_port: u16,
}

struct HttpRejection {
    status: StatusCode,
    message: &'static str,
    www_authenticate: bool,
}

impl HttpRejection {
    fn unauthorized(message: &'static str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message,
            www_authenticate: true,
        }
    }
    fn forbidden(message: &'static str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message,
            www_authenticate: false,
        }
    }
    fn misdirected(message: &'static str) -> Self {
        Self {
            status: StatusCode::MISDIRECTED_REQUEST,
            message,
            www_authenticate: false,
        }
    }
}

impl IntoResponse for HttpRejection {
    fn into_response(self) -> Response {
        let mut resp = (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response();
        if self.www_authenticate {
            resp.headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        resp
    }
}

async fn handle_mcp_post(
    State(state): State<RouteState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(rej) = state.server.verify_request(state.bound_port, &headers) {
        return rej.into_response();
    }
    let settings = state
        .server
        .core
        .settings()
        .map(|s| s.local_mcp)
        .unwrap_or_default();
    match handle_jsonrpc(&state.server.core, &settings, body).await {
        Some(reply) => (StatusCode::OK, Json(reply)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn handle_mcp_get(State(state): State<RouteState>, headers: HeaderMap) -> Response {
    if let Err(rej) = state.server.verify_request(state.bound_port, &headers) {
        return rej.into_response();
    }
    // Streamable HTTP servers that do not offer an unsolicited GET SSE stream return 405.
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

async fn handle_mcp_delete(State(state): State<RouteState>, headers: HeaderMap) -> Response {
    if let Err(rej) = state.server.verify_request(state.bound_port, &headers) {
        return rej.into_response();
    }
    StatusCode::OK.into_response()
}

struct ConnPermit(Arc<AtomicUsize>);

impl Drop for ConnPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn try_acquire_conn(counter: &Arc<AtomicUsize>) -> Option<ConnPermit> {
    let mut cur = counter.load(Ordering::SeqCst);
    loop {
        if cur >= MAX_CONNECTIONS {
            return None;
        }
        match counter.compare_exchange_weak(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(ConnPermit(counter.clone())),
            Err(next) => cur = next,
        }
    }
}

async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    mut shutdown: oneshot::Receiver<()>,
    conns: Arc<AtomicUsize>,
) {
    use tower::ServiceExt;

    let (closing_tx, closing_rx) = watch::channel(false);
    let mut connections = tokio::task::JoinSet::new();
    loop {
        let (stream, _peer) = tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok(conn) => conn,
                Err(e) => {
                    warn!("local MCP server accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
            },
        };
        let Some(permit) = try_acquire_conn(&conns) else {
            drop(stream);
            continue;
        };
        let router = router.clone();
        let mut closing = closing_rx.clone();
        connections.spawn(async move {
            let _permit = permit;
            let service = hyper::service::service_fn(
                move |req: axum::extract::Request<hyper::body::Incoming>| {
                    router.clone().oneshot(req)
                },
            );
            let conn = hyper::server::conn::http1::Builder::new()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT)
                .serve_connection(hyper_util::rt::TokioIo::new(stream), service);
            tokio::pin!(conn);
            tokio::select! {
                _ = conn.as_mut() => {}
                () = async {
                    let _ = closing.wait_for(|c| *c).await;
                } => {
                    conn.as_mut().graceful_shutdown();
                    let _ = conn.await;
                }
            }
        });
        while connections.try_join_next().is_some() {}
    }
    drop(listener);
    let _ = closing_tx.send(true);
    while connections.join_next().await.is_some() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use kvistcode_core::{EventSink, KvistcodeEvent};
    use serde_json::json;

    struct NoopSink;
    impl EventSink for NoopSink {
        fn emit(&self, _event: KvistcodeEvent) {}
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kvistcode-local-mcp-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn http_post(
        addr: SocketAddr,
        host_override: Option<&str>,
        extra_headers: &[(&str, &str)],
        body: Value,
    ) -> (u16, String) {
        let url = format!("http://{addr}/mcp");
        let host_override = host_override.map(str::to_string);
        let extra: Vec<(String, String)> = extra_headers
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let payload = serde_json::to_vec(&body).unwrap();
        tokio::task::spawn_blocking(move || {
            if let Some(host) = host_override {
                // Raw HTTP/1.1 request so we can send an arbitrary Host header to test DNS rebinding defense.
                use std::io::{Read, Write};
                let mut stream = std::net::TcpStream::connect(addr).unwrap();
                let mut req = format!(
                    "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    payload.len()
                );
                for (k, v) in &extra {
                    req.push_str(&format!("{k}: {v}\r\n"));
                }
                req.push_str("\r\n");
                stream.write_all(req.as_bytes()).unwrap();
                stream.write_all(&payload).unwrap();
                let mut resp = String::new();
                stream.read_to_string(&mut resp).unwrap();
                let status = resp
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|c| c.parse::<u16>().ok())
                    .unwrap_or(0);
                let body = resp
                    .split_once("\r\n\r\n")
                    .map(|(_, b)| b.to_string())
                    .unwrap_or_default();
                return (status, body);
            }
            let agent = ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .build(),
            );
            let mut req = agent
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json");
            for (k, v) in &extra {
                req = req.header(k.as_str(), v.as_str());
            }
            let mut res = req.send(payload.as_slice()).unwrap();
            let status = res.status().as_u16();
            let text = res.body_mut().read_to_string().unwrap_or_default();
            (status, text)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn loopback_http_security_auth_rotation_and_tools() {
        let dir = temp_dir("e2e");
        let core = Core::open(dir.clone(), Arc::new(NoopSink)).unwrap();
        let server = LocalMcpServer::new(core.clone(), dir.clone());

        // Start on an OS-assigned loopback port (0) to avoid collisions in parallel tests.
        let tok = server.ensure_token().unwrap();
        let addr = server.start_on_port(0, false).await.unwrap();
        assert!(addr.ip().is_loopback());

        // Enable in settings (read tools on, write tools off)
        core.update_settings(|s| {
            s.local_mcp.enabled = true;
            s.local_mcp.port = addr.port();
            s.local_mcp.allow_read_tools = true;
            s.local_mcp.allow_write_tools = false;
        })
        .unwrap();

        let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });

        // 1. Unauthenticated request -> 401
        let (st, _) = http_post(addr, None, &[], ping.clone()).await;
        assert_eq!(st, 401);

        // 2. Wrong bearer token -> 401
        let (st, _) = http_post(
            addr,
            None,
            &[("Authorization", "Bearer wrong")],
            ping.clone(),
        )
        .await;
        assert_eq!(st, 401);

        // 3. DNS rebinding Host header -> 421
        let auth = format!("Bearer {tok}");
        let (st, _) = http_post(
            addr,
            Some("evil.example:47822"),
            &[("Authorization", &auth)],
            ping.clone(),
        )
        .await;
        assert_eq!(st, 421);

        // 4. Cross-origin browser request -> 403
        let (st, _) = http_post(
            addr,
            None,
            &[("Authorization", &auth), ("Origin", "https://evil.example")],
            ping.clone(),
        )
        .await;
        assert_eq!(st, 403);

        // 5. Cross-site Sec-Fetch-Site -> 403
        let (st, _) = http_post(
            addr,
            None,
            &[("Authorization", &auth), ("Sec-Fetch-Site", "cross-site")],
            ping.clone(),
        )
        .await;
        assert_eq!(st, 403);

        // 6. Valid initialize + notification (202) + tools/list + tools/call
        let (st, body) = http_post(
            addr,
            None,
            &[("Authorization", &auth)],
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "initialize",
                "params": { "protocolVersion": "2025-06-18" }
            }),
        )
        .await;
        assert_eq!(st, 200);
        let init: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "kvistcode");

        let (st, _) = http_post(
            addr,
            None,
            &[("Authorization", &auth)],
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await;
        assert_eq!(st, 202);

        let (st, body) = http_post(
            addr,
            None,
            &[("Authorization", &auth)],
            json!({ "jsonrpc": "2.0", "id": 11, "method": "tools/list" }),
        )
        .await;
        assert_eq!(st, 200);
        let listed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);

        // 7. Token rotation invalidates old token immediately and accepts new token
        let rotated_status = server.rotate_token().unwrap();
        let new_tok = rotated_status.token.unwrap();
        assert_ne!(new_tok, tok);

        let (st, _) = http_post(addr, None, &[("Authorization", &auth)], ping.clone()).await;
        assert_eq!(st, 401);

        let new_auth = format!("Bearer {new_tok}");
        let (st, _) = http_post(addr, None, &[("Authorization", &new_auth)], ping.clone()).await;
        assert_eq!(st, 200);

        // 7b. Kvistcode's own HTTP MCP client handshake works against our live loopback server.
        let mcp_entry = kvistcode_core::McpServer {
            id: "kvistcode-local-mcp".into(),
            name: "kvistcode".into(),
            enabled: true,
            transport: kvistcode_core::McpTransport::Http,
            command: String::new(),
            args: Vec::new(),
            env: Vec::new(),
            url: format!("http://{addr}/mcp"),
            headers: vec![kvistcode_core::KeyValue {
                name: "Authorization".into(),
                value: new_auth,
            }],
            project_path: String::new(),
        };
        let summary = core.test_mcp_server(mcp_entry).await.unwrap();
        assert!(summary.contains("Kvistcode"), "{summary}");
        assert!(summary.contains("4 tools"), "{summary}");

        // 8. Shutdown stops the server cleanly
        server.shutdown().await;
        assert!(!server.status().running);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_status_is_opt_in_off() {
        let dir = temp_dir("status-off");
        let core = Core::open(dir.clone(), Arc::new(NoopSink)).unwrap();
        let server = LocalMcpServer::new(core, dir.clone());
        let st = server.status();
        assert!(!st.enabled);
        assert!(!st.running);
        assert_eq!(st.port, kvistcode_core::local_mcp::DEFAULT_LOCAL_MCP_PORT);
        assert!(st.url.is_none());
        assert!(st.token.is_none());
        assert_eq!(st.tools_enabled.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
