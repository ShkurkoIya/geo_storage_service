use reqwest_middleware::ClientWithMiddleware;
use std::io::Cursor;
use tiff::decoder::Decoder;
use tiff::tags::Tag;

pub struct CogClient<'a> {
    pub(crate) http_client: &'a ClientWithMiddleware,
}

#[derive(Debug)]
pub struct CogMetadata {
    pub width: u32,
    pub height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
}

impl<'a> CogClient<'a> {
    pub fn new(http_client: &'a ClientWithMiddleware) -> Self {
        Self { http_client }
    }

    /// Временная заглушка для ленивого чтения куска COG-файла.
    /// Сюда мы на следующем шаге внедрим HTTP Range Requests и твой стриминг.
    pub async fn read_range(&self, url: &str, byte_range: &str) -> Result<Vec<u8>, String> {
        let response = self
            .http_client
            .get(url)
            .header("Range", format!("bytes={}", byte_range))
            .send()
            .await
            .map_err(|e| format!("Error COG Range request: {}", e))?;

        if response.status() != 206 && !response.status().is_success() {
            return Err(format!(
                "Server COG returnd error code: {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Error loading bytes COG: {}", e))?;

        Ok(bytes.to_vec())
    }

    pub async fn read_metadata(&self, url: &str) -> Result<CogMetadata, String> {
        // Качаем только заголовок (первые 16 КБ)
        let header_bytes = self.read_range(url, "0-16384").await?;

        // Оборачиваем байты в Cursor, так как tiff::Decoder требует трейт Read + Seek
        let cursor = Cursor::new(header_bytes);

        let mut decoder = Decoder::new(cursor)
            .map_err(|e| format!("Could not initialize TIFF-decoder: {}", e))?;

        // Вытаскиваем геопространственные размеры матрицы пикселей
        let (width, height) = decoder
            .dimensions()
            .map_err(|e| format!("TIFF size read error: {}", e))?;

        let tile_width = decoder.get_tag_u32(Tag::TileWidth).unwrap_or(0);

        let tile_height = decoder.get_tag_u32(Tag::TileLength).unwrap_or(0);

        Ok(CogMetadata {
            width,
            height,
            tile_width,
            tile_height,
        })
    }
}
