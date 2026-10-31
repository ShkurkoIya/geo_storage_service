use crate::models::{NivelerFilterParams, NivelerNodeDomain, NivelerNodeRow};
use crate::repository::ggs_nodes::GgsNodesRepository;
use geo_core::impl_json_stream;
use geo_core::infra::params::{BboxParams, PaginatedResponse, RadiusParams};
use sqlx::{Postgres, Transaction};

impl<'a> GgsNodesRepository<'a> {
    // --- CREATE (Одиночная вставка одного репера нивелирной сети) ---

    /// **Добавить новый нивелирный репер высотных сетей в базу данных**
    ///
    /// Производит одиночную вставку записи в таблицу `reference_geo.niveler_nodes` напрямую из доменной модели.
    /// Автоматически рассчитывает пространственные 3D-точки для глобального маппинга (WGS 84 / SRID 4326)
    /// и локального рендеринга (Web Mercator / SRID 3857).
    ///
    /// ### Параметры:
    /// * `node` — Ссылка на доменную бизнес-модель нивелирного репера `NivelerNodeDomain`.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Порядок ГИС-координат**: В функцию `ST_MakePoint($7, $6, 0.0)` параметры передаются строго в
    ///   географическом порядке: `($7: Longitude, $6: Latitude, Z: 0.0)`.
    /// * **Compile-time типизация**: Конечные вызовы пространственных функций защищены суффиксом `::geometry`.
    ///   Это избавляет планировщик макросов `sqlx` от неопределенности возвращаемых СУБД типов данных.
    /// * **Идемпотентность**: Конструкция `ON CONFLICT (id) DO NOTHING` страхует от падения потока, если репер с таким ID уже есть в системе.
    pub async fn insert_niveler_node(&self, node: &NivelerNodeDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
               r#"
               INSERT INTO reference_geo.niveler_nodes (
                   id, node_index, name, region_name, district_name, latitude_raw, longitude_raw, geom_global, geom_local
               )
               VALUES (
                   $1, $2, $3, $4, $5, $6, $7,
                   -- Добавлен явный каст к ::geometry для стабильной сборки макроса sqlx
                   ST_SetSRID(ST_MakePoint($7, $6, 0.0), 4326)::geometry,
                   ST_Transform(ST_SetSRID(ST_MakePoint($7, $6, 0.0), 4326), 3857)::geometry
               )
               ON CONFLICT (id) DO NOTHING;
               "#,
               node.id,            // $1
               node.node_index,    // $2
               node.name,          // $3
               node.region_name,   // $4
               node.district_name, // $5
               node.latitude_raw,  // $6
               node.longitude_raw  // $7
           )
           .execute(self.pool)
           .await?;
        Ok(())
    }

    // --- READ (Получить нивелирный репер по ID) ---

    /// **Получить подробную информацию о нивелирном репере по его уникальному ID**
    ///
    /// Выполняет точечную атомарную выборку из таблицы `reference_geo.niveler_nodes`.
    /// Автоматически маппит плоскую строку СУБД (`NivelerNodeRow`) в готовую доменную
    /// модель бизнес-логики (`NivelerNodeDomain`) для передачи на верхние уровни приложения.
    ///
    /// ### Параметры:
    /// * `id` — Уникальный системный идентификатор нивелирного репера (`i32`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Option<NivelerNodeDomain>, sqlx::Error>` — В случае успеха возвращает:
    ///   * `Some(NivelerNodeDomain)` — Если репер с таким ID найден.
    ///   * `None` — Если в базе данных нет записи с указанным ID.
    /// * Возвращает `Err(sqlx::Error)` при сетевых сбоях или ошибках пула соединений.
    pub async fn find_niveler_by_id(
        &self,
        id: i32,
    ) -> Result<Option<NivelerNodeDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
            NivelerNodeRow,
            r#"
            SELECT id, node_index, name, region_name, district_name, latitude_raw, longitude_raw
            FROM reference_geo.niveler_nodes WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool)
        .await?;

        // Трансформируем плоскую запись из базы в стерильную доменную сущность
        Ok(row.map(|r| r.into_domain()))
    }

    // --- UPDATE (Обновить метаданные и 3D-геометрии нивелира) ---

    /// **Обновить данные существующего нивелирного репера высотных сетей**
    ///
    /// Полностью перезаписывает атрибуты репера в таблице `reference_geo.niveler_nodes` по его уникальному `id`.
    /// На основе измененных сырых координат автоматически пересчитывает PostGIS-колонки `geom_global`
    /// (глобальная WGS 84 / SRID 4326) и `geom_local` (метрический Web Mercator / SRID 3857).
    ///
    /// ### Параметры:
    /// * `node` — Ссылка на доменную бизнес-модель нивелирного репера `NivelerNodeDomain`.
    ///
    /// ### Технические особенности и ГИС-валидация:
    /// * **Порядок осей**: Функция `ST_MakePoint($6, $5, 0.0)` принимает параметры строго в географическом
    ///   формате: `($6: Longitude, $5: Latitude, Z: 0.0)`. Нарушение порядка сместит объект на карте.
    /// * **Подсказка типов**: К пространственным колонкам добавлен явный суффикс приведения типов `::geometry`.
    ///   Это защищает абстрактное синтаксическое дерево (AST) макроса `sqlx` от ошибок несовместимости типов СУБД.
    pub async fn update_niveler(&self, node: &NivelerNodeDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE reference_geo.niveler_nodes
            SET
                node_index = $1, name = $2, region_name = $3, district_name = $4,
                latitude_raw = $5, longitude_raw = $6,
                -- ИСПРАВЛЕНО: Добавлен каст ::geometry и проверен порядок осей (Долгота $6, Широта $5)
                geom_global = ST_SetSRID(ST_MakePoint($6, $5, 0.0), 4326)::geometry,
                geom_local = ST_Transform(ST_SetSRID(ST_MakePoint($6, $5, 0.0), 4326), 3857)::geometry
            WHERE id = $7
            "#,
            node.node_index,    // $1
            node.name,          // $2
            node.region_name,   // $3
            node.district_name, // $4
            node.latitude_raw,  // $5
            node.longitude_raw, // $6
            node.id             // $7
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }

    // --- DELETE (Удаление нивелирного репера по ID) ---

    /// **Физически удалить нивелирный репер высотных сетей из базы данных**
    ///
    /// Производит жесткое, необратимое удаление (`HARD DELETE`) записи из таблицы
    /// `reference_geo.niveler_nodes` по её уникальному системному идентификатору `id`.
    ///
    /// ### Параметры:
    /// * `id` — Уникальный целочисленный идентификатор удаляемого нивелирного репера (`i32`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешного удаления (или если записи с таким ID
    ///   изначально не существовало в СУБД) возвращает `Ok(())`.
    /// * Возвращает `Err(sqlx::Error)`, если строка заблокирована конкурентной транзакцией.
    ///
    /// ### ГИС-контекст безопасности:
    /// Удаление строки автоматически очищает связанные пространственные индексы `GIST` для данного объекта.
    /// Перед вызовом убедись, что на этот репер не завязаны расчетные геодезические ходы или
    /// активные профили нивелирования в смежных модулях системы.
    pub async fn delete_niveler(&self, id: i32) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM reference_geo.niveler_nodes WHERE id = $1", id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // --- READ LIST (Пагинация списка нивелиров) ---

    /// **Получить список нивелирных реперов высотных сетей с поддержкой постраничной пагинации**
    ///
    /// Метод осуществляет строго лимитированную выборку данных. Сначала вычисляется общее
    /// количество записей в таблице `reference_geo.niveler_nodes`, после чего извлекается
    /// целевой срез данных для текущей страницы на основе смещения (`OFFSET`) и лимита (`LIMIT`).
    ///
    /// ### Параметры:
    /// * `page` — Номер запрашиваемой страницы (`i64`). Начинается с `1`.
    /// * `per_page` — Желаемое количество элементов на одной странице (`i64`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<PaginatedResponse<NivelerNodeDomain>, sqlx::Error>` — Объект постраничного ответа,
    ///   содержащий отфильтрованный вектор доменных моделей реперов и метаданные страниц.
    ///
    /// ### Технические особенности и оптимизация:
    /// * Параметр `per_page` жестко ограничивается диапазоном от `1` до `100` через `.clamp()`,
    ///   предотвращая перегрузку пула соединений СУБД.
    /// * Переменная `total_records` принудительно кастится к базовому типу `i64` через развертку `Option`,
    ///   что гарантирует прохождение compile-time проверок макроса `sqlx`.
    /// * Расчет общего числа страниц защищен от деления на ноль на случай работы с пустой таблицей.
    pub async fn find_niveler_paginated(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedResponse<NivelerNodeDomain>, sqlx::Error> {
        let limit = per_page.clamp(1, 100);
        let normalized_page = page.max(1);
        let offset = (normalized_page - 1) * limit;

        // Получаем общее количество записей в таблице нивелиров
        // Явно принуждаем к типу i64 через unwrap_or(0), страхуясь от капризов компилятора sqlx
        let total_records: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) FROM reference_geo.niveler_nodes")
                .fetch_one(self.pool)
                .await?
                .unwrap_or(0);

        // Вытаскиваем конкретную страницу данных
        let rows = sqlx::query_as!(
            NivelerNodeRow,
            r#"
            SELECT id, node_index, name, region_name, district_name, latitude_raw, longitude_raw
            FROM reference_geo.niveler_nodes
            ORDER BY id ASC
            LIMIT $1 OFFSET $2
            "#,
            limit,
            offset
        )
        .fetch_all(self.pool)
        .await?;

        // Трансформируем плоские строки ответов СУБД в стерильные доменные модели
        let data = rows.into_iter().map(|r| r.into_domain()).collect();

        // Безопасный расчет общего количества страниц (избегаем деления на 0 при пустой выборке)
        let total_pages = if total_records == 0 {
            1
        } else {
            (total_records as f64 / limit as f64).ceil() as i64
        };

        Ok(PaginatedResponse {
            data,
            current_page: normalized_page,
            per_page: limit,
            total_records,
            total_pages,
        })
    }

    // --- GEOSPATIAL (Радиусный поиск нивелиров) ---

    /// **Найти нивелирные реперы высотных сетей в заданном радиусе от географической точки**
    ///
    /// Выполняет пространственный поиск объектов (PostGIS) с использованием пространственного
    /// индекса `GIST` по полю `geom_global`. Результаты автоматически сортируются по возрастанию
    /// метрического расстояния (от ближайшего репера к самому дальнему).
    ///
    /// ### Параметры:
    /// * `params` — Ссылка на общую структуру параметров `RadiusParams`, содержащую
    ///   координаты центра (`lon`, `lat`) и радиус поиска в метрах (`radius_m`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Vec<NivelerNodeDomain>, sqlx::Error>` — В случае успеха возвращает вектор доменных
    ///   моделей реперов, попавших в указанную окружность.
    ///
    /// ### Технические особенности и PostGIS-оптимизация:
    /// * **Устранение конфликта типов**: Подзапрос `WITH center_point AS (...)` создает
    ///   географическую точку центра ровно один раз. Это исключает дублирование параметров `$1`, `$2`
    ///   в SQL и гарантирует, что макрос `sqlx` корректно свяжет типы аргументов Rust и Postgres.
    /// * **Производительность**: Использование функции `ST_DWithin` отсекает лишние строки на уровне
    ///   индекса базы, а планировщик СУБД берет уже готовую геометрию центра для сортировки в `ORDER BY`.
    pub async fn find_niveler_within_radius(
        &self,
        params: &RadiusParams,
    ) -> Result<Vec<NivelerNodeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            NivelerNodeRow,
            r#"
                WITH center_point AS (
                    SELECT ST_SetSRID(ST_MakePoint($1, $2, 0.0), 4326)::geography AS geo
                )
                SELECT
                    id, node_index, name, region_name, district_name, latitude_raw, longitude_raw
                FROM reference_geo.niveler_nodes, center_point
                WHERE ST_DWithin(geom_global, center_point.geo, $3)
                ORDER BY ST_Distance(geom_global, center_point.geo) ASC
                "#,
            params.lon,      // $1 - f64
            params.lat,      // $2 - f64
            params.radius_m  // $3 - f64
        )
        .fetch_all(self.pool)
        .await?;

        // Трансформируем плоские строки базы данных в чистые доменные бизнес-сущности
        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // --- GEOSPATIAL (BBox поиск нивелиров) ---

    /// **Найти нивелирные реперы высотных сетей внутри географического экстента (BBox)**
    ///
    /// Метод используется для динамической фильтрации и подгрузки точек на фронтенде веб-карт
    /// или в ГИС-плагинах (например, QGIS) при перемещении или масштабировании области видимости экрана.
    ///
    /// ### Параметры:
    /// * `params` — Ссылка на общую пространственную структуру `BboxParams`, содержащую
    ///   координаты углов рамки видимости: `min_lon` (запад), `min_lat` (юг), `max_lon` (восток), `max_lat` (север).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Vec<NivelerNodeDomain>, sqlx::Error>` — В случае успеха возвращает вектор доменных
    ///   моделей реперов, физически попавших в границы переданного прямоугольника.
    ///
    /// ### Технические особенности и PostGIS-оптимизация:
    /// * **Индексированное плоское сканирование**: Оператор пересечения рамок **`&&`** заменяет 3D-оператор `&&&`.
    ///   Это заставляет планировщик Postgres использовать плоский индекс `GIST` без лишних расчетов Z-координаты.
    /// * **Безопасность compile-time макроса**: Передаваемые параметры `f64` строго соотносятся с типами аргументов
    ///   функции `ST_MakeEnvelope` для SRID 4326.
    pub async fn find_niveler_in_bbox(
        &self,
        params: &BboxParams,
    ) -> Result<Vec<NivelerNodeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            NivelerNodeRow,
            r#"
            SELECT id, node_index, name, region_name, district_name, latitude_raw, longitude_raw
            FROM reference_geo.niveler_nodes
            -- ИСПРАВЛЕНО: Применяем 2D оператор && для работы по плоскому GIST-индексу
            WHERE geom_global && ST_MakeEnvelope($1, $2, $3, $4, 4326)
            "#,
            params.min_lon, // $1
            params.min_lat, // $2
            params.max_lon, // $3
            params.max_lat  // $4
        )
        .fetch_all(self.pool)
        .await?;

        // Трансформируем плоские строки ответов СУБД в стерильные доменные модели
        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // --- SEARCH & FILTER (Умная фильтрация нивелиров с пагинацией) ---

    /// **Выполнить гибкий поиск нивелирных реперов с динамической фильтрацией и пагинацией**
    ///
    /// Осуществляет многофакторный поиск по текстовому совпадению (регистронезависимо через `ILIKE`)
    /// названия пункта и наименования региона. Все фильтры опциональны: если параметр передан как `None`,
    /// СУБД пропустит соответствующее условие в блоке `WHERE`.
    ///
    /// ### Параметры:
    /// * `filters` — Структура `NivelerFilterParams`, содержащая опциональные текстовые селекторы поиска,
    ///   а также метаданные пагинации: номер страницы (`page`) и лимит элементов (`per_page`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<PaginatedResponse<NivelerNodeDomain>, sqlx::Error>` — Унифицированный объект постраничного
    ///   ответа, готовый для сериализации в JSON и отправки на фронтенд.
    ///
    /// ### Технические особенности и оптимизация:
    /// * **Безопасность типов**: Входящие текстовые срезы принудительно приводятся к типу `Option<String>`
    ///   перед биндингом в аргументы макроса. Это защищает compile-time парсер `sqlx` от ошибок несовместимости схем.
    /// * **Защита от сбоев расчета**: Переменная `total_records` жестко кастится к базовому типу `i64`
    ///   через развертку контейнера, а счетчик страниц застрахован от деления на ноль при пустой выборке.
    pub async fn filter_niveler(
        &self,
        filters: NivelerFilterParams,
    ) -> Result<PaginatedResponse<NivelerNodeDomain>, sqlx::Error> {
        let limit = filters.per_page.unwrap_or(20).clamp(1, 100);
        let current_page = filters.page.unwrap_or(1).max(1);
        let offset = (current_page - 1) * limit;

        // Явно приводим поисковые параметры под стандарты ILIKE и Option<String> для sqlx
        let name_like: Option<String> = filters.name_query.map(|q| format!("%{}%", q.trim()));
        let region_like: Option<String> = filters.region_name.map(|q| format!("%{}%", q.trim()));

        // Выборка отфильтрованной страницы данных нивелиров
        let rows = sqlx::query_as!(
            NivelerNodeRow,
            r#"
            SELECT id, node_index, name, region_name, district_name, latitude_raw, longitude_raw
            FROM reference_geo.niveler_nodes
            WHERE ($1::text IS NULL OR name ILIKE $1)
              AND ($2::text IS NULL OR region_name ILIKE $2)
            ORDER BY name ASC
            LIMIT $3 OFFSET $4
            "#,
            name_like,   // $1 - Option<String>
            region_like, // $2 - Option<String>
            limit,       // $3 - i64
            offset       // $4 - i64
        )
        .fetch_all(self.pool)
        .await?;

        // Считаем общее количество записей по ТЕМ ЖЕ фильтрам
        // Явно принуждаем к типу i64 через unwrap_or(0) для защиты от капризов компилятора sqlx
        let total_records: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM reference_geo.niveler_nodes
            WHERE ($1::text IS NULL OR name ILIKE $1)
              AND ($2::text IS NULL OR region_name ILIKE $2)
            "#,
            name_like,
            region_like
        )
        .fetch_one(self.pool)
        .await?
        .unwrap_or(0);

        // Трансформируем плоские строки ответов СУБД в стерильные доменные модели
        let data = rows.into_iter().map(|r| r.into_domain()).collect();

        // Безопасный расчет страниц (избегаем деления на ноль при пустой выборке)
        let total_pages = if total_records == 0 {
            1
        } else {
            (total_records as f64 / limit as f64).ceil() as i64
        };

        Ok(PaginatedResponse {
            data,
            current_page,
            per_page: limit,
            total_records,
            total_pages,
        })
    }

    // --- EXPORT (Нативный GeoJSON FeatureCollection для нивелиров) ---

    /// **Экспортировать нивелирные реперы определенного региона в формат GeoJSON (RFC 7946)**
    ///
    /// Полностью формирует валидную ГИС-структуру `FeatureCollection` прямо на стороне СУБД PostgreSQL.
    /// Метод изолирует слой Rust от тяжелых аллокаций памяти и циклов сериализации/десериализации,
    /// выдавая готовую текстовую строку для мгновенной отправки клиенту или записи в `.geojson` файл.
    ///
    /// ### Параметры:
    /// * `region_name` — Наименование региона для фильтрации (поддерживает неполное совпадение через `ILIKE`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<String, sqlx::Error>` — Валидная GeoJSON-строка. Если по указанному региону реперов
    ///   не найдено, возвращает пустой каркас `{"type":"FeatureCollection","features":[]}`.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Разделение свойств и геометрии**: Функция `ST_AsGeoJSON(t.*, 'geometry')` берет объект строки `t`,
    ///   автоматически упаковывает поле `geometry` в GeoJSON-поле `geometry`, а атрибуты метаданных (`node_index`,
    ///   `district_name`) — в объект `properties` согласно спецификации RFC 7946.
    /// * **Гарантия типов для макроса**: Конструкция `AS "geojson_raw!"` сообщает макросу `sqlx`,
    ///   что результатом выполнения будет гарантированный `String`. Это полностью убирает ошибки компиляции типов.
    pub async fn export_niveler_to_geojson(
        &self,
        region_name: &str,
    ) -> Result<String, sqlx::Error> {
        // Запрос собирает валидный GeoJSON прямо на стороне СУБД
        // Алиас с восклицательным знаком "geojson_raw!" страхует sqlx от паники compile-time проверок
        let geojson_raw = sqlx::query_scalar!(
            r#"
            SELECT json_build_object(
                'type', 'FeatureCollection',
                'features', COALESCE(json_agg(ST_AsGeoJSON(t.*, 'geometry')::json), '[]'::json)
            )::text AS "geojson_raw!"
            FROM (
                SELECT id, name, node_index, district_name, geom_global AS geometry
                FROM reference_geo.niveler_nodes
                WHERE region_name ILIKE $1
            ) t;
            "#,
            region_name.trim()
        )
        .fetch_one(self.pool)
        .await?;

        // Подстраховываем логику на случай, если СУБД по какой-то причине вернула пустую строку
        if geojson_raw.is_empty() {
            return Ok(r#"{"type":"FeatureCollection","features":[]}"#.to_string());
        }

        Ok(geojson_raw)
    }
}

impl_json_stream!(
    GgsNodesRepository,
    NivelerNodeDomain,
    insert_niveler_batch,
    process_niveler_json_stream
);
impl GgsNodesRepository<'_> {
    // --- BULK INSERT (Пакетная вставка нивелиров для Стрима) ---

    /// **Выполнить пакетную вставку (батчинг) нивелирных реперов в рамках транзакции**
    ///
    /// Метод принимает срез данных (`slice`) и последовательно выполняет параметризованные
    /// инсерты, используя контекст уже открытой внешней транзакции. Идеально стыкуется
    /// с универсальным асинхронным движком потоковой обработки данных `DbStreamProcessor`.
    ///
    /// ### Параметры:
    /// * `tx` — Мутабельная ссылка на активную транзакцию СУБД (`&mut Transaction`).
    /// * `batch` — Срез доменных моделей `&[NivelerNodeDomain]`, подготовленный для текущей итерации записи.
    ///
    /// ### Технические особенности и PostGIS-валидация:
    /// * **Безопасность заимствований**: Мутабельный экзекутор передается как `&mut **tx`, что позволяет
    ///   повторно использовать соединение на каждой итерации цикла `for`, не теряя владение контекстом.
    /// * **ГИС-координаты**: Порядок осей в `ST_MakePoint($7, $6, 0.0)` строго соответствует формату:
    ///   `($7: Longitude, $6: Latitude, Z: 0.0)`.
    /// * **Подсказка для макроса**: Обе пространственные функции снабжены явным кастом к типу `::geometry`,
    ///   что гарантирует прохождение compile-time тестов схемы базы данных в `sqlx`.
    pub async fn insert_niveler_batch(
        tx: &mut Transaction<'_, Postgres>,
        batch: &[NivelerNodeDomain],
    ) -> Result<(), sqlx::Error> {
        for node in batch {
            sqlx::query!(
                r#"
                INSERT INTO reference_geo.niveler_nodes (
                    id, node_index, name, region_name, district_name, latitude_raw, longitude_raw, geom_global, geom_local
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    -- ИСПРАВЛЕНО: Добавлен явный каст ::geometry для стабильной сборки макроса sqlx
                    ST_SetSRID(ST_MakePoint($7, $6, 0.0), 4326)::geometry,
                    ST_Transform(ST_SetSRID(ST_MakePoint($7, $6, 0.0), 4326), 3857)::geometry
                )
                ON CONFLICT (id) DO NOTHING;
                "#,
                node.id,            // $1
                node.node_index,    // $2
                node.name,          // $3
                node.region_name,   // $4
                node.district_name, // $5
                node.latitude_raw,  // $6
                node.longitude_raw  // $7
            )
            .execute(&mut **tx) // Стабильное двойное разыменование
            .await?;
        }
        Ok(())
    }
}
