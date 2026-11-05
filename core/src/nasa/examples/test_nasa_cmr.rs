use geo_core::nasa::client::{CmrClient, CmrQuery, CollectionSelector};
use geo_core::nasa::middleware::NasaAuthMiddleware;
use geo_core::nasa::product::{NasaProduct, StorageFormat};
use geo_types::{coord, Rect};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::{env, error::Error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Нативная инициализация логов без лишнего бойлерплейта
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    dotenvy::dotenv().ok();
    let token = env::var("NASA_API_KEY").unwrap_or_else(|_| "mock_token_for_dev".to_string());

    // Полная обойма продуктов ДЗЗ, зашитая в ядро нашего ГИС-конвейера
    let products_to_test = [
        NasaProduct::Hlsl30,
        NasaProduct::Hlss30,
        NasaProduct::Hlsl30Vi,
        NasaProduct::Hss30Vi,
        NasaProduct::Mod09Ga,
        NasaProduct::Myd09Ga,
        NasaProduct::Vj109Ga,
        NasaProduct::Eco2Lste,
        NasaProduct::EmitL2ARefl,
    ];

    // Наш проверенный расширенный BBox над Австралией (Moore Anomaly)
    let wide_target_region = Rect::new(coord!(x: 135.0, y: -35.0), coord!(x: 145.0, y: -30.0));
    let temporal_range = "2025-06-01T00:00:00Z,2025-06-30T23:59:59Z";

    // 2. Сборка сетевого пула с помощью идиоматичного конвейера Builder
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
    println!("🚀 ЗАПУСК УЛЬТИМАТИВНОГО ИНТЕГРАЦИОННОГО КОНВЕЙЕРА NASA CMR");
    println!("================================================================================");

    for product in &products_to_test {
        let short_name = product.as_str();
        let format = product.storage_format();

        println!(
            "\n🛰️  СКАНИРОВАНИЕ МИССИИ: {short_name} (v{}) | ФОРМАТ: {format}",
            product.default_version()
        );
        println!("   Concept ID: {}", product.default_concept_id());

        // Конфигурируем идиоматичный CmrQuery. Использование pattern-matching
        // на Supports Cloud Filter делает код чистым и изолированным.
        let mut query = CmrQuery::new(
            CollectionSelector::ShortName(short_name.to_string()),
            wide_target_region,
            temporal_range,
        )
        .with_page_size(3);

        if product.supports_cmr_cloud_filter() {
            query = query.with_cloud_cover(100);
        }

        // Выполняем асинхронный запрос и обрабатываем результат через лаконичный match
        match cmr_client.fetch_granules(query).await {
            Ok(collection) if collection.is_empty() => {
                println!(
                    "   ⚠️  Данные отсутствуют в этом пространственно-временном окне. Пропуск."
                );
            }
            Ok(collection) => {
                println!(
                    "   📊 Всего гранул найдено в архиве NASA: {}",
                    collection.len()
                );

                // Лениво извлекаем последнюю гранату и обрабатываем через `if let`
                if let Ok(granule) = collection.latest() {
                    println!("   📦 Concept ID:  {}", granule.concept_id);
                    println!("   📄 Имя гранулы: {}", granule.title);
                    println!("   📅 Дата съемки: {}", granule.time_start);
                    println!("   ☁️  Облачность:  {}%", granule.cloud_cover);

                    if let Some(ref preview) = granule.browse_url {
                        println!("   🖼️  Превью (JPG): {preview}");
                    }

                    println!("   🗺️  Структурированная карта ГИС-активов (Assets):");

                    // Декларативное макро matches! избавляет от громоздких ветвлений match/if
                    if matches!(format, StorageFormat::Cog) {
                        println!("      [Режим: Рассредоточенные COG-растры]");
                        // Идиоматичный функциональный итератор вместо императивных циклов for
                        granule.bands.iter().take(6).for_each(|(name, href)| {
                            let short_href = if href.len() > 65 {
                                format!("{}...", &href[..65])
                            } else {
                                href.clone()
                            };
                            println!("         🔹 Канал [{name}]: {short_href}");
                        });

                        if granule.bands.len() > 6 {
                            println!(
                                "         ... и еще {} спектральных каналов",
                                granule.bands.len() - 6
                            );
                        }
                    } else {
                        println!("      [Режим: Монолитный куб данных]");
                        // Элегантная распаковка HashMap через встроенный метод .get()
                        match (granule.bands.get("data_cube"), granule.bands.get("metadata_xml")) {
                            (Some(cube), Some(meta)) => {
                                println!("         📦 [DATA_CUBE]     -> {cube}");
                                println!("         📄 [METADATA_XML]  -> {meta}");
                            }
                            (Some(cube), None) => println!("         📦 [DATA_CUBE]     -> {cube}"),
                            _ => println!("         ❌ КРИТИЧЕСКАЯ ОШИБКА: Ссылки на куб данных отсутствуют в ответе!"),
                        }
                    }

                    // Отдельно и лаконично выводим нативные облачные AWS S3 линки
                    if !granule.s3_links.is_empty() {
                        println!("      ☁️  Прямые пути к AWS S3 бакетам (http://esipfed.org):");
                        granule.s3_links.iter().take(2).for_each(|(key, s3_href)| {
                            println!("         🔸 [S3_{key}] -> {s3_href}");
                        });
                    }
                }
            }
            Err(err) => println!("   ❌ Ошибка сетевого контура API для {short_name}: {err}"),
        }
        println!(
            "--------------------------------------------------------------------------------"
        );
    }

    println!("\n🎉 Сквозной тест ядра завершен! Архитектура линков полностью детерминирована.");
    Ok(())
}
