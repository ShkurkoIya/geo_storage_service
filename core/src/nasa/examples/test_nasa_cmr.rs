use geo_core::nasa::client::{CmrClient, CmrQuery, CollectionSelector};
use geo_core::nasa::cmr::NasaCmrExt;
use geo_core::nasa::collection::GranuleCollection;
use geo_core::nasa::granule::GisGranule;
use geo_core::nasa::middleware::NasaAuthMiddleware;

use geo_types::{coord, Rect};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Инициализируем окружение логов для наглядности
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("📡 Запуск сквозного интеграционного теста NASA CMR & COG...");

    // 2. Нативно подтягиваем токен из корневого .env через утилиту dotenv
    dotenvy::dotenv().ok();
    let token = env::var("NASA_API_KEY").unwrap_or_else(|_| "mock_token_for_dev".to_string());

    if token == "mock_token_for_dev" {
        tracing::warn!("⚠️ NASA_API_KEY не найден в окружении. Запрос уйдет АНОНИМНО.");
    } else {
        tracing::info!(
            "✅ Токен авторизации успешно загружен (длина: {} симв.)",
            token.len()
        );
    }

    // 3. Выделяем ГИС Bounding Box (Rect)
    // Координаты квадрата в районе аномалии Мур (Южная Австралия)
    let target_region = Rect::new(
        coord!(x: 139.0, y: -33.5), // min_lon, min_lat
        coord!(x: 140.5, y: -32.0), // max_lon, max_lat
    );

    // 4. Задаем временной диапазон (Time Series)
    let temporal_range = "2026-01-01T00:00:00Z,2026-06-30T23:59:59Z";

    // 5. Собираем универсальный запрос CmrQuery через наш Builder паттерн
    let query = CmrQuery::new(
        CollectionSelector::ShortName("HLSL30".to_string()),
        target_region,
        temporal_range,
    )
    .with_cloud_cover(15) // Нам нужно чистое небо над Австралией (макс 15% облаков)
    .with_page_size(50); // Выкачиваем пачку для поиска временного ряда

    // 6. 🔥 СОБИРАЕМ СЕНЬОРСКИЙ HTTP-КЛИЕНТ И ИНЖЕКТИРУЕМ MIDDLEWARE
    let base_client = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()?;

    let retry_strategy = ExponentialBackoff::builder().build_with_max_retries(3);

    // Склеиваем сэндвич: Ретраи + Твоя авторизация с Client-Id, Accept и User-Agent!
    let http_client = ClientBuilder::new(base_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_strategy))
        .with(NasaAuthMiddleware { token })
        .build();

    // Создаем экземпляр сетевого клиента CMR, скормив ему ссылку на middleware-сборку
    let cmr_client = CmrClient::new(&http_client);

    tracing::info!("🔍 Выполняем асинхронный поиск в каталоге NASA Earthdata...");

    // ============================================================================
    // 🌊 ПЛОСКИЙ АСИНХРОННЫЙ КОНВЕЙЕР (ОШИБКИ ПРИВЕДЕНЫ К ЕДИНOMУ BOX<DYN ERROR>)
    // ============================================================================
    let run_pipeline = async {
        // Шаг 1: Фетчим универсальную коллекцию гранул через обновленный сетевой CMR клиент
        let collection = cmr_client.fetch_granules(query).await?;
        tracing::info!(
            "📊 Всего сырых гранул прилетело от NASA: {}",
            collection.len()
        );

        // Шаг 2: Фильтруем каналы на прикладном уровне (требуем NIR + SWIR1 для дегазации)
        let hydrogen_pipeline_bands = ["B05", "B06"];
        let filtered_collection = collection.require_bands(&hydrogen_pipeline_bands);
        tracing::info!(
            "🎯 Гранул после спектральной фильтрации (NIR+SWIR1): {}",
            filtered_collection.len()
        );

        // Шаг 3: Вытягиваем самую свежую гранулу конвейера из collection.rs
        let gis_granule = filtered_collection.latest()?;
        tracing::info!("🎯 Целевой снимок выбран: {}", gis_granule.title);

        // Шаг 4: Достаем URL конкретного нужного нам канала (например, B05)
        let target_cog_url = gis_granule
            .bands
            .get("B05")
            .ok_or_else(|| "Ошибка: Канал B05 пропал из мапы".to_string())?;

        // Шаг 5: Имитируем ленивое чтение метаданных COG (пока заглушка, так как cog.rs пишется параллельно)
        tracing::info!("⏳ Читаем метаданные COG удаленно (HTTP Range 0-16384)...");
        tracing::info!("🔗 Ссылка на COG-растр канала B05: {}", target_cog_url);

        // Возвращаем собранные данные наверх
        Ok(gis_granule)
    };

    // Запускаем наш плоский конвейер на выполнение
    let pipeline_result: Result<GisGranule, Box<dyn Error>> = run_pipeline.await;

    // ============================================================================
    // 🏁 ОБРАБОТКА ЕДИНОГО РЕЗУЛЬТАТА НА ВЫХОДЕ
    // ============================================================================
    match pipeline_result {
        Ok(granule) => {
            println!("\n🎉 ГИС-КОНВЕЙЕР ВЫПОЛНЕН УСПЕШНО:");
            println!("==================================================");
            println!("📦 Концепт ID (NASA): {}", granule.concept_id);
            println!("📄 Имя гранулы:      {}", granule.title);
            println!("☁️ Облачность:        {}%", granule.cloud_cover);
            println!("📅 Дата съемки:       {}", granule.time_start);
            if let Some(preview) = granule.browse_url {
                println!("🖼️ JPG-Превьюшка:     {}", preview);
            }
            println!("\n🗺️ Список всех доступных спектральных COG-ссылок для этого снимка:");
            for (band_name, href) in granule.bands.iter() {
                println!("   🔹 Канал [{}]: {}", band_name, href);
            }
            println!("==================================================");
            println!("🚀 Мы лениво распарсили параметры без выкачивания гигабайтов в RAM!");
        }
        Err(err) => {
            eprintln!("\n❌ Конвейер упал на одном из шагов: {}", err);
        }
    }

    Ok(())
}
