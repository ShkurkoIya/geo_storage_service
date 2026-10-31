use axum::{
    routing::{get, post},
    Router,
};
use geo_core::nasa::NasaProvider;
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::mpsc;

pub mod config;
pub mod crawler;
pub mod downloader;
pub mod handlers;
pub mod models;
pub mod repository;
pub mod telemetry;
pub mod utils;

/// Сигналы для ручного управления фоновыми демонами
pub enum CrawlerSignal {
    TriggerNow,
}

pub enum DownloaderSignal {
    RetryTask(uuid::Uuid),
}

/// Атомарное состояние модуля управления данными NASA
pub struct ModuleState {
    pub db_pool: PgPool,
    pub nasa_provider: NasaProvider,
    // Каналы для отправки пинков из Axum-ручек в фоновые циклы
    pub crawler_tx: mpsc::Sender<CrawlerSignal>,
    pub downloader_tx: mpsc::Sender<DownloaderSignal>,
    /// Bbox чаво качаем с насы
    pub target_roi: geo_types::Rect,

    // --- Статистика ---
    /// Флаг активной работы кроулера прямо сейчас (`true` - сканирует CMR, `false` - спит)
    pub crawler_is_running: AtomicBool,
    /// Сколько всего насовских сцен (гранул) было найдено и обработано за текущую сессию
    pub crawler_processed_scenes: AtomicU64,

    /// Инжектим конфиг
    pub config: config::NasaStorageConfig,
}

pub fn routes(state: Arc<ModuleState>) -> Router {
    Router::new()
        // ============================================================================
        // 🤖 УПРАВЛЕНИЕ КРОУЛЕРОМ (CMR NASA MONITORS) -> /api/v1/nasa/crawler
        // ============================================================================
        .nest(
            "/crawler",
            Router::new()
                .route("/status", get(handlers::get_crawler_status_api))
                .route("/trigger", post(handlers::trigger_crawler_manually_api)),
        )
        // ============================================================================
        // 📥 УПРАВЛЕНИЕ ЗАГРУЗЧИКОМ (COG DOWNLOAD QUEUE) -> /api/v1/nasa/downloader
        // ============================================================================
        .nest(
            "/downloader",
            Router::new()
                .route("/queue/stats", get(handlers::get_downloader_stats_api))
                .route("/queue/retry/{id}", post(handlers::retry_failed_task_api)),
        )
        .with_state(state)
}

