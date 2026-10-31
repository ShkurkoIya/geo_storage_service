use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BboxParams {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

#[derive(Debug, Deserialize)]
pub struct RadiusParams {
    pub lon: f64,
    pub lat: f64,
    pub radius_m: f64,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub current_page: i64,
    pub per_page: i64,
    pub total_records: i64,
    pub total_pages: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}
