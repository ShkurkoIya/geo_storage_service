use axum::{routing::get, Json, Router};
use geo_core::nasa::NasaProvider;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

use crate::config::AppConfig;

/// **Точка сборки модульного монолита МГТС**
///
/// Метод принимает объект конфигурации `AppConfig`, настраивает пул соединений СУБД,
/// инициализирует изолированные стейты для каждого доменного крейта воркспейса,
/// монтирует их маршруты через метод `.routes()` и запускает веб-сервер.
pub async fn run_server(config: AppConfig) {
    // Инициализируем пул соединений Postgres, используя лимиты из твоего конфига
    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .connect(&config.database_url)
        .await
        .expect("❌ Критический сбой: не удалось подключиться к СУБД PostgreSQL");

    let token = std::env::var("NASA_API_KEY").unwrap_or_else(|_| "mock_token_for_dev".to_string());

    if token == "mock_token_for_dev" {
        println!("⚠️ Предупреждение: NASA_API_KEY не найден в окружении. Запрос уйдет АНОНИМНО.");
    } else {
        println!(
            "✅ Токен авторизации успешно загружен (длина: {} симв.)",
            token.len()
        );
    }
    let nasa_provider = NasaProvider::new(token);

    // Инициализируем изолированный стейт и роутер для Государственной геодезии (reference_geo)
    let ref_geo_state = Arc::new(reference_geo::ModuleState {
        db_pool: pool.clone(),
    });
    let ref_geo_router = reference_geo::routes(ref_geo_state);

    // Инициализируем изолированный стейт и роутер для Космоаналитики (space_analytics)
    let space_analytics_state = Arc::new(space_analytics::ModuleState {
        db_pool: pool.clone(),
        nasa_provider: nasa_provider.clone(),
    });
    let space_analytics_router = space_analytics::routes(space_analytics_state);

    // Инициализируем изолированный стейт и роутер для Геологии Недр (core_storage)
    let core_storage_state = Arc::new(core_storage::ModuleState {
        db_pool: pool.clone(),
    });
    let core_storage_router = core_storage::routes(core_storage_state);

    // Склеиваем независимые роутеры в единую матрицу путей с помощью префиксов .nest()
    let app_router = Router::new()
        // Системный эндпоинт оркестратора
        .route("/health", get(handle_health))
        // Монтируем домен Государственной геодезической опоры (схемы reference_geo)
        .nest("/api/v1/reference", ref_geo_router)
        // Монтируем домен Космоаналитики и калибровки снимков (схемы science_degas)
        .nest("/api/v1/analytics", space_analytics_router)
        // Монтируем домен Промышленной геологии и 3D симуляции недр (схемы core_storage)
        .nest("/api/v1/geology", core_storage_router);

    // Поднимаем сетевой стек, используя адрес сервера из твоего конфига
    let listen_address = &config.server_address;
    let listener = tokio::net::TcpListener::bind(listen_address)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "❌ Критический сбой: не удалось занять сетевой адрес {}",
                listen_address
            )
        });

    tracing::info!(
        "🚀 Api Gateway успешно запущен на порту http://{}...",
        listen_address
    );

    axum::serve(listener, app_router)
        .await
        .expect("❌ Критический сбой при работе веб-сервера axum");
}

// =========================================================================
// 🩺 ГЛОБАЛЬНЫЕ СИСТЕМНЫЕ ЭНДПОИНТЫ ORCHESTRATOR LAYER
// =========================================================================

/// Инфраструктурный Healthcheck оркестратора
pub async fn handle_health() -> impl axum::response::IntoResponse {
    (
        axum::http::StatusCode::OK,
        Json(json!({
            "status": "up",
            "gateway_layer": "active",
            "service": "mgts_modular_monolith"
        })),
    )
}
