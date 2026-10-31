use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SatteliteProduct {
    /// Harmonized Landsat 8 (HLS L30)
    Landsat8Hls,
    /// Sentinel-2 MSI (HLS S30)
    Sentinel2Hls,
}

impl SatteliteProduct {
    /// Возращает ShortName Для NASA CMR запроса
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Landsat8Hls => "HLSL30",
            Self::Sentinel2Hls => "HLSS30",
        }
    }
}
