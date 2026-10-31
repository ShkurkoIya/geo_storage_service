use axum::{
    routing::{delete, get, post, put},
    Router,
};
use geo_core::nasa::NasaProvider;
use sqlx::PgPool;
use std::sync::Arc;

pub mod handlers;
pub mod models;
pub mod repository;

/// Атомарное состояние модуля пространственного анализа космических данных
pub struct ModuleState {
    pub db_pool: PgPool,
    pub nasa_provider: NasaProvider,
}

/// Собирает и инициализирует полную сеть HTTP API-эндпоинтов для `space_analytics`
pub fn routes(state: Arc<ModuleState>) -> Router {
    Router::new()
        // ============================================================================
        // 🌋 МАРШРУТЫ ДЛЯ ТЕРМАЛЬНЫХ АНОМАЛИЙ (THERMAL HOTSPOTS) -> /api/v1/analytics/hotspots
        // ============================================================================
        .nest(
            "/hotspots",
            Router::new()
                .route("/", post(handlers::create_thermal_hotspot_api))
                .route(
                    "/radius",
                    get(handlers::find_thermal_hotspots_within_radius_api),
                )
                .route(
                    "/bbox",
                    get(handlers::find_thermal_hotspots_within_bbox_api),
                )
                .route("/{id}", get(handlers::get_thermal_hotspot_by_id_api))
                .route("/{id}", put(handlers::update_thermal_hotspot_api))
                .route("/{id}", delete(handlers::delete_thermal_hotspot_api)),
        )
        // ============================================================================
        // 🌬️ МАРШРУТЫ ДЛЯ ГАЗОВЫХ ШЛЕЙФОВ (GAS PLUMES) -> /api/v1/analytics/plumes
        // ============================================================================
        .nest(
            "/plumes",
            Router::new()
                .route("/", post(handlers::create_gas_plume_api))
                .route("/bbox", get(handlers::find_gas_plumes_within_bbox_api))
                .route("/radius", get(handlers::find_gas_plumes_within_radius_api))
                .route("/{id}", get(handlers::get_gas_plume_by_id_api))
                .route("/{id}", put(handlers::update_gas_plume_api))
                .route("/{id}", delete(handlers::delete_gas_plume_api)),
        )
        // ============================================================================
        // 🌿 МАРШРУТЫ ДЛЯ ВОДОРОДНЫХ КОЛЬЦЕВЫХ СТРУКТУР (HYDROGEN RINGS) -> /api/v1/analytics/rings
        // ============================================================================
        .nest(
            "/rings",
            Router::new()
                .route("/", post(handlers::create_hydrogen_ring_api))
                .route("/", get(handlers::fetch_all_hydrogen_rings_api))
                .route("/bbox", get(handlers::find_hydrogen_rings_within_bbox_api))
                .route(
                    "/radius",
                    get(handlers::find_hydrogen_rings_within_radius_api),
                )
                .route("/{id}", get(handlers::get_hydrogen_ring_by_id_api))
                .route("/{id}", put(handlers::update_hydrogen_ring_api))
                .route("/{id}", delete(handlers::delete_hydrogen_ring_api)),
        )
        // ============================================================================
        // 🧬 МАРШРУТЫ КРОСС-СЛОЙНОЙ ГЕОАНАЛИТИКИ -> /api/v1/analytics/cross-layer
        // ============================================================================
        .nest(
            "/cross-layer",
            Router::new()
                // ГЛАВНЫЙ НАУЧНЫЙ ДВИЖОК: Выдача комплексных пересечений факторов на ГИС-карту
                .route(
                    "/hydrogen/prime-targets",
                    get(handlers::get_prime_hydrogen_targets_api),
                )
                // Анализ ST_Contains: Поиск точек тепла разломов строго внутри контура шлейфа газов
                .route(
                    "/plumes/{id}/hotspots",
                    get(handlers::find_hotspots_inside_plume_api),
                )
                // Привязка к лицензиям: Поиск шлейфов газов, пересекающих границы отвода (deposit)
                .route(
                    "/deposits/{id}/plumes",
                    get(handlers::find_gas_plumes_by_deposit_api),
                )
                // Анализ ST_DWithin: Поиск колец вегетации в буферной зоне от известных разломов
                .route(
                    "/rings/near-faults",
                    get(handlers::find_rings_near_faults_api),
                ),
        )
        // ============================================================================
        // 🗑️ СИСТЕМНЫЕ СЛУЖЕБНЫЕ МАРШРУТЫ -> /api/v1/analytics/entity
        // ============================================================================
        // Универсальный динамический деструктор записей по типу таблицы и UUID
        .route(
            "/entity/{entity_type}/{id}",
            delete(handlers::delete_space_entity_api),
        )
        // Инжектируем разделяемое состояние бэкенд-модуля
        .with_state(state)
}
