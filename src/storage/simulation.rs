use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 🌋 Математическая 3D-модель рудной линзы для симулятора Гинтелл
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticOreBody {
    pub id: Uuid,
    pub name: String,
    pub center_east: f64,
    pub center_north: f64,
    pub center_elevation: f64,
    pub length_m: f64,
    pub width_m: f64,
    pub dip_angle: f64,
    pub wkt_polygon: String,
}

/// 🎯 Сгенерированная точка оптимального бурения под стандарт ГКЗ категории С1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GkzProposalPoint {
    pub id: Uuid,
    pub proposed_code: String,
    pub easting: f64,
    pub northing: f64,
    pub target_depth: f64,
    pub priority_weight: f64,
}
