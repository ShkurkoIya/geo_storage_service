use super::cmr::{CmrResponse, NasaCmrExt};
use crate::nasa::collection::GranuleCollection;
use geo_types::Rect;
use http::header::{ACCEPT, USER_AGENT};
use reqwest_middleware::ClientWithMiddleware;

#[derive(Debug, Clone)]
pub struct CmrQuery {
    pub selector: CollectionSelector,
    pub bbox: Rect<f64>,
    pub temporal: String,
    pub cloud_cover_max: Option<u8>,
    pub page_size: u16,
    pub sort_by_start_date_desc: bool,
}

#[derive(Debug, Clone)]
pub enum CollectionSelector {
    /// Поиск по человеческому имени продукта (например "MOD11A1")
    ShortName(String),
    ConceptId(String),
}

impl CmrQuery {
    pub fn new(selector: CollectionSelector, bbox: Rect<f64>, temporal: &str) -> Self {
        Self {
            selector,
            bbox,
            temporal: temporal.to_string(),
            cloud_cover_max: None,
            page_size: 100,
            sort_by_start_date_desc: true,
        }
    }

    pub fn to_query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("bounding_box", self.bbox.to_cmr_string()),
            ("temporal", self.temporal.clone()),
            ("page_size", self.page_size.to_string()),
        ];

        match &self.selector {
            CollectionSelector::ShortName(name) => params.push(("short_name", name.clone())),
            CollectionSelector::ConceptId(id) => params.push(("concept_id", id.clone())),
        }

        if self.sort_by_start_date_desc {
            params.push(("sort_key", "-start_date".to_string()));
        }

        if let Some(cloud) = self.cloud_cover_max {
            params.push(("cloud_cover", format!("0,{}", cloud)));
        }

        params
    }

    pub fn with_cloud_cover(mut self, max_percent: u8) -> Self {
        self.cloud_cover_max = Some(max_percent);
        self
    }

    pub fn with_page_size(mut self, size: u16) -> Self {
        self.page_size = size.min(2000);
        self
    }
}

pub struct CmrClient<'a> {
    pub(crate) http_client: &'a ClientWithMiddleware,
}

impl<'a> CmrClient<'a> {
    pub fn new(http_client: &'a ClientWithMiddleware) -> Self {
        Self { http_client }
    }

    pub async fn fetch_granules(&self, query: CmrQuery) -> Result<GranuleCollection, String> {
        let url = "https://cmr.earthdata.nasa.gov/search/granules.json";

        let query_params = query.to_query_params();

        let response = self
            .http_client
            .get(url)
            .query(&query_params)
            .header(USER_AGENT, "HydrogenGateway/1.0")
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("Critial error requesting CMR API: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "NASA CMR API returned server error. Status: {}",
                response.status()
            ));
        }

        let cmr_data = response.json::<CmrResponse>().await.map_err(|e| {
            format!(
                "Could not deserialize response structure from NASA CMR: {}",
                e
            )
        })?;

        Ok(cmr_data.into_collection())
    }
}
