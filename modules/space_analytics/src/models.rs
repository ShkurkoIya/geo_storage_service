use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// STRUCTS FOR SQLX (RAW DB ROWS)
// ============================================================================

pub struct ThermalHotspotRow {
    pub id: Uuid,
    pub source_id: Option<Uuid>,
    pub acquisition_time: NaiveDateTime,
    pub temp_kelvin: f64,
    pub is_night_scene: bool,
    pub latitude_raw: f64,
    pub longitude_raw: f64,
    pub created_at: NaiveDateTime,
}

pub struct GasPlumeRow {
    pub id: Uuid,
    pub source_id: Option<Uuid>,
    pub gas_type: String,
    pub pixel_value_max: Option<f64>,
    pub pixel_value_avg: Option<f64>,
    pub confidence_score: Option<i32>,
    pub geom_geojson: Option<String>,
    pub created_at: NaiveDateTime,
}

pub struct HydrogenRingRow {
    pub id: Uuid,
    pub deposit_id: Option<Uuid>,
    pub structure_type: Option<String>,
    pub ring_diameter_meters: Option<f64>,
    pub ndvi_drop_percentage: Option<f64>,
    pub geological_setting: Option<String>,
    pub geom_geojson: Option<String>,
    pub created_at: NaiveDateTime,
}

pub struct PrimeTargetRow {
    pub ring_id: Uuid,
    pub deposit_id: Option<Uuid>,
    pub ring_diameter_meters: Option<f64>,
    pub ndvi_drop_percentage: Option<f64>,
    pub hotspot_temp_c: Option<f64>,
    pub gas_type: String,
    pub gas_concentration_max: Option<f64>,
    pub geom_geojson: Option<String>,
}

// ============================================================================
// DOMAIN MODELS (CLEAN BUSINESS OBJECTS)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalHotspotDomain {
    pub id: Uuid,
    pub source_id: Option<Uuid>,
    pub acquisition_time: NaiveDateTime,
    pub temp_kelvin: f64,
    pub temp_celsius: f64,
    pub is_night_scene: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasPlumeDomain {
    pub id: Uuid,
    pub source_id: Option<Uuid>,
    pub gas_type: String,
    pub pixel_value_max: f64,
    pub pixel_value_avg: f64,
    pub confidence_score: i32,
    pub geometry: serde_json::Value,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HydrogenRingDomain {
    pub id: Uuid,
    pub deposit_id: Option<Uuid>,
    pub structure_type: String,
    pub ring_diameter_meters: f64,
    pub ndvi_drop_percentage: f64,
    pub geological_setting: Option<String>,
    pub geometry: serde_json::Value,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimeTargetDomain {
    pub ring_id: Uuid,
    pub deposit_id: Option<Uuid>,
    pub ring_diameter_meters: f64,
    pub ndvi_drop_percentage: f64,
    pub hotspot_temp_c: f64,
    pub gas_type: String,
    pub gas_concentration_max: f64,
    pub geometry: serde_json::Value,
}

// ============================================================================
// MAPPING IMPLEMENTATIONS (.into_domain())
// ============================================================================

impl ThermalHotspotRow {
    pub fn into_domain(self) -> ThermalHotspotDomain {
        ThermalHotspotDomain {
            id: self.id,
            source_id: self.source_id,
            acquisition_time: self.acquisition_time,
            temp_kelvin: self.temp_kelvin,
            temp_celsius: self.temp_kelvin - 273.15,
            is_night_scene: self.is_night_scene,
            latitude: self.latitude_raw,
            longitude: self.longitude_raw,
            created_at: self.created_at,
        }
    }
}

impl GasPlumeRow {
    pub fn into_domain(self) -> GasPlumeDomain {
        let gj_str = self.geom_geojson.unwrap_or_else(|| "{}".to_string());
        let geometry = serde_json::from_str(&gj_str).unwrap_or(serde_json::Value::Null);
        GasPlumeDomain {
            id: self.id,
            source_id: self.source_id,
            gas_type: self.gas_type.trim().to_uppercase(),
            pixel_value_max: self.pixel_value_max.unwrap_or(0.0),
            pixel_value_avg: self.pixel_value_avg.unwrap_or(0.0),
            confidence_score: self.confidence_score.unwrap_or(0),
            geometry,
            created_at: self.created_at,
        }
    }
}

impl HydrogenRingRow {
    pub fn into_domain(self) -> HydrogenRingDomain {
        let gj_str = self.geom_geojson.unwrap_or_else(|| "{}".to_string());
        let geometry = serde_json::from_str(&gj_str).unwrap_or(serde_json::Value::Null);
        HydrogenRingDomain {
            id: self.id,
            deposit_id: self.deposit_id,
            structure_type: self
                .structure_type
                .unwrap_or_else(|| "Sub-circular depression".to_string()),
            ring_diameter_meters: self.ring_diameter_meters.unwrap_or(0.0),
            ndvi_drop_percentage: self.ndvi_drop_percentage.unwrap_or(0.0),
            geological_setting: self.geological_setting,
            geometry,
            created_at: self.created_at,
        }
    }
}

impl PrimeTargetRow {
    pub fn into_domain(self) -> PrimeTargetDomain {
        let gj_str = self.geom_geojson.unwrap_or_else(|| "{}".to_string());
        let geometry = serde_json::from_str(&gj_str).unwrap_or(serde_json::Value::Null);
        PrimeTargetDomain {
            ring_id: self.ring_id,
            deposit_id: self.deposit_id,
            ring_diameter_meters: self.ring_diameter_meters.unwrap_or(0.0),
            ndvi_drop_percentage: self.ndvi_drop_percentage.unwrap_or(0.0),
            hotspot_temp_c: self.hotspot_temp_c.unwrap_or(0.0),
            gas_type: self.gas_type,
            gas_concentration_max: self.gas_concentration_max.unwrap_or(0.0),
            geometry,
        }
    }
}

// ============================================================================
// API INCOMING REQUEST DTOS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateThermalHotspotRequest {
    pub source_id: Option<Uuid>,
    pub acquisition_time: NaiveDateTime,
    pub temp_kelvin: f64,
    pub is_night_scene: bool,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Deserialize)]
pub struct CreateGasPlumeRequest {
    pub source_id: Option<Uuid>,
    pub gas_type: String,
    pub pixel_value_max: Option<f64>,
    pub pixel_value_avg: Option<f64>,
    pub confidence_score: Option<i32>,
    pub geometry_wkt: String, // Принимаем контур в WKT, СУБД спарсит его через ST_GeomFromText
}

#[derive(Debug, Deserialize)]
pub struct CreateHydrogenRingRequest {
    pub deposit_id: Option<Uuid>,
    pub structure_type: Option<String>,
    pub ring_diameter_meters: Option<f64>,
    pub ndvi_drop_percentage: Option<f64>,
    pub geological_setting: Option<String>,
    pub geometry_wkt: String,
}

#[derive(Debug, Deserialize)]
pub struct GasPlumeFilterQuery {
    pub gas_type: Option<String>,
}
