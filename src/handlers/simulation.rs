use crate::storage::repository::GeoRepository;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrillholeProfile {
    pub line_code: String,
    pub northing_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticVoxel {
    pub id: Uuid,
    pub body_id: Uuid,
    pub voxel_east: f64,
    pub voxel_north: f64,
    pub voxel_elevation: f64,
    pub voxel_size_m: f64,
    pub grade_au: f64,
    pub gkz_category: String, // 'C1', 'C2', 'Barren'
}

pub struct SimulationHandler;

impl SimulationHandler {
    pub async fn generate_advanced_simulation(
        repo: &GeoRepository<'_>,
        lens_name: &str,
        c_east: f64,  // Прилетает Долгота X (напр. 89.15) [context]
        c_north: f64, // Прилетает Широта Y (напр. 54.55) [context]
    ) -> Result<Uuid, Box<dyn std::error::Error>> {
        info!(
            "🏭 Симулятор: Запуск тяжелого генератора 3D-недр для '{}'...",
            lens_name
        );

        let body_id = Uuid::new_v4();
        let length_m = 600.0; // Длина линзы по оси X (метров)
        let width_m = 300.0; // Ширина линзы по оси Y (метров)

        // Переводим метры в градусы географии WGS84 (приблизительно 111 000 метров в 1 градусе)
        let r_east = length_m / 2.0 / 111000.0; // Радиус по долготе (X)
        let r_north = width_m / 2.0 / 111000.0; // Радиус по широте (Y)
        let h_center = 200.0; // Глубина залегания рудного тела (Z)

        // 🚀 ГЕОДЕЗИЧЕСКИЙ ЗАМКНУТЫЙ КОНТУР: Первой идёт Долгота X (c_east), Второй — Широта Y (c_north) [context]
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

        // 1. Заливаем материнское рудное тело в PostGIS
        repo.insert_synthetic_ore_body(
            body_id, lens_name, c_east, c_north, h_center, length_m, width_m, 0.0, &wkt_poly,
        )
        .await?;

        // 2. СЕТЬ СКВАЖИН ПО ГЕОЛОГИЧЕСКИМ ПРОФИЛЯМ (Правдоподобная сетка Михалыча)
        // Нарезаем 3 параллельные разведочные линии (профили) по широте Y (North) [context]
        let profiles = vec![
            c_north - (r_north * 0.5),
            c_north,
            c_north + (r_north * 0.5),
        ];
        let mut drillhole_positions = Vec::new();

        let mut dh_idx = 1;
        for (p_idx, &profile_y) in profiles.iter().enumerate() {
            // Вдоль каждого профиля бурим 4 скважины по долготе X (East) [context]
            for x_step in 1..=4 {
                let fraction = (x_step as f64) / 5.0;
                let dh_east = c_east - r_east + (r_east * 2.0 * fraction);
                let dh_north = profile_y;

                let dh_id = Uuid::new_v4();
                let code = format!("DH-PR-{}-№{}", p_idx + 1, dh_idx);
                let max_depth = 150.0; // Скважина уходит вглубь на 150м [context]

                // Аналитическая проверка: пересекает ли ствол скважины эллипс нашей линзы в плане?
                let dx = (dh_east - c_east) / r_east;
                let dy = (dh_north - c_north) / r_north;
                let is_inside_lens = (dx * dx + dy * dy) <= 1.0;

                // Вызываем инсерт: первой уходит Долгота (dh_east), второй — Широта (dh_north), высота устья 250м [context]
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
                    // Запоминаем рудные точки для расчёта сети ГКЗ категорий
                    drillhole_positions.push((dh_east, dh_north));
                }
                dh_idx += 1;
            }
        }

        // 3. АВТОМАТИЧЕСКАЯ БЛОЧНАЯ МОДЕЛЬ (3D Дискретизация регулярных вокселей)
        info!("📦 Симулятор: Дискретизация пространства линзы на 3D воксели ГКЗ...");
        let voxel_size = 0.0001; // Шаг сетки (приблизительно 11 метров ребро кубика)

        // Сканируем пространство прямоугольного отвода в правильных осях
        let mut e_steps = c_east - r_east;
        while e_steps <= c_east + r_east {
            let mut n_steps = c_north - r_north;
            while n_steps <= c_north + r_north {
                let dx = (e_steps - c_east) / r_east;
                let dy = (n_steps - c_north) / r_north;

                // Если мы попали внутрь контура линзы — оцениваем блок
                if (dx * dx + dy * dy) <= 1.0 {
                    // Считаем евклидово расстояние до ближайшей рудной выработки в метрах
                    let mut min_dist = f64::MAX;
                    for &(dh_e, dh_n) in &drillhole_positions {
                        let dist =
                            ((e_steps - dh_e).powi(2) + (n_steps - dh_n).powi(2)).sqrt() * 111000.0;
                        if dist < min_dist {
                            min_dist = dist;
                        }
                    }

                    // Жёсткий стандарт ГКЗ классификации запасов по плотности сети:
                    // До скважины <= 40 метров — Категория С1 (балансовая промышленная руда) [context]
                    // От 40 до 90 метров — Категория С2 (оцененные запасы) [context]
                    // Всё что дальше — Брак / Требует эксплуатационной доразведки! [context]
                    let (category, grade) = if min_dist <= 40.0 {
                        ("C1", 4.5 + (dx * 1.2)) // Содержание Au г/т затухает к флангам
                    } else if min_dist <= 90.0 {
                        ("C2", 1.8)
                    } else {
                        ("Брак", 0.0)
                    };

                    let voxel = SyntheticVoxel {
                        id: Uuid::new_v4(),
                        body_id,
                        voxel_east: e_steps,  // Долгота X (89.x) [context]
                        voxel_north: n_steps, // Широта Y (54.x) [context]
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

        info!("🎰 Симулятор: 3D Блочная модель успешно построена. Инфраструктура готова к запуску Оптимизатора!");
        Ok(body_id)
    }

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
        let width = 150.0; // Мощность линзы

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

        // 1. Заливаем линзу в схему sandbox_simulation
        repo.insert_synthetic_ore_body(
            body_id, lens_name, c_east, c_north, 250.0, length, width, 45.0, &wkt_poly,
        )
        .await?;

        // 2. СЕТЬ МИХАЛЫЧА: Симулируем 10 исторических хаотичных выработок
        for i in 1..=10 {
            let dh_id = Uuid::new_v4();
            let code = format!("DH-HIST-{}", i);

            let shift_e = (i as f64 * 0.013) % r_east;
            let shift_n = (i as f64 * 0.007) % r_north;
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };

            let dh_east = c_east + (shift_e * sign);
            let dh_north = c_north + (shift_n * sign);
            let depth = 120.0 + (i as f64 * 5.0);
            let is_barren = i % 4 == 0;

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
}
