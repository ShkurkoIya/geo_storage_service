use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tracing::{error, info};

use crate::handlers::import_ggs::GgsImportHandler;
use crate::handlers::simulation::SimulationHandler;

use crate::models::reference::{GgsNodeDomain, LostNodeDomain, NivelerNodeDomain};
use crate::storage::repository::GeoRepository;

// Импортируем типы ошибок и калибратора из твоего ядра
use geo_core::{CalibrationError, UniversalCalibrationRequest, UniversalCalibrator};

/// Глобальное состояние веб-сервера
pub struct AppState {
    pub calibrator: Box<dyn UniversalCalibrator + Send + Sync>,
    pub db_pool: Pool<Postgres>,
}

impl AppState {
    pub fn configure_routes(state: Arc<Self>) -> Router {
        // Изолированный подмаршрут для локального импорта файлов с диска [source: 2.3]
        let local_import_router = Router::new()
            .route("/ggs", post(import_local_ggs))
            .route("/lost", post(import_local_lost))
            .route("/niveler", post(import_local_niveler));

        // Изолированный подмаршрут для прямого Big Data стрима по сети [source: 2.3]
        let stream_import_router = Router::new()
            .route("/ggs", post(stream_ggs_api))
            .route("/lost", post(stream_lost_api))
            .route("/niveler", post(stream_niveler_api));

        let simulation_router = Router::new().route("/generate", post(handle_simulation_generate));

        Router::new()
            .route("/health", get(handle_health))
            .route("/api/v1/calibrate", post(handle_calibration))
            .nest("/api/v1/import/local", local_import_router)
            .nest("/api/v1/import/stream", stream_import_router)
            .nest("/api/v1/simulation", simulation_router)
            .with_state(state)
    }
}

/// Универсальный DTO-запрос для триггера локального файла с диска
#[derive(Deserialize)]
pub struct LocalImportRequest {
    pub file_path: String,
}

#[derive(Deserialize)]
pub struct SimulationRequest {
    pub name: String,
    pub center_longitude: f64,
    pub center_latitude: f64,
}

// =========================================================================
// 🩺 HEALTHCHECK & CALIBRATION ENDPOINTS
// =========================================================================

pub async fn handle_health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "status": "up", "service": "geo_storage_service" })),
    )
}

pub async fn handle_calibration(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UniversalCalibrationRequest>,
) -> Result<impl IntoResponse, AppError> {
    let result = state.calibrator.calibrate(payload)?;
    Ok((StatusCode::OK, Json(result)))
}

// =========================================================================
// 🚀 ЛОКАЛЬНЫЙ ИМПОРТ (ВАРИАНТ А: ТРИУГГЕР ФАЙЛОВ С ДИСКА СЕРВЕРА)
// =========================================================================

pub async fn import_local_ggs(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LocalImportRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);
    GgsImportHandler::import_ggs(&repo, &payload.file_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((
        StatusCode::OK,
        Json(json!({ "status": "success", "message": "Файл ГГС успешно обработан воркером" })),
    ))
}

pub async fn import_local_lost(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LocalImportRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);
    GgsImportHandler::import_lost(&repo, &payload.file_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((
        StatusCode::OK,
        Json(
            json!({ "status": "success", "message": "Карантин брака lost_nodes успешно импортирован" }),
        ),
    ))
}

pub async fn import_local_niveler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LocalImportRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);
    GgsImportHandler::import_niveler(&repo, &payload.file_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((
        StatusCode::OK,
        Json(
            json!({ "status": "success", "message": "Высотные эталоны нивелирования успешно загружены" }),
        ),
    ))
}

// =========================================================================
// 🌐 СЕТЕВОЙ ИМПОРТ (ВАРИАНТ Б: ПРЯМОЙ Big Data СТРИМ JSON ПО СЕТИ)
// =========================================================================

pub async fn stream_ggs_api(
    State(state): State<Arc<AppState>>,
    Json(nodes): Json<Vec<GgsNodeDomain>>, // 💥 ХАК: Серде парсит JSON и выдает стерильный Domain!
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);
    info!(
        "🌐 API Стрим: Получено {} пунктов ГГС по сети. Запись в PostGIS...",
        nodes.len()
    );

    let mut count = 0;
    for node in &nodes {
        repo.insert_ggs_node(node)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        count += 1;
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "status": "success", "imported_count": count })),
    ))
}

pub async fn stream_lost_api(
    State(state): State<Arc<AppState>>,
    Json(nodes): Json<Vec<LostNodeDomain>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);
    info!(
        "🌐 API Стрим: Получено {} lost_nodes по сети. Запись в PostGIS...",
        nodes.len()
    );

    let mut count = 0;
    for node in &nodes {
        repo.insert_lost_node(node)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        count += 1;
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "status": "success", "imported_count": count })),
    ))
}

pub async fn stream_niveler_api(
    State(state): State<Arc<AppState>>,
    Json(nodes): Json<Vec<NivelerNodeDomain>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);
    info!(
        "🌐 API Стрим: Получено {} реперов нивелирования по сети. Запись в PostGIS...",
        nodes.len()
    );

    let mut count = 0;
    for node in &nodes {
        repo.insert_niveler_node(node)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        count += 1;
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "status": "success", "imported_count": count })),
    ))
}

pub async fn handle_simulation_generate(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SimulationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GeoRepository::new(&state.db_pool);

    let body_id = SimulationHandler::generate_advanced_simulation(
        &repo,
        &payload.name,
        payload.center_longitude,
        payload.center_latitude,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "message": "3D-линза и сеть скважин успешно сгенерированы в схеме sandbox_simulation!",
            "synthetic_ore_body_id": body_id
        })),
    ))
}
// =========================================================================
// 🔥 МАППИНГ ОШИБОК API (AppError)
// =========================================================================
pub struct AppError(CalibrationError);

impl From<CalibrationError> for AppError {
    fn from(err: CalibrationError) -> Self {
        AppError(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match &self.0 {
            CalibrationError::InsufficientPoints { provided, required } => (
                StatusCode::BAD_REQUEST,
                "INSUFFICIENT_POINTS",
                format!(
                    "Недостаточно реперов. Передано: {}, нужно минимум: {}",
                    provided, required
                ),
            ),
            CalibrationError::InvalidGeometry => (
                StatusCode::BAD_REQUEST,
                "INVALID_GEOMETRY",
                "Ошибка геометрии: точки наложились друг на друга".to_string(),
            ),
            CalibrationError::ProjectionError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "PROJECTION_ENGINE_ERROR",
                format!("Критический сбой ГИС-движка PROJ: {}", msg),
            ),
            CalibrationError::ToleranceNotMet => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "TOLERANCE_NOT_MET",
                "Невязка сети превышает ваш допуск".to_string(),
            ),
            CalibrationError::UnsupportedTransformation => (
                StatusCode::BAD_REQUEST,
                "UNSUPPORTED_TRANSFORMATION",
                "Комбинация систем координат не поддерживается".to_string(),
            ),
        };

        error!("❌ Ошибка калибровки [{}]: {}", error_code, message);
        let body = Json(json!({ "status": "error", "code": error_code, "message": message }));
        (status, body).into_response()
    }
}
