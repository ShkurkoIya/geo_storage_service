use gdal::raster::Buffer;
use gdal::vector::LayerOptions;
use gdal::Dataset;
use geo_types::Polygon;
use std::error::Error;

pub struct CogAnalyzer;

impl CogAnalyzer {
    /// Лениво читает два удаленных канала по HTTPS, считает индекс и возвращает полигоны аномалий.
    /// `window_offset`: (x, y) верхний левый угол пикселя в системе координат снимка
    /// `window_size`: (width, height) размер анализируемого окна (например, 3000х3000 пикселей)
    pub fn detect_hydrogen_anomaly(
        nir_url: &str,
        swir_url: &str,
        window_offset: (isize, isize),
        window_size: (usize, usize),
        threshold: f32,
    ) -> Result<Vec<Polygon<f64>>, Box<dyn Error>> {
        // 1. Обертываем URL в виртуальную файловую систему GDAL /vsicurl/ для ленивого чтения по сети
        let vsi_nir = format!("/vsicurl/{nir_url}");
        let vsi_swir = format!("/vsicurl/{swir_url}");

        let ds_nir = Dataset::open(&vsi_nir)?;
        let ds_swir = Dataset::open(&vsi_swir)?;

        let band_nir = ds_nir.rasterband(1)?;
        let band_swir = ds_swir.rasterband(1)?;

        // 2. Читаем матрицы пикселей только для нашего окна интереса
        tracing::info!("⏳ Удаленное чтение матриц пикселей из NASA Cloud...");
        let buffer_nir = band_nir.read_as::<f32>(window_offset, window_size, window_size, None)?;
        let buffer_swir =
            band_swir.read_as::<f32>(window_offset, window_size, window_size, None)?;

        let total_pixels = window_size.0 * window_size.1;

        // Маска аномалий: 1 — аномалия (водород), 0 — норма
        let mut mask_buffer = vec![0u8; total_pixels];

        // 3. НАУЧНЫЙ РАСЧЕТ МАТРИЦЫ ИНДЕКСОВ
        // Считаем каноничный индекс: (NIR - SWIR) / (NIR + SWIR)
        for i in 0..total_pixels {
            let nir = buffer_nir.data().get(i)?;
            let swir = buffer_swir.data().get(i)?;

            if (nir + swir) > 0.0 {
                let index = (nir - swir) / (nir + swir);
                if index > threshold {
                    mask_buffer[i] = 1;
                }
            }
        }

        tracing::info!("🎯 Матричный расчет завершен. Запускаем векторизацию аномалий...");

        // 4. ВЕКТОРИЗАЦИЯ (Превращаем пиксели маски в честные векторные полигоны)
        let mem_driver = gdal::Driver::get("MEM")?;

        // Создаем Dataset в памяти с указанием размеров
        let mut tmp_ds = mem_driver.create::<u8>("", window_size.0, window_size.1, 1)?;

        if let Ok(geo_transform) = ds_nir.geo_transform() {
            tmp_ds.set_geo_transform(&geo_transform)?;
        }

        let mut tmp_band = tmp_ds.rasterband(1)?;

        // Оборачиваем вектор маски в честный GDAL Buffer
        let gdal_buffer = Buffer::new((window_size.0, window_size.1), mask_buffer);
        tmp_band.write((0, 0), window_size, &gdal_buffer)?;

        // Создаем слой через LayerOptions структуру
        let mut out_ds = mem_driver.create::<u8>("", 0, 0, 0)?;
        let out_layer = out_ds.create_layer(LayerOptions {
            name: "anomaly",
            ty: gdal::vector::OGRwkbGeometryType::wkbPolygon,
            srs: None,
        })?;

        // Запуск алгоритма polygonize
        gdal::raster::polygonize(&tmp_band, &out_layer, None, -1, &[], None)?;

        // 5. Конвертируем полигоны GDAL в нативные полигоны geo-types
        let mut rust_polygons = Vec::new();

        for feature in out_layer.features() {
            // Векторизатор по умолчанию пишет значение маски пикселя (0 или 1) в первое поле
            if let Some(field_val) = feature.field(0)? {
                if field_val.into_int() == Some(1) {
                    if let Some(gdal_geom) = feature.geometry() {
                        let wkt = gdal_geom.to_wkt()?;
                        if let Ok(geo_types_geom) = wkt.parse::<geo_types::Geometry<f64>>() {
                            if let geo_types::Geometry::Polygon(poly) = geo_types_geom {
                                rust_polygons.push(poly);
                            }
                        }
                    }
                }
            }
        }

        tracing::info!(
            "🎉 Успешно выделено полигонов дегазации водорода: {}",
            rust_polygons.len()
        );
        Ok(rust_polygons)
    }
}

