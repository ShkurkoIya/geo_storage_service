use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use geo_core::infra::params::{BboxParams, RadiusParams};

use crate::models::{
    CreateGasPlumeRequest, CreateHydrogenRingRequest, CreateThermalHotspotRequest,
    GasPlumeFilterQuery,
};
use crate::repository::SpaceAnalyticsRepository;
use crate::ModuleState;

// ============================================================================
// 🌋 THERMAL HOTSPOTS: HTTP HANDLERS (CRUD & SPATIAL API)
// ============================================================================

/// **POST `/api/v1/space/hotspots`**
///
/// Эндпоинт для регистрации новой термальной аномалии (выхода глубинного тепла).
/// Сюда будут пушить данные твои ИИ-пайплайны после калибровки ночных снимков NASA.
pub async fn create_thermal_hotspot_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<CreateThermalHotspotRequest>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.insert_thermal_hotspot(payload).await {
        Ok(id) => (StatusCode::CREATED, Json(id)).into_response(),
        Err(e) => {
            eprintln!("❌ [Space Analytics API] Ошибка инсерта hotspot: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/hotspots/:id`**
///
/// Возвращает детальную информацию о конкретной термальной точке по её UUID.
pub async fn get_thermal_hotspot_by_id_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_hotspot_by_id(id).await {
        Ok(Some(hotspot)) => (StatusCode::OK, Json(hotspot)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json("Заданная термальная точка не найдена в системе"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **GET `/api/v1/space/hotspots/radius`**
///
/// Пространственный поиск термальных аномалий в радиусе метров вокруг гео-точки.
/// Напрямую использует структуру `RadiusParams` из твоего ядра core [image_6N4y6X.png].
pub async fn find_thermal_hotspots_within_radius_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<RadiusParams>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_thermal_hotspots_within_radius(params).await {
        Ok(hotspots) => (StatusCode::OK, Json(hotspots)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка поиска hotspots в радиусе: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/hotspots/bbox`**
///
/// Динамический пространственный поиск точек, попавших в текущую рамку экрана монитора.
/// Напрямую использует структуру `BBoxParams` из твоего ядра core [image_6N4y6X.png].
/// Идеально подходит для плагинов QGIS и веб-карт фронтенда.
pub async fn find_thermal_hotspots_within_bbox_api(
    State(state): State<Arc<ModuleState>>,
    Query(bbox): Query<BboxParams>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_hotspots_within_bbox(bbox).await {
        Ok(hotspots) => (StatusCode::OK, Json(hotspots)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка поиска hotspots в BBox: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// Структура входящего JSON-пайлоада для калибровки метрик тепла
#[derive(serde::Deserialize)]
pub struct UpdateThermalMetricsPayload {
    pub temp_kelvin: f64,
    pub is_night_scene: bool,
}

/// **PUT `/api/v1/space/hotspots/:id`**
///
/// Обновление физических характеристик аномалии (градусов Кельвина и признака времени суток).
pub async fn update_thermal_hotspot_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateThermalMetricsPayload>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo
        .update_hotspot_metrics(id, payload.temp_kelvin, payload.is_night_scene)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json("Метрики термальной точки успешно обновлены"),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json("Точка с указанным ID не найдена в базе данных"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **DELETE `/api/v1/space/hotspots/:id`**
///
/// Физическое жесткое удаление термальной аномалии из системы.
pub async fn delete_thermal_hotspot_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.delete_hotspot(id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(), // 204 Успешно удалено, тела ответа нет
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json("Точка с указанным ID не найдена в базе данных"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// ============================================================================
// 🌬️ GAS PLUMES: HTTP HANDLERS (CRUD & SPATIAL API)
// ============================================================================

/// **POST `/api/v1/space/plumes`**
///
/// Эндпоинт для импорта контура нового газового шлейфа (метана или водорода).
/// Принимает геометрию в текстовом формате WKT от ИИ-моделей.
pub async fn create_gas_plume_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<CreateGasPlumeRequest>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.insert_gas_plume(payload).await {
        Ok(id) => (StatusCode::CREATED, Json(id)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка инсерта шлейфа газа: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/plumes/:id`**
///
/// Извлекает из базы детальный контур конкретного шлейфа по UUID в формате GeoJSON.
pub async fn get_gas_plume_by_id_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_gas_plume_by_id(id).await {
        Ok(Some(plume)) => (StatusCode::OK, Json(plume)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json("Указанный газовый шлейф не найден"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **GET `/api/v1/space/plumes/bbox`**
///
/// Нарезает и отдает контуры газовых облаков (MultiPolygon) строго под рамку текущего экрана карты.
/// Поддерживает опциональный Query-фильтр по типу газа `?gas_type=CH4` [image_6N4y6X.png].
pub async fn find_gas_plumes_within_bbox_api(
    State(state): State<Arc<ModuleState>>,
    Query(bbox): Query<BboxParams>,
    Query(filter): Query<GasPlumeFilterQuery>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo
        .find_gas_plumes_within_bbox(bbox, filter.gas_type)
        .await
    {
        Ok(plumes) => (StatusCode::OK, Json(plumes)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка поиска шлейфов в BBox: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/plumes/radius`**
///
/// Пространственный поиск шлейфов газов в радиусе вокруг гео-точки (использует сферические метры ядра) [image_6N4y6X.png].
pub async fn find_gas_plumes_within_radius_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<RadiusParams>,
    Query(filter): Query<GasPlumeFilterQuery>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo
        .find_gas_plumes_within_radius(params, filter.gas_type)
        .await
    {
        Ok(plumes) => (StatusCode::OK, Json(plumes)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка поиска шлейфов в радиусе: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// Структура входящего JSON для калибровки шлейфа газов
#[derive(serde::Deserialize)]
pub struct UpdateGasMetricsPayload {
    pub pixel_value_max: Option<f64>,
    pub pixel_value_avg: Option<f64>,
    pub confidence_score: Option<i32>,
}

/// **PUT `/api/v1/space/plumes/:id`**
///
/// Обновляет ИИ-метрики плотности и достоверности шлейфа без перезаписи геометрии.
pub async fn update_gas_plume_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateGasMetricsPayload>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo
        .update_gas_plume_metrics(
            id,
            payload.pixel_value_max,
            payload.pixel_value_avg,
            payload.confidence_score,
        )
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json("Метрики газового шлейфа успешно обновлены"),
        )
            .into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json("Шлейф с таким ID не найден")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **DELETE `/api/v1/space/plumes/:id`**
///
/// Удаление контура газовой аномалии по её UUID.
pub async fn delete_gas_plume_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.delete_gas_plume(id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json("Шлейф с таким ID не найден")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// ============================================================================
// 🌿 HYDROGEN RINGS: HTTP HANDLERS (CRUD & SPATIAL API)
// ============================================================================

/// **POST `/api/v1/space/rings`**
///
/// Регистрирует новую кольцевую структуру ("ведьмино кольцо" угнетения тайги).
/// Принимает контур в формате WKT от твоих ИИ-моделей сегментации.
pub async fn create_hydrogen_ring_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<CreateHydrogenRingRequest>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.insert_hydrogen_ring(payload).await {
        Ok(id) => (StatusCode::CREATED, Json(id)).into_response(),
        Err(e) => {
            eprintln!("❌ [Space Analytics API] Ошибка инсерта кольца: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/rings/:id`**
///
/// Извлекает из базы детальный контур конкретного кольца по UUID в формате GeoJSON.
pub async fn get_hydrogen_ring_by_id_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_hydrogen_ring_by_id(id).await {
        Ok(Some(ring)) => (StatusCode::OK, Json(ring)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json("Указанное водородное кольцо не найдено"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **GET `/api/v1/space/rings`**
///
/// Возвращает полный глобальный реестр всех зафиксированных круговых структур (базовый листинг).
pub async fn fetch_all_hydrogen_rings_api(
    State(state): State<Arc<ModuleState>>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.fetch_all_hydrogen_rings().await {
        Ok(rings) => (StatusCode::OK, Json(rings)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **GET `/api/v1/space/rings/bbox`**
///
/// Вырезает и отдает контуры колец вегетации (Polygon) строго под рамку экрана монитора QGIS.
/// Переиспользует твой `BBoxParams` из ядра [image_6N4y6X.png].
pub async fn find_hydrogen_rings_within_bbox_api(
    State(state): State<Arc<ModuleState>>,
    Query(bbox): Query<BboxParams>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_hydrogen_rings_within_bbox(bbox).await {
        Ok(rings) => (StatusCode::OK, Json(rings)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка поиска колец в BBox: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/rings/radius`**
///
/// Ищет кольцевые структуры в радиусе метров вокруг гео-точки (использует твой `RadiusParams` из ядра) [image_6N4y6X.png].
pub async fn find_hydrogen_rings_within_radius_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<RadiusParams>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_hydrogen_rings_within_radius(params).await {
        Ok(rings) => (StatusCode::OK, Json(rings)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка поиска колец в радиусе: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// Структура входящего JSON-пайлоада для ИИ-коррекции параметров кольца
#[derive(serde::Deserialize)]
pub struct UpdateRingMetricsPayload {
    pub ring_diameter_meters: Option<f64>,
    pub ndvi_drop_percentage: Option<f64>,
    pub geological_setting: Option<String>,
}

/// **PUT `/api/v1/space/rings/:id`**
///
/// Калибрует физические и спектральные метрики водородного круга.
pub async fn update_hydrogen_ring_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateRingMetricsPayload>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo
        .update_hydrogen_ring_metrics(
            id,
            payload.ring_diameter_meters,
            payload.ndvi_drop_percentage,
            payload.geological_setting,
        )
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json("Метрики водородного кольца успешно обновлены"),
        )
            .into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json("Кольцо с таким ID не найдено")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **DELETE `/api/v1/space/rings/:id`**
///
/// Физическое удаление кольцевой структуры по UUID.
pub async fn delete_hydrogen_ring_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.delete_hydrogen_ring(id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json("Кольцо с таким ID не найдено")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// **GET `/api/v1/space/hydrogen/prime-targets`**
///
/// ГИС-движок бэкенда: возвращает итоговые высокоперспективные водородные цели.
/// Метод выдергивает данные из пространственного SQL View, где пересекаются
/// геометрические контуры "ведьминых колец", шлейфы газов и разломы.
/// Геометрия отдается в чистом GeoJSON, готовом для отрисовки в QGIS.
pub async fn get_prime_hydrogen_targets_api(
    State(state): State<Arc<ModuleState>>,
) -> impl IntoResponse {
    // Инициализируем наш проверенный репозиторий
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.fetch_prime_hydrogen_targets().await {
        Ok(targets) => {
            // Возвращаем статус 200 OK и массив доменных целей в JSON
            (StatusCode::OK, Json(targets)).into_response()
        }
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка вычисления prime targets: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

// ============================================================================
// 🗑️ SYSTEM ENDPOINTS (DYNAMIC OPERATIONS)
// ============================================================================

/// **DELETE `/api/v1/space/entity/:entity_type/:id`**
///
/// Универсальный "уборщик" для системных воркеров и ИИ-скриптов.
/// Позволяет одной ручкой удалять аномалии из разных таблиц (`hotspots`, `plumes`, `rings`).
pub async fn delete_space_entity_api(
    State(state): State<Arc<ModuleState>>,
    Path((entity_type, id)): Path<(String, Uuid)>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.delete_entity(&entity_type, id).await {
        Ok(affected) if affected > 0 => StatusCode::NO_CONTENT.into_response(), // 204 Успешно стерто
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json("Запись с указанным ID в данной таблице не найдена"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// ============================================================================
// 🧬 CROSS-LAYER ANALYTICS: HTTP HANDLERS (ADVANCED GEOPROCESSING)
// ============================================================================

/// **GET `/api/v1/space/analytics/plumes/:id/hotspots`**
///
/// Находит эпицентры тепла разломов, которые находятся *внутри* контура конкретного газового облака.
/// Доказывает причинно-следственную связь утечки газов.
pub async fn find_hotspots_inside_plume_api(
    State(state): State<Arc<ModuleState>>,
    Path(plume_id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_hotspots_inside_plume(plume_id).await {
        Ok(hotspots) => (StatusCode::OK, Json(hotspots)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка анализа ST_Contains для шлейфа: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// **GET `/api/v1/space/analytics/deposits/:id/plumes`**
///
/// Находит все контуры газов, пересекающие границы конкретного лицензионного участка (`deposit`).
/// Критично для планирования заложения поисковых скважин недропользователем.
pub async fn find_gas_plumes_by_deposit_api(
    State(state): State<Arc<ModuleState>>,
    Path(deposit_id): Path<Uuid>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_gas_plumes_by_deposit(deposit_id).await {
        Ok(plumes) => (StatusCode::OK, Json(plumes)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка пересечения шлейфов с лицензией: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}

/// Входящие query-параметры для буферного ГИС-анализа
#[derive(serde::Deserialize)]
pub struct DistanceQueryParam {
    pub max_distance_meters: f64,
}

/// **GET `/api/v1/space/analytics/rings/near-faults`**
///
/// Находит водородные кольца ("ведьмины круги"), расположенные в буферной зоне активных разломов.
/// Расстояние передается параметром в метрах (например, `?max_distance_meters=5000.0`).
pub async fn find_rings_near_faults_api(
    State(state): State<Arc<ModuleState>>,
    Query(param): Query<DistanceQueryParam>,
) -> impl IntoResponse {
    let repo = SpaceAnalyticsRepository::new(&state.db_pool);

    match repo.find_rings_near_faults(param.max_distance_meters).await {
        Ok(rings) => (StatusCode::OK, Json(rings)).into_response(),
        Err(e) => {
            eprintln!(
                "❌ [Space Analytics API] Ошибка буферного анализа ST_DWithin: {:?}",
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response()
        }
    }
}
