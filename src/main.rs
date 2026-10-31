mod config;
mod server;

use config::AppConfig;
use tracing::{info, info_span};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env()?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "geo_api_gateway=info,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let main_span = info_span!("runtime_init");
    let _enter = main_span.enter();

    info!("🚀 Инициализация модульного монолита GeoApiGateway...");

    server::run_server(config).await;

    Ok(())
}
