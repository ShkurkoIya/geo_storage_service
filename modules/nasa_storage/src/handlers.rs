use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use uuid::Uuid;

use crate::{repository::NasaRepository, CrawlerSignal, DownloaderSignal, ModuleState};

// ============================================================================
// 🤖 CRAWLER HANDLERS (МОНИТОРИНГ NASA CMR)
// ============================================================================

/// **GET /api/v1/nasa/crawler/status**
///
/// Возвращает текущий операционный статус кроулера.
pub async fn get_crawler_status_api(State(state): State<Arc<ModuleState>>) -> impl IntoResponse {
    let is_running = state.crawler_is_running.load(Ordering::Relaxed);
    let processed_count = state.crawler_processed_scenes.load(Ordering::Relaxed);

    let status_info = serde_json::json!({
        "service": "nasa_cmr_crawler",
        "status": if is_running { "scanning_cmr_api" } else { "idle_waiting_timer" },
        "metrics": {
            "processed_scenes_current_session": processed_count,
        },
        "config": {
            "target_region": "Yakutia (May-September)",
            "monitored_collections": ["landsat-8-c2-l2", "sentinel-2-l2a"]
        }
    });

    (StatusCode::OK, Json(status_info))
}

/// **POST /api/v1/nasa/crawler/trigger**
///
/// Ручной пинок кроулера. Асинхронно отправляет сигнал воркеру
/// мгновенно начать опрос NASA CMR API вне очереди таймера.
pub async fn trigger_crawler_manually_api(
    State(state): State<Arc<ModuleState>>,
) -> impl IntoResponse {
    match state.crawler_tx.send(CrawlerSignal::TriggerNow).await {
        Ok(_) => {
            let response = serde_json::json!({
                "status": "success",
                "message": "Сигнал на принудительный сбор метаданных отправлен воркеру CMR"
            });
            (StatusCode::ACCEPTED, Json(response))
        }
        Err(e) => {
            let error_info = serde_json::json!({
                "status": "error",
                "message": "Не удалось разбудить воркер кроулера",
                "details": e.to_string()
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_info))
        }
    }
}

// ============================================================================
// 📥 DOWNLOADER HANDLERS (УПРАВЛЕНИЕ ОЧЕРЕДЬЮ COG)
// ============================================================================

/// **GET /api/v1/nasa/downloader/queue/stats**
///
/// Запрашивает из БД агрегированную статистику по состояниям задач в очереди.
pub async fn get_downloader_stats_api(State(state): State<Arc<ModuleState>>) -> impl IntoResponse {
    let repo = NasaRepository::new(&state.db_pool);

    match repo.get_queue_statistics().await {
        Ok(stats) => (StatusCode::OK, Json(stats)).into_response(),
        Err(e) => {
            let error_info = serde_json::json!({
                "error": "Ошибка сбора статистики очереди",
                "details": e.to_string()
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_info)).into_response()
        }
    }
}

/// **POST /api/v1/nasa/downloader/queue/retry/{id}**
///
/// Перезапуск конкретной упавшей задачи. Сбрасывает её статус в БД на `pending`,
/// обнуляет попытки и пинает воркер `downloader` для мгновенной обработки.
pub async fn retry_failed_task_api(
    State(state): State<Arc<ModuleState>>,
    Path(task_uuid): Path<Uuid>,
) -> impl IntoResponse {
    let repo = NasaRepository::new(&state.db_pool);

    // 1. Атомарно откатываем статус задачи в Postgres обратно в pending
    if let Err(e) = repo.force_retry_task(task_uuid).await {
        let error_info = serde_json::json!({
            "status": "error",
            "message": "Не удалось обновить статус задачи в СУБД",
            "details": e.to_string()
        });
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_info)).into_response();
    }

    // 2. Отправляем сигнал в канал даунлоадера, чтобы он не спал, а сразу пошел в базу
    match state
        .downloader_tx
        .send(DownloaderSignal::RetryTask(task_uuid))
        .await
    {
        Ok(_) => {
            let success_info = serde_json::json!({
                "status": "queued",
                "granule_id": task_uuid,
                "message": "Задача успешно возвращена в обработку и воркер оповещен"
            });
            (StatusCode::OK, Json(success_info)).into_response()
        }
        Err(e) => {
            // Если канал закрыт, статус в БД все равно изменился (обработается по таймеру),
            // но пользователя предупреждаем, что мгновенный пинок не прошел.
            let warning_info = serde_json::json!({
                "status": "warning",
                "granule_id": task_uuid,
                "message": "Статус в БД сброшен, но фоновый канал занят. Задача подхватится по таймеру.",
                "details": e.to_string()
            });
            (StatusCode::ACCEPTED, Json(warning_info)).into_response()
        }
    }
}
