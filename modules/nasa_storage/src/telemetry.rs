use crate::config::TelemetryConfig;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// **Инициализация слоя телеметрии (Tracing)**
///
/// Настраивает глобальный subscriber для асинхронного логирования приложения.
/// Принимает параметры конфигурации для определения уровня логов и формата вывода (Text / JSON).
pub fn init_telemetry(config: &TelemetryConfig) {
    // 1. Формируем фильтр на основе переданного log_level (например: "info", "debug")
    // Если в системе уже взведена переменная окружения RUST_LOG, приоритет отдается ей.
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    // 2. Инициализируем пайплайн вывода логов в зависимости от настроек окружения
    if config.json_output {
        // Промышленный структурированный JSON-формат для продакшена и Docker-контейнеров
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        // Человекочитаемый, красиво раскрашенный (ANSI) вывод в stdout для локальной разработки
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().with_ansi(true))
            .init();
    }

    // Первый живой лог в системе, подтверждающий успешный старт трейсинга
    tracing::info!("🎥 Системная телеметрия `tracing` успешно инициализирована");
}
