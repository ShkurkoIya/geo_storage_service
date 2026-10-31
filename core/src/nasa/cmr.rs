use super::utils::extract_band_name;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use geo_types::Polygon;
use geo_types::Rect;
use http::header::{ACCEPT, USER_AGENT};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use std::collections::HashMap;

/// Универсальная модель космического снимка (гранулы)
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
    pub cloud_cover: Option<f32>,

    /// Распарсенный footprint геометрии (Полигон PostGIS).
    /// Оборачиваем в Option, так как сцена может быть привязана точкой или bbox'ом.
    pub geometry: Option<Polygon<f64>>,

    /// Динамическая карта ПРЯМЫХ ссылок на скачивание (HTTPS)
    /// Ключ — абстрактное имя канала или расширение ("HDF", "B05", "QA"),
    /// Значение — прямой HTTPS URL.
    pub bands: HashMap<String, String>,

    /// Карта нативных S3-линков (`s3://...`) для облачного процессинга в будущем
    pub s3_links: HashMap<String, String>,
}

impl GisGranule {
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

pub trait NasaCmrExt {
    fn to_cmr_string(&self) -> String;
}

impl NasaCmrExt for Rect<f64> {
    fn to_cmr_string(&self) -> String {
        format!(
            "{},{},{},{}",
            self.min().x,
            self.min().y,
            self.max().x,
            self.max().y
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct CmrResponse {
    pub feed: CmrFeed,
}

impl CmrResponse {
    /// Парсит сырой ответ в очищенный вектор
    pub fn extract_links(self) -> Vec<(String, String)> {
        let mut target_links = Vec::new();

        for entry in self.feed.entry {
            if let Some(links) = entry.links {
                if let Some(valid_link) = links.into_iter().find(|l| l.is_valid_https_data_source())
                {
                    target_links.push((entry.title, valid_link.href));
                }
            }
        }

        target_links
    }

    pub fn into_collection(self) -> GranuleCollection {
        let mut target_granules = Vec::new();

        for entry in self.feed.entry {
            if let Some(links) = entry.links {
                let mut bands_map = HashMap::new();
                let mut s3_links_map = HashMap::new();

                // Итерируемся по ссылкам внутри текущей гранулы
                for link in links {
                    // Проверяем по твоему валидатору HTTPS источников
                    if link.is_valid_https_data_source() {
                        if let Some(band_name) = extract_band_name(&link.href) {
                            bands_map.insert(band_name, link.href);
                        } else {
                            // Если имя канала не вытащилось из урла, пишем расширение (например, HDF)
                            let ext = link
                                .href
                                .split('.')
                                .last()
                                .unwrap_or("UNKNOWN")
                                .to_uppercase();
                            bands_map.insert(ext, link.href);
                        }
                    }
                    // Параллельно собираем нативные S3 ссылки для будущего (s3://...)
                    else if link.rel == "http://esipfed.org/ns/fedsearch/1.1/s3#" {
                        let ext = link
                            .href
                            .split('.')
                            .last()
                            .unwrap_or("UNKNOWN")
                            .to_uppercase();
                        s3_links_map.insert(ext, link.href);
                    }
                }

                // Парсим таймстампы из ответа NASA CMR
                let time_start = entry
                    .time_start
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now());
                let time_end = entry
                    .time_end
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now());

                let granule_size_mb = entry.granule_size.parse::<f64>().unwrap_or(0.0);

                // Вызываем написанный нами парсер полигонов PostGIS
                let geometry = entry.polygons.and_then(|p| parse_polygon_from_cmr(&p));

                // Если мы нашли хотя бы один полезный бэнд — собираем сеньорскую гранулу
                if !bands_map.is_empty() {
                    target_granules.push(GisGranule {
                        concept_id: entry.id, // твой внутренний G-ID от NASA
                        title: entry.title,   // имя файла
                        collection_concept_id: entry.collection_concept_id,
                        time_start,
                        time_end,
                        granule_size_mb,
                        cloud_cover: entry.cloud_cover,
                        geometry,
                        bands: bands_map,
                        s3_links: s3_links_map,
                    });
                }
            }
        }

        GranuleCollection {
            granules: target_granules,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CmrFeed {
    pub updated: String,
    pub id: String,
    pub title: String,
    pub entry: Vec<CmrEntry>,
}

#[derive(Debug, Deserialize)]
pub struct CmrEntry {
    pub id: String,
    pub title: String,
    pub collection_concept_id: String,
    pub time_start: String,
    pub time_end: String,
    pub granule_size: String,

    #[serde(default)]
    pub cloud_cover: Option<f32>,

    #[serde(default)]
    pub polygons: Option<Vec<Vec<String>>>,

    #[serde(default)]
    pub links: Option<Vec<CmrLink>>,
}

#[derive(Debug, Deserialize)]
pub struct CmrLink {
    pub rel: String,
    pub href: String,
    #[serde(default)]
    title: Option<String>,
}

impl CmrLink {
    pub fn is_valid_https_data_source(&self) -> bool {
        let rel = self.rel.clone();
        if !rel.contains("data#") || !self.href.starts_with("https://") {
            return false;
        }

        let href_lower = self.href.to_lowercase();
        let is_junk_extension = href_lower.ends_with(".jpg")
            || href_lower.ends_with(".jpeg")
            || href_lower.ends_with(".xml")
            || href_lower.ends_with(".json")
            || href_lower.ends_with(".txt")
            || href_lower.ends_with(".md5");

        !is_junk_extension
    }
}

#[derive(Debug, Clone)]
pub struct CmrQuery {
    pub selector: CollectionSelector,
    pub bbox: Rect<f64>,
    pub temporal: String,
    pub cloud_cover_max: Option<u8>,
    pub page_size: u16,
    pub sort_by_start_date_desc: bool,
}

#[derive(Debug, Clone)]
pub enum CollectionSelector {
    /// Поиск по человеческому имени продукта (например "MOD11A1")
    ShortName(String),
    ConceptId(String),
}

impl CmrQuery {
    pub fn new(selector: CollectionSelector, bbox: Rect<f64>, temporal: &str) -> Self {
        Self {
            selector,
            bbox,
            temporal: temporal.to_string(),
            cloud_cover_max: None,
            page_size: 100,
            sort_by_start_date_desc: true,
        }
    }

    pub fn to_query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("bounding_box", self.bbox.to_cmr_string()),
            ("temporal", self.temporal.clone()),
            ("page_size", self.page_size.to_string()),
        ];

        match &self.selector {
            CollectionSelector::ShortName(name) => params.push(("short_name", name.clone())),
            CollectionSelector::ConceptId(id) => params.push(("concept_id", id.clone())),
        }

        if self.sort_by_start_date_desc {
            params.push(("sort_key", "-start_date".to_string()));
        }

        if let Some(cloud) = self.cloud_cover_max {
            params.push(("cloud_cover", format!("0,{}", cloud)));
        }

        params
    }

    pub fn with_cloud_cover(mut self, max_percent: u8) -> Self {
        self.cloud_cover_max = Some(max_percent);
        self
    }

    pub fn with_page_size(mut self, size: u16) -> Self {
        self.page_size = size.min(2000);
        self
    }
}

pub struct CmrClient<'a> {
    pub(crate) http_client: &'a ClientWithMiddleware,
}

impl<'a> CmrClient<'a> {
    pub fn new(http_client: &'a ClientWithMiddleware) -> Self {
        Self { http_client }
    }

    pub async fn fetch_granules(&self, query: CmrQuery) -> Result<GranuleCollection, String> {
        let url = "https://cmr.earthdata.nasa.gov/search/granules.json";

        let query_params = query.to_query_params();

        let response = self
            .http_client
            .get(url)
            .query(&query_params)
            .header(USER_AGENT, "HydrogenGateway/1.0")
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("Critial error requesting CMR API: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "NASA CMR API returned server error. Status: {}",
                response.status()
            ));
        }

        let cmr_data = response.json::<CmrResponse>().await.map_err(|e| {
            format!(
                "Could not deserialize response structure from NASA CMR: {}",
                e
            )
        })?;

        Ok(cmr_data.into_collection())
    }
}
