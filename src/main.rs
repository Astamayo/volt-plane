mod api;
mod bench;

use std::net::SocketAddr;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4173);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let state = bench::AppState::new();
    state.spawn_ticker();

    let app = api::router(state);
    let listener = TcpListener::bind(addr)
        .await
        .unwrap_or_else(|err| panic!("bind {addr}: {err}"));
    tracing::info!("Volt Plane console at http://{addr}");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|err| panic!("server: {err}"));
}
