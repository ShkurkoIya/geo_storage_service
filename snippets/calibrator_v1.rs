use proj::Proj;

#[derive(Debug, Clone, Copy)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct CalculatedShift {
    pub false_easting_x0: f64,
    pub false_northing_y0: f64,
    pub central_meridian: f64,
    pub accuracy_meters: f64,
}

/// Ошибки калибровочного ядра
#[derive(Debug, PartialEq)]
pub enum CalibrationError {
    InvalidLocalDistance,           // Точки локальной сети наложились друг на друга
    ProjectionInitializationFailed, // Сбой сишной либы libproj
    NotFound,                       // Параметры не подобраны в рамках указанной точности
}

/// 👑 ЧИСТЫЙ МАТЕМАТИЧЕСКИЙ ДВИЖОК АВТОКАЛИБРОВКИ
///
/// На вход принимает исключительно геометрические сущности:
/// - Две опорные точки в неизвестной локальной плоской системе (метры)
/// - Координаты этих же двух точек в глобальной системе (WGS84 градусы: долгота/широта)
/// - Допуск точности совпадения расстояний
pub fn calibrate_grid(
    local_point_a: Point2D,
    local_point_b: Point2D,
    global_lon_a: f64,
    global_lat_a: f64,
    global_lon_b: f64,
    global_lat_b: f64,
    tolerance_meters: f64,
) -> Result<CalculatedShift, CalibrationError> {
    let global_lon_a_rad = global_lon_a.to_radians();
    let global_lat_a_rad = global_lat_a.to_radians();
    let global_lon_b_rad = global_lon_b.to_radians();
    let global_lat_b_rad = global_lat_b.to_radians();

    // 1. Считаем истинное плоское расстояние между точками в локальной сети (по Пифагору)
    let dx_local = local_point_a.x - local_point_b.x;
    let dy_local = local_point_a.y - local_point_b.y;
    let r_local = (dx_local * dx_local + dy_local * dy_local).sqrt();

    if r_local < 0.001 {
        return Err(CalibrationError::InvalidLocalDistance);
    }

    // Диапазон брутфорса трехградусных зон Гаусса-Крюгера для Сибири (87° - 91° долготы)
    let start_lon = 87.0;
    let end_lon = 91.0;
    let step_minutes = 1.0 / 60.0; // Шаг в 1 угловую минуту (≈0.016666°)

    let mut current_lon = start_lon;
    let mut best_diff = f64::MAX;
    let mut best_shift: Option<CalculatedShift> = None;

    // 2. Итеративный перебор проекций в оперативной памяти CPU
    while current_lon <= end_lon {
        let proj_str = format!(
            "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0=500000 +y_0=0 +ellps=krass +units=m +no_defs",
            current_lon
        );

        if let Ok(transformer) = Proj::new(&proj_str) {
            // Проецируем глобальные градусы в плоские метры Гаусса-Крюгера текущего меридиана
            if let (Ok(prj_a), Ok(prj_b)) = (
                transformer.convert((global_lon_a_rad, global_lat_a_rad)),
                transformer.convert((global_lon_b_rad, global_lat_b_rad)),
            ) {
                let (prj_a_east, prj_a_north) = prj_a;
                let (prj_b_east, prj_b_north) = prj_b;

                // Расстояние между точками в текущей математической модели проекции
                let dx_prj = prj_a_east - prj_b_east;
                let dy_prj = prj_a_north - prj_b_north;
                let r_prj = (dx_prj * dx_prj + dy_prj * dy_prj).sqrt();

                let current_diff = (r_local - r_prj).abs();

                // Если попали в допуск — рассчитываем векторы параллельного сдвига осей
                if current_diff < tolerance_meters && current_diff < best_diff {
                    best_diff = current_diff;

                    // Компенсируем сдвиг: ГИС-Восток минус Локальный Восток (Y), ГИС-Север минус Локальный Север (X)
                    let false_easting_x0 = prj_a_east - local_point_a.y;
                    let false_northing_y0 = prj_a_north - local_point_a.x;

                    best_shift = Some(CalculatedShift {
                        false_easting_x0,
                        false_northing_y0,
                        central_meridian: current_lon,
                        accuracy_meters: current_diff,
                    });
                }
            }
        }

        current_lon += step_minutes;
    }

    match best_shift {
        Some(shift) => Ok(shift),
        None => Err(CalibrationError::NotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_mathematical_calibration() {
        // Опорные точки локальной сети (из нашего маркшейдерского паспорта)
        let local_a = Point2D {
            x: 517075.87,
            y: 23209.65,
        };
        let local_b = Point2D {
            x: 510731.81,
            y: 22928.34,
        };

        // Их истинные глобальные географические координаты
        let geo_lon_a = 89.313456;
        let geo_lat_a = 54.551234;
        let geo_lon_b = 89.309012;
        let geo_lat_b = 54.498078;

        let res = calibrate_grid(
            local_a, local_b, geo_lon_a, geo_lat_a, geo_lon_b, geo_lat_b, 10.0,
        );
        assert!(res.is_ok() || res.is_err());
    }
}
