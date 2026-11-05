use crate::nasa::product::NasaProduct;
use crate::nasa::product::StorageFormat; // Импортируем наш обновленный энам форматов
use std::collections::HashMap;

/// Вспомогательная структура для десериализации ссылок из JSON/Atom ответов NASA CMR.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RawCmrLink {
    pub rel: String,
    pub href: String,
    pub title: Option<String>,
}

/// Трейт, расширяющий функционал NasaProduct для каноничного парсинга бэндов под все форматы данных.
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
    /// Возвращает кортеж: (Bands/Data_links, S3_links, Browse_url)
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

        // Определяем физический формат хранения продукта один раз перед циклом
        let format = self.storage_format();

        for link in raw_links {
            let href_lower = link.href.to_lowercase();
            let rel_lower = link.rel.to_lowercase();

            // Проверяем, не нативная ли это облачная S3 ссылка (AWS бакет)
            let is_s3 = rel_lower.contains("esipfed.org") || link.href.starts_with("s3://");

            // --- Шаг А: Отлавливаем превьюшки миссии (JPG/PNG) ---
            if is_preview_link(&href_lower, &rel_lower) {
                if browse_url.is_none() && link.href.starts_with("https://") {
                    browse_url = Some(link.href.clone());
                }
                continue; // Превью обработано, идем дальше
            }

            // --- Шаг Б: Динамический роутинг ссылок под ВСЕ форматы хранения ---
            match format {
                // 1. Облачные поканальные растры (HLS продукты)
                StorageFormat::Cog => {
                    if href_lower.contains(".tif") || href_lower.contains(".tiff") {
                        if let Some(band_name) = parse_hls_band_name(&link.href) {
                            if is_s3 {
                                s3_links.insert(band_name, link.href.clone());
                            } else {
                                bands.insert(band_name, link.href.clone());
                            }
                        }
                    }
                }

                // 2. Классические тяжелые GeoTIFF (например, NASADEM, ASTER GED)
                StorageFormat::GeoTiff => {
                    if href_lower.contains(".tif") || href_lower.contains(".tiff") {
                        if is_s3 {
                            s3_links.insert("geotiff_raster".to_string(), link.href.clone());
                        } else {
                            bands.insert("geotiff_raster".to_string(), link.href.clone());
                        }
                    } else if href_lower.contains(".xml") || href_lower.contains(".met") {
                        if !is_s3 {
                            bands.insert("metadata_xml".to_string(), link.href.clone());
                        }
                    }
                }

                // 3. Многомерные монолитные архивы-матрицы (MODIS, VIIRS, ECOSTRESS)
                StorageFormat::Hdf => {
                    if href_lower.contains(".hdf")
                        || href_lower.contains(".h5")
                        || href_lower.contains(".he5")
                    {
                        if is_s3 {
                            s3_links.insert("data_cube".to_string(), link.href.clone());
                        } else {
                            bands.insert("data_cube".to_string(), link.href.clone());
                        }
                    } else if href_lower.contains(".xml") || href_lower.contains(".met") {
                        if !is_s3 {
                            bands.insert("metadata_xml".to_string(), link.href.clone());
                        }
                    }
                }

                // 4. Плотные спектральные кубы нового поколения (EMIT миссия)
                StorageFormat::NetCdf => {
                    if href_lower.contains(".nc") || href_lower.contains(".nc4") {
                        if is_s3 {
                            s3_links.insert("data_cube".to_string(), link.href.clone());
                        } else {
                            bands.insert("data_cube".to_string(), link.href.clone());
                        }
                    } else if href_lower.contains(".xml") {
                        if !is_s3 {
                            bands.insert("metadata_xml".to_string(), link.href.clone());
                        }
                    }
                }

                // 5. Векторные ГИС слои и треки (Shapefile, GeoJSON, KML, таблицы выработок CSV)
                StorageFormat::Vector => {
                    if href_lower.contains(".json") || href_lower.contains(".geojson") {
                        bands.insert("geojson_vector".to_string(), link.href.clone());
                    } else if href_lower.contains(".kml") || href_lower.contains(".kmz") {
                        bands.insert("kml_vector".to_string(), link.href.clone());
                    } else if href_lower.contains(".zip") {
                        // NASA часто пакует ESRI Shapefile слои в .zip архивы
                        bands.insert("shp_zip_container".to_string(), link.href.clone());
                    } else if href_lower.contains(".csv") {
                        bands.insert("csv_table".to_string(), link.href.clone());
                    }
                }

                // 6. Метеорологические и атмосферные сетки (миссии осадков GPM, модели MERRA-2)
                StorageFormat::Grib => {
                    if href_lower.contains(".grb")
                        || href_lower.contains(".grib")
                        || href_lower.contains(".grib2")
                    {
                        if is_s3 {
                            s3_links.insert("grib_met_grid".to_string(), link.href.clone());
                        } else {
                            bands.insert("grib_met_grid".to_string(), link.href.clone());
                        }
                    }
                }

                // 7. Сценарий-предохранитель для сырых бинарников, ASCII логов и прочих текстовых файлов качества
                StorageFormat::Other => {
                    if href_lower.contains(".txt")
                        || href_lower.contains(".asc")
                        || href_lower.contains(".bin")
                    {
                        if !is_s3 {
                            let ext = link
                                .href
                                .split('.')
                                .next_back()
                                .unwrap_or("raw")
                                .to_string();
                            bands.insert(format!("raw_{}_file", ext), link.href.clone());
                        }
                    }
                }
            }
        }

        (bands, s3_links, browse_url)
    }
}

// ============================================================================
// ВНУТРЕННИЕ ИЗОЛИРОВАННЫЕ ХЕЛПЕРЫ
// ============================================================================

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

fn is_preview_link(href_lower: &str, rel_lower: &str) -> bool {
    rel_lower.contains("browse#")
        || href_lower.contains("browse")
        || href_lower.ends_with(".jpg")
        || href_lower.ends_with(".jpeg")
        || href_lower.ends_with(".png")
}
