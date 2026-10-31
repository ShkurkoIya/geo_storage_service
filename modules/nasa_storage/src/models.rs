use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// 🌋 МЕТАДАННЫЕ И СТРУКТУРЫ ДЛЯ CRAWLER
// ============================================================================

/// Карта целевых каналов растра, сохраняемая в Postgres как JSONB.
///
/// Маппит спектральные каналы NIR [B5], SWIR [B6, B7], тепловой канал TIRS [B10]
/// и системную маску качества/облачности QA_PIXEL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandUrls {
    #[serde(rename = "B5")]
    pub b5: String,
    #[serde(rename = "B6")]
    pub b6: String,
    #[serde(rename = "B7")]
    pub b7: String,
    #[serde(rename = "B10")]
    pub b10: String,
    #[serde(rename = "QA")]
    pub qa: String,
}

/// Структура для пакетной вставки новой накрытой области (гранулы) из CMR NASA
#[derive(Debug, Clone)]
pub struct NewGranule {
    pub nasa_granule_id: String,
    pub collection_id: String,
    pub acquisition_date: DateTime<Utc>,
    pub cloud_cover: f32,
    /// Строка геометрии в формате WKT, например: "POLYGON((120.0 60.0, 121.0 60.0, 121.0 61.0, 120.0 61.0, 120.0 60.0))"
    pub footprint_wkt: String,
    pub band_urls: BandUrls,
}

impl NewGranule {
    pub fn from_cmr_response(
        nasa_id: String,
        cloud: f32,
        date_str: &str,
        coords: &[(f64, f64)],
        urls: BandUrls,
    ) -> Result<Self, String> {
        // Парсим ISO дату от NASA (например, "2026-10-31T07:56:00Z") в хроно-таймстамп с таймзоной Utc
        let acquisition_date = date_str
            .parse::<DateTime<Utc>>()
            .map_err(|e| format!("Invalid NASA date format: {}", e))?;

        // Генерируем валидный WKT полигон
        let footprint_wkt = super::utils::coords_to_wkt(coords);

        Ok(Self {
            nasa_granule_id: nasa_id,
            collection_id: "landsat-8-c2-l2".to_string(), // Дефолтная коллекция для MVP
            acquisition_date,
            cloud_cover: cloud,
            footprint_wkt,
            band_urls: urls, // Передаем собранную структуру линков без синтаксиса `..`
        })
    }
}

// ============================================================================
// 📥 СОСТОЯНИЕ ОЧЕРЕДИ И СТРУКТУРЫ ДЛЯ DOWNLOADER
// ============================================================================

/// Зеркало типа данных `nasa.task_status` из СУБД Postgres.
/// Позволяет `sqlx` автоматически маппить строки базы данных на валидный Rust-enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "nasa.task_status", rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Processing,
    Complete,
    Failed,
}

/// Маппинг строки таблицы `nasa.download_tasks` для обработки внутри воркера
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTaskRow {
    pub id: i64,
    pub granule_id: Uuid,
    pub status: TaskStatus,
    pub attempts: i32,
}

// ============================================================================
// 📊 СТРУКТУРЫ ОТВЕТОВ ДЛЯ AXUM API (УПРАВЛЕНИЕ)
// ============================================================================

/// Объект ответа для выдачи агрегированной статистики по очереди скачивания COG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub pending: i64,
    pub processing: i64,
    pub complete: i64,
    pub failed: i64,
}

/// DTO для ручного перезапуска или принудительного изменения статуса задачи
#[derive(Debug, Clone, Deserialize)]
pub struct RetryTaskRequest {
    pub task_uuid: Uuid,
}
