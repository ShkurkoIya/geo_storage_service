mod config;
mod handlers;
mod models;
mod storage;

use config::AppConfig;
use handlers::server::{
    handle_calibration, handle_health, import_local_ggs, import_local_lost, import_local_niveler,
    stream_ggs_api, stream_lost_api, stream_niveler_api, AppState,
};
use storage::db::DbManager;

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, info_span};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use geo_core::GeoCalibrator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env()?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "geo_storage_service=info,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let main_span = info_span!("runtime_init");
    let _enter = main_span.enter();

    info!("🚀 Инициализация промышленного рантайма geo_storage_service...");

    let pool = DbManager::create_pool(&config.database_url).await?;
    info!("📡 Пул соединений к СУБД PostGIS успешно развернут.");

    DbManager::run_migrations(&pool).await?;

    // 5. Упаковываем калибратор и пул БД в состояние (State)
    let app_state = Arc::new(AppState {
        calibrator: Box::new(GeoCalibrator::new(1.0)),
        db_pool: pool,
    });

    let app = AppState::configure_routes(app_state);

    let listener = TcpListener::bind(&config.server_address).await?;
    info!(
        "API успешно запущено: http://{}",
        config.server_address
    );

    axum::serve(listener, app).await?;
    Ok(())
}
