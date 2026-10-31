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
