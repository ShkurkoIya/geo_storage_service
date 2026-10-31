use geo_core::nasa::cmr::{CmrQuery, CollectionSelector};
use geo_core::nasa::NasaProvider;
use geo_types::{coord, Rect};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Инициализируем окружение логов
    println!("📡 Запуск сквозного интеграционного теста NASA CMR & COG...");

    // 2. Нативно подтягиваем токен из корневого .env через утилиту dotenv
    let token = std::env::var("NASA_API_KEY").unwrap_or_else(|_| "mock_token_for_dev".to_string());

    if token == "mock_token_for_dev" {
        println!("⚠️ Предупреждение: NASA_API_KEY не найден в окружении. Запрос уйдет АНОНИМНО.");
    } else {
        println!(
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
    // Используем продукт HLSL30 (Harmonized Landsat 8) — он хранится строго в COG
    let query = CmrQuery::new(
        CollectionSelector::ShortName("HLSL30".to_string()),
        target_region,
        temporal_range,
    )
    .with_cloud_cover(15) // Нам нужно чистое небо над Австралией (макс 15% облаков)
    .with_page_size(50); // Выкачиваем пачку для поиска временного ряда

    // 6. Инициализируем наш инфраструктурный провайдер ядра
    let provider = NasaProvider::new(token);

    println!("🔍 Выполняем асинхронный поиск в каталоге NASA Earthdata...");

    // ============================================================================
    // 🌊 ПЛОСКИЙ АСИНХРОННЫЙ КОНВЕЙЕР (БЕЗ ЛЕСЕНИК И КОМБИНАТОРОВ AND_THEN)
    // ============================================================================
    let run_pipeline = async {
        // Шаг 1: Фетчим универсальную коллекцию гранул через сетевой CMR клиент
        let collection = provider.cmr().fetch_granules(query).await?;
        println!(
            "📊 Всего сырых гранул прилетело от NASA: {}",
            collection.len()
        );

        // Шаг 2: Фильтруем каналы на прикладном уровне (требуем NIR + SWIR1 для водорода)
        let hydrogen_pipeline_bands = ["B05", "B06"];
        let filtered_collection = collection.require_bands(&hydrogen_pipeline_bands);
        println!(
            "🎯 Гранул после спектральной фильтрации (NIR+SWIR1): {}",
            filtered_collection.len()
        );

        // Шаг 3: Вытягиваем самый свежий снимок временного ряда
        let gis_granule = filtered_collection.latest()?;
        println!("\n🎯 Целевой снимок выбран: {}", gis_granule.title);

        // Шаг 4: Достаем URL конкретного нужного нам канала (например, B05)
        let target_cog_url = gis_granule
            .bands
            .get("B05")
            .ok_or_else(|| "Ошибка: Канал B05 пропал из мапы".to_string())?;

        println!("⏳ Читаем метаданные COG удаленно (HTTP Range 0-16384)...");

        // Шаг 5: Лениво читаем метаданные через COG клиент (HTTP Range запрос)
        let meta = provider.cog().read_metadata(target_cog_url).await?;

        // Возвращаем собранные данные наверх
        Ok((gis_granule, meta))
    };

    // Запускаем наш плоский конвейер на выполнение
    let pipeline_result: Result<_, String> = run_pipeline.await;

    // ============================================================================
    // 🏁 ОБРАБОТКА ЕДИНОГО РЕЗУЛЬТАТА НА ВЫХОДЕ
    // ============================================================================
    match pipeline_result {
        Ok((granule, meta)) => {
            println!("\n🎉 ГИС-КОНВЕЙЕР ВЫПОЛНЕН УСПЕШНО:");
            println!("==================================================");
            println!("📦 Гранула: {}", granule.title);
            println!("📐 Гео-матрица: {} x {} пикселей", meta.width, meta.height);
            println!(
                "🧱 Внутренний тайл COG: {} x {}",
                meta.tile_width, meta.tile_height
            );
            println!("\n🗺️ Список всех доступных спектральных COG-ссылок для этого снимка:");
            for (band_name, href) in granule.bands.iter() {
                println!("   🔹 Канал [{}]: {}", band_name, href);
            }
            println!("==================================================");
            println!("🚀 Мы лениво распарсили параметры без выкачивания гигабайтов в RAM!");
        }
        Err(err) => {
            println!("\n❌ Конвейер упал на одном из шагов: {}", err);
        }
    }

    Ok(())
}
