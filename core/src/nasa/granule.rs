use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use geo_types::Polygon;

use crate::nasa::{extractor::CmrLinkParser, product::NasaProduct};

use super::cmr::CmrEntry;

#[derive(Debug, Clone)]
pub struct GisGranule {
    /// Внутренний уникальный ID гранулы от NASA (например: "G3948763653-LPCLOUD")
    /// Идеален как естественный ключ для ON CONFLICT фильтрации дубликатов.
    pub concept_id: String,

    /// Человекочитаемое название/имя файла сцены (например: "MOD11A1.A2026001...")
    pub title: String,

    /// Идентификатор коллекции (например: "C1748058432-LPCLOUD")
    pub collection_concept_id: String,

    /// Точное время начала съемки. Если парсинг названия файла упадет,
    /// это поле — твой железный бэкап для TimescaleDB секционирования.
    pub time_start: DateTime<Utc>,

    /// Точное время окончания съемки
    pub time_end: DateTime<Utc>,

    /// Объем файла в Мегабайтах (f64, так как в JSON приходит строка/число с плавающей точкой)
    pub granule_size_mb: f64,

    /// Процент облачности. Внимание: CMR возвращает его не для всех продуктов!
    /// Поэтому делаем Option. Если поля нет — выставляем в Some(0.0) или None.
    pub cloud_cover: f32,

    /// Распарсенный footprint геометрии (Полигон PostGIS).
    /// Оборачиваем в Option, так как сцена может быть привязана точкой или bbox'ом.
    pub geometry: Option<Polygon<f64>>,

    /// Динамическая карта ПРЯМЫХ ссылок на скачивание (HTTPS)
    /// Ключ — абстрактное имя канала или расширение ("HDF", "B05", "QA"),
    /// Значение — прямой HTTPS URL.
    pub bands: HashMap<String, String>,

    /// Карта нативных S3-линков (`s3://...`) для облачного процессинга в будущем
    pub s3_links: HashMap<String, String>,

    /// Ссылка на дефолтную JPG-превьюшку низкого разрешения.
    /// Извлекается из тегов ответа NASA CMR с типом `rel: .../browse#`.
    /// Используется для быстрой визуальной оценки сцены на фронтенде.
    pub browse_url: Option<String>,
}

impl GisGranule {
    pub fn from_cmr_entry(entry: CmrEntry, active_product: NasaProduct) -> Option<Self> {
        let (bands, s3_links, browse_url) = active_product.parse_links(&entry.raw_links);

        Some(Self {
            concept_id: entry.id,
            title: entry.title,
            collection_concept_id: entry.collection_concept_id,
            time_start: entry.time_start,
            time_end: entry.time_end,
            granule_size_mb: entry.granule_size,
            cloud_cover: entry.cloud_cover.unwrap_or(0.0),
            geometry: entry.polygons,
            bands,
            s3_links,
            browse_url,
        })
    }

    /// Метод ядра для извлечения даты съемки из title
    /// Парсит как Landsat (L30), так и Sentinel (S30) форматы.
    pub fn parse_acquisition_date(&self) -> Option<DateTime<Utc>> {
        let tokens: Vec<&str> = self.title.split('.').collect();
        let date_token = tokens.get(3)?;

        if date_token.len() < 7 {
            return None;
        }

        let year: i32 = date_token[0..4].parse().ok()?;
        let day_of_year: u32 = date_token[4..7].parse().ok()?;

        let naive_date = NaiveDate::from_yo_opt(year, day_of_year)?;
        let naive_datetime = naive_date.and_hms_opt(0, 0, 0)?;

        Some(Utc.from_utc_datetime(&naive_datetime))
    }
}

/// Универсальная коллекция снимков
#[derive(Debug, Clone)]
pub struct GranuleCollection {
    pub granules: Vec<GisGranule>,
}

impl GranuleCollection {
    pub fn len(&self) -> usize {
        self.granules.len()
    }

    pub fn require_bands(mut self, required_bands: &[&str]) -> Self {
        self.granules.retain(|granule| {
            required_bands
                .iter()
                .all(|band| granule.bands.contains_key(*band))
        });
        self
    }

    pub fn latest(self) -> Result<GisGranule, String> {
        self.granules.into_iter().next().ok_or_else(|| {
            "GIS-Error: Granules with matching bands parameters not found".to_string()
        })
    }

    pub fn is_empty(&self) -> bool {
        self.granules.is_empty()
    }
}
