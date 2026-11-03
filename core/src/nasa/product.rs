use std::fmt;
use std::str::FromStr;

/// Физический формат хранения и распространения данных на архивных серверах NASA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageFormat {
    /// Облачный оптимизированный GeoTIFF (HLS продукты).
    /// Идеален для распределенных облачных воркеров: поддерживает ленивое чтение метаданных по HTTP Range.
    Cog,

    /// Классический GeoTIFF/TIFF без оптимизации под Range-запросы.
    /// Требует полного скачивания файла для парсинга структуры растра.
    GeoTiff,

    /// Hierarchical Data Format (версии 4 или 5). Монолитные многомерные архивы-матрицы
    /// (MODIS, VIIRS, ECOSTRESS). Все бэнды лежат внутри одного файла.
    Hdf,

    /// Network Common Data Form. Используется для сложных спектральных кубов (EMIT).
    NetCdf,

    /// Векторные ГИС-данные (GeoJSON, KML, Shapefile, таблицы CSV).
    Vector,

    /// Метеорологические бинарные сетки (GRIB/GRIB2).
    Grib,

    /// Вариант-предохранитель для любых других редких, сырых или проприетарных форматов NASA
    /// Спасает конвейер от паники (panic!).
    Other,
}

impl StorageFormat {
    pub fn is_cloud_optimized(&self) -> bool {
        matches!(self, Self::Cog)
    }
}

impl fmt::Display for StorageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cog => write!(f, "COG"),
            Self::Hdf => write!(f, "HDF"),
            Self::NetCdf => write!(f, "NetCDF"),
            Self::GeoTiff => write!(f, "GeoTIFF"),
            Self::Vector => write!(f, "Vector"),
            Self::Grib => write!(f, "GRIB"),
            Self::Other => write!(f, "Other"),
        }
    }
}

/// Перечисление поддерживаемых продуктов ДЗЗ NASA Earthdata для ГИС-конвейера.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NasaProduct {
    /// HLS Landsat 30m (Снимки Landsat 8/9, подогнанные под сетку Sentinel-2 MGRS)
    Hlsl30,

    /// HLS Sentinel 30m (Снимки Sentinel-2A/2B/2C)
    Hlss30,

    /// HLS Landsat Vegetation Indices (Рассчитанные вегетационные индексы на базе Landsat)
    Hlsl30Vi,

    /// HLS Sentinel Vegetation Indices (Рассчитанные вегетационные индексы на базе Sentinel)
    Hss30Vi, // Исправлена опечатка в имени (было Hlss30Vi, канонично для Sentinel использовать S)

    /// MODIS/Terra Surface Reflectance Daily Global (Разрешение 250м-500м)
    Mod09Ga,

    /// MODIS/Aqua Surface Reflectance Daily Global (Разрешение 250м-500м)
    Myd09Ga,

    /// VIIRS/Suomi-NPP Surface Reflectance Daily Global (Разрешение 375м-750м)
    Vj109Ga,

    /// ECOSTRESS LST (Термальные данные высокого разрешения с МКС)
    Eco2Lste,

    /// EMIT L2A Estimated Surface Reflectance (Гиперспектральный прибор на МКС)
    EmitL2ARefl,
}

impl NasaProduct {
    /// Возвращает точный `short_name` для передачи в CMR API поисковым параметром `short_name=`
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hlsl30 => "HLSL30",
            Self::Hlss30 => "HLSS30",
            Self::Hlsl30Vi => "HLSL30_VI",
            Self::Hss30Vi => "HLSS30_VI",
            Self::Mod09Ga => "MOD09GA",
            Self::Myd09Ga => "MYD09GA",
            Self::Vj109Ga => "VJ109GA",
            Self::Eco2Lste => "ECO2LSTE",
            Self::EmitL2ARefl => "EMIT_L2A_REFL",
        }
    }

    /// Постоянный и уникальный Concept ID коллекции в NASA CMR (LPCLOUD провайдер).
    /// Идеально подходит для автоматического заполнения внешних ключей (Foreign Keys) в Postgres!
    pub fn default_concept_id(&self) -> &'static str {
        match self {
            Self::Hlsl30 => "C2021957657-LPCLOUD",
            Self::Hlss30 => "C2021957295-LPCLOUD",
            Self::Hlsl30Vi => "C2202030283-LPCLOUD",
            Self::Hss30Vi => "C2202026300-LPCLOUD",
            Self::Mod09Ga => "C2202497474-LPCLOUD",
            Self::Myd09Ga => "C2202445704-LPCLOUD",
            Self::Vj109Ga => "C2631841524-LPCLOUD",
            Self::Eco2Lste => "C2145334135-LPCLOUD",
            Self::EmitL2ARefl => "C2413411322-LPCLOUD",
        }
    }

    /// Возвращает текущую стабильную версию продукта в CMR (параметр `version=`)
    pub fn default_version(&self) -> &'static str {
        match self {
            Self::Hlsl30 | Self::Hlss30 | Self::Hlsl30Vi | Self::Hss30Vi => "2.0",
            Self::Mod09Ga | Self::Myd09Ga => "061",
            Self::Vj109Ga => "002",
            Self::Eco2Lste => "002",
            Self::EmitL2ARefl => "001",
        }
    }

    /// Возвращает формат хранения данных для правильной маршрутизации в воркерах загрузки
    pub fn storage_format(&self) -> StorageFormat {
        match self {
            Self::Hlsl30 | Self::Hlss30 | Self::Hlsl30Vi | Self::Hss30Vi => StorageFormat::Cog,
            Self::Mod09Ga | Self::Myd09Ga | Self::Vj109Ga | Self::Eco2Lste => StorageFormat::Hdf,
            Self::EmitL2ARefl => StorageFormat::NetCdf,
        }
    }

    /// Логический переключатель для CMR-билдера.
    pub fn supports_cmr_cloud_filter(&self) -> bool {
        matches!(
            self,
            Self::Hlsl30 | Self::Hlss30 | Self::Hlsl30Vi | Self::Hss30Vi
        )
    }

    /// Возвращает массив семантически важных каналов, необходимых для базовых научных расчетов
    pub fn target_bands(&self) -> &'static [&'static str] {
        match self {
            Self::Hlsl30 => &["B02", "B03", "B04", "B05", "B06", "B07", "Fmask"],
            Self::Hlss30 => &["B02", "B03", "B04", "B08", "B11", "B12", "FMASK"],
            Self::Hlsl30Vi | Self::Hss30Vi => &["NDVI", "EVI", "NDMI", "SAVI", "FMASK"],
            Self::Mod09Ga | Self::Myd09Ga => &["data_cube", "metadata_xml"],
            Self::Vj109Ga => &["data_cube", "metadata_xml"],
            Self::Eco2Lste => &["data_cube"],
            Self::EmitL2ARefl => &["data_cube"],
        }
    }
}

impl fmt::Display for NasaProduct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Реализуем FromStr для безопасной конвертации строк обратно в Enum
impl FromStr for NasaProduct {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "HLSL30" => Ok(Self::Hlsl30),
            "HLSS30" => Ok(Self::Hlss30),
            "HLSL30_VI" => Ok(Self::Hlsl30Vi),
            "HLSS30_VI" => Ok(Self::Hss30Vi),
            "MOD09GA" => Ok(Self::Mod09Ga),
            "MYD09GA" => Ok(Self::Myd09Ga),
            "VJ109GA" => Ok(Self::Vj109Ga),
            "ECO2LSTE" => Ok(Self::Eco2Lste),
            "EMIT_L2A_REFL" => Ok(Self::EmitL2ARefl),
            _ => Err(format!(
                "Неизвестный или неподдерживаемый продукт NASA: {}",
                s
            )),
        }
    }
}

