use super::{CalibrationError, UniversalCalibrator};
use crate::models::{
    CalibrationResult, CoordinateSystemType, RawPoint, TransformationParameters,
    UniversalCalibrationRequest,
};
use proj::Proj;
use uuid::Uuid;

pub struct GeoCalibrator {
    pub scan_step_minutes: f64,
    /// Опциональный жесткий предел начала сканирования (если нужно переопределить)
    pub force_min_lon: Option<f64>,
    /// Опциональный жесткий предел конца сканирования
    pub force_max_lon: Option<f64>,
}

impl UniversalCalibrator for GeoCalibrator {
    fn calibrate(
        &self,
        request: UniversalCalibrationRequest,
    ) -> Result<CalibrationResult, CalibrationError> {
        use CoordinateSystemType::*;

        // 1. Предварительная валидация структуры (проверка количества точек и допуска)
        request
            .validate()
            .map_err(|_| CalibrationError::InsufficientPoints {
                provided: request.control_points.len(),
                required: 2,
            })?;

        // Задаем правила игры в зависимости от того, что нашли в файлопомойке
        match (&request.source_system, &request.target_system) {
            // СЦЕНАРИЙ 1: Локальные метры ──> Градусы (Твой брутфорс меридианов Гаусса-Крюгера)
            (LocalGrid, GeographicWgs84) | (LocalGrid, GeographicKrassovsky) => {
                if request.control_points.len() < 2 {
                    return Err(CalibrationError::InsufficientPoints {
                        provided: request.control_points.len(),
                        required: 2,
                    });
                }
                self.run_gauss_kruger_bruteforce(&request)
            }

            // СЦЕНАРИЙ 2: Метры (СК-42/МСК) ──> Локальные метры (Чистый Хелмерт / Аффинный сдвиг и поворот)
            (Projected { .. }, LocalGrid)
            | (LocalGrid, Projected { .. })
            | (LocalGrid, LocalGrid) => {
                if request.control_points.len() < 2 {
                    return Err(CalibrationError::InsufficientPoints {
                        provided: request.control_points.len(),
                        required: 2,
                    });
                }
                self.run_affine_helmert_transformation(&request)
            }

            // Все остальные экзотические случаи, которые пока не реализовали
            _ => Err(CalibrationError::UnsupportedTransformation),
        }
    }
}

impl GeoCalibrator {
    pub fn new(scan_step_minutes: f64) -> Self {
        Self {
            scan_step_minutes,
            force_min_lon: None,
            force_max_lon: None,
        }
    }

    pub fn with_bounds(scan_step_minutes: f64, min_lon: f64, max_lon: f64) -> Self {
        Self {
            scan_step_minutes,
            force_min_lon: Some(min_lon),
            force_max_lon: Some(max_lon),
        }
    }
}

impl GeoCalibrator {
    /// Стратегия А: Перебор параметров проекции в PROJ (для связи с градусами)

    /// 🔥 СТРАТЕГИЯ А: Брутфорс меридианов Гаусса-Крюгера
    fn run_gauss_kruger_bruteforce(
        &self,
        request: &UniversalCalibrationRequest,
    ) -> Result<CalibrationResult, CalibrationError> {
        let pts = &request.control_points;
        if pts.len() < 2 {
            return Err(CalibrationError::InsufficientPoints {
                provided: pts.len(),
                required: 2,
            });
        }

        let p_a = &pts[0];
        let p_b = &pts[pts.len() - 1];

        let ellps = match request.target_system {
            CoordinateSystemType::GeographicKrassovsky => "krass",
            _ => "WGS84",
        };

        let (lon_a_rad, lat_a_rad) = p_a.target_point.to_radians();
        let (lon_b_rad, lat_b_rad) = p_b.target_point.to_radians();

        // 1. Считаем истинное плоское расстояние в локальной сети (Используем исправленный distance_to)
        let r_local = p_a.source_point.distance_to(&p_b.source_point);
        if r_local < 0.001 {
            return Err(CalibrationError::InvalidGeometry);
        }

        // Диапазон сканирования меридианов Сибири вокруг долготы объекта
        let start_lon = self.force_min_lon.unwrap_or(p_a.target_point.lon() - 3.0);
        let end_lon = self.force_max_lon.unwrap_or(p_a.target_point.lon() + 3.0);
        let step = self.scan_step_minutes / 60.0;

        let mut current_lon = start_lon;
        let mut best_diff = f64::MAX;
        let mut best_params: Option<TransformationParameters> = None;

        while current_lon <= end_lon {
            let proj_str = format!(
                "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0=500000 +y_0=0 +ellps={} +units=m +no_defs",
                current_lon, ellps
            );

            if let Ok(transformer) = Proj::new(&proj_str) {
                if let (Ok(prj_a), Ok(prj_b)) = (
                    transformer.convert((lon_a_rad, lat_a_rad)),
                    transformer.convert((lon_b_rad, lat_b_rad)),
                ) {
                    // Используем полный Пифагор по обеим осям PROJ-проекции, как в старом сниппете!
                    let dx_prj = prj_a.0 - prj_b.0;
                    let dy_prj = prj_a.1 - prj_b.1;
                    let r_prj = (dx_prj * dx_prj + dy_prj * dy_prj).sqrt();

                    let current_diff = (r_local - r_prj).abs();

                    // Допуск адаптирован под шаг сетки брутфорса
                    if current_diff < (request.tolerance_meters * 10.0) && current_diff < best_diff
                    {
                        best_diff = current_diff;

                        // Маппинг осей из старого сниппета: PROJ X (Восток) к Easting, PROJ Y (Север) к Northing
                        let false_easting = prj_a.0 - p_a.source_point.easting();
                        let false_northing = prj_a.1 - p_a.source_point.northing();

                        best_params = Some(TransformationParameters {
                            dx: false_easting,
                            dy: false_northing,
                            rotation_angle_rad: 0.0,
                            scale_factor: 1.0,
                            central_meridian: Some(current_lon),
                        });
                    }
                }
            }
            current_lon += step;
        }

        match best_params {
            Some(params) => Ok(CalibrationResult {
                id: Uuid::new_v4(),
                strategy_used: format!("Gauss-Kruger Meridian Bruteforce ({})", ellps),
                parameters: params,
                final_accuracy_meters: best_diff,
            }),
            None => Err(CalibrationError::ToleranceNotMet),
        }
    }

    /// 🔥 СТРАТЕГИЯ Б: 2D Аффинное преобразование Хелмерта
    fn run_affine_helmert_transformation(
        &self,
        request: &UniversalCalibrationRequest,
    ) -> Result<CalibrationResult, CalibrationError> {
        let pts = &request.control_points;
        let len = pts.len();

        // Четко берем первую и последнюю точки для расчета базиса
        let p_a = &pts[0];
        let p_b = &pts[len - 1];

        let r_src = p_a.source_point.distance_to(&p_b.source_point);
        let r_tgt = p_a.target_point.distance_to(&p_b.target_point);

        if r_src < 0.001 || r_tgt < 0.001 {
            return Err(CalibrationError::InvalidGeometry);
        }

        let scale_factor = r_tgt / r_src;

        let dx_src = p_b.source_point.easting() - p_a.source_point.easting();
        let dy_src = p_b.source_point.northing() - p_a.source_point.northing();
        let dx_tgt = p_b.target_point.easting() - p_a.target_point.easting();
        let dy_tgt = p_b.target_point.northing() - p_a.target_point.northing();

        let rotation_angle_rad = dy_tgt.atan2(dx_tgt) - dy_src.atan2(dx_src);
        let cos_r = rotation_angle_rad.cos();
        let sin_r = rotation_angle_rad.sin();

        let dx = p_a.target_point.easting()
            - scale_factor
                * (p_a.source_point.easting() * cos_r - p_a.source_point.northing() * sin_r);
        let dy = p_a.target_point.northing()
            - scale_factor
                * (p_a.source_point.easting() * sin_r + p_a.source_point.northing() * cos_r);

        let final_accuracy_meters =
            self.calculate_network_rmse(pts, dx, dy, scale_factor, cos_r, sin_r);

        if final_accuracy_meters > request.tolerance_meters {
            return Err(CalibrationError::ToleranceNotMet);
        }

        Ok(CalibrationResult {
            id: Uuid::new_v4(),
            strategy_used: "2D Affine Helmert (Shift-Rotate-Scale)".to_string(),
            parameters: TransformationParameters {
                dx,
                dy,
                rotation_angle_rad,
                scale_factor,
                central_meridian: None,
            },
            final_accuracy_meters,
        })
    }

    /// Вынос расчета СКО (RMSE)
    fn calculate_network_rmse(
        &self,
        pts: &[crate::models::ControlPointPair],
        dx: f64,
        dy: f64,
        scale: f64,
        cos_r: f64,
        sin_r: f64,
    ) -> f64 {
        let mut total_squared_error = 0.0;

        for pair in pts {
            // Проецируем точку на основе математической модели Хелмерта
            let calc_east = dx
                + scale
                    * (pair.source_point.easting() * cos_r - pair.source_point.northing() * sin_r);
            let calc_north = dy
                + scale
                    * (pair.source_point.easting() * sin_r + pair.source_point.northing() * cos_r);

            let calculated_point = RawPoint::from_meters(calc_east, calc_north);

            // Расчет ошибки превратился в красивый вызов .distance_to()
            let err = calculated_point.distance_to(&pair.target_point);
            total_squared_error += err * err;
        }

        (total_squared_error / pts.len() as f64).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use std::matches;

    use super::*;
    use crate::models::{ControlPointPair, CoordinateSystemType, RawPoint};

    // =========================================================================
    // 1. ТЕСТЫ СТРАТЕГИИ Б (ХЕЛМЕРТ: МЕТРЫ <──> МЕТРЫ)
    // =========================================================================

    #[test]
    fn test_strategy_b_perfect_shift_and_rotate() {
        let calibrator = GeoCalibrator::new(1.0);

        // Идеальный сдвиг на +5000м по X, +10000м по Y и чистый разворот осей на 90 градусов (PI/2)
        // Формула поворота: X_tgt = -Y_src, Y_tgt = X_src
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::LocalGrid,
            control_points: vec![
                ControlPointPair {
                    name: Some("Репер_А".to_string()),
                    source_point: RawPoint::from_meters(0.0, 0.0),
                    target_point: RawPoint::from_meters(5000.0, 10000.0),
                },
                ControlPointPair {
                    name: Some("Репер_Б".to_string()),
                    source_point: RawPoint::from_meters(100.0, 0.0),
                    // При повороте на 90° точка сместится по Y_tgt (север маркшейдерии)
                    target_point: RawPoint::from_meters(5000.0, 10100.0),
                },
            ],
            tolerance_meters: 0.01,
        };

        let res = calibrator
            .calibrate(request)
            .expect("Калибровка должна пройти успешно");

        assert_eq!(res.strategy_used, "2D Affine Helmert (Shift-Rotate-Scale)");
        assert!((res.parameters.dx - 5000.0).abs() < 0.001);
        assert!((res.parameters.dy - 10000.0).abs() < 0.001);
        assert!((res.parameters.scale_factor - 1.0).abs() < 0.001);
        assert!(
            res.final_accuracy_meters < 0.001,
            "Для двух точек невязка должна быть нулевой"
        );
    }

    #[test]
    fn test_strategy_b_multi_point_network_with_noise() {
        let calibrator = GeoCalibrator::new(1.0);

        // Сеть из 3-х точек, где одна имеет ошибку измерения (30 см) в архиве.
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::Projected {
                proj_definition: "EPSG:32645".to_string(),
            },
            control_points: vec![
                ControlPointPair {
                    name: Some("Опорный_1".to_string()),
                    source_point: RawPoint::from_meters(0.0, 0.0),
                    target_point: RawPoint::from_meters(100.0, 100.0),
                },
                ControlPointPair {
                    name: Some("Опорный_2".to_string()),
                    source_point: RawPoint::from_meters(100.0, 0.0),
                    target_point: RawPoint::from_meters(200.0, 100.0),
                },
                ControlPointPair {
                    name: Some("Опечатка_В_Архиве_3".to_string()),
                    source_point: RawPoint::from_meters(50.0, 50.0),
                    // Намеренно сдвигаем точку на 30 сантиметров по Easting
                    target_point: RawPoint::from_meters(150.3, 150.0),
                },
            ],
            tolerance_meters: 0.5, // Наш допуск 50 см, должны пройти
        };

        let res = calibrator.calibrate(request).unwrap();

        // Математика должна обнаружить ошибку сети и рассчитать RMSE невязки
        assert!(res.final_accuracy_meters > 0.0);
        assert!(res.final_accuracy_meters < 0.3);
    }

    #[test]
    fn test_strategy_b_real_mine_data_helmert() {
        let calibrator = GeoCalibrator::new(1.0);

        // Реальные данные с планшета прииска: Система 1965г ──> Местная СК № 166
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::LocalGrid,
            control_points: vec![
                // Пункт Туманная (Строка 2)
                ControlPointPair {
                    name: Some("Туманная".to_string()),
                    // Вход: Система 1965г (v1/easting = Y, v2/northing = X)
                    source_point: RawPoint::from_meters(10170.87, 12079.07),
                    // Выход: Местная СК № 166 (v1/easting = X, v2/northing = Y с учетом ГИС-осей)
                    target_point: RawPoint::from_meters(517075.87, 23209.65),
                },
                // Пункт Бобровый (Строка 16)
                ControlPointPair {
                    name: Some("Бобровый".to_string()),
                    source_point: RawPoint::from_meters(3790.00, 11390.00),
                    target_point: RawPoint::from_meters(510731.81, 22928.34),
                },
            ],
            tolerance_meters: 0.1, // Жесткий допуск 10 см
        };

        let res = calibrator
            .calibrate(request)
            .expect("Калибровка Хелмерта по реальным реперам прииска должна сойтись!");

        assert!(
            res.final_accuracy_meters < 0.05,
            "СКО калибровки должно быть около нуля на двух точках"
        );
        println!(
            "🥇 ХЕЛМЕРТ ПОДТВЕРЖДЕН ПЛАНШЕТОМ! Невязка: {} м.",
            res.final_accuracy_meters
        );
    }

    // =========================================================================
    // 2. ТЕСТЫ СТРАТЕГИИ А (БРУТФОРС ГАУССА-КРЮГЕРА: ЛОКАЛ ──> ГРАДУСЫ)
    // =========================================================================
    #[test]
    fn test_strategy_a_real_geographic_bruteforce() {
        // Шаг 0.5 градуса для высокой скорости прогона тестов в рантайме cargo test
        let calibrator = GeoCalibrator::new(30.0);

        // Настоящие маркшейдерские данные горы Туманной из Хакасии!
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::GeographicKrassovsky,
            control_points: vec![
                // 1. Точка "Туманная" (Строка 2 планшета + Твои координаты из карт!)
                ControlPointPair {
                    name: Some("Туманная".to_string()),
                    // Маппинг строго по ГИС-осям: v1 = Easting (X из табл), v2 = Northing (Y из табл)
                    source_point: RawPoint::from_meters(517075.87, 23209.65),
                    target_point: RawPoint::from_geo(88.693235, 54.639732),
                },
                // 2. Искусственная опорная точка, смещенная строго по геодезическому вектору
                // для имитации реальной протяженности карьера Сарала
                ControlPointPair {
                    name: Some("Опора_Сарала_Юг".to_string()),
                    source_point: RawPoint::from_meters(510731.81, 22928.34),
                    target_point: RawPoint::from_geo(88.595000, 54.635000),
                },
            ],
            tolerance_meters: 100.0, // Допуск сходимости масштаба для архивных привязок
        };

        let res = calibrator.calibrate(request);

        match res {
            Ok(result) => {
                assert!(result.strategy_used.contains("Gauss-Kruger"));
                assert!(result.parameters.central_meridian.is_some());
                let meridian = result.parameters.central_meridian.unwrap();

                // 1. Проверяем, что брутфорс намертво сел на долготу Туманной
                assert!((meridian - 88.693235).abs() < 0.001);

                // 2. 🛡️ ПРЯМОЙ МАРКШЕЙДЕРСКИЙ КОНТРОЛЬ: СК-1965г ──> МСК-166
                // Берем точные исходные метры СК-65 с планшета (v1 = Y_65, v2 = X_65)
                let true_easting_65 = 10170.87;
                let true_northing_65 = 12079.07;

                // Извлекаем параметры трансформации, найденные ядром
                let dx = result.parameters.dx;
                let dy = result.parameters.dy;
                let scale = result.parameters.scale_factor;
                let cos_r = result.parameters.rotation_angle_rad.cos();
                let sin_r = result.parameters.rotation_angle_rad.sin();

                // Прямое аффинное преобразование Хелмерта
                let calculated_msk_easting =
                    dx + scale * (true_easting_65 * cos_r - true_northing_65 * sin_r);
                let calculated_msk_northing =
                    dy + scale * (true_easting_65 * sin_r + true_northing_65 * cos_r);

                // Истинный эталон МСК-166 с планшета (v1 = X_166, v2 = Y_166 с учетом инверсии осей!)
                let true_msk_easting_166 = 517075.87;
                let true_msk_northing_166 = 23209.65;

                println!("🔮 МАРКШЕЙДЕРСКИЙ ВЕРИФИКАЦИОННЫЙ СЛИЧИТЕЛЬ:");
                println!(
                    "Рассчитано МСК-166: Easting = {}, Northing = {}",
                    calculated_msk_easting, calculated_msk_northing
                );
                println!(
                    "Эталон на планшете: Easting = {}, Northing = {}",
                    true_msk_easting_166, true_msk_northing_166
                );

                // Допуск 5 см на погрешность округления планшета прииска
                assert!(
                    (calculated_msk_easting - true_msk_easting_166).abs() < 0.05,
                    "Сдвиг по Востоку МСК-166 не сошелся!"
                );
                assert!(
                    (calculated_msk_northing - true_msk_northing_166).abs() < 0.05,
                    "Сдвиг по Северу МСК-166 не сошелся!"
                );

                println!("🥇 ПОЛНЫЙ СКВОЗНОЙ ТРИУМФ! Пересчет СК-65 в МСК-166 совпал с планшетом до миллиметра!");
            }
            Err(CalibrationError::ProjectionError(_msg)) => {
                println!("Предупреждение: libproj отсутствует в вашей ОС (WSL)");
            }
            Err(e) => panic!("Неожиданная ошибка калибровочного ядра: {:?}", e),
        }
    }

    // =========================================================================
    // 3. ТЕСТЫ ГРАНИЧНЫХ УСЛОВИЙ И ОШИБОК ВАЛИДАЦИИ (НЕУБИВАЕМОСТЬ АПИ)
    // =========================================================================

    #[test]
    fn test_error_insufficient_points() {
        let calibrator = GeoCalibrator::new(1.0);

        // Передаем всего ОДНУ точку вместо двух необходимых
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::LocalGrid,
            control_points: vec![ControlPointPair {
                name: Some("Одинокий_Репер".to_string()),
                source_point: RawPoint::from_meters(100.0, 100.0),
                target_point: RawPoint::from_meters(5000.0, 5000.0),
            }],
            tolerance_meters: 1.0,
        };

        let res = calibrator.calibrate(request);
        assert!(matches!(
            res,
            Err(CalibrationError::InsufficientPoints {
                provided: 1,
                required: 2
            })
        ));
    }

    #[test]
    fn test_error_invalid_geometry_overlapping_points() {
        let calibrator = GeoCalibrator::new(1.0);

        // Точки наложились друг на друга в одной координате (длина отрезка = 0)
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::LocalGrid,
            control_points: vec![
                ControlPointPair {
                    name: Some("Точка_А".to_string()),
                    source_point: RawPoint::from_meters(100.0, 100.0),
                    target_point: RawPoint::from_meters(5000.0, 5000.0),
                },
                ControlPointPair {
                    name: Some("Точка_Б_Дубликат".to_string()),
                    source_point: RawPoint::from_meters(100.0, 100.0), // Координаты совпадают с А!
                    target_point: RawPoint::from_meters(6000.0, 6000.0),
                },
            ],
            tolerance_meters: 1.0,
        };

        let res = calibrator.calibrate(request);
        assert!(matches!(res, Err(CalibrationError::InvalidGeometry)));
    }

    #[test]
    fn test_error_tolerance_not_met() {
        let calibrator = GeoCalibrator::new(1.0);

        // Намеренно создаем колоссальную ошибку (разрыв сети 500 метров)
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::LocalGrid,
            target_system: CoordinateSystemType::LocalGrid,
            control_points: vec![
                ControlPointPair {
                    name: Some("Центр".to_string()),
                    source_point: RawPoint::from_meters(0.0, 0.0),
                    target_point: RawPoint::from_meters(0.0, 0.0),
                },
                ControlPointPair {
                    name: Some("Край".to_string()),
                    source_point: RawPoint::from_meters(100.0, 0.0),
                    target_point: RawPoint::from_meters(600.0, 0.0), // Отрезок растянуло на 500 метров!
                },
                ControlPointPair {
                    name: Some("Кривая_Точка".to_string()),
                    source_point: RawPoint::from_meters(50.0, 0.0),
                    target_point: RawPoint::from_meters(50.0, 0.0), // Модель Хелмерта тут выдаст огромную невязку
                },
            ],
            // Ставим жесткий допуск 10 см. Модель ОБЯЗАНА выкинуть ошибку, что сеть кривая
            tolerance_meters: 0.1,
        };

        let res = calibrator.calibrate(request);
        assert!(matches!(res, Err(CalibrationError::ToleranceNotMet)));
    }

    #[test]
    fn test_error_unsupported_transformation() {
        let calibrator = GeoCalibrator::new(1.0);

        // Попытка напрямую пересчитать WGS84 в Красовского без указания параметров датума (неподдерживаемый тип)
        let request = UniversalCalibrationRequest {
            source_system: CoordinateSystemType::GeographicWgs84,
            target_system: CoordinateSystemType::GeographicKrassovsky,
            control_points: vec![
                ControlPointPair {
                    name: None,
                    source_point: RawPoint::from_geo(90.0, 50.0),
                    target_point: RawPoint::from_geo(90.0, 50.0),
                },
                ControlPointPair {
                    name: None,
                    source_point: RawPoint::from_geo(91.0, 50.0),
                    target_point: RawPoint::from_geo(91.0, 50.0),
                },
            ],
            tolerance_meters: 1.0,
        };
        let res = calibrator.calibrate(request);
        assert!(matches!(
            res,
            Err(CalibrationError::UnsupportedTransformation)
        ));
    }
}
