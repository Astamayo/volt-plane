//! HTTP and WebSocket surface for the bench console.
//!
//! The browser only talks to this module. Hardware access stays behind
//! `bench::Bench`, which is the seam for the C++ HAL.

use std::path::PathBuf;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;

use crate::bench::{AppState, Snapshot};

pub fn router(state: AppState) -> Router {
    let web = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");
    Router::new()
        .route("/api/snapshot", get(snapshot))
        .route("/api/stream", get(stream))
        .route("/api/scan", post(scan))
        .route("/api/connect-all", post(connect_all))
        .route("/api/outputs/off", post(outputs_off))
        .route("/api/instruments/{id}/connect", post(connect))
        .route("/api/instruments/{id}/disconnect", post(disconnect))
        .route("/api/instruments/{id}/source", post(source))
        .route("/api/instruments/{id}/output", post(output))
        .route("/api/instruments/{id}/command", post(command))
        .route("/api/instruments/{id}/gpio", post(gpio))
        .route("/api/sequences/{id}/run", post(run_sequence))
        .route("/api/sequences/abort", post(abort_sequence))
        .route("/favicon.ico", get(favicon))
        .with_state(state)
        .fallback_service(ServeDir::new(web))
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn snapshot(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.snapshot().await)
}

async fn stream(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let mut rx = state.subscribe();
    let (mut sink, mut stream) = socket.split();
    let initial = serde_json::to_string(&state.snapshot().await).unwrap_or_else(|_| "{}".into());
    if sink.send(Message::Text(initial.into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if sink.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn scan(
    State(state): State<AppState>,
    Json(body): Json<ScanRequest>,
) -> Result<Json<Snapshot>, ApiError> {
    let bus = body.bus.as_deref();
    map_err(state.commit(|bench| bench.begin_scan(bus)).await)?;
    tokio::time::sleep(Duration::from_millis(450)).await;
    map_err(state.commit(|bench| bench.finish_scan(bus)).await)?;
    Ok(Json(state.snapshot().await))
}

async fn connect_all(State(state): State<AppState>) -> Json<Snapshot> {
    tokio::time::sleep(Duration::from_millis(180)).await;
    state.commit(|bench| bench.connect_all()).await;
    Json(state.snapshot().await)
}

async fn outputs_off(State(state): State<AppState>) -> Json<Snapshot> {
    state.commit(|bench| bench.outputs_off()).await;
    Json(state.snapshot().await)
}

async fn connect(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Snapshot>, ApiError> {
    tokio::time::sleep(Duration::from_millis(140)).await;
    map_err(state.commit(|bench| bench.connect(&id)).await)?;
    Ok(Json(state.snapshot().await))
}

async fn disconnect(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Snapshot>, ApiError> {
    map_err(state.commit(|bench| bench.disconnect(&id)).await)?;
    Ok(Json(state.snapshot().await))
}

async fn source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SourceBody>,
) -> Result<Json<Snapshot>, ApiError> {
    map_err(
        state
            .commit(|bench| {
                bench.set_source(&id, &body.channel, &body.mode, body.level, body.compliance)
            })
            .await,
    )?;
    Ok(Json(state.snapshot().await))
}

async fn output(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<OutputBody>,
) -> Result<Json<Snapshot>, ApiError> {
    map_err(
        state
            .commit(|bench| bench.set_output(&id, &body.channel, body.enabled))
            .await,
    )?;
    Ok(Json(state.snapshot().await))
}

async fn command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CommandBody>,
) -> Result<Json<CommandResponse>, ApiError> {
    let response = map_err(state.commit(|bench| bench.command(&id, &body.line)).await)?;
    Ok(Json(CommandResponse {
        response,
        snapshot: state.snapshot().await,
    }))
}

async fn gpio(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<GpioBody>,
) -> Result<Json<Snapshot>, ApiError> {
    map_err(
        state
            .commit(|bench| bench.set_gpio(&id, &body.line, body.on))
            .await,
    )?;
    Ok(Json(state.snapshot().await))
}

async fn run_sequence(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Snapshot>, ApiError> {
    map_err(state.commit(|bench| bench.prepare_sequence(&id)).await)?;
    crate::bench::spawn_sequence(state.clone(), id);
    Ok(Json(state.snapshot().await))
}

async fn abort_sequence(State(state): State<AppState>) -> Result<Json<Snapshot>, ApiError> {
    map_err(state.commit(|bench| bench.request_abort()).await)?;
    Ok(Json(state.snapshot().await))
}

fn map_err<T>(result: Result<T, String>) -> Result<T, ApiError> {
    result.map_err(|message| {
        let code = if message.contains("already") {
            StatusCode::CONFLICT
        } else if message.starts_with("unknown") || message.starts_with("no ") {
            StatusCode::NOT_FOUND
        } else if message.contains("running") {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        ApiError { code, message }
    })
}

struct ApiError {
    code: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.code, Json(serde_json::json!({ "message": self.message }))).into_response()
    }
}

#[derive(Deserialize)]
struct ScanRequest {
    bus: Option<String>,
}

#[derive(Deserialize)]
struct SourceBody {
    channel: String,
    mode: String,
    level: f64,
    compliance: f64,
}

#[derive(Deserialize)]
struct OutputBody {
    channel: String,
    enabled: bool,
}

#[derive(Deserialize)]
struct CommandBody {
    line: String,
}

#[derive(Deserialize)]
struct GpioBody {
    line: String,
    on: bool,
}

#[derive(Serialize)]
struct CommandResponse {
    response: String,
    snapshot: Snapshot,
}
