use chrono::{Datelike, Utc};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, MissedTickBehavior};

// Импортируем типы из твоего geo_core ядра
use geo_core::nasa::cmr::{CmrQuery, CollectionSelector};
use geo_types::{coord, Rect};

use crate::{
    models::{BandUrls, NewGranule},
    repository::NasaRepository,
    CrawlerSignal, ModuleState,
};

/// Бесконечный фоновый цикл мониторинга NASA CMR API
pub async fn run_background_loop(state: Arc<ModuleState>, mut rx: mpsc::Receiver<CrawlerSignal>) {
    let mut timer = interval(Duration::from_secs(6 * 3600)); // Опрос раз в 6 часов
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    println!(
        "🤖 Системный кроулер NASA CMR запущен. Ожидание планового таймера или внешнего пинка..."
    );

    loop {
        tokio::select! {
            _ = timer.tick() => {
                println!("⏰ Сработал таймер: запуск планового сканирования Якутии...");
                let _ = execute_crawler_sync(&state).await;
            }
            Some(signal) = rx.recv() => {
                match signal {
                    CrawlerSignal::TriggerNow => {
                        println!("⚡ Получен внешний сигнал /trigger: форсирую внеочередное сканирование...");
                        let _ = execute_crawler_sync(&state).await;
                    }
                }
            }
        }
    }
}

/// Высокоуровневая оркестрация конвейера сбора метаданных
async fn execute_crawler_sync(
    state: &Arc<ModuleState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state.crawler_is_running.store(true, Ordering::Relaxed);
    let repo = NasaRepository::new(&state.db_pool);

    // 1. ДИНАМИЧЕСКОЕ ОПРЕДЕЛЕНИЕ ПРОСТРАНСТВЕННО-ВРЕМЕННЫХ ГРАНИЦ (ROI)
    // Задаем полигон Якутии (примерный Bounding Box основных алмазоносных и водородных узлов)
    let target_region = Rect::new(
        coord!(x: 110.0, y: 55.0), // min_lon, min_lat
        coord!(x: 140.0, y: 73.0), // max_lon, max_lat
    );

    // Считаем временной интервал: проверяем последние 30 дней, чтобы докачивать свежие сцены
    let now = Utc::now();
    let past = now - chrono::Duration::days(30);
    let temporal_range = format!(
        "{}Z,{}Z",
        past.format("%Y-%m-%dT%H:%M:%S"),
        now.format("%Y-%m-%dT%H:%M:%S")
    );

    println!("🗺️ Гео-зона сканирования: Якутия (BBox: 110-140E, 55-73N)");
    println!("⏳ Временное окно поиска: {}", temporal_range);

    // 2. СБОРКА СИСТЕМНОГО ЗАПРОСА ЧЕРЕЗ БИЛДЕР КЛИЕНТА
    let query = CmrQuery::new(
        CollectionSelector::ShortName("HLSL30".to_string()), // Твой целевой COG-продукт
        target_region,
        &temporal_range,
    )
    .with_cloud_cover(30) // МГРИ-фильтр: максимум 30% облачности на этапе запроса к NASA API
    .with_page_size(100);

    // 3. АСИНХРОННЫЙ ВЫЗОВ ЯДРА
    println!("🔍 Запрос к API каталога NASA Earthdata через cmr().fetch_granules()...");
    let collection = match state.nasa_provider.cmr().fetch_granules(query).await {
        Ok(coll) => coll,
        Err(e) => {
            eprintln!("❌ Сбой запроса к NASA CMR: {:?}", e);
            state.crawler_is_running.store(false, Ordering::Relaxed);
            return Err(e.into());
        }
    };

    println!("📊 От NASA получено сырых гранул: {}", collection.len());

    // 4. СПЕКТРАЛЬНАЯ ФИЛЬТРАЦИЯ И СТРИМИНГ В БД
    // Для водородных колец и термальных аномалий нам критически важен полный набор каналов
    let required_bands = ["B05", "B06", "B07", "B10", "Fmask"];
    let filtered_collection = collection.require_bands(&required_bands);

    println!(
        "🎯 Прошли жесткий спектральный фильтр (B5, B6, B7, B10, Fmask): {}",
        filtered_collection.len()
    );

    for gis_granule in filtered_collection.granules.iter() {
        // Дополнительный фильтр по сезону (май - сентябрь)
        let month = gis_granule.acquisition_date.month();
        if !(5..=9).contains(&month) {
            continue; // Пропускаем зимние снимки, покрытые снегом
        }

        // Собираем мапу ссылок в нашу жестко типизированную структуру `BandUrls`
        let bands = BandUrls {
            b5: gis_granule.bands.get("B05").cloned().unwrap_or_default(),
            b6: gis_granule.bands.get("B06").cloned().unwrap_or_default(),
            b7: gis_granule.bands.get("B07").cloned().unwrap_or_default(),
            b10: gis_granule.bands.get("B10").cloned().unwrap_or_default(),
            qa: gis_granule.bands.get("Fmask").cloned().unwrap_or_default(),
        };

        // Извлекаем геометрию полигона из гранулы твоего клиента
        // (Обычно это набор точек, формирующих footprint)
        let coords: Vec<(f64, f64)> = gis_granule.geometry.iter().map(|c| (c.x, c.y)).collect();

        // Собираем объект для вставки в СУБД
        match NewGranule::from_cmr_response(
            gis_granule.title.clone(), // Внутренний ID сцены
            gis_granule.cloud_cover as f32,
            &gis_granule.acquisition_date.to_rfc3339(),
            &coords,
            bands,
        ) {
            Ok(granule) => {
                // Атомарно сохраняем метаданные в PostGIS и ставим таску загрузчику в `download_tasks`
                match repo.insert_granule_and_create_task(granule).await {
                    Ok(task_uuid) => {
                        println!(
                            "✅ Сцена {} поставлена в очередь на скачивание. UUID: {}",
                            gis_granule.title, task_uuid
                        );
                        // Инкрементируем счетчик живой сессии для Axum /status
                        state
                            .crawler_processed_scenes
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        eprintln!("❌ Ошибка записи гранулы в PostGIS: {:?}", e);
                    }
                }
            }
            Err(err) => {
                eprintln!(
                    "⚠️ Сбой конвертации геометрии для гранулы {}: {}",
                    gis_granule.title, err
                );
            }
        }
    }

    state.crawler_is_running.store(false, Ordering::Relaxed);
    println!("💤 Цикл завершен. Кроулер перешел в режим ожидания.");
    Ok(())
}
