pub mod client;
pub mod cmr;
pub mod cog;
pub mod collection;
pub mod extractor;
pub mod granule;
pub mod middleware;
pub mod models;
pub mod product;
pub mod utils;

use middleware::NasaAuthMiddleware;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

pub use client::CmrClient;
pub use cog::CogClient;

#[derive(Clone)]
pub struct NasaProvider {
    http_client: ClientWithMiddleware,
}

impl NasaProvider {
    pub fn new(token: String) -> Self {
        let base_client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .unwrap_or_default();

        let http_client = ClientBuilder::new(base_client)
            .with(NasaAuthMiddleware { token })
            .build();

        Self { http_client }
    }

    pub fn cmr(&self) -> CmrClient<'_> {
        CmrClient::new(&self.http_client)
    }

    pub fn cog(&self) -> CogClient<'_> {
        CogClient::new(&self.http_client)
    }
}
