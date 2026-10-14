use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;
use uuid::Uuid;

// =========================================================================
// МЕТАДАННЫЕ И ИИ-ПАСПОРТИЗАЦИЯ
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMapping {
    pub hole_id_col: String,
    pub easting_col: String,
    pub northing_col: String,
    pub elevation_col: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiParsedPassport {
    pub deposit_name: String,
    pub geological_year: Option<i32>,
    pub column_mapping: ColumnMapping,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresenceIndicator {
    Measured,   // Есть честное цифровое содержание (например, 79.1)
    Traces,     // "сл" (Следы) — металл есть, но взвесить нельзя
    Barren,     // "пс" (Пустая порода) — металла точно ноль
    NoAnalysis, // "‒", "NaN", "анализов нет"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssayComponent {
    pub raw_text: String,
    pub clean_value: f64,
    pub indicator: PresenceIndicator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssayAttributes {
    pub components: HashMap<String, AssayComponent>, // Карта элементов: "au", "ag", "cu"
    pub remarks: Option<String>,
}

// =========================================================================
// МЕСТОРОЖДЕНИЕ И ИСТОЧНИКИ ДАННЫХ
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositDomain {
    pub id: Uuid,
    pub name: String,
    pub polygon_boundary: Option<String>, // WKT геометрия полигона горного отвода
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceDomain {
    pub id: Uuid,
    pub deposit_id: Option<Uuid>,
    pub file_name: String,
    pub sha256: String,
    pub deposit_name: String,
    pub geological_year: Option<i32>,
    pub status: String, // 'ingested', 'processed', 'error'
    pub ai_passport: Option<AiParsedPassport>,
    pub created_at: Option<NaiveDateTime>,
}

// =========================================================================
// СКВАЖИНЫ (DRILLHOLE COLLARS)
// =========================================================================

/// Внутренний DTO-слой: перехватывает ублюдский текстовый сырой ввод скважин
#[derive(Deserialize)]
pub struct DrillholeCollarDto {
    pub id: Option<String>,
    pub source_id: String,
    pub hole_id: String,
    pub line_id: Option<String>,
    pub easting_raw: String,
    pub northing_raw: String,
    pub elevation_raw: Option<String>,
    pub max_depth: String,
}

/// Стерильный домен устья скважины
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "DrillholeCollarDto")]
pub struct DrillholeCollarDomain {
    pub id: Uuid,
    pub source_id: Uuid,
    pub hole_id: String,
    pub line_id: Option<String>,
    pub easting_raw: f64,
    pub northing_raw: f64,
    pub collar_elevation: f64,
    pub max_depth: f64,
    pub geom_world: Option<String>, // 3D LineString ствола скважины
    pub created_at: Option<NaiveDateTime>,
}

impl TryFrom<DrillholeCollarDto> for DrillholeCollarDomain {
    type Error = String;

    fn try_from(dto: DrillholeCollarDto) -> Result<Self, Self::Error> {
        let id_clean = dto
            .id
            .and_then(|i| Uuid::parse_str(&i).ok())
            .unwrap_or_else(Uuid::new_v4);
        let source_uuid =
            Uuid::parse_str(&dto.source_id).map_err(|e| format!("Bad source_id UUID: {}", e))?;
        let east: f64 = dto
            .easting_raw
            .parse()
            .map_err(|e| format!("Bad easting_raw: {}", e))?;
        let north: f64 = dto
            .northing_raw
            .parse()
            .map_err(|e| format!("Bad northing_raw: {}", e))?;
        let elev: f64 = dto
            .elevation_raw
            .as_deref()
            .unwrap_or("0.0")
            .parse()
            .unwrap_or(0.0);
        let depth: f64 = dto
            .max_depth
            .parse()
            .map_err(|e| format!("Bad max_depth: {}", e))?;

        Ok(DrillholeCollarDomain {
            id: id_clean,
            source_id: source_uuid,
            hole_id: dto.hole_id.trim().to_string(),
            line_id: dto.line_id.map(|l| l.trim().to_string()),
            easting_raw: east,
            northing_raw: north,
            collar_elevation: elev,
            max_depth: depth,
            geom_world: None,
            created_at: None,
        })
    }
}

// =========================================================================
// ИНТЕРВАЛЫ ОПРОБОВАНИЯ
// =========================================================================
#[derive(Deserialize)]
pub struct DrillholeIntervalDto {
    pub id: Option<String>,
    pub collar_id: String,
    pub sample_id: Option<String>, // Номер пробы (напр. 'А-667')
    pub depth_from: String,        // От (м) — принимаем строкой во избежание паники Serde
    pub depth_to: String,          // До (м)
    pub thickness: String,         // Мощность интервала (м)
    pub quality_attributes: serde_json::Value, // Динамический JSONB-контейнер химии ("au", "ag")
}

/// Чистый, стерильный домен интервала опробования для PostGIS и математики ядра
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "DrillholeIntervalDto")]
pub struct DrillholeIntervalDomain {
    pub id: Uuid,
    pub collar_id: Uuid,
    pub sample_id: Option<String>,
    pub depth_from: f64,
    pub depth_to: f64,
    pub thickness: f64,
    pub quality_attributes: AssayAttributes, // Перемалываем сырой JSONB в наш типизированный AssayAttributes хаус!
    pub geom_poly: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

impl TryFrom<DrillholeIntervalDto> for DrillholeIntervalDomain {
    type Error = String;

    fn try_from(dto: DrillholeIntervalDto) -> Result<Self, Self::Error> {
        let id_clean = dto
            .id
            .and_then(|i| Uuid::parse_str(&i).ok())
            .unwrap_or_else(Uuid::new_v4);
        let collar_uuid =
            Uuid::parse_str(&dto.collar_id).map_err(|e| format!("Bad collar_id UUID: {}", e))?;

        // Наносекундная санация и безопасный парсинг маркшейдерских глубин
        let from: f64 = dto
            .depth_from
            .parse()
            .map_err(|e| format!("Bad depth_from f64 parse: {}", e))?;
        let to: f64 = dto
            .depth_to
            .parse()
            .map_err(|e| format!("Bad depth_to f64 parse: {}", e))?;
        let thick: f64 = dto
            .thickness
            .parse()
            .map_err(|e| format!("Bad thickness f64 parse: {}", e))?;

        // Мапим динамический JSONB-контейнер в нашу жесткую структуру AssayAttributes
        let quality: AssayAttributes =
            serde_json::from_value(dto.quality_attributes).map_err(|e| {
                format!(
                    "Ошибка десериализации quality_attributes в AssayAttributes: {}",
                    e
                )
            })?;

        Ok(DrillholeIntervalDomain {
            id: id_clean,
            collar_id: collar_uuid,
            sample_id: dto.sample_id.map(|s| s.trim().to_string()),
            depth_from: from,
            depth_to: to,
            thickness: thick,
            quality_attributes: quality,
            geom_poly: None,
            created_at: None,
        })
    }
}

// =========================================================================
// ТЭО ОЦЕНКА: ГЕОЛОГИЧЕСКИЕ БЛОКИ ЗАПАСОВ ГКЗ
// =========================================================================

/// Внутренний DTO-слой: принимает сырую текстовую матрицу блоков ГКЗ Михалыча
#[derive(Deserialize)]
pub struct GeologicalBlockDto {
    pub id: Option<String>,
    pub deposit_id: String,
    pub block_number: String, // Сырое имя из JSON
    pub ore_body_name: String,
    pub horizon_level: String,    // Сырое имя из JSON
    pub reserve_category: String, // Сырое имя из JSON
    pub area_m2: Option<String>,
    pub thickness_m: Option<String>,
    pub volume_m3: Option<String>,
    pub ore_tonnage: Option<String>,
    pub dip_angle: Option<String>,
    pub grade_and_economics_attributes: serde_json::Value,
    pub wkt_polygon: Option<String>,
}

/// Чистый доменный объект блока запасов ГКЗ
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "GeologicalBlockDto")]
pub struct GeologicalBlockDomain {
    pub id: Uuid,
    pub deposit_id: Uuid,
    pub block_code: String, // Чистый международный стандарт
    pub ore_body_name: String,
    pub horizon_elevation: f64,    // Чистый международный стандарт
    pub resource_category: String, // Чистый международный стандарт
    pub area_m2: Option<f64>,
    pub thickness_m: Option<f64>,
    pub volume_m3: Option<f64>,
    pub ore_tonnage: Option<f64>,
    pub dip_angle: Option<f64>,
    pub grade_and_economics_attributes: serde_json::Value,
    pub geom_poly: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

impl TryFrom<GeologicalBlockDto> for GeologicalBlockDomain {
    type Error = String;

    fn try_from(dto: GeologicalBlockDto) -> Result<Self, Self::Error> {
        let id_clean = dto
            .id
            .and_then(|i| Uuid::parse_str(&i).ok())
            .unwrap_or_else(Uuid::new_v4);
        let deposit_uuid =
            Uuid::parse_str(&dto.deposit_id).map_err(|e| format!("Bad deposit_id UUID: {}", e))?;

        let horizon: f64 = dto
            .horizon_level
            .parse()
            .map_err(|e| format!("Bad horizon_level: {}", e))?;

        let area = dto.area_m2.as_ref().and_then(|v| v.parse::<f64>().ok());
        let thickness = dto.thickness_m.as_ref().and_then(|v| v.parse::<f64>().ok());
        let volume = dto.volume_m3.as_ref().and_then(|v| v.parse::<f64>().ok());
        let tonnage = dto.ore_tonnage.as_ref().and_then(|v| v.parse::<f64>().ok());
        let dip = dto.dip_angle.as_ref().and_then(|v| v.parse::<f64>().ok());

        Ok(GeologicalBlockDomain {
            id: id_clean,
            deposit_id: deposit_uuid,
            block_code: dto.block_number.trim().to_string(),
            ore_body_name: dto.ore_body_name.trim().to_string(),
            horizon_elevation: horizon,
            resource_category: dto.reserve_category.trim().to_string(),
            area_m2: area,
            thickness_m: thickness,
            volume_m3: volume,
            ore_tonnage: tonnage,
            dip_angle: dip,
            grade_and_economics_attributes: dto.grade_and_economics_attributes,
            geom_poly: dto.wkt_polygon,
            created_at: None,
        })
    }
}
