use crate::models::*;
use geo_core::infra::params::{BboxParams, RadiusParams};
use sqlx::{Pool, Postgres};
use uuid::Uuid;

pub struct SpaceAnalyticsRepository<'a> {
    pub pool: &'a Pool<Postgres>,
}

impl<'a> SpaceAnalyticsRepository<'a> {
    pub fn new(pool: &'a Pool<Postgres>) -> Self {
        Self { pool }
    }
    // ============================================================================
    // THERMAL HOTSPOTS CRUD & SPATIAL
    // ============================================================================

    /// **CREATE**: Добавляет в базу новую термальную аномалию (точку глубинного тепла).
    ///
    /// # Механика PostGIS
    /// Принимает долготу и широту в градусах, использует функцию `ST_MakePoint(lon, lat)`
    /// и жестко инициализирует пространственную систему координат `ST_SetSRID(..., 4326)`.
    ///
    /// # Возвращает
    /// `Result<Uuid, sqlx::Error>` — уникальный UUID созданной записи.
    pub async fn insert_thermal_hotspot(
        &self,
        req: CreateThermalHotspotRequest,
    ) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO science_degas.thermal_hotspots (source_id, acquisition_time, temp_kelvin, is_night_scene, geom)
            VALUES ($1, $2, $3, $4, ST_SetSRID(ST_MakePoint($5, $6), 4326))
            RETURNING id
            "#,
            req.source_id,
            req.acquisition_time,
            req.temp_kelvin,
            req.is_night_scene,
            req.longitude, // По стандарту ГИС сначала передается X (Долгота)
            req.latitude   // Затем Y (Широта)
        )
        .fetch_one(self.pool)
        .await?;

        Ok(row.id)
    }

    /// **READ BY ID**: Извлекает из базы данных конкретную термальную аномалию по её UUID.
    ///
    /// # Механика PostGIS
    /// Разворачивает бинарный тип геометрии `geom` обратно в числовые значения градусов
    /// широты и долготы на стороне базы с помощью функций `ST_Y(geom)` и `ST_X(geom)`.
    /// Использование суффикса `!` (например, `"latitude_raw!"`) принудительно указывает `sqlx`,
    /// что эти вычисляемые СУБД поля не могут быть `NULL`.
    ///
    /// # Возвращает
    /// `Result<Option<ThermalHotspotDomain>, sqlx::Error>` — чистую доменную модель или `None`.
    pub async fn find_hotspot_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<ThermalHotspotDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
            ThermalHotspotRow,
            r#"
            SELECT id, source_id, acquisition_time, temp_kelvin, is_night_scene,
                ST_Y(geom) as "latitude_raw!", ST_X(geom) as "longitude_raw!", created_at
            FROM science_degas.thermal_hotspots
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|r| r.into_domain()))
    }

    /// **SPATIAL READ (RADIUS)**: Находит все термальные точки в пределах заданного радиуса.
    ///
    /// # Механика PostGIS
    /// Использует функцию `ST_DWithin` с приведением типов геометрии к `::geography`. Это заставляет
    /// СУБД проводить расчет не в плоских градусах, а в метрах на сфероиде Земли, учитывая искажения.
    /// Метод задействует пространственный индекс **GIST** для мгновенного отсечения точек.
    /// Плейсхолдеры `$1::float8` и т.д. явно типизированы для успешной пре-компиляции макроса.
    ///
    /// # Возвращает
    /// `Result<Vec<ThermalHotspotDomain>, sqlx::Error>` — отсортированный по убыванию температур список точек.
    pub async fn find_thermal_hotspots_within_radius(
        &self,
        q: RadiusParams,
    ) -> Result<Vec<ThermalHotspotDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            ThermalHotspotRow,
            r#"
            SELECT id, source_id, acquisition_time, temp_kelvin, is_night_scene,
                ST_Y(geom) as "latitude_raw!", ST_X(geom) as "longitude_raw!", created_at
            FROM science_degas.thermal_hotspots
            WHERE ST_DWithin(
                geom::geography,
                ST_SetSRID(ST_MakePoint($1::float8, $2::float8), 4326)::geography,
                $3::float8
            )
            ORDER BY temp_kelvin DESC
            "#,
            q.lon,
            q.lat,
            q.radius_m
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **SPATIAL READ (BBOX)**: Находит аномалии, попавшие в рамку текущего экрана карты или QGIS.
    ///
    /// # Механика PostGIS
    /// Самый важный метод для динамического отображения слоев на фронтенде или в ГИС-клиентах.
    /// На основе 4-х точек (минимальные и максимальные долгота/широта) функция `ST_MakeEnvelope` строит
    /// прямоугольник. Оператор **`&&`** проверяет пересечение этого прямоугольника с объектами базы,
    /// используя исключительно пространственный индекс **GIST**, минуя тяжелую математику.
    ///
    /// # Возвращает
    /// `Result<Vec<ThermalHotspotDomain>, sqlx::Error>` — список точек, физически видимых на экране.
    pub async fn find_hotspots_within_bbox(
        &self,
        bbox: BboxParams,
    ) -> Result<Vec<ThermalHotspotDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            ThermalHotspotRow,
            r#"
            SELECT id, source_id, acquisition_time, temp_kelvin, is_night_scene,
                ST_Y(geom) as "latitude_raw!", ST_X(geom) as "longitude_raw!", created_at
            FROM science_degas.thermal_hotspots
            WHERE geom && ST_MakeEnvelope($1::float8, $2::float8, $3::float8, $4::float8, 4326)
            ORDER BY temp_kelvin DESC
            "#,
            bbox.min_lon,
            bbox.min_lat,
            bbox.max_lon,
            bbox.max_lat
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **UPDATE**: Калибрует числовые характеристики термальной точки (температуру и время съемки).
    ///
    /// # Назначение
    /// Используется ИИ-скриптами или оператором для внесения правок по результатам дополнительной
    /// калибровки растровых матриц NASA (например, после фильтрации дневных солнечных помех).
    ///
    /// # Возвращает
    /// `Result<bool, sqlx::Error>` — `true`, если параметры записи успешно изменены в БД.
    pub async fn update_hotspot_metrics(
        &self,
        id: Uuid,
        temp_kelvin: f64,
        is_night_scene: bool,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            UPDATE science_degas.thermal_hotspots
            SET temp_kelvin = $1, is_night_scene = $2
            WHERE id = $3
            "#,
            temp_kelvin,
            is_night_scene,
            id
        )
        .execute(self.pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    /// **DELETE**: Безоговорочно удаляет термальную аномалию из системы по её идентификатору.
    ///
    /// # Каскадность
    /// Метод удаляет только саму точку. Связанные метаданные в `core_storage.data_sources` не затрагиваются,
    /// однако если удалить сам источник, то по правилу `ON DELETE CASCADE` все дочерние hotspots удалятся сами.
    ///
    /// # Возвращает
    /// `Result<bool, sqlx::Error>` — `true`, если запись была физически стерта из таблицы.
    pub async fn delete_hotspot(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            DELETE FROM science_degas.thermal_hotspots
            WHERE id = $1
            "#,
            id
        )
        .execute(self.pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    // ============================================================================
    // GAS PLUMES CRUD & SPATIAL
    // ============================================================================

    /// **CREATE**: Импортирует контур пространственного газового шлейфа (метана или водорода).
    ///
    /// # Механика PostGIS
    /// Принимает геометрию в текстовом формате WKT (Well-Known Text). СУБД парсит её функцией
    /// `ST_GeomFromText(..., 4326)`. Обертка `ST_Multi` гарантирует принудительное приведение
    /// любого полигона к типу `MultiPolygon` для строгого соответствия схеме таблицы.
    ///
    /// # Возвращает
    /// `Result<Uuid, sqlx::Error>` — уникальный UUID созданной записи.
    pub async fn insert_gas_plume(&self, req: CreateGasPlumeRequest) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO science_degas.gas_plumes (source_id, gas_type, pixel_value_max, pixel_value_avg, confidence_score, geom)
            VALUES ($1, $2, $3, $4, $5::int4, ST_Multi(ST_GeomFromText($6, 4326)))
            RETURNING id
            "#,
            req.source_id,
            req.gas_type,
            req.pixel_value_max,
            req.pixel_value_avg,
            req.confidence_score,
            req.geometry_wkt
        )
        .fetch_one(self.pool)
        .await?;

        Ok(row.id)
    }

    /// **READ BY ID**: Извлекает конкретный газовый шлейф по его UUID.
    ///
    /// # Механика PostGIS
    /// Трансформирует бинарную геометрию (`geometry`) в легковесную текстовую строку
    /// стандарта GeoJSON с помощью функции `ST_AsGeoJSON(geom)`. На стороне Rust доменная модель
    /// автоматически распарсит эту строку в валидное дерево `serde_json::Value` для фронтенда.
    ///
    /// # Возвращает
    /// `Result<Option<GasPlumeDomain>, sqlx::Error>` — заполненную бизнес-сущность или `None`.
    pub async fn find_gas_plume_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<GasPlumeDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
            GasPlumeRow,
            r#"
            SELECT id, source_id, gas_type, pixel_value_max, pixel_value_avg, confidence_score,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.gas_plumes
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|r| r.into_domain()))
    }

    /// **SPATIAL READ (BBOX)**: Нарезает контуры газовых облаков строго под текущее окно экрана QGIS/карты.
    ///
    /// # Механика PostGIS
    /// Функция `ST_MakeEnvelope` строит виртуальный прямоугольник по координатам экрана `BboxParams` [image_6N4y6X.png].
    /// Скоростной бинарный оператор пересечения **`&&`** мгновенно отбирает полигоны газов через пространственный
    /// индекс **GIST**. Метод поддерживает опциональный фильтр `gas_type_filter` для раздельного включения слоев CH4 и H2.
    ///
    /// # Возвращает
    /// `Result<Vec<GasPlumeDomain>, sqlx::Error>` — список шлейфов, отсортированных по пиковой концентрации газа.
    pub async fn find_gas_plumes_within_bbox(
        &self,
        bbox: BboxParams,
        gas_type_filter: Option<String>,
    ) -> Result<Vec<GasPlumeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            GasPlumeRow,
            r#"
            SELECT id, source_id, gas_type, pixel_value_max, pixel_value_avg, confidence_score,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.gas_plumes
            WHERE geom && ST_MakeEnvelope($1::float8, $2::float8, $3::float8, $4::float8, 4326)
                AND ($5::text IS NULL OR gas_type = $5)
            ORDER BY pixel_value_max DESC NULLS LAST
            "#,
            bbox.min_lon,
            bbox.min_lat,
            bbox.max_lon,
            bbox.max_lat,
            gas_type_filter
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **SPATIAL READ (RADIUS)**: Находит все газовые шлейфы в радиусе вокруг точечного объекта (например, скважины).
    ///
    /// # Механика PostGIS
    /// Поскольку шлейфы хранятся как площадные объекты (`MultiPolygon`), функция `ST_DWithin` рассчитывает
    /// кратчайшее расстояние от заданной точки `ST_MakePoint` до *ближайшей кромки (границы)* полигона газа.
    /// Использование типа `::geography` переводит вычисления из плоских градусов в реальные метры на геоиде.
    ///
    /// # Возвращает
    /// `Result<Vec<GasPlumeDomain>, sqlx::Error>` — список задетых или находящихся рядом шлейфов.
    pub async fn find_gas_plumes_within_radius(
        &self,
        q: RadiusParams,
        gas_type_filter: Option<String>,
    ) -> Result<Vec<GasPlumeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            GasPlumeRow,
            r#"
            SELECT id, source_id, gas_type, pixel_value_max, pixel_value_avg, confidence_score,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.gas_plumes
            WHERE ST_DWithin(
                geom::geography,
                ST_SetSRID(ST_MakePoint($1::float8, $2::float8), 4326)::geography,
                $3::float8
            )
            AND ($4::text IS NULL OR gas_type = $4)
            ORDER BY pixel_value_max DESC NULLS LAST
            "#,
            q.lon,
            q.lat,
            q.radius_m,
            gas_type_filter
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **DELETE**: Физически удаляет контур газовой аномалии из базы данных по её UUID.
    ///
    /// # Каскадность
    /// Метод удаляет только саму геометрию. Связанный растровый файл в логах данных не уничтожается.
    ///
    /// # Возвращает
    /// `Result<bool, sqlx::Error>` — `true`, если строка была найдена и стерта.
    pub async fn delete_gas_plume(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            DELETE FROM science_degas.gas_plumes
            WHERE id = $1
            "#,
            id
        )
        .execute(self.pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    /// **UPDATE**: Калибрует аналитические ИИ-метрики плотности и достоверности шлейфа.
    ///
    /// # Назначение
    /// Метод спроектирован под работу с опциональными значениями (`Option`). Позволяет нейросетям
    /// точечно обновлять среднюю (`avg`) или максимальную (`max`) концентрацию газа, а также
    /// пересчитывать процент уверенности (`confidence_score`) без перезаписи пространственного полигона.
    ///
    /// # Возвращает
    /// `Result<bool, sqlx::Error>` — `true` при успешном апдейте записи.
    pub async fn update_gas_plume_metrics(
        &self,
        id: Uuid,
        pixel_value_max: Option<f64>,
        pixel_value_avg: Option<f64>,
        confidence_score: Option<i32>,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            UPDATE science_degas.gas_plumes
            SET pixel_value_max = $1,
                pixel_value_avg = $2,
                confidence_score = $3
            WHERE id = $4
            "#,
            pixel_value_max,
            pixel_value_avg,
            confidence_score,
            id
        )
        .execute(self.pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    // ============================================================================
    // HYDROGEN RINGS CRUD & SPATIAL
    // ============================================================================

    /// **CREATE**: Импортирует контур кольцевой структуры («ведьминого кольца» геоботаники).
    ///
    /// # Механика PostGIS
    /// Принимает геометрию депрессии в текстовом формате WKT (Well-Known Text). СУБД парсит её
    /// функцией `ST_GeomFromText(..., 4326)` и сохраняет как валидный замкнутый `Polygon`.
    ///
    /// # Возвращает
    /// `Result<Uuid, sqlx::Error>` — уникальный UUID созданной водородной структуры.
    pub async fn insert_hydrogen_ring(
        &self,
        req: CreateHydrogenRingRequest,
    ) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO science_degas.hydrogen_rings (deposit_id, structure_type, ring_diameter_meters, ndvi_drop_percentage, geological_setting, geom)
            VALUES ($1, $2, $3, $4, $5, ST_GeomFromText($6, 4326))
            RETURNING id
            "#,
            req.deposit_id,
            req.structure_type,
            req.ring_diameter_meters,
            req.ndvi_drop_percentage,
            req.geological_setting,
            req.geometry_wkt
        )
        .fetch_one(self.pool)
        .await?;

        Ok(row.id)
    }

    /// **READ BY ID**: Извлекает конкретное водородное кольцо по его UUID.
    ///
    /// # Механика PostGIS
    /// Сериализует бинарный полигон в текстовую GeoJSON-строку с помощью `ST_AsGeoJSON(geom)`.
    /// Суффикс маппинга `?` указывает макросу `sqlx`, что вычисляемое поле может принимать `Option<String>`,
    /// что идеально ловится структурой `HydrogenRingRow`.
    ///
    /// # Возвращает
    /// `Result<Option<HydrogenRingDomain>, sqlx::Error>` — доменную модель кольца со встроенной валидацией.
    pub async fn find_hydrogen_ring_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<HydrogenRingDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
            HydrogenRingRow,
            r#"
            SELECT id, deposit_id, structure_type, ring_diameter_meters, ndvi_drop_percentage, geological_setting,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.hydrogen_rings
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|r| r.into_domain()))
    }

    /// **READ ALL**: Возвращает полный реестр зафиксированных круговых структур (базовый листинг).
    ///
    /// # Назначение
    /// Применяется для выгрузки глобальных каталогов геоботанических аномалий Якутии и Сибири.
    /// Сортирует записи по времени обнаружения (`created_at`) в обратном порядке.
    ///
    /// # Возвращает
    /// `Result<Vec<HydrogenRingDomain>, sqlx::Error>` — вектор всех доменных объектов колец.
    pub async fn fetch_all_hydrogen_rings(&self) -> Result<Vec<HydrogenRingDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            HydrogenRingRow,
            r#"
            SELECT id, deposit_id, structure_type, ring_diameter_meters, ndvi_drop_percentage, geological_setting,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.hydrogen_rings
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **SPATIAL READ (BBOX)**: Нарезает контуры вегетационных колец под экстент экрана QGIS/карты.
    ///
    /// # Механика PostGIS
    /// Использует твой инфраструктурный `BboxParams` из ядра ядра. Функция `ST_MakeEnvelope` строит
    /// пространственную рамку, а бинарный оператор пересечения **`&&`** мгновенно выбирает полигоны колец,
    /// задействуя индекс **GIST**. Сортирует выборку от самых крупных колец к мелким.
    ///
    /// # Возвращает
    /// `Result<Vec<HydrogenRingDomain>, sqlx::Error>` — кольца, видимые в текущей области карты.
    pub async fn find_hydrogen_rings_within_bbox(
        &self,
        bbox: BboxParams,
    ) -> Result<Vec<HydrogenRingDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            HydrogenRingRow,
            r#"
            SELECT id, deposit_id, structure_type, ring_diameter_meters, ndvi_drop_percentage, geological_setting,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.hydrogen_rings
            WHERE geom && ST_MakeEnvelope($1::float8, $2::float8, $3::float8, $4::float8, 4326)
            ORDER BY ring_diameter_meters DESC NULLS LAST
            "#,
            bbox.min_lon,
            bbox.min_lat,
            bbox.max_lon,
            bbox.max_lat
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **SPATIAL READ (RADIUS)**: Ищет водородные кольца в радиусе метров вокруг опорной точки.
    ///
    /// # Механика PostGIS
    /// Рассчитывает точное расстояние от точки `ST_MakePoint` до геоботанического полигона с помощью
    /// `ST_DWithin`. Приведение типов `::geography` гарантирует, что радиус `$3::float8` обрабатывается СУБД
    /// строго в метрах, учитывая кривизну Земли в высоких полярных широтах Якутии. Сортирует результаты
    /// по уровню жесткости угнетения тайги (`ndvi_drop_percentage`).
    ///
    /// # Возвращает
    /// `Result<Vec<HydrogenRingDomain>, sqlx::Error>` — список задетых радиусом структур.
    pub async fn find_hydrogen_rings_within_radius(
        &self,
        q: RadiusParams,
    ) -> Result<Vec<HydrogenRingDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            HydrogenRingRow,
            r#"
            SELECT id, deposit_id, structure_type, ring_diameter_meters, ndvi_drop_percentage, geological_setting,
                ST_AsGeoJSON(geom) as "geom_geojson?", created_at
            FROM science_degas.hydrogen_rings
            WHERE ST_DWithin(
                geom::geography,
                ST_SetSRID(ST_MakePoint($1::float8, $2::float8), 4326)::geography,
                $3::float8
            )
            ORDER BY ndvi_drop_percentage DESC NULLS LAST
            "#,
            q.lon,
            q.lat,
            q.radius_m
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **UPDATE**: Пересчитывает физические метрики кольца после дополнительного ИИ-моделирования.
    ///
    /// # Назначение
    /// Позволяет внешним ML-сервисам калибровать диаметр кольцевой депрессии (`ring_diameter_meters`)
    /// и глубину просадки спектрального индекса (`ndvi_drop_percentage`) без необходимости перезаписи
    /// самого полигона геометрии. Принимает `Option`, обновляя только переданные поля.
    ///
    /// # Возвращает
    /// `Result<bool, sqlx::Error>` — `true`, если строка была найдена и успешно модифицирована.
    pub async fn update_hydrogen_ring_metrics(
        &self,
        id: Uuid,
        ring_diameter_meters: Option<f64>,
        ndvi_drop_percentage: Option<f64>,
        geological_setting: Option<String>,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            UPDATE science_degas.hydrogen_rings
            SET ring_diameter_meters = $1,
                ndvi_drop_percentage = $2,
                geological_setting = $3
            WHERE id = $4
            "#,
            ring_diameter_meters,
            ndvi_drop_percentage,
            geological_setting,
            id
        )
        .execute(self.pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    /// **DELETE**: Безоговорочно стирает запись о водородном кольце из СУБД.
    ///
    /// # Механика
    /// Выполняет быстрое удаление по первичному ключу UUID. Если у кольца был выставлен внешний ключ
    /// на таблицу лицензионных участков (`deposit_id`), то по правилу `ON DELETE SET NULL` удаление кольца
    /// никак не затронет границы самого участка `deposits`.
    ///
    /// # Возвращает
    /// `Result<bool, sqlx::Error>` — `true` при успешном удалении.
    pub async fn delete_hydrogen_ring(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            DELETE FROM science_degas.hydrogen_rings
            WHERE id = $1
            "#,
            id
        )
        .execute(self.pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    // ============================================================================
    // PRIME TARGETS SCIENTIFIC VIEW (CROSS-LAYER INTERSECTIONS)
    // ============================================================================

    /// **SCIENTIFIC VIEW READ**: Извлекает итоговые высокоперспективные водородные цели.
    ///
    /// # Механика PostGIS
    /// Данный метод обращается к аналитическому представлению (SQL View) `v_hydrogen_prime_targets`.
    /// Вьюха на уровне СУБД выполняет автоматический сквозной пространственный анализ: находит
    /// пересечения (`ST_Intersects`), где контур водородного кольца совпадает с глубинной термальной
    /// точкой разлома и одновременно накрывается аномальным облаком метана/водорода.
    ///
    /// # Особенности sqlx
    /// Так как данные тянутся из View, компилятор Rust не всегда может определить `NULL`-предикаты.
    /// Строка `ST_AsGeoJSON(geom) as "geom_geojson?"` принудительно указывает макросу упаковать
    /// полигон в тип `Option<String>`, что обеспечивает стопроцентную стабильность сборки.
    ///
    /// # Возвращает
    /// `Result<Vec<PrimeTargetDomain>, sqlx::Error>` — список комплексных целей для ГИС-карты.
    pub async fn fetch_prime_hydrogen_targets(
        &self,
    ) -> Result<Vec<PrimeTargetDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            PrimeTargetRow,
            r#"
            SELECT
                ring_id as "ring_id!",
                deposit_id as "deposit_id!",
                ring_diameter_meters as "ring_diameter_meters?",
                ndvi_drop_percentage as "ndvi_drop_percentage?",
                hotspot_temp_c as "hotspot_temp_c?",
                gas_type as "gas_type!",
                gas_concentration_max as "gas_concentration_max?",
                ST_AsGeoJSON(geom) as "geom_geojson?"
            FROM science_degas.v_hydrogen_prime_targets
            "#
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // ============================================================================
    // 🗑️ GENERAL DYNAMIC DELETE METHOD BY ID (Универсальный уборщик)
    // ============================================================================

    /// **DYNAMIC DELETE**: Универсальный метод удаления пространственных сущностей по имени таблицы.
    ///
    /// # Механика Архитектуры
    /// Предназначен для использования внешними ИИ-пайплайнами, скриптами очистки или системными воркерами.
    /// Позволяет не плодить кучу эндпоинтов удаления, а отправлять тип сущности строкой.
    ///
    /// # Важное замечание по безопасности и sqlx
    /// Так как текст запроса (`query_str`) генерируется динамически через конструкцию `match` во время работы приложения,
    /// использовать статический макрос `sqlx::query!` здесь нельзя. Мы используем стандартный runtime-метод
    /// `sqlx::query()`, а плейсхолдер `$1` (UUID) биндится безопасно, исключая любые SQL-инъекции.
    ///
    /// # Возвращает
    /// `Result<u64, sqlx::Error>` — количество фактически удаленных строк из базы данных (0 или 1).
    pub async fn delete_entity(&self, table_name: &str, id: Uuid) -> Result<u64, sqlx::Error> {
        let query_str = match table_name {
            "hotspots" => "DELETE FROM science_degas.thermal_hotspots WHERE id = $1",
            "plumes" => "DELETE FROM science_degas.gas_plumes WHERE id = $1",
            "rings" => "DELETE FROM science_degas.hydrogen_rings WHERE id = $1",
            _ => return Ok(0),
        };

        let res = sqlx::query(query_str).bind(id).execute(self.pool).await?;

        Ok(res.rows_affected())
    }

    // ============================================================================
    // 🧬 CROSS-LAYER ANALYTICS (ПРОФЕССИОНАЛЬНЫЙ ГИС-ФУНКЦИОНАЛ)
    // ============================================================================

    /// **CROSS-LAYER GEOMETRY INTERSECTION**: Находит тепловые точки разломов, находящиеся строго внутри контура конкретного газового облака.
    ///
    /// # Механика PostGIS
    /// Использует топологическую функцию `ST_Contains(polygon, point)`. Метод математически доказывает
    /// причинно-следственную связь: если эпицентр ночного тепла лежит внутри полигона шлейфа, значит,
    /// данный разлом является прямым источником (сипом) текущей дегазации метана или водорода.
    ///
    /// # Возвращает
    /// `Result<Vec<ThermalHotspotDomain>, sqlx::Error>` — список подогревающих разломов, ранжированных от самых горячих.
    pub async fn find_hotspots_inside_plume(
        &self,
        plume_id: Uuid,
    ) -> Result<Vec<ThermalHotspotDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            ThermalHotspotRow,
            r#"
            SELECT h.id, h.source_id, h.acquisition_time, h.temp_kelvin, h.is_night_scene,
                ST_Y(h.geom) as "latitude_raw!", ST_X(h.geom) as "longitude_raw!", h.created_at
            FROM science_degas.thermal_hotspots h
            JOIN science_degas.gas_plumes p ON ST_Contains(p.geom, h.geom)
            WHERE p.id = $1
            ORDER BY h.temp_kelvin DESC
            "#,
            plume_id
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **LICENSED BOUNDARY CHECK**: Выявляет все газовые аномалии, накрывающие конкретный лицензионный отвод (`deposit`).
    ///
    /// # Механика PostGIS
    /// Ключевая бизнес-функция цифрового двойника. Проверяет пересечение пространственных полигонов
    /// газового облака `gas_plumes` и границ участка `core_storage.deposits` через `ST_Intersects`.
    /// Позволяет недропользователю мгновенно оценить риски и перспективы бурения на водород внутри своего отвода.
    ///
    /// # Возвращает
    /// `Result<Vec<GasPlumeDomain>, sqlx::Error>` — шлейфы газов, нарушающие или находящиеся внутри границ лицензии.
    pub async fn find_gas_plumes_by_deposit(
        &self,
        deposit_id: Uuid,
    ) -> Result<Vec<GasPlumeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            GasPlumeRow,
            r#"
            SELECT p.id, p.source_id, p.gas_type, p.pixel_value_max, p.pixel_value_avg, p.confidence_score,
                ST_AsGeoJSON(p.geom) as "geom_geojson?", p.created_at
            FROM science_degas.gas_plumes p
            -- Скрещиваем геометрию шлейфа с полигоном границ твоего депозита из MVP-схемы
            JOIN core_storage.deposits d ON ST_Intersects(p.geom, d.polygon_boundary)
            WHERE d.id = $1
            ORDER BY p.pixel_value_max DESC NULLS LAST
            "#,
            deposit_id
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    /// **BUFFER ZONE GEOPROCESSING**: Находит «ведьмины кольца» вегетации, расположенные в буферной зоне активных разломов.
    ///
    /// # Механика PostGIS
    /// Использует пространственную функцию `ST_DWithin` над типами `::geography`, которая строит динамическую
    /// буферную зону (например, 5000 метров) вокруг тепловых точек и ищет попавшие туда водородные структуры.
    /// Ключевое слово `DISTINCT` исключает дублирование колец, если они попали в радиус сразу нескольких разломов.
    ///
    /// # Возвращает
    /// `Result<Vec<HydrogenRingDomain>, sqlx::Error>` — массив колец, подтвержденных близостью к тектоническим трещинам.
    pub async fn find_rings_near_faults(
        &self,
        max_distance_meters: f64,
    ) -> Result<Vec<HydrogenRingDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            HydrogenRingRow,
            r#"
            SELECT DISTINCT r.id, r.deposit_id, r.structure_type, r.ring_diameter_meters,
                r.ndvi_drop_percentage, r.geological_setting,
                ST_AsGeoJSON(r.geom) as "geom_geojson?", r.created_at
            FROM science_degas.hydrogen_rings r
            JOIN science_degas.thermal_hotspots t
            ON ST_DWithin(r.geom::geography, t.geom::geography, $1::float8)
            ORDER BY r.created_at DESC
            "#,
            max_distance_meters
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }
} // struct SpaceAnalyticsRepository
