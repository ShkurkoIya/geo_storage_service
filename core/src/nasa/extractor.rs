use crate::nasa::product::NasaProduct;
use crate::nasa::product::StorageFormat;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RawCmrLink {
    pub rel: String,
    pub href: String,
    pub title: Option<String>,
}

pub trait CmrLinkParser {
    fn parse_links(
        &self,
        raw_links: &[RawCmrLink],
    ) -> (
        HashMap<String, String>,
        HashMap<String, String>,
        Option<String>,
    );
}

impl CmrLinkParser for NasaProduct {
    fn parse_links(
        &self,
        raw_links: &[RawCmrLink],
    ) -> (
        HashMap<String, String>,
        HashMap<String, String>,
        Option<String>,
    ) {
        let mut bands = HashMap::new();
        let mut s3_links = HashMap::new();
        let mut browse_url = None;

        let format = self.storage_format();
        let is_cog = matches!(format, StorageFormat::Cog);

        for link in raw_links {
            let href_lower = link.href.to_lowercase();
            let rel_lower = link.rel.to_lowercase();

            // 1. Отсекаем мусорные ссылки на документацию и сайты, которые сует NASA
            if href_lower.contains("doi.org")
                || href_lower.contains("html")
                || href_lower.contains("/centers")
            {
                continue;
            }

            // 2. Отлавливаем превьюшки (JPG/PNG/Browse)
            if rel_lower.contains("browse")
                || href_lower.contains("browse")
                || href_lower.ends_with(".jpg")
                || href_lower.ends_with(".jpeg")
            {
                if browse_url.is_none() && link.href.starts_with("https://") {
                    browse_url = Some(link.href.clone());
                }
                continue;
            }

            // Изолируем маркер S3-протокола (прямой s3:// префикс или esip-метка)
            let is_s3 = rel_lower.contains("esipfed.org") || link.href.starts_with("s3://");

            // 3. УНИВЕРСАЛЬНЫЙ РОУТИНГ КАНАЛОВ ПО РАСШИРЕНИЯМ ФАЙЛОВ
            if is_cog {
                // Если продукт поканальный (HLS) — ищем все .tif растры
                if href_lower.contains(".tif") || href_lower.contains(".tiff") {
                    if let Some(band_name) = parse_hls_band_name(&link.href) {
                        if is_s3 {
                            s3_links.insert(band_name, link.href.clone());
                        } else {
                            bands.insert(band_name, link.href.clone());
                        }
                    }
                }
            } else {
                // Если продукт монолитный (MODIS, VIIRS, EMIT) — ищем кубы данных
                if href_lower.contains(".hdf")
                    || href_lower.contains(".h5")
                    || href_lower.contains(".nc")
                    || href_lower.contains(".zip")
                {
                    if is_s3 {
                        s3_links.insert("data_cube".to_string(), link.href.clone());
                    } else {
                        bands.insert("data_cube".to_string(), link.href.clone());
                    }
                }
                // Ищем сопутствующие XML файлы метаданных качества
                else if href_lower.contains(".xml") || href_lower.contains(".met") {
                    if !is_s3 {
                        bands.insert("metadata_xml".to_string(), link.href.clone());
                    }
                }
            }
        }

        // 🔥 НАШ СЕНЬОРСКИЙ ФОЛБЭК ДЛЯ МОНОЛИТОВ:
        // Если для MODIS/VIIRS нет открытой HTTPS ссылки на куб (только S3),
        // мы копируем S3 адрес в bands, трансформируя его в публичный веб-линк!
        if !is_cog && bands.get("data_cube").is_none() {
            if let Some(s3_url) = s3_links.get("data_cube") {
                let https_fallback = s3_url.replace("s3://", "https://nasa.gov");
                bands.insert("data_cube".to_string(), https_fallback);
            }
        }

        (bands, s3_links, browse_url)
    }
}

fn parse_hls_band_name(url: &str) -> Option<String> {
    let clean_url = url.split('?').next()?;
    let parts: Vec<&str> = clean_url.split('.').collect();
    if parts.len() >= 2 {
        let band_part = parts[parts.len() - 2];
        let normalized = band_part.to_uppercase();
        if normalized.len() <= 8 {
            Some(normalized)
        } else {
            None
        }
    } else {
        None
    }
}

