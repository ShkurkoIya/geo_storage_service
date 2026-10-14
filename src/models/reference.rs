use geo_types::Point;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

#[derive(Deserialize)]
pub struct GgsNodeUtcDto {
    pub id: String,
    pub index: String,
    pub name: String,
    pub class_accuracy: Option<String>,
    #[serde(rename = "map_1_50_000")]
    pub map_50k: Option<String>,
    #[serde(rename = "map_1_200_000")]
    pub map_200k: Option<String>,
    pub sistem_h: Option<String>,
    pub type_h: Option<String>,
    pub type_level: Option<String>,
    pub mark: Option<String>,
    pub type_centr: Option<String>,
    pub type_znak: Option<String>,
    pub h_znak: Option<String>,
    pub code_region: Option<String>,
    pub name_region: Option<String>,
    pub district: Option<String>,
    pub latitude: String,
    pub longitude: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "GgsNodeUtcDto")]
pub struct GgsNodeDomain {
    pub id: i32,
    pub node_index: String,
    pub name: String,
    pub accuracy_class: Option<String>,
    pub map_sheet_50k: Option<String>,
    pub map_sheet_200k: Option<String>,
    pub height_system: Option<String>,
    pub height_determination_method: Option<String>,
    pub leveling_type: Option<String>,
    pub monument_mark: Option<String>,
    pub center_structure_type: Option<String>,
    pub surface_monument_type: Option<String>,
    pub monument_height_m: f64,
    pub region_code: Option<String>,
    pub region_name: Option<String>,
    pub district_name: Option<String>,
    pub latitude_raw: f64,
    pub longitude_raw: f64,
    #[serde(skip)]
    pub geom_global: Point<f64>,
}
impl TryFrom<GgsNodeUtcDto> for GgsNodeDomain {
    type Error = String;

    fn try_from(dto: GgsNodeUtcDto) -> Result<Self, Self::Error> {
        let lat: f64 = dto
            .latitude
            .parse()
            .map_err(|e| format!("Bad latitude f64 parse: {}", e))?;
        let lon: f64 = dto
            .longitude
            .parse()
            .map_err(|e| format!("Bad longitude f64 parse: {}", e))?;
        let h: f64 = dto
            .h_znak
            .as_deref()
            .unwrap_or("0.0")
            .parse()
            .unwrap_or(0.0);

        Ok(GgsNodeDomain {
            id: dto.id.parse().unwrap_or(0),
            node_index: dto.index,
            name: dto.name.trim().to_string(),
            accuracy_class: dto.class_accuracy,
            map_sheet_50k: dto.map_50k,
            map_sheet_200k: dto.map_200k,
            height_system: dto.sistem_h,
            height_determination_method: dto.type_h,
            leveling_type: dto.type_level,
            monument_mark: dto.mark,
            center_structure_type: dto.type_centr,
            surface_monument_type: dto.type_znak,
            monument_height_m: h,
            region_code: dto.code_region,
            region_name: dto.name_region,
            district_name: dto.district,
            latitude_raw: lat,
            longitude_raw: lon,
            geom_global: Point::new(lon, lat),
        })
    }
}

#[derive(Deserialize)]
pub struct LostNodeUtcDto {
    pub id: String,
    pub index: String,
    pub name: String,
    pub class: Option<String>,
    #[serde(rename = "map_1_50_000")]
    pub map_50k: Option<String>,
    #[serde(rename = "map_1_200_000")]
    pub map_200k: Option<String>,
    pub type_h: Option<String>,
    pub mark: Option<String>,
    pub type_centr: Option<String>,
    pub type_znak: Option<String>,
    pub h_znak: Option<String>,
    pub code_region: Option<String>,
    pub name_region: Option<String>,
    pub district: Option<String>,
    pub condition_znak: Option<String>,
    pub condition_centr: Option<String>,
    pub verification_year: Option<String>,
    pub latitude: String,
    pub longitude: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "LostNodeUtcDto")]
pub struct LostNodeDomain {
    pub id: i32,
    pub node_index: String,
    pub name: String,
    pub accuracy_class: Option<String>,
    pub map_sheet_50k: Option<String>,
    pub map_sheet_200k: Option<String>,
    pub height_determination_method: Option<String>,
    pub monument_mark: Option<String>,
    pub center_structure_type: Option<String>,
    pub surface_monument_type: Option<String>,
    pub monument_height_m: f64,
    pub region_code: Option<String>,
    pub region_name: Option<String>,
    pub district_name: Option<String>,
    pub condition_monument: Option<String>,
    pub condition_center: Option<String>,
    pub verification_year: Option<i32>,
    pub latitude_raw: f64,
    pub longitude_raw: f64,
    #[serde(skip)]
    pub geom_global: Point<f64>,
}
impl TryFrom<LostNodeUtcDto> for LostNodeDomain {
    type Error = String;

    fn try_from(dto: LostNodeUtcDto) -> Result<Self, Self::Error> {
        let lat: f64 = dto
            .latitude
            .parse()
            .map_err(|e| format!("Bad lost latitude f64 parse: {}", e))?;
        let lon: f64 = dto
            .longitude
            .parse()
            .map_err(|e| format!("Bad lost longitude f64 parse: {}", e))?;
        let h: f64 = dto
            .h_znak
            .as_deref()
            .unwrap_or("0.0")
            .parse()
            .unwrap_or(0.0);

        let year_clean: Option<i32> = dto.verification_year.as_ref().and_then(|y| y.parse().ok());

        Ok(LostNodeDomain {
            id: dto.id.parse().unwrap_or(0),
            node_index: dto.index,
            name: dto.name.trim().to_string(),
            accuracy_class: dto.class,
            map_sheet_50k: dto.map_50k,
            map_sheet_200k: dto.map_200k,
            height_determination_method: dto.type_h,
            monument_mark: dto.mark,
            center_structure_type: dto.type_centr,
            surface_monument_type: dto.type_znak,
            monument_height_m: h,
            region_code: dto.code_region,
            region_name: dto.name_region,
            district_name: dto.district,
            condition_monument: dto.condition_znak,
            condition_center: dto.condition_centr,
            verification_year: year_clean,
            latitude_raw: lat,
            longitude_raw: lon,
            geom_global: Point::new(lon, lat),
        })
    }
}

#[derive(Deserialize)]
pub struct NivelerNodeUtcDto {
    pub id: String,
    pub index: String,
    pub name: String,
    pub region: Option<String>,
    pub district: Option<String>,
    pub latitude: String,
    pub longitude: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "NivelerNodeUtcDto")]
pub struct NivelerNodeDomain {
    pub id: i32,
    pub node_index: String,
    pub name: String,
    pub region_name: Option<String>,
    pub district_name: Option<String>,
    pub latitude_raw: f64,
    pub longitude_raw: f64,
    #[serde(skip)]
    pub geom_global: Point<f64>,
}
impl TryFrom<NivelerNodeUtcDto> for NivelerNodeDomain {
    type Error = String;

    fn try_from(dto: NivelerNodeUtcDto) -> Result<Self, Self::Error> {
        let lat: f64 = dto
            .latitude
            .parse()
            .map_err(|e| format!("Bad niveler latitude f64 parse: {}", e))?;
        let lon: f64 = dto
            .longitude
            .parse()
            .map_err(|e| format!("Bad niveler longitude f64 parse: {}", e))?;

        Ok(NivelerNodeDomain {
            id: dto.id.parse().unwrap_or(0),
            node_index: dto.index,
            name: dto.name.trim().to_string(),
            region_name: dto.region,
            district_name: dto.district,
            latitude_raw: lat,
            longitude_raw: lon,
            geom_global: Point::new(lon, lat),
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChemicalElementDomain {
    pub code: String,             // 'au', 'ag', 'cu'
    pub display_name: String,     // 'Золото', 'Серебро'
    pub measurement_unit: String, // 'г/т', '%'
    pub display_color: String,    // '#FFD700'
}
