use crate::nasa::product::NasaProduct;
use std::collections::HashMap;

/// Вспомогательная структура для десериализации ссылок из JSON/Atom ответов NASA CMR.
/// Сюда Serde маппит массив "links" из ответа API.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RawCmrLink {
    pub href: String,
    pub title: Option<String>,
}

/// Трейт, расширяющий функционал NasaProduct для каноничного парсинга бэндов.
pub trait CmrLinkParser {
    fn parse_links(&self, raw_links: &[RawCmrLink]) -> HashMap<String, String>;
}

impl CmrLinkParser for NasaProduct {
    fn parse_links(&self, raw_links: &[RawCmrLink]) -> HashMap<String, String> {
        let mut assets = HashMap::new();

        match self {
            // =================================================================
            // СТРАТЕГИЯ 1: CLOUD OPTIMIZED GEOTIFF (Каждый канал — отдельный файл)
            // =================================================================
            Self::Hlsl30 | Self::Hlss30 | Self::Hlsl30Vi | Self::Hss30Vi => {
                for link in raw_links {
                    let href = &link.href;

                    if href.ends_with(".tif") || href.ends_with(".tiff") {
                        // Извлекаем чистый токен канала (например, "...v2.0.B05.tif" -> "B05")
                        if let Some(band_name) = parse_hls_band_name(href) {
                            assets.insert(band_name, href.clone());
                        }
                    } else if is_preview_link(href) {
                        assets.insert("preview".to_string(), href.clone());
                    } else if href.contains("s3credentials") {
                        assets.insert("s3_credentials_api".to_string(), href.clone());
                    }
                }
            }

            // =================================================================
            // СТРАТЕГИЯ 2: МОНОЛИТНЫЕ КОНТЕЙНЕРЫ (Все каналы внутри одного файла)
            // =================================================================
            Self::Mod09Ga | Self::Myd09Ga | Self::Vj109Ga | Self::Eco2Lste | Self::EmitL2ARefl => {
                for link in raw_links {
                    let href = &link.href;

                    if href.ends_with(".hdf") || href.ends_with(".h5") || href.ends_with(".nc") {
                        // Вместо динамических мусорных таймштампов жестко фиксируем ключ дата-куба
                        assets.insert("data_cube".to_string(), href.clone());
                    } else if href.ends_with(".xml") {
                        assets.insert("metadata_xml".to_string(), href.clone());
                    } else if is_preview_link(href) {
                        assets.insert("preview".to_string(), href.clone());
                    } else if href.contains("s3credentials") {
                        assets.insert("s3_credentials_api".to_string(), href.clone());
                    }
                }
            }
        }

        assets
    }
}

// ============================================================================
// ВНУТРЕННИЕ ИЗОЛИРОВАННЫЕ ХЕЛПЕРЫ (ПРИВАТНЫЕ ДЛЯ МОДУЛЯ)
// ============================================================================

/// Логика безопасного вычленения бэнда из HLS-ссылки.
/// Пример строки: "https://.../HLS.L30.T54HUJ.2025176T002639.v2.0.B05.tif"
fn parse_hls_band_name(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('.').collect();
    if parts.len() >= 2 {
        let band_part = parts[parts.len() - 2]; // Предпоследний элемент перед расширением файла

        // Приводим к верхнему регистру для единообразия в базе (Fmask -> FMASK, b01 -> B01)
        let normalized = band_part.to_uppercase();

        // Дополнительный предохранитель: если кусок слишком длинный (например, NASA изменило структуру URL),
        // мы не пишем мусор в имя бэнда. Имя канала у HLS всегда короткое (до 8 символов).
        if normalized.len() <= 8 {
            Some(normalized)
        } else {
            None
        }
    } else {
        None
    }
}

/// Универсальный маркер превью-ссылок
fn is_preview_link(url: &str) -> bool {
    url.ends_with(".jpg")
        || url.ends_with(".jpeg")
        || url.ends_with(".png")
        || url.contains("browse")
}
