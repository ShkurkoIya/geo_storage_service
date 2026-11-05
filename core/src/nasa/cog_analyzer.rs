use geo_types::{Coord, LineString, Polygon};
use std::error::Error;
use terrio::{open_raster, FloatReadOptions, Window};

pub struct CogAnalyzer;

impl CogAnalyzer {
    /// Лениво качает матрицы по HTTPS через terrio, считает водородный индекс
    /// и векторизует аномалии в нативные полигоны Rust.
    pub fn detect_hydrogen_anomaly(
        nir_url: &str,
        swir_url: &str,
        window_x: u32,
        window_y: u32,
        window_w: u32,
        window_h: u32,
        threshold: f32,
    ) -> Result<Vec<Polygon<f64>>, Box<dyn Error>> {
        tracing::info!("⏳ Ленивое HTTP-открытие COG растров через terrio...");
        // 🔥 ПОЧИНИЛИ: Убрали лишний аргумент bool, теперь строго один параметр!
        let nir_ds = open_raster(nir_url)?;
        let swir_ds = open_raster(swir_url)?;

        // Приводим u32 к usize
        let window = Window::new(
            window_x.try_into().unwrap(),
            window_y.try_into().unwrap(),
            window_w.try_into().unwrap(),
            window_h.try_into().unwrap(),
        );

        let read_options = FloatReadOptions::default();

        tracing::info!("📡 Загрузка целевых матриц пикселей из NASA Cloud...");
        let matrix_nir: Vec<f32> = nir_ds.read_window_f32(window, &read_options)?;
        let matrix_swir: Vec<f32> = swir_ds.read_window_f32(window, &read_options)?;

        let mut rust_polygons = Vec::new();

        let pixel_size_deg = 0.000269;
        let base_lon = 139.5;
        let base_lat = -32.5;

        tracing::info!("🎯 Запуск матричного расчета водородного индекса...");

        for row in 0..window_h {
            for col in 0..window_w {
                let idx = (row * window_w + col) as usize;
                let nir = matrix_nir[idx];
                let swir = matrix_swir[idx];

                if (nir + swir) > 0.0 {
                    let index = (nir - swir) / (nir + swir);

                    if index > threshold {
                        let lon_min = base_lon + (col as f64) * pixel_size_deg;
                        let lat_max = base_lat - (row as f64) * pixel_size_deg;
                        let lon_max = lon_min + pixel_size_deg;
                        let lat_min = lat_max - pixel_size_deg;

                        let exterior = LineString::new(vec![
                            Coord {
                                x: lon_min,
                                y: lat_min,
                            },
                            Coord {
                                x: lon_max,
                                y: lat_min,
                            },
                            Coord {
                                x: lon_max,
                                y: lat_max,
                            },
                            Coord {
                                x: lon_min,
                                y: lat_max,
                            },
                            Coord {
                                x: lon_min,
                                y: lat_min,
                            },
                        ]);

                        rust_polygons.push(Polygon::new(exterior, vec![]));
                    }
                }
            }
        }

        tracing::info!(
            "🎉 Чистокровный Rust-движок успешно выделил аномалий: {}",
            rust_polygons.len()
        );
        Ok(rust_polygons)
    }
}

