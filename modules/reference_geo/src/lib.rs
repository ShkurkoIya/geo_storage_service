pub mod handlers;
pub mod models;
pub mod repository;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use sqlx::PgPool;
use std::sync::Arc;

pub struct ModuleState {
    pub db_pool: PgPool,
}

/// Инициализирует полную сеть HTTP API-эндпоинтов для `reference_geo`
pub fn routes(state: Arc<ModuleState>) -> Router {
    Router::new()
        // =========================================================================
        // 🧪 1. ХИМИЧЕСКИЙ СПРАВОЧНИК СЫРЬЯ
        // =========================================================================
        .nest(
            "/chemistry",
            Router::new()
                .route("/", post(handlers::upsert_element_api))
                .route("/", get(handlers::get_chemistry_list_api))
                .route("/{code}", get(handlers::get_element_by_code_api))
                .route("/{code}", delete(handlers::delete_element_api)),
        )
        // =========================================================================
        // ⛰️ 2. ГОСУДАРСТВЕННАЯ ГЕОДЕЗИЧЕСКАЯ СЕТЬ (GGS)
        // =========================================================================
        .nest(
            "/ggs",
            Router::new()
                .route("/stream", post(handlers::stream_ggs_api))
                .route("/", post(handlers::create_ggs_node_api))
                .route("/", get(handlers::get_ggs_list_paginated_api))
                .route("/", put(handlers::update_ggs_node_api))
                .route("/search", get(handlers::filter_ggs_api))
                .route("/export", get(handlers::export_ggs_geojson_api))
                .route("/spatial/radius", get(handlers::find_ggs_radius_api))
                .route("/spatial/bbox", get(handlers::find_ggs_bbox_api))
                .route("/{id}", get(handlers::get_ggs_by_id_api))
                .route("/{id}", delete(handlers::delete_ggs_node_api)),
        )
        // =========================================================================
        // 📉 3. УТРАЧЕННЫЕ И ПОВРЕЖДЕННЫЕ МАРКШЕЙДЕРСКИЕ ПУНКТЫ (LOST)
        // =========================================================================
        .nest(
            "/lost",
            Router::new()
                .route("/stream", post(handlers::stream_lost_api))
                .route("/", post(handlers::create_lost_node_api))
                .route("/", get(handlers::get_lost_list_paginated_api))
                .route("/", put(handlers::update_lost_node_api))
                .route("/search", get(handlers::filter_lost_api))
                .route("/export", get(handlers::export_lost_geojson_api))
                .route("/spatial/radius", get(handlers::find_lost_radius_api))
                .route("/spatial/bbox", get(handlers::find_lost_bbox_api))
                .route("/{id}", get(handlers::get_lost_by_id_api))
                .route("/{id}", delete(handlers::delete_lost_node_api)),
        )
        // =========================================================================
        // 🎚️ 4. ВЫСОТНЫЕ НИВЕЛИРНЫЕ СЕТИ (NIVELER)
        // =========================================================================
        .nest(
            "/niveler",
            Router::new()
                .route("/stream", post(handlers::stream_niveler_api))
                .route("/", post(handlers::create_niveler_node_api))
                .route("/", get(handlers::get_niveler_list_paginated_api))
                .route("/", put(handlers::update_niveler_node_api))
                .route("/search", get(handlers::filter_niveler_api))
                .route("/export", get(handlers::export_niveler_geojson_api))
                .route("/spatial/radius", get(handlers::find_niveler_radius_api))
                .route("/spatial/bbox", get(handlers::find_niveler_bbox_api))
                .route("/{id}", get(handlers::get_niveler_by_id_api))
                .route("/{id}", delete(handlers::delete_niveler_node_api)),
        )
        // Вшиваем стейт модуля в собранную матрицу
        .with_state(state)
}
