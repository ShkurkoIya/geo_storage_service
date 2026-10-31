// Вспомогательная функция для сборки полигона в формате WKT
// Переводит массив координат [(lon, lat)] в строку, понятную PostGIS ST_GeomFromText
pub fn coords_to_wkt(coords: &[(f64, f64)]) -> String {
    if coords.is_empty() {
        return "POLYGON EMPTY".to_string();
    }

    let points: Vec<String> = coords
        .iter()
        .map(|(lon, lat)| format!("{} {}", lon, lat))
        .collect();

    format!("POLYGON(({}))", points.join(", "))
}
