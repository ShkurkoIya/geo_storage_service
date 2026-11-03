use geo::Intersects;
use geo_types::{Polygon, Rect};

use crate::nasa::granule::GisGranule;

#[derive(Debug, Clone)]
pub struct GranuleCollection {
    pub granules: Vec<GisGranule>,
}

impl GranuleCollection {
    /// Пустая ли коллекция
    pub fn is_empty(&self) -> bool {
        self.granules.is_empty()
    }

    /// Сколько граннул в коллекции
    pub fn len(&self) -> usize {
        self.granules.len()
    }

    /// Вытаскивает самую свежую гарнулу в коллекции
    pub fn latest(self) -> Result<GisGranule, String> {
        self.granules.into_iter().next().ok_or_else(|| {
            "GIS-Error: Granules with matching bands parameters not found".to_string()
        })
    }

    /// Жесткий фильтр бэндов оставляет только полные сцены
    pub fn require_bands(mut self, required_bands: &[&str]) -> Self {
        self.granules.retain(|granule| {
            required_bands
                .iter()
                .all(|band| granule.bands.contains_key(*band))
        });
        self
    }

    /// Фильтр облачности: отсекает всё, что затянуто тучами выше лимита
    pub fn filter_by_cloud_cover(mut self, max_percent: f32) -> Self {
        self.granules
            .retain(|granule| granule.cloud_cover <= max_percent);
        self
    }

    /// Оставляет только те гранулы NASA, которые физически пересекаются с регионом интереса (ROI).
    /// Защищает систему от сохранения мусорных сцен, задевающих Якутию по касательной.
    pub fn intersecting_roi(mut self, roi: &Rect<f64>) -> Self {
        // Баг #1 пофикшен: Безопасная конвертация Rect в Polygon без ручного вызова to_line_string
        let roi_polygon: Polygon<f64> = (*roi).into();

        self.granules.retain(|granule| -> bool {
            match &granule.geometry {
                Some(granule_poly) => granule_poly.intersects(&roi_polygon),
                // Если у гранулы нет геометрии, условно оставляем её
                None => true,
            }
        });
        self
    }

    /// Сортировка по времени съемки (от старых к свежим)
    pub fn sort_by_acquisition(&mut self) {
        self.granules.sort_by_key(|granule| granule.time_start);
    }
}

