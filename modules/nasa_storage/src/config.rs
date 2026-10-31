use geo_types::{coord, Rect};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, Deserialize)]
pub struct NasaStorageConfig {
    /// Логирование
    pub log_level: String,

    /// URL подключения к локальной базе данных Postgres
    pub database_url: String,

    /// API ключ для авторизации в NASA Earthdata
    pub nasa_api_key: String,

    /// Конфигурация локальной S3 / MiniIO хранилища для COG растров
    pub s3: S3StorageConfig,

    /// Географические параметры региона исследований (ROI)
    pub geo: GeoRoiConfig,

    /// Параметры тюнинга фоновых воркеров очереди
    pub workders: WorkerPoolConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    /// Уровень логирования: "trace", "debug", "info", "warn", "error"
    pub log_level: String,

    /// Выключить JSON-формать для продакшена
    pub json_output: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3StorageConfig {
    pub endpoint_url: String,
    pub bucket_name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerPoolConfig {
    /// Интервал планового запуска кроулера в часах
    pub crawler_interval_hours: u64,

    /// Таймаут сна загрузчика при пустой очереди в секундах
    pub downloader_sleep_secs: u64,

    /// Максимальное количество перебросок одной упавшей таски
    pub max_task_attempts: i32,
}

/// Кастомный десериализатор для безопасной конвертации String -> Rect
fn deserialize_bbox<'de, D>(deserializer: D) -> Result<Rect, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    let coords: Vec<f64> = s
        .split(',')
        .map(|val| val.trim().parse().map_err(serde::de::Error::custom))
        .collect::<Result<Vec<f64>, _>>()?;

    if coords.len() != 4 {
        return Err(serde::de::Error::custom(
            "Формат ROI_BBOX должен быть строго: min_lon,min_lat,max_lon,max_lat",
        ));
    }

    Ok(Rect::new(
        coord!(x: coords[0], y: coords[1]),
        coord!(x: coords[2], y: coords[3]),
    ))
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeoRoiConfig {
    /// Кастомный парсинг строки "min_lon,min_lat,max_lon,max_lat" в нативный тип Rect
    #[serde(deserialize_with = "deserialize_bbox")]
    pub bounding_box: Rect,

    /// Лимит облачности для первичного отсечения сцен (0-100)
    pub max_cloud_cover_percent: f32,
}

impl NasaStorageConfig {
    /// **Инициализация**: Собирает слои конфигурации.
    /// Приоритет: Системный .env > Файл config.toml > Дефолтные значения
    pub fn load() -> Result<Self, config::ConfigError> {
        let run_mode = std::env::var("RUN_MODE").unwrap_or_else(|_| "development".to_string());

        let s = config::Config::builder()
            // Подтягиваем дефолты базового конфига (если файла нет, сработают переменные окружения)
            .add_source(config::File::with_name("config/base").required(false))
            // Подтягиваем оверрайды под окружение (development / production)
            .add_source(config::File::with_name(&format!("config/{}", run_mode)).required(false))
            // Читаем локальный .env файл, если он есть
            .add_source(config::Environment::default().separator("__"))
            .build()?;

        s.try_deserialize()
    }
}
