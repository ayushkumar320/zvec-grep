use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header::HOST, uri::Authority},
    routing::{get, post},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tracing::info;
use zg_engine::OperationExecutor;
use zg_transport_mcp::AgentMcpServer;

use crate::{DaemonError, ServerConfig, controller::InstanceLock};

#[derive(Clone)]
struct ControlState {
    shutdown: CancellationToken,
}

pub(crate) async fn run_server(
    config: ServerConfig,
    executor: Arc<dyn OperationExecutor>,
) -> Result<(), DaemonError> {
    let mut instance = InstanceLock::acquire(&config).await?;
    let listener = match tokio::net::TcpListener::bind(config.listen.socket_addr()).await {
        Ok(listener) => listener,
        Err(error) => {
            instance.release().await?;
            return Err(error.into());
        }
    };
    let shutdown = CancellationToken::new();
    let mcp_config = StreamableHttpServerConfig::default()
        .with_cancellation_token(shutdown.child_token())
        .with_allowed_hosts([
            "localhost".to_owned(),
            "127.0.0.1".to_owned(),
            "::1".to_owned(),
            config.listen.socket_addr().to_string(),
        ])
        .with_max_request_body_bytes(1024 * 1024);
    let handler_executor = Arc::clone(&executor);
    let mcp_service = StreamableHttpService::new(
        move || Ok(AgentMcpServer::new(Arc::clone(&handler_executor))),
        LocalSessionManager::default().into(),
        mcp_config,
    );
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/control/shutdown", post(request_shutdown))
        .nest_service("/mcp", mcp_service)
        .with_state(ControlState {
            shutdown: shutdown.clone(),
        });

    if let Err(error) = instance.mark_ready().await {
        instance.release().await?;
        return Err(error);
    }
    info!(url = %config.listen.server_url(), "zvec-grep daemon ready");
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        signal_shutdown.cancel();
    });
    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown.clone().cancelled_owned())
        .await;
    shutdown.cancel();
    let release_result = instance.release().await;
    serve_result?;
    release_result
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn request_shutdown(
    State(state): State<ControlState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !has_loopback_host(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        );
    }
    state.shutdown.cancel();
    (StatusCode::ACCEPTED, Json(json!({ "status": "stopping" })))
}

fn has_loopback_host(headers: &HeaderMap) -> bool {
    headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<Authority>().ok())
        .is_some_and(|authority| matches!(authority.host(), "localhost" | "127.0.0.1" | "::1"))
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let terminate = signal(SignalKind::terminate());
    if let Ok(mut terminate) = terminate {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
