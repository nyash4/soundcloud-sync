mod config;
mod http;
mod ids;
mod protocol;
mod ratelimit;
mod room;
mod soundcloud;
mod store;
mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::http::AppState;
use crate::ratelimit::KeyedLimiter;
use crate::room::actor::Rooms;
use crate::soundcloud::SoundCloud;
use crate::store::Store;

const ROOM_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const ROOM_CREATIONS_PER_MINUTE: u32 = 10;
const IMPORTS_PER_MINUTE: u32 = 20;

/// Server time: Unix epoch milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let store = Store::open(&config.db_path)?;
    let removed = store
        .delete_unused_since(now_ms() - ROOM_RETENTION_MS)
        .await?;
    if removed > 0 {
        info!(removed, "deleted rooms unused for 30 days");
    }

    let soundcloud = SoundCloud::new(config.soundcloud.clone(), config.max_import_tracks)?;
    if !soundcloud.has_api() {
        warn!(
            "SC_CLIENT_ID/SC_CLIENT_SECRET not set: importing via oEmbed, clients expand collections"
        );
    }
    if !config.static_dir.join("index.html").is_file() {
        warn!(dir = %config.static_dir.display(), "static directory has no index.html; only the API will work");
    }

    let rooms = Rooms::new(store.clone(), config.timing);
    let addr = SocketAddr::new(config.host, config.port);
    let state = Arc::new(AppState {
        config,
        store,
        rooms: rooms.clone(),
        soundcloud,
        create_limiter: KeyedLimiter::per_minute(ROOM_CREATIONS_PER_MINUTE),
        import_limiter: KeyedLimiter::per_minute(IMPORTS_PER_MINUTE),
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("listening on http://{}", listener.local_addr()?);
    let app = http::router(state).into_make_service_with_connect_info::<SocketAddr>();
    tokio::select! {
        result = axum::serve(listener, app) => result?,
        () = shutdown_signal() => {
            info!("shutting down, saving rooms");
            rooms.flush_all().await;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
