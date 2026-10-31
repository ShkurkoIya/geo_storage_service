use crate::models::SyntheticVoxel;
use crate::repository::GeoRepository;
use crate::ModuleState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::{info, instrument};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct SimulationPayload {
    pub name: String,
    pub center_longitude: f64, // Прилетает Долгота X
    pub center_latitude: f64,  // Прилетает Широта Y
}

/// **Высокопроизводительный процедурный генератор 3D-модели недр рудного тела.**
///
/// ### Назначение:
/// Функция осуществляет автоматизированное математическое моделирование скрытых под землей рудных тел,
/// профильных сетей разведочных скважин и формирует блочную модель вокселей на честной геологической глубине.
///
/// ### Геодезическая логика и тригонометрия:
/// 1. Принимает референтные координаты проектного центра рудного поля в глобальной системе WGS84 (`c_east`, `c_north`).
/// 2. Осуществляет приближенный перевод линейных метров в географические градусы на основе базовой сетки меркатора:
///    * Шаг по долготе (ось X / Easting): 1 градус ~ `111 000` метров.
///    * Шаг по широте (ось Y / Northing): 1 градус ~ `111 000` метров (для прецизионных расчетов средних широт РФ).
/// 3. Формирует валидный 3D замкнутый GIS-контур (`POLYGON Z`) в текстовом формате OGC WKT, жестко соблюдая правило правой руки
///    и гарантируя замыкание геометрии (первая и последняя точки полигона идентичны).
///
/// ### Алгоритм классификации запасов ГКЗ РФ:
/// Функция симулирует разведочную сеть по 3-м параллельным профилям (линиям Михалыча). Для каждого дискретного 3D-кубика (вокселя)
/// вычисляется евклидово расстояние в метрах до ближайшего ствола скважины, попавшей в контур промышленного оруденения:
/// * **Категория C1 (Промышленные запасы):** Удаление от скважины `≤ 40 метров`. Сетка ГРР достаточна для ТЕО.
///   Содержание золота (`grade_au`) моделируется функцией затухания от эпицентра к флангам.
/// * **Категория C2 (Оцененные запасы):** Удаление от `40` до `90 метров`. Высокая степень неопределенности флангов.
/// * **Брак (Внебалансовые зоны):** Удаление `> 90 метров`. Зоны геометрического шума, требующие точечного удара ИИ-оптимизатора.
///
/// ### Параметры:
/// * `repo` - Ссылка на пространственный ГИС-репозиторий базы данных PostGIS.
/// * `lens_name` - Уникальное строковое имя генерируемой жилы (например, "Жила Параллельная").
/// * `c_east` - Долгота центральной точки рудного отвода (X в WGS84, например, 89.1500).
/// * `c_north` - Широта центральной точки рудного отвода (Y в WGS84, например, 54.5500).
///
/// ### Возвращаемое значение:
/// Возвращает атомарный идентификатор `Result<Uuid, Box<dyn std::error::Error>>` созданного рудного тела для сквозной интеграции в QGIS.
/// Входной JSON-пакет данных (DTO), прилетающий от клиента/QGIS
/// `#[axum::debug_handler]` гарантирует, что если типы стейта или ответов поменяются,
/// компилятор выдаст точечную человеческую подсказку прямо в консоль.
#[axum::debug_handler]
pub async fn handle_generate_simulation(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<SimulationPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Инициализируем твой маркшейдерский репозиторий, используя пул соединений из стейта
    let repo = GeoRepository::new(&state.db_pool);

    info!("🛰️ API Gateway: Получен запрос на генерацию недр. Передача в расчетное ядро...");

    // Перенаправляем чистые типы в сервисный слой бизнес-математики
    match generate_advanced_simulation(
        &repo,
        &payload.name,
        payload.center_longitude,
        payload.center_latitude,
    )
    .await
    {
        Ok(body_id) => Ok((
            StatusCode::CREATED,
            Json(json!({
                "status": "success",
                "ore_body_id": body_id,
                "message": "3D блочная модель недр успешно выдута в sandbox_simulation"
            })),
        )),
        Err(err) => {
            tracing::error!("💥 Критический крах симулятора недр: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Внутренний сбой расчетного ядра: {}", err),
            ))
        }
    }
}

/// **Высокопроизводительный процедурный генератор 3D-модели недр**
///
/// Производит дискретизацию рудного поля, строит сеть скважин и считает категории ГКЗ.
/// Полностью изолирован от HTTP-семантики Axum.
#[instrument(skip(repo, lens_name), fields(lens = %lens_name))]
pub async fn generate_advanced_simulation(
    repo: &GeoRepository<'_>,
    lens_name: &str,
    c_east: f64,  // Долгота X
    c_north: f64, // Широта Y
) -> Result<Uuid, Box<dyn std::error::Error>> {
    info!(
        "🏭 Симулятор: Запуск тяжелого генератора 3D-недр для '{}'...",
        lens_name
    );

    let body_id = Uuid::new_v4();
    let length_m = 600.0;
    let width_m = 300.0;

    // Переводим метры в градусы географии WGS84
    let r_east = length_m / 2.0 / 111000.0;
    let r_north = width_m / 2.0 / 111000.0;
    let h_center = 200.0;

    // 🚀 ГЕОДЕЗИЧЕСКИЙ ЗАМКНУТЫЙ КОНТУР
    let wkt_poly = format!(
        "POLYGON Z (({} {} {}, {} {} {}, {} {} {}, {} {} {}, {} {} {}))",
        c_east - r_east,
        c_north - r_north,
        h_center,
        c_east + r_east,
        c_north - r_north,
        h_center,
        c_east + r_east,
        c_north + r_north,
        h_center,
        c_east - r_east,
        c_north + r_north,
        h_center,
        c_east - r_east,
        c_north - r_north,
        h_center
    );

    // 1. Заливаем материнское рудное тело в схему sandbox_simulation PostGIS
    repo.insert_synthetic_ore_body(
        body_id, lens_name, c_east, c_north, h_center, length_m, width_m, 0.0, &wkt_poly,
    )
    .await?;

    // 2. СЕТЬ СКВАЖИН ПО ГЕОЛОГИЧЕСКИМ ПРОФИЛЯМ
    let profiles = vec![
        c_north - (r_north * 0.5),
        c_north,
        c_north + (r_north * 0.5),
    ];
    let mut drillhole_positions = Vec::new();

    let mut dh_idx = 1;
    for (p_idx, &profile_y) in profiles.iter().enumerate() {
        for x_step in 1..=4 {
            let fraction = (x_step as f64) / 5.0;
            let dh_east = c_east - r_east + (r_east * 2.0 * fraction);
            let dh_north = profile_y;

            let dh_id = Uuid::new_v4();
            let code = format!("DH-PR-{}-№{}", p_idx + 1, dh_idx);
            let max_depth = 150.0;

            let dx = (dh_east - c_east) / r_east;
            let dy = (dh_north - c_north) / r_north;
            let is_inside_lens = (dx * dx + dy * dy) <= 1.0;

            repo.insert_synthetic_drillhole(
                dh_id,
                body_id,
                &code,
                dh_east,
                dh_north,
                250.0,
                max_depth,
                !is_inside_lens,
            )
            .await?;

            if is_inside_lens {
                drillhole_positions.push((dh_east, dh_north));
            }
            dh_idx += 1;
        }
    }

    // 3. АВТОМАТИЧЕСКАЯ БЛОЧНАЯ МОДЕЛЬ (3D Дискретизация)
    info!("📦 Симулятор: Дискретизация пространства линзы на 3D воксели ГКЗ...");
    let voxel_size = 0.0001;

    let mut e_steps = c_east - r_east;
    while e_steps <= c_east + r_east {
        let mut n_steps = c_north - r_north;
        while n_steps <= c_north + r_north {
            let dx = (e_steps - c_east) / r_east;
            let dy = (n_steps - c_north) / r_north;

            if (dx * dx + dy * dy) <= 1.0 {
                let mut min_dist = f64::MAX;
                for &(dh_e, dh_n) in &drillhole_positions {
                    let dist =
                        ((e_steps - dh_e).powi(2) + (n_steps - dh_n).powi(2)).sqrt() * 111000.0;
                    if dist < min_dist {
                        min_dist = dist;
                    }
                }

                let (category, grade) = if min_dist <= 40.0 {
                    ("C1", 4.5 + (dx * 1.2))
                } else if min_dist <= 90.0 {
                    ("C2", 1.8)
                } else {
                    ("Брак", 0.0)
                };

                let voxel = SyntheticVoxel {
                    id: Uuid::new_v4(),
                    body_id,
                    voxel_east: e_steps,
                    voxel_north: n_steps,
                    voxel_elevation: h_center,
                    voxel_size_m: 10.0,
                    grade_au: grade,
                    gkz_category: category.to_string(),
                };

                repo.insert_synthetic_voxel(&voxel).await?;
            }
            n_steps += voxel_size;
        }
        e_steps += voxel_size;
    }

    info!("🎰 Симулятор: 3D Блочная модель успешно построена.");
    Ok(body_id)
}

/// **Генератор линз симуляционной песочницы и исторических выработок.**
///
/// ### Назначение:
/// Функция предназначена для быстрой инициализации тестовых полигонов, имитирующих архивные
/// геологоразведочные данные ("Сеть Михалыча"), накопленные предприятием до внедрения ИИ.
///
/// ### Алгоритм работы:
/// 1. Процедурно строит плоское рудное тело меньшего масштаба (400м на 150м) со смещением по глубине `Z = 250м`.
/// 2. Имитирует хаотичную разведку: через детерминированный математический сдвиг на базе оператора остатка от деления (`%`)
///    разбрасывает 10 исторических стволов скважин.
/// 3. Каждая 4-я скважина искусственно маркируется флагом `is_barren = true` (пустая выработка без рудного пересечения)
///    для проверки качества фильтрации геометрического шума алгоритмом 3D DBSCAN.
///
/// ### Параметры:
/// * `repo` - Ссылка на пространственный ГИС-репозиторий базы данных PostGIS.
/// * `lens_name` - Строковое имя архивного рудного участка.
/// * `c_east` - Центральная долгота (X в WGS84).
/// * `c_north` - Центральная широта (Y в WGS84).
///
/// ### Возвращаемое значение:
/// Идентификатор `Result<Uuid, Box<dyn std::error::Error>>` созданного архивного блока.
#[instrument(skip(repo, lens_name), fields(lens = %lens_name))]
pub async fn generate_lens_sandbox(
    repo: &GeoRepository<'_>,
    lens_name: &str,
    c_east: f64,
    c_north: f64,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    info!(
        "🧬 Симулятор: Запуск генерации синтетического рудного тела '{}'...",
        lens_name
    );

    let body_id = Uuid::new_v4();
    let length = 400.0; // Длина линзы в метрах
    let width = 150.0; // Мощность (ширина) линзы
                       // Считаем плоский WKT-полигон отвода вокруг центра
    let r_east = length / 2.0 / 111000.0;
    let r_north = width / 2.0 / 111000.0;
    let h = 250.0;
    let wkt_poly = format!(
        "POLYGON Z (({} {} {}, {} {} {}, {} {} {}, {} {} {}, {} {} {}))",
        c_east - r_east,
        c_north - r_north,
        h,
        c_east + r_east,
        c_north - r_north,
        h,
        c_east + r_east,
        c_north + r_north,
        h,
        c_east - r_east,
        c_north + r_north,
        h,
        c_east - r_east,
        c_north - r_north,
        h
    );
    // 1. Заливаем линзу в схему sandbox_simulation базы данных PostGIS
    repo.insert_synthetic_ore_body(
        body_id, lens_name, c_east, c_north, 250.0, length, width, 45.0, &wkt_poly,
    )
    .await?;

    // 2. СЕТЬ МИХАЛЫЧА: Симулируем 10 исторических хаотичных выработок
    for i in 1..=10 {
        let dh_id = Uuid::new_v4();
        let code = format!("DH-HIST-{}", i);
        // Псевдохаотичный сдвиг координат на основе индекса итерации
        let shift_e = (i as f64 * 0.013) % r_east;
        let shift_n = (i as f64 * 0.007) % r_north;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let dh_east = c_east + (shift_e * sign);
        let dh_north = c_north + (shift_n * sign);
        let depth = 120.0 + (i as f64 * 5.0);
        let is_barren = i % 4 == 0; // Каждая четвертая скважина — пустая в плане золота
        repo.insert_synthetic_drillhole(
            dh_id, body_id, &code, dh_east, dh_north, 245.0, depth, is_barren,
        )
        .await?;
    }
    info!(
        "✅ Симулятор: Рудное тело '{}' успешно материализовано! Набурено 10 скважин.",
        lens_name
    );
    Ok(body_id)
}
