use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Описание типа системы координат, прилетевшей например из файла
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoordinateSystemType {
    /// Географические градусы на эллипсоиде WGS84 (GPS/ГЛОНАСС)
    GeographicWgs84,
    /// Географические градусы на эллипсоиде Красовского (СК-42 / СК-95)
    GeographicKrassovsky,
    /// Плоские прямоугольные метры (СК-42 зона X, МСК-субъекта, СК-63)
    /// Требует передачи PROJ-строки или EPSG-кода для инициализации
    Projected { proj_definition: String },
    /// Чисто локальная маркшейдерская сетка предприятия (метрическая плоскость без привязки)
    LocalGrid,
}

/// Универсальная точка, хранящая сырые координаты
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RawPoint {
    pub v1: f64,
    pub v2: f64,
}

impl RawPoint {
    pub fn from_geo(lon: f64, lat: f64) -> Self {
        Self { v1: lon, v2: lat }
    }

    pub fn from_meters(easting: f64, northing: f64) -> Self {
        Self {
            v1: easting,
            v2: northing,
        }
    }

    pub fn lon(&self) -> f64 {
        self.v1
    }

    pub fn lat(&self) -> f64 {
        self.v2
    }

    pub fn easting(&self) -> f64 {
        self.v1
    }

    pub fn northing(&self) -> f64 {
        self.v2
    }

    pub fn to_radians(&self) -> (f64, f64) {
        (self.v1.to_radians(), self.v2.to_radians())
    }

    pub fn distance_to(&self, other: &RawPoint) -> f64 {
        let dx = self.v1 - other.v1;
        let dy = self.v2 - other.v2;

        (dx * dx + dy * dy).sqrt()
    }
}

/// Пара точек для калибровки (Опорный репер)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlPointPair {
    /// Опциональное имя репера (например, "Опора-12" или "Скважина-4")
    #[serde(default)]
    pub name: Option<String>,
    /// Точка в исходной системе (например, то, что намерил тахеометр в карьере)
    pub source_point: RawPoint,
    /// Эта же точка в целевой системе (например, то, что раскопали в архивах или GPS)
    pub target_point: RawPoint,
}

impl ControlPointPair {
    pub fn new(source: RawPoint, target: RawPoint) -> Self {
        Self {
            name: None,
            source_point: source,
            target_point: target,
        }
    }

    pub fn display_name(&self, index: usize) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("Точка #{}", index + 1))
    }
}

/// Универсальный запрос на калибровку для ЛЮБЫХ сценариев недропользования
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalCalibrationRequest {
    /// Какого типа координаты на входе (Source)?
    pub source_system: CoordinateSystemType,
    /// Какого типа координаты на выходе (Target)?
    pub target_system: CoordinateSystemType,
    /// Набор опорных точек (для Хелмерта нужно от 2-3, для брутфорса меридианов — 2)
    pub control_points: Vec<ControlPointPair>,
    /// Допуск по точности (метрический)
    pub tolerance_meters: f64,
}

impl UniversalCalibrationRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.control_points.len() < 2 {
            return Err("Для проведения калибровки необходимо как минимум 2 опорных репера");
        }
        if self.tolerance_meters <= 0.0 {
            return Err("Допуск точности (tolerance_meters) должен быть строго больше нуля");
        }
        Ok(())
    }
}

/// Единый унифицированный результат трансформации
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub id: Uuid,
    /// Какую математическую стратегию выбрало ядро
    pub strategy_used: String,
    /// Коэффициенты сдвига / трансформации (зависят от стратегии)
    pub parameters: TransformationParameters,
    /// Итоговая невязка (ошибка) калибровки в метрах
    pub final_accuracy_meters: f64,
}

/// Параметры трансформации, покрывающие все случаи недропользования
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformationParameters {
    pub dx: f64,                       // Сдвиг по X / False Easting
    pub dy: f64,                       // Сдвиг по Y / False Northing
    pub rotation_angle_rad: f64,       // Угол разворота осей (для Аффинного/Хелмерта)
    pub scale_factor: f64,             // Масштабный коэффициент (искажения сети)
    pub central_meridian: Option<f64>, // Подобранный меридиан (если был брутфорс Гаусса-Крюгера)
}
