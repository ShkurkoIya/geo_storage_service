pub fn extract_band_name(href: &str) -> Option<String> {
    let path = href.split('/').last()?;
    let parts: Vec<&str> = path.split('.').collect();

    // В названии файлов NASA HLS канал идет предпоследним перед расширением:
    // напр. HLS.L30.T10SFG.2026150T184521.v2.0.B11.tif -> ["HLS", ..., "v2", "0", "B11", "tif"]
    if parts.len() >= 2 {
        let band_part = parts[parts.len() - 2];
        Some(band_part.to_uppercase())
    } else {
        None
    }
}
/// Парсит сырой массив координат из ответа NASA CMR в нативный гео-тип Polygon.
/// NASA возвращает полигоны в виде Vec<Vec<String>>, где каждая строка содержит
/// координаты точек, разделенные пробелами (например: "62.5 120.0 62.6 120.1 ...")
pub fn parse_polygon_from_cmr(cmr_polygons: &[Vec<String>]) -> Option<geo_types::Polygon<f64>> {
    // Берем первое внешнее кольцо полигона (остальные если есть — это внутренние вырезы/дырки)
    let first_poly_ring = cmr_polygons.first()?;
    let mut coords = Vec::new();

    for ring_str in first_poly_ring {
        let tokens: Vec<&str> = ring_str.split_whitespace().collect();

        // Координаты в CMR идут парами: [Latitude, Longitude]
        // Итерируемся шагом по 2 элемента
        for chunk in tokens.chunks_exact(2) {
            let lat: f64 = chunk[0].parse().ok()?;
            let lon: f64 = chunk[1].parse().ok()?;

            // В geo_types координаты создаются в формате (X: Longitude, Y: Latitude)
            coords.push(geo_types::Coord { x: lon, y: lat });
        }
    }

    if coords.is_empty() {
        return None;
    }

    // Если NASA прислала незамкнутое кольцо, PostGIS/geo-types упадут при валидации.
    // Автоматически страхуемся: замыкаем полигон, если первая и последняя точки не совпадают.
    if coords.first() != coords.last() {
        if let Some(&first_coord) = coords.first() {
            coords.push(first_coord);
        }
    }

    // Строим и возвращаем готовый полигон без внутренних дыр
    Some(geo_types::Polygon::new(
        geo_types::LineString::new(coords),
        vec![],
    ))
}
