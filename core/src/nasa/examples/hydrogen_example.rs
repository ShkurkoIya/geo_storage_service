use geo_core::nasa::client::{CmrClient, CmrQuery, CollectionSelector};
use geo_core::nasa::cog_analyzer::CogAnalyzer;
use geo_core::nasa::middleware::NasaAuthMiddleware;
use geo_core::nasa::product::NasaProduct;
use geo_types::{coord, Rect};
use reqwest_middleware::ClientBuilder;
use std::{env, error::Error, fs::File, io::Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    dotenvy::dotenv().ok();
    let token = env::var("NASA_API_KEY").unwrap_or_else(|_| "mock_token_for_dev".to_string());

    // 1. Ищем снимки Landsat 8/9 через твой CmrClient
    let base_client = reqwest::Client::new();
    let http_client = ClientBuilder::new(base_client)
        .with(NasaAuthMiddleware { token })
        .build();
    let cmr_client = CmrClient::new(&http_client);

    let product = NasaProduct::Hlsl30;
    let target_region = Rect::new(coord!(x: 135.0, y: -35.0), coord!(x: 145.0, y: -30.0));
    let temporal_range = "2025-06-23T00:00:00Z,2025-06-26T23:59:59Z";

    tracing::info!("📡 Сканируем архив NASA CMR твоим клиентом...");
    let query = CmrQuery::new(
        CollectionSelector::ShortName(product.as_str().to_string()),
        target_region,
        temporal_range,
    )
    .with_page_size(3);

    let collection = cmr_client.fetch_granules(query).await?;

    if let Some(first_raw) = collection.granules.first() {
        println!("\n🔍 ДЕБАГ КЛЮЧЕЙ ТВОЕЙ ГРАНУЛЫ:");
        println!("   📦 Имя: {}", first_raw.title);
        println!(
            "   🗺️  Доступные ключи в bands: {:?}",
            first_raw.bands.keys().collect::<Vec<&String>>()
        );
        println!(
            "================================================================================\n"
        );
    }

    let best_granule = collection
        .granules
        .into_iter()
        .find(|g| g.bands.contains_key("B05") && g.bands.contains_key("B06"))
        .ok_or(
            "В прилетевшей пачке гранул не найдено ни одного полного снимка с NIR+SWIR1 каналами",
        )?;

    // Достаем из твоей HashMap стерильные HTTPS адреса бэндов
    let nir_url = best_granule.bands.get("B05").ok_or("NIR канал пропал")?;
    let swir_url = best_granule.bands.get("B06").ok_or("SWIR1 канал пропал")?;

    println!("\n================================================================================");
    println!("🛰️  ТВОЯ ЛИБА НАШЛА ЦЕЛЕВЫЕ HTTPS-ССЫЛКИ:");
    println!("   📦 Имя снимка: {}", best_granule.title);
    println!("   🔗 NIR Линк:   {}", nir_url);
    println!("   🔗 SWIR Линк:  {}", swir_url);
    println!("================================================================================");

    // 2. ЮЗАЕМ TERRIO ДВИЖОК: Лениво считаем водородную матрицу по сети
    // Вырезаем окно 500х500 пикселей в центре космического снимка
    let offset_x = 1500;
    let offset_y = 1500;
    let width = 500;
    let height = 500;
    let scientific_threshold = 0.38; // Порог аномалии

    let anomalies = CogAnalyzer::detect_hydrogen_anomaly(
        nir_url,
        swir_url,
        offset_x,
        offset_y,
        width,
        height,
        scientific_threshold,
    )?;

    // НАУЧНЫЙ РАСЧЕТ ПЛОЩАДИ: Один пиксель Landsat 8/9 равен 30х30 метров = 900 кв. метров.
    // Считаем общую площадь обнаруженных газовых факелов дегазации
    let total_anomalies_count = anomalies.len();
    let area_sq_km = (total_anomalies_count as f64 * 900.0) / 1_000_000.0;

    // 3. ИДИОМАТИЧНЫЙ ЭКСПОРТ В GEOJSON ДЛЯ ВЕРИФИКАЦИИ В QGIS
    tracing::info!("💾 Экспорт найденных полигонов в GeoJSON...");

    let features: Vec<geojson::Feature> = anomalies
        .into_iter()
        .map(|poly| geojson::Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new(geojson::Value::from(&poly))),
            id: None,
            properties: Some(
                serde_json::json!({
                    "layer": "hydrogen_gas_anomaly",
                    "engine": "terrio_pure_rust"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            foreign_members: None,
        })
        .collect();

    let feature_collection = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let mut file = File::create("pure_rust_anomalies.geojson")?;
    file.write_all(feature_collection.to_string().as_bytes())?;

    println!("\n🎉 ЧИСТОКРОВНЫЙ RUST-КОНВЕЙЕР ВЫПОЛНЕН УСПЕШНО!");
    println!("================================================================================");
    println!("📊 Обнаружено аномальных пикселей: {total_anomalies_count} шт.");
    println!("🗺️  Суммарная площадь дегазации:   {area_sq_km:.4} кв. км.");
    println!("📦 Файл сохранен: pure_rust_anomalies.geojson");
    println!("🚀 Ты полностью независим от C++ библиотек и системных пакетов!");
    println!("================================================================================");

    Ok(())
}

