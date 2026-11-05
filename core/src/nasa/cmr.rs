use super::utils::extract_band_name;
use crate::nasa::collection::GranuleCollection;
use crate::nasa::extractor::{CmrLinkParser, RawCmrLink};
use crate::nasa::granule::GisGranule;
use crate::nasa::product::NasaProduct;

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

fn deserialize_nasa_links<'de, D>(deserializer: D) -> Result<Vec<RawCmrLink>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_links: Vec<RawCmrLink> = Deserialize::deserialize(deserializer)?;
    Ok(raw_links)
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
    pub raw_links: Vec<RawCmrLink>,
}

impl CmrResponse {
    pub fn into_collection(self, active_product: NasaProduct) -> GranuleCollection {
        let target_granules: Vec<GisGranule> = self
            .feed
            .entry
            .into_iter()
            .filter_map(|entry| GisGranule::from_cmr_entry(entry, active_product))
            .collect();

        GranuleCollection {
            granules: target_granules,
        }
    }
}
