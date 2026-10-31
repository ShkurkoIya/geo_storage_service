use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::mpsc;

use geo_core::nasa::NasaProvider;
use nasa_storage::{
    config::NasaStorageConfig, routes, telemetry::init_telemetry, CrawlerSignal, DownloaderSignal,
    ModuleState,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // ЗАГРУЗКА И ДЕКЛАРАТИВНЫЙ ПАРСИНГ КОНФИГУРАЦИИ
    // Собирает слои: config/base.toml -> .env -> переменные окружения
    let config = match NasaStorageConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "❌ Критическая ошибка: Не удалось инициализировать конфигурацию сервиса: {:?}",
                e
            );
            std::process::exit(1);
        }
    };

    // ИНИЦИАЛИЗАЦИЯ ГЛОБАЛЬНОГО ДИСПЕТЧЕРА ЛОГОВ (TRACING)
    // Взводит subscriber. Все макросы tracing::* во всех модулях воркспейса пишут в stdout.
    init_telemetry(&config.telemetry);

    tracing::info!("📡 Запуск изолированного микросервиса [NASA Storage Server]...");

    // АСИНХРОННОЕ ПОДКЛЮЧЕНИЕ К СУБД POSTGRES
    tracing::info!(target_db = %config.database_url, "Установка соединения с Postgres пулом...");
    let pool = match PgPool::connect(&config.database_url).await {
        Ok(p) => {
            tracing::info!("💾 Успешное подключение к СУБД PostGIS. Слой хранения активен.");
            p
        }
        Err(e) => {
            tracing::error!(error = %e, "❌ Не удалось подключиться к базе данных. Завершение процесса.");
            std::process::exit(1);
        }
    };

    // ИНИЦИАЛИЗАЦИЯ МЕЖПОТОЧНЫХ КАНАЛОВ УПРАВЛЕНИЯ (MPSC)
    // Размер буфера 32 защищает оперативку от переполнения при лавинообразных сигналах из API
    let (crawler_tx, crawler_rx) = mpsc::channel::<CrawlerSignal>(32);
    let (downloader_tx, downloader_rx) = mpsc::channel::<DownloaderSignal>(32);

    // ИНИЦИАЛИЗАЦИЯ ИНФРАСТРУКТУРНОГО ПРОВАЙДЕРА ИЗ GEO_CORE
    // Передаем API-ключ, выгруженный из типизированного конфига
    let provider = NasaProvider::new(config.nasa_api_key.clone());

    // СБОРКА АТОМАРНОГО СОСТОЯНИЯ МОДУЛЯ (MODULE STATE)
    let state = Arc::new(ModuleState {
        db_pool: pool.clone(),
        nasa_provider: provider,
        crawler_tx,
        downloader_tx,
        target_roi: config.geo.bounding_box.clone(),
        config: config.clone(),
        crawler_is_running: AtomicBool::new(false),
        crawler_processed_scenes: AtomicU64::new(0),
    });

    // ВЗРЫВАЕМ (SPAWN) ФОНОВЫЙ ЦИКЛ КРОУЛЕРА (NASA CMR MONITOR)
    // Запускаем асинхронный воркер в отдельном Tokio-таске
    let crawler_state = state.clone();
    tracing::info!("🤖 Активация фонового демона Кроулера (опрос NASA CMR API)...");
    tokio::spawn(async move {
        nasa_storage::crawler::run_background_loop(crawler_state, crawler_rx).await;
    });

    // ВЗРЫВАЕМ ФОНОВЫЙ ЦИКЛ ЗАГРУЗЧИКА (COG DOWNLOADER QUEUE)
    // Запускаем воркер очереди скачивания по паттерну FOR UPDATE SKIP LOCKED
    let downloader_state = state.clone();
    tracing::info!("📥 Активация фонового демона Загрузчика (обработка очереди COG)...");
    tokio::spawn(async move {
        nasa_storage::downloader::run_background_loop(downloader_state, downloader_rx).await;
    });

    // ПОДНИМАЕМ ИЗОЛИРОВАННЫЙ HTTP API СЕРВЕР AXUM
    // Подключаем роуты, экспортируемые из нашей библиотеки nasa_storage::routes
    let app = routes(state);

    // Выделенный операционный порт для нашего standalone-сервиса
    let addr = "0.0.0.0:8085";
    tracing::info!(
        "🌐 Операционный HTTP API микросервиса доступен по адресу: http://{}",
        addr
    );

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "❌ Не удалось занять порт {}. Завершение процесса.", addr);
            return Err(e.into());
        }
    };

    // Запускаем бесконечный рантайм Axum
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!(error = %e, "❌ Критический сбой внутри HTTP сервера Axum");
        return Err(e.into());
    }

    Ok(())
}
