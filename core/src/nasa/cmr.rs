use super::utils::extract_band_name;
use crate::nasa::collection::GranuleCollection;
use crate::nasa::granule::GisGranule;

use chrono::{DateTime, Utc};
use geo_types::{Polygon, Rect};
use serde::{Deserialize, Deserializer};
use serde_aux::field_attributes::deserialize_number_from_string;
use serde_aux::field_attributes::deserialize_option_number_from_string;
use std::collections::HashMap;

// ============================================================================
// ГЕО-РАСШИРЕНИЯ ДЛЯ NASA CMR
// ============================================================================

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

pub trait NasaCmrDecodedExt: Sized {
    /// Десериализует насовский кастомный массив строк в нативную геометрию
    fn from_cmr_polygons(cmr_polygons: &[Vec<String>]) -> Option<Self>;
}

impl NasaCmrDecodedExt for geo_types::Polygon<f64> {
    fn from_cmr_polygons(cmr_polygons: &[Vec<String>]) -> Option<Self> {
        let first_poly_ring = cmr_polygons.first()?;
        let mut coords = Vec::new();

        for ring_str in first_poly_ring {
            let tokens: Vec<&str> = ring_str.split_whitespace().collect();
            // Защита: парсим строго парами [Latitude, Longitude]
            for i in (0..tokens.len()).step_by(2) {
                let lat_str = match tokens.get(i) {
                    Some(s) => s,
                    None => break,
                };
                let lon_str = match tokens.get(i + 1) {
                    Some(s) => s,
                    None => break,
                };

                let lat: f64 = lat_str.parse().ok()?;
                let lon: f64 = lon_str.parse().ok()?;

                coords.push(geo_types::Coord { x: lon, y: lat });
            }
        }

        if coords.is_empty() {
            return None;
        }

        // Автозамыкание контура для PostGIS (первая и последняя точка должны совпадать)
        if coords.first() != coords.last() {
            if let Some(&first_coord) = coords.first() {
                coords.push(first_coord);
            }
        }

        Some(geo_types::Polygon::new(
            geo_types::LineString::new(coords),
            vec![],
        ))
    }
}

// ============================================================================
// СЕРДЕ-ДЕЗЕРИАЛИЗАТОРЫ (СИНХРОНИЗИРОВАНО С БД)
// ============================================================================

/// Структура-контейнер, рождающаяся из Serde-пайплайна
#[derive(Debug, Clone)]
pub struct ParsedCmrLinks {
    pub bands: HashMap<String, String>,
    pub s3_links: HashMap<String, String>,
    pub browse_url: Option<String>,
}

fn deserialize_nasa_links<'de, D>(deserializer: D) -> Result<ParsedCmrLinks, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct RawLink {
        rel: String,
        href: String,
    }

    let raw_links: Vec<RawLink> = Deserialize::deserialize(deserializer)?;
    let junk_extensions = [".xml", ".json", ".txt", ".md5"];

    let (bands, s3_links, browse_url) = raw_links.into_iter().fold(
        (HashMap::new(), HashMap::new(), None),
        |(mut bands_acc, mut s3_acc, mut browse_acc), link| {
            let rel_lower = link.rel.to_lowercase();
            let href_lower = link.href.to_lowercase();

            // Это дефолтная JPG превьюшка из rel: .../browse#
            if rel_lower.contains("browse#")
                && (href_lower.ends_with(".jpg") || href_lower.ends_with(".jpeg"))
            {
                if browse_acc.is_none() && link.href.starts_with("https://") {
                    browse_acc = Some(link.href);
                }
            }
            // Это целевой спектральный HTTPS канал растра
            else if rel_lower.contains("data#")
                && link.href.starts_with("https://")
                && !junk_extensions
                    .iter()
                    .any(|junk| href_lower.ends_with(junk))
            {
                let key = extract_band_name(&link.href).unwrap_or_else(|| {
                    link.href
                        .split('.')
                        .next_back()
                        .unwrap_or("UNKNOWN")
                        .to_uppercase()
                });
                bands_acc.insert(key, link.href);
            }
            // Сценарий В: Это нативная облачная S3-ссылка
            else if link.rel == "http://esipfed.org" {
                let ext = link
                    .href
                    .split('.')
                    .next_back()
                    .unwrap_or("UNKNOWN")
                    .to_uppercase();
                s3_acc.insert(ext, link.href);
            }

            (bands_acc, s3_acc, browse_acc)
        },
    );

    Ok(ParsedCmrLinks {
        bands,
        s3_links,
        browse_url,
    })
}

fn deserialize_cmr_geometry<'de, D>(deserializer: D) -> Result<Option<Polygon<f64>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<Vec<Vec<String>>> = Option::deserialize(deserializer)?;
    match raw {
        Some(p) => Ok(Polygon::from_cmr_polygons(&p)),
        None => Ok(None),
    }
}

// ============================================================================
// СТРУКТУРЫ ОТВЕТА
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CmrResponse {
    pub feed: CmrFeed,
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
    pub time_start: DateTime<Utc>,
    pub time_end: DateTime<Utc>,

    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub granule_size: f64,

    #[serde(default, deserialize_with = "deserialize_option_number_from_string")]
    pub cloud_cover: Option<f32>,

    #[serde(default, deserialize_with = "deserialize_cmr_geometry")]
    pub polygons: Option<Polygon<f64>>,

    #[serde(rename = "links", deserialize_with = "deserialize_nasa_links")]
    pub parsed_links: ParsedCmrLinks,
}

impl CmrResponse {
    pub fn into_collection(self) -> GranuleCollection {
        let target_granules: Vec<GisGranule> = self
            .feed
            .entry
            .into_iter()
            .filter_map(|entry| {
                if entry.parsed_links.bands.is_empty() {
                    return None;
                }

                Some(GisGranule {
                    concept_id: entry.id,
                    title: entry.title,
                    collection_concept_id: entry.collection_concept_id,
                    time_start: entry.time_start,
                    time_end: entry.time_end,
                    granule_size_mb: entry.granule_size,
                    cloud_cover: entry.cloud_cover.unwrap_or(0.0),
                    geometry: entry.polygons,
                    bands: entry.parsed_links.bands,
                    s3_links: entry.parsed_links.s3_links,
                    browse_url: entry.parsed_links.browse_url,
                })
            })
            .collect();

        GranuleCollection {
            granules: target_granules,
        }
    }
}
