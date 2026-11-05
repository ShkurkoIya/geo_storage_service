use geo_core::nasa::client::{CmrClient, CmrQuery, CollectionSelector};
use geo_core::nasa::middleware::NasaAuthMiddleware;
use geo_types::{coord, Rect};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::env;
use std::error::Error;

#[derive(Debug, Clone, Copy)]
enum NasaProduct {
    Hlsl30,
    Hlss30,
    Mod09Ga,
    Vj109Ga,
    Eco2Lste,
}

impl NasaProduct {
    fn short_name(&self) -> &'static str {
        match self {
            Self::Hlsl30 => "HLSL30",
            Self::Hlss30 => "HLSS30",
            Self::Mod09Ga => "MOD09GA",
            Self::Vj109Ga => "VJ109GA",
            Self::Eco2Lste => "ECO2LSTE",
        }
    }

    fn version(&self) -> &'static str {
        match self {
            Self::Hlsl30 | Self::Hlss30 => "2.0",
            Self::Mod09Ga => "061",
            Self::Vj109Ga => "002",
            Self::Eco2Lste => "002",
        }
    }

    // Хак: определяет, нужно ли нашему билдеру навязывать cloud_cover в строке запроса
    fn supports_cmr_cloud_filter(&self) -> bool {
        match self {
            Self::Hlsl30 | Self::Hlss30 => true,
            _ => false, // Для MODIS, VIIRS и ECOSTRESS в CMR этот параметр часто ломает выдачу
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    dotenvy::dotenv().ok();
    let token = env::var("NASA_API_KEY").unwrap_or_else(|_| "mock_token_for_dev".to_string());

    let products_to_test = [
        NasaProduct::Hlsl30,
        NasaProduct::Hlss30,
        NasaProduct::Mod09Ga,
        NasaProduct::Vj109Ga,
        NasaProduct::Eco2Lste,
    ];

    // УВЕЛИЧИВАЕМ BBOX: Расширяем рамки, чтобы гарантированно пересечь
    // массивные синусоидальные тайлы MODIS (h30v12) и полосы пролета МКС
    let wide_target_region = Rect::new(
        coord!(x: 135.0, y: -35.0), // Делаем захват шире по долготе и широте
        coord!(x: 145.0, y: -30.0),
    );

    // Временное окно — берем стабильный период 2025 года
    let temporal_range = "2025-06-01T00:00:00Z,2025-06-30T23:59:59Z";

    let base_client = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()?;

    let retry_strategy = ExponentialBackoff::builder().build_with_max_retries(3);
    let http_client = ClientBuilder::new(base_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_strategy))
        .with(NasaAuthMiddleware { token })
        .build();

    let cmr_client = CmrClient::new(&http_client);

    println!("\n================================================================================");
    println!("🛰️  ЗАПУСК СВЕРХУНИВЕРСАЛЬНОГО ТЕСТА С РАСШИРЕННЫМ BBOX");
    println!("================================================================================");

    for product in products_to_test.iter() {
        println!(
            "\n🔍 Запрос для продукта: {} (v{})",
            product.short_name(),
            product.version()
        );

        // Базовый конструктор запроса
        let mut query = CmrQuery::new(
            CollectionSelector::ShortName(product.short_name().to_string()),
            wide_target_region,
            temporal_range,
        )
        .with_page_size(3);

        // 🔥 Динамически настраиваем фильтр облачности в зависимости от специфики миссии в CMR
        if product.supports_cmr_cloud_filter() {
            query = query.with_cloud_cover(100); // Для HLS разрешаем всё, но контролируем параметр
        } else {
            // Если твой билдер CmrQuery по умолчанию ставит cloud_cover = 0 или другой дефолт,
            // убедись, что твоя библиотека под капотом не шлет "cloud_cover=" для таких продуктов.
            // (Если метод `.with_cloud_cover` принимает Option, тут стоило бы передать None).
        }

        match cmr_client.fetch_granules(query).await {
            Ok(collection) => {
                println!("   📊 Найдено гранул: {}", collection.len());

                if collection.is_empty() {
                    println!("   ⚠️  Данные отсутствуют. Попробуй сместить BBox на экватор или расширить даты.");
                    continue;
                }

                if let Ok(gis_granule) = collection.latest() {
                    println!("   📦 ID:       {}", gis_granule.concept_id);
                    println!("   📄 Имя:      {}", gis_granule.title);
                    println!("   📅 Дата:     {}", gis_granule.time_start);

                    println!("   🗺️  Доступные каналы (первые 5 строк):");
                    for (band_name, href) in gis_granule.bands.iter().take(5) {
                        let short_href = if href.len() > 65 {
                            format!("{}...", &href[..65])
                        } else {
                            href.clone()
                        };
                        println!("      🔹 [{}] -> {}", band_name, short_href);
                    }
                }
            }
            Err(err) => {
                println!("   ❌ Ошибка API: {}", err);
            }
        }
        println!(
            "--------------------------------------------------------------------------------"
        );
    }

    Ok(())
}
