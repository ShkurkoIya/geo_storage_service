use super::GgsNodesRepository;
use crate::models::{GgsFilterParams, GgsNodeDomain, GgsNodeRow};
use geo_core::{
    impl_json_stream,
    infra::params::{BboxParams, PaginatedResponse, RadiusParams},
};
use sqlx::{Postgres, Transaction};

impl<'a> GgsNodesRepository<'a> {
    // --- READ (Получить пункт по ID) ---

    /// **Получить геодезический пункт ГГС по его уникальному идентификатору (ID)**
    ///
    /// Выполняет высокоэффективный точечный запрос к базе данных PostgreSQL в схеме `reference_geo`.
    /// Автоматически маппит низкоуровневую сырую строку БД (`GgsNodeRow`) в доменную модель бизнес-логики (`GgsNodeDomain`).
    ///
    /// ### Параметры:
    /// * `id` — Уникальный целочисленный системный идентификатор пункта (`i32`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Option<GgsNodeDomain>, sqlx::Error>` — В случае успеха возвращает:
    ///   * `Some(GgsNodeDomain)` — Если пункт с таким ID успешно найден.
    ///   * `None` — Если в базе данных нет записи с указанным ID.
    /// * Возвращает `Err(sqlx::Error)`, если произошел сбой сети, ошибка пула или синтаксиса СУБД.
    ///
    /// ### Технические особенности:
    /// Использует compile-time макрос проверки типов `sqlx::query_as!`. Все перечисленные
    /// поля таблицы `reference_geo.ggs_nodes` строго валидируются компилятором Rust на этапе
    /// сборки проекта на соответствие типам полей структуры `GgsNodeRow`.
    pub async fn find_ggs_by_id(&self, id: i32) -> Result<Option<GgsNodeDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
               GgsNodeRow,
               /* sql */
               r#"
               SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system,
                      height_determination_method, leveling_type, monument_mark, center_structure_type,
                      surface_monument_type, monument_height_m, region_code, region_name, district_name,
                      latitude_raw, longitude_raw
               FROM reference_geo.ggs_nodes WHERE id = $1
               "#,
               id
           )
           .fetch_optional(self.pool)
           .await?;

        Ok(row.map(|r| r.into_domain()))
    }

    // --- READ (Получить страницу пунктов с пагинацией) ---

    /// **Получить список геодезических пунктов ГГС с поддержкой постраничной пагинации**
    ///
    /// Метод осуществляет контролируемую выборку данных. Сначала вычисляется общее
    /// количество записей в таблице, после чего с помощью смещения (`OFFSET`) и лимита (`LIMIT`)
    /// извлекается целевой срез данных для текущей страницы.
    ///
    /// ### Параметры:
    /// * `page` — Номер запрашиваемой страницы (`i64`). Начинается с `1`.
    /// * `per_page` — Желаемое количество элементов на одной странице (`i64`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<PaginatedResponse<GgsNodeDomain>, sqlx::Error>` — В случае успеха возвращает
    ///   структурированный ответ `PaginatedResponse`, содержащий вектор доменных моделей пунктов,
    ///   текущую страницу, лимит, а также счетчики общего количества записей и страниц.
    ///
    /// ### Технические особенности и оптимизация:
    /// * Параметр `per_page` жестко ограничивается диапазоном от `1` до `100` через `.clamp()`,
    ///   защищая СУБД от перегрузки (DoS) слишком тяжелыми выборками.
    /// * Корректно преобразует типы данных для Postgres (`LIMIT` и `OFFSET` биндятся как `i64`).
    /// * Расчет `total_pages` защищен от деления на ноль на случай пустой таблицы.
    pub async fn find_ggs_paginated(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedResponse<GgsNodeDomain>, sqlx::Error> {
        // Ограничиваем аппетиты пользователей, чтобы защитить СУБД от перегрузки
        let limit = per_page.clamp(1, 100);
        let normalized_page = page.max(1);
        let offset = (normalized_page - 1) * limit;

        // 1. Получаем общее количество записей в таблице ГГС
        // В Postgres COUNT(*) гарантированно возвращает i64 (без Option), unwrap_or здесь не нужен
        let total_records = sqlx::query_scalar!("SELECT COUNT(*) FROM reference_geo.ggs_nodes")
            .fetch_one(self.pool)
            .await?
            .unwrap_or(0);

        // 2. Вытаскиваем конкретную страницу данных
        let rows = sqlx::query_as!(
            GgsNodeRow,
            r#"
            SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system,
                   height_determination_method, leveling_type, monument_mark, center_structure_type,
                   surface_monument_type, monument_height_m, region_code, region_name, district_name,
                   latitude_raw, longitude_raw
            FROM reference_geo.ggs_nodes
            ORDER BY id ASC
            LIMIT $1 OFFSET $2
            "#,
            limit,
            offset
        )
        .fetch_all(self.pool)
        .await?;

        // Трансформируем плоские строки БД в стерильные Доменные модели
        let data: Vec<GgsNodeDomain> = rows.into_iter().map(|r| r.into_domain()).collect();

        // Безопасно считаем общее количество страниц (избегаем деления на 0)
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

    // --- GEOSPATIAL (Радиусный поиск пунктов) ---

    /// **Найти геодезические пункты ГГС в заданном радиусе от географической точки**
    ///
    /// Выполняет пространственный поиск объектов (PostGIS) с использованием пространственного
    /// индекса по полю `geom_global`. Результаты автоматически сортируются по возрастанию
    /// расстояния (от ближайшего к самому дальнему).
    ///
    /// ### Параметры:
    /// * `params` — Ссылка на структуру `RadiusParams`, содержащую координаты центра
    ///   (`lon`, `lat`) и радиус поиска в метрах (`radius_m`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Vec<GgsNodeDomain>, sqlx::Error>` — В случае успеха возвращает вектор доменных
    ///   моделей пунктов, попавших в указанную окружность.
    ///
    /// ### Технические особенности и PostGIS-оптимизация:
    /// * Использует функцию `ST_DWithin` для эффективной фильтрации по индексу (работает
    ///   в разы быстрее, чем ручной расчет расстояния через `ST_Distance` в блоке `WHERE`).
    /// * Внутренний подзапрос `WITH center_point AS (...)` используется для того, чтобы
    ///   создать географическую точку центра один раз. Это исключает дублирование параметров `$1`, `$2`
    ///   в SQL и гарантирует, что макрос `sqlx` корректно свяжет типы аргументов Rust и Postgres.
    pub async fn find_ggs_within_radius(
        &self,
        params: &RadiusParams,
    ) -> Result<Vec<GgsNodeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
            GgsNodeRow,
            r#"
            WITH center_point AS (
                SELECT ST_SetSRID(ST_MakePoint($1, $2, 0.0), 4326)::geography AS geo
            )
            SELECT
                id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system,
                height_determination_method, leveling_type, monument_mark, center_structure_type,
                surface_monument_type, monument_height_m, region_code, region_name, district_name,
                latitude_raw, longitude_raw
            FROM reference_geo.ggs_nodes, center_point
            WHERE ST_DWithin(geom_global, center_point.geo, $3)
            ORDER BY ST_Distance(geom_global, center_point.geo) ASC
            "#,
            params.lon,      // $1 - f64
            params.lat,      // $2 - f64
            params.radius_m  // $3 - f64 (обычно double precision в СУБД для метров в geography)
        )
        .fetch_all(self.pool)
        .await?;

        // Трансформируем сырые строки из базы в чистые доменные бизнес-сущности
        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // --- GEOSPATIAL (Поиск пунктов внутри BBox) ---

    /// **Найти геодезические пункты ГГС внутри географического экстента (BBox)**
    ///
    /// Метод используется для динамической подгрузки геоданных при перемещении пользователем
    /// рамки экрана веб-карты или рабочей области в QGIS. Извлекает только те объекты, которые
    /// попадают в заданные границы координат.
    ///
    /// ### Параметры:
    /// * `params` — Ссылка на структуру `BboxParams`, содержащую минимальные и максимальные
    ///   координаты охвата: `min_lon` (запад), `min_lat` (юг), `max_lon` (восток), `max_lat` (север).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Vec<GgsNodeDomain>, sqlx::Error>` — В случае успеха возвращает вектор доменных
    ///   моделей пунктов, находящихся в границах переданного прямоугольника видимости.
    ///
    /// ### Технические особенности и PostGIS-оптимизация:
    /// * Использует нативный пространственный оператор пересечения ограничивающих рамок **`&&`**.
    ///   Это гарантирует максимальное быстродействие за счет прямого сканирования пространственного
    ///   индекса `GIST` / `SP-GIST`, развернутого на колонке `geom_global`.
    /// * Функция `ST_MakeEnvelope` строит валидный плоский полигон-конверт в системе координат SRID 4326.
    pub async fn find_ggs_in_bbox(
        &self,
        params: &BboxParams,
    ) -> Result<Vec<GgsNodeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
                GgsNodeRow,
                r#"
                SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system,
                       height_determination_method, leveling_type, monument_mark, center_structure_type,
                       surface_monument_type, monument_height_m, region_code, region_name, district_name,
                       latitude_raw, longitude_raw
                FROM reference_geo.ggs_nodes
                -- Используем оператор && для эффективной плоской фильтрации по 2D-индексу GIST
                WHERE geom_global && ST_MakeEnvelope($1, $2, $3, $4, 4326)
                "#,
                params.min_lon, // $1 - f64
                params.min_lat, // $2 - f64
                params.max_lon, // $3 - f64
                params.max_lat  // $4 - f64
            )
            .fetch_all(self.pool)
            .await?;

        // Трансформируем плоские строки БД в стерильные Доменные модели
        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // --- UPDATE (Обновить пункт ГГС) ---

    /// **Обновить данные существующего геодезического пункта ГГС**
    ///
    /// Перезаписывает все атрибуты пункта в базе данных по его уникальному `id`.
    /// На основе обновленных сырых координат (`latitude_raw`, `longitude_raw`) и высоты знака
    /// автоматически пересчитывает и обновляет пространственные PostGIS-поля на плоскости и в 3D.
    ///
    /// ### Параметры:
    /// * `node` — Ссылка на доменную модель `GgsNodeDomain`, содержащую новые валидные бизнес-данные.
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешного апдейта возвращает `Ok(())`.
    /// * Возвращает `Err(sqlx::Error)`, если запись с таким `id` заблокирована, отсутствует или произошел сбой СУБД.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Синхронизация координат**: Функции `ST_SetSRID` и `ST_MakePoint` пересобирают глобальную 3D-геометрию
    ///   в системе координат WGS 84 (SRID 4326), где аргументы передаются строго в ГИС-формате: `(Longitude, Latitude, Z)`.
    /// * **Локальная проекция**: Функция `ST_Transform` автоматически выполняет математическую проекцию точки
    ///   в метрическую систему Web Mercator (SRID 3857) для ускорения рендеринга на плоских веб-картах.
    /// * Явное приведение типов `::geometry` защищает компилятор Rust от двусмысленности типов при разборе макроса `sqlx`.
    pub async fn update_ggs(&self, node: &GgsNodeDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE reference_geo.ggs_nodes
            SET
                node_index = $1,
                name = $2,
                accuracy_class = $3,
                map_sheet_50k = $4,
                map_sheet_200k = $5,
                height_system = $6,
                height_determination_method = $7,
                leveling_type = $8,
                monument_mark = $9,
                center_structure_type = $10,
                surface_monument_type = $11,
                monument_height_m = $12,
                region_code = $13,
                region_name = $14,
                district_name = $15,
                latitude_raw = $16,
                longitude_raw = $17,
                -- Добавлено явное приведение к ::geometry, чтобы у sqlx не ехали типы при компиляции
                geom_global = ST_SetSRID(ST_MakePoint($17, $16, $12), 4326)::geometry,
                geom_local = ST_Transform(ST_SetSRID(ST_MakePoint($17, $16, $12), 4326), 3857)::geometry
            WHERE id = $18
            "#,
            node.node_index,                  // $1
            node.name,                        // $2
            node.accuracy_class,              // $3
            node.map_sheet_50k,               // $4
            node.map_sheet_200k,              // $5
            node.height_system,               // $6
            node.height_determination_method, // $7
            node.leveling_type,               // $8
            node.monument_mark,               // $9
            node.center_structure_type,       // $10
            node.surface_monument_type,       // $11
            node.monument_height_m,           // $12
            node.region_code,                 // $13
            node.region_name,                 // $14
            node.district_name,               // $15
            node.latitude_raw,                // $16
            node.longitude_raw,               // $17
            node.id                           // $18
        )
        .execute(self.pool)
        .await?;

        Ok(())
    }

    // --- DELETE (Удалить пункт по ID) ---

    /// **Удалить геодезический пункт ГГС из базы данных**
    ///
    /// Производит физическое удаление (`HARD DELETE`) записи из таблицы `reference_geo.ggs_nodes`
    /// по её уникальному идентификатору `id`.
    ///
    /// ### Параметры:
    /// * `id` — Уникальный целочисленный системный идентификатор удаляемого пункта (`i32`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешного удаления строки (или если строка с таким ID
    ///   и так не существовала) возвращает `Ok(())`.
    /// * Возвращает `Err(sqlx::Error)`, если запись заблокирована другой транзакцией или нарушены
    ///   внешние ключи (`Foreign Key Constraints`).
    ///
    /// ### Безопасность и каскадность:
    /// Перед использованием жесткого удаления убедись, что на таблицу `ggs_nodes` не завязаны
    /// другие аналитические сущности (например, логи измерений или пользовательские треки),
    /// иначе СУБД заблокирует операцию, выдав ошибку нарушения целостности связей.
    pub async fn delete_ggs(&self, id: i32) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM reference_geo.ggs_nodes WHERE id = $1", id)
            .execute(self.pool)
            .await?;

        Ok(())
    }

    // --- CREATE (Одиночная вставка одной ноды ГГС) ---

    /// **Добавить новый геодезический пункт ГГС в базу данных**
    ///
    /// Производит одиночную вставку записи напрямую из доменной модели `GgsNodeDomain`.
    /// На основе переданных плоских координат автоматически инициализирует пространственные 3D-объекты.
    /// Если запись с указанным `id` уже существует, операция будет проигнорирована без вызова ошибки.
    ///
    /// ### Параметры:
    /// * `node` — Ссылка на доменную модель `GgsNodeDomain`, содержащую атрибуты создаваемого пункта.
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешной вставки (или срабатывания конфликта усечения) возвращает `Ok(())`.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Порядок осей**: Функция `ST_MakePoint` принимает аргументы строго в формате `(Долгота / $18, Широта / $17, Высота / $13)`. При нарушении порядка геоданные улетят в другую полусферу.
    /// * **Пространственные индексы**: `geom_global` сохраняется в исходной WGS 84 (SRID 4326), а `geom_local` проецируется в Web Mercator (SRID 3857) для мгновенного веб-маппинга.
    /// * **Стабильность макроса**: Конструкции функций снабжены явным кастом `::geometry`, что гарантирует прохождение compile-time проверок `sqlx`.
    /// * **Идемпотентность**: Конструкция `ON CONFLICT (id) DO NOTHING` делает метод безопасным при повторных вызовах с одними и теми же ID.
    pub async fn insert_ggs_node(&self, node: &GgsNodeDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO reference_geo.ggs_nodes (
                id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system, height_determination_method,
                leveling_type, monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                district_name, latitude_raw, longitude_raw, geom_global, geom_local
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
                ST_SetSRID(ST_MakePoint($18, $17, $13), 4326)::geometry,
                ST_Transform(ST_SetSRID(ST_MakePoint($18, $17, $13), 4326), 3857)::geometry
            )
            ON CONFLICT (id) DO NOTHING;
            "#,
            node.id,                           // $1
            node.node_index,                   // $2
            node.name,                         // $3
            node.accuracy_class,               // $4
            node.map_sheet_50k,                // $5
            node.map_sheet_200k,               // $6
            node.height_system,                // $7
            node.height_determination_method,  // $8
            node.leveling_type,                // $9
            node.monument_mark,                // $10
            node.center_structure_type,        // $11
            node.surface_monument_type,        // $12
            node.monument_height_m,            // $13
            node.region_code,                  // $14
            node.region_name,                  // $15
            node.district_name,                // $16
            node.latitude_raw,                 // $17
            node.longitude_raw                 // $18
        )
        .execute(self.pool) // Работает напрямую с пулом базы без транзакций
        .await?;

        Ok(())
    }

    // --- FILTER (Поиск с динамической фильтрацией и пагинацией) ---

    /// **Выполнить гибкий поиск пунктов ГГС с фильтрацией и пагинацией**
    ///
    /// Метод реализует серверную фильтрацию по текстовому совпадению (регистронезависимо через `ILIKE`)
    /// названия пункта, номенклатурного листа масштаба 1:50 000 и наименования региона.
    /// Параметры фильтрации являются опциональными: если поле передано как `None`, СУБД проигнорирует его.
    ///
    /// ### Параметры:
    /// * `filters` — Структура `GgsFilterParams`, содержащая опциональные поля `name_query`,
    ///   `map_sheet_50k`, `region_name`, а также параметры пагинации `page` и `per_page`.
    ///
    /// ### Возвращаемое значение:
    /// * `Result<PaginatedResponse<GgsNodeDomain>, sqlx::Error>` — Объект постраничного ответа,
    ///   содержащий отфильтрованный вектор доменных моделей и метаданные пагинации.
    ///
    /// ### Технические особенности реализации:
    /// * **Динамический SQL без конкатенации**: Конструкция `($1::text IS NULL OR column ILIKE $1)`
    ///   позволяет использовать преимущества compile-time проверок `sqlx` без написания сырого построителя строк.
    /// * **Подготовка параметров**: Все текстовые фильтры перед отправкой в макрос приводятся к типу
    ///   `Option<String>`, что гарантирует корректную валидацию типов на стороне драйвера Postgres.
    pub async fn filter_ggs(
        &self,
        filters: GgsFilterParams,
    ) -> Result<PaginatedResponse<GgsNodeDomain>, sqlx::Error> {
        let limit = filters.per_page.unwrap_or(20).clamp(1, 100);
        let current_page = filters.page.unwrap_or(1).max(1);
        let offset = (current_page - 1) * limit;

        // Нормализуем текстовый запрос для ILIKE. Приводим строго к Option<String>
        let name_like = filters.name_query.map(|q| format!("%{}%", q.trim()));
        let map_sheet = filters.map_sheet_50k.map(|s| s.trim().to_string());
        let region_like = filters.region_name.map(|r| format!("%{}%", r.trim()));

        // 1. Запрос данных с динамическими фильтрами
        let rows = sqlx::query_as!(
                GgsNodeRow,
                r#"
                SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system,
                       height_determination_method, leveling_type, monument_mark, center_structure_type,
                       surface_monument_type, monument_height_m, region_code, region_name, district_name,
                       latitude_raw, longitude_raw
                FROM reference_geo.ggs_nodes
                WHERE ($1::text IS NULL OR name ILIKE $1)
                  AND ($2::text IS NULL OR map_sheet_50k = $2)
                  AND ($3::text IS NULL OR region_name ILIKE $3)
                ORDER BY name ASC
                LIMIT $4 OFFSET $5
                "#,
                name_like,   // $1 - Option<String>
                map_sheet,   // $2 - Option<String>
                region_like, // $3 - Option<String>
                limit,       // $4 - i64
                offset       // $5 - i64
            )
            .fetch_all(self.pool)
            .await?;

        // 2. Считаем общее количество записей, удовлетворяющих ТЕМ ЖЕ фильтрам
        // COUNT(*) возвращает i64 напрямую, без Option контейнера
        let total_records: i64 = sqlx::query_scalar!(
            r#"
                SELECT COUNT(*)
                FROM reference_geo.ggs_nodes
                WHERE ($1::text IS NULL OR name ILIKE $1)
                  AND ($2::text IS NULL OR map_sheet_50k = $2)
                  AND ($3::text IS NULL OR region_name ILIKE $3)
                "#,
            name_like,
            map_sheet,
            region_like
        )
        .fetch_one(self.pool)
        .await?
        .unwrap_or(0);

        // Маппим плоские строки БД в доменную бизнес-логику
        let data: Vec<GgsNodeDomain> = rows.into_iter().map(|r| r.into_domain()).collect();

        // Безопасный расчет страниц
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

    // --- EXPORT (Экспорт данных в форматы ГИС) ---

    /// **Экспортировать пункты ГГС определенного номенклатурного листа в формат GeoJSON (RFC 7946)**
    ///
    /// Полностью формирует валидную структуру `FeatureCollection` на стороне СУБД PostgreSQL.
    /// Это исключает затраты на аллокацию памяти и десериализацию/сериализацию объектов внутри Rust.
    /// Идеально подходит для скачивания файлов ГИС-специалистами и интеграции с QGIS / Leaflet.
    ///
    /// ### Параметры:
    /// * `map_sheet_50k` — Номенклатурное имя листа масштаба 1:50 000 (например, "N-37-001").
    ///
    /// ### Возвращаемое значение:
    /// * `Result<String, sqlx::Error>` — Строка в формате GeoJSON, готовая к отправке клиенту или записи в файл.
    ///   Если по указанному листу пунктов не найдено, возвращается пустой каркас `{"type":"FeatureCollection","features":[]}`.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Атрибутивный маппинг**: Функция `ST_AsGeoJSON(t.*, 'geometry')` берет всю виртуальную строку `t`,
    ///   автоматически помещает поле `geometry` в секцию GeoJSON `geometry`, а все остальные поля (id, name и т.д.) — в секцию `properties`.
    /// * **Предотвращение паники макроса**: Конструкция `SELECT ...::text AS "geojson_raw!"` принудительно указывает
    ///   макросу `sqlx`, что на выходе будет гарантированная, не-NULL строка типа `String`.
    pub async fn export_ggs_to_geojson(&self, map_sheet_50k: &str) -> Result<String, sqlx::Error> {
        // Запрос собирает валидный GeoJSON прямо на стороне СУБД
        // Добавлен алиас с восклицательным знаком "geojson_raw!", чтобы sqlx точно знал, что поле не Null и имеет тип String
        let geojson_raw = sqlx::query_scalar!(
            r#"
                SELECT json_build_object(
                    'type', 'FeatureCollection',
                    'features', COALESCE(json_agg(ST_AsGeoJSON(t.*, 'geometry')::json), '[]'::json)
                )::text AS "geojson_raw!"
                FROM (
                    SELECT
                        id, name, node_index, accuracy_class, monument_height_m, region_name,
                        geom_global AS geometry
                    FROM reference_geo.ggs_nodes
                    WHERE map_sheet_50k = $1
                ) t;
                "#,
            map_sheet_50k.trim()
        )
        .fetch_one(self.pool)
        .await?;

        // Если в выборке вообще не было строк, Postgres из-за агрегатора json_build_object
        // все равно вернет строку с пустым массивом фичей, но подстрахуем логику на случай неожиданностей
        if geojson_raw.is_empty() {
            return Ok(r#"{"type":"FeatureCollection","features":[]}"#.to_string());
        }

        Ok(geojson_raw)
    }
}

impl_json_stream!(
    GgsNodesRepository,
    GgsNodeDomain,
    insert_ggs_batch,
    process_ggs_json_stream
);
impl GgsNodesRepository<'_> {
    // --- BULK INSERT (Пакетная вставка для Стрима) ---

    /// **Выполнить пакетную вставку (батчинг) пунктов ГГС в рамках транзакции**
    ///
    /// Метод принимает срез данных (`slice`) и последовательно выполняет параметризованные
    /// инсерты, используя контекст уже открытой внешней транзакции. Идеально интегрируется
    /// с универсальным движком потоковой обработки данных `DbStreamProcessor`.
    ///
    /// ### Параметры:
    /// * `tx` — Эксклюзивная мутабельная ссылка на активную транзакцию СУБД (`&mut Transaction`).
    /// * `batch` — Срез доменных моделей `&[GgsNodeDomain]`, подготовленный для текущей итерации вставки.
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешной записи всех элементов пачки возвращает `Ok(())`.
    /// * При сбое любого инсерта выполнение прерывается, возвращая ошибку, что приводит к автоматическому `ROLLBACK` всей пачки.
    ///
    /// ### Технические особенности:
    /// * **Безопасность транзакций**: Конструкция `&mut ***tx` (или явный заимствованный экзекутор `&mut **tx`)
    ///   необходима макросу `sqlx` для повторного использования соединения внутри цикла без потери владения.
    /// * **PostGIS Валидация**: Добавлено явное приведение типов `::geometry` к пространственным функциям
    ///   генерации геометрий, предотвращая падение компилятора из-за неопределенности типов в `sqlx::query!`.
    pub async fn insert_ggs_batch(
        tx: &mut Transaction<'_, Postgres>,
        batch: &[GgsNodeDomain],
    ) -> Result<(), sqlx::Error> {
        for node in batch {
            sqlx::query!(
                   r#"
                   INSERT INTO reference_geo.ggs_nodes (
                       id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_system, height_determination_method,
                       leveling_type, monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                       district_name, latitude_raw, longitude_raw, geom_global, geom_local
                   )
                   VALUES (
                       $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
                       ST_SetSRID(ST_MakePoint($18, $17, $13), 4326)::geometry,
                       ST_Transform(ST_SetSRID(ST_MakePoint($18, $17, $13), 4326), 3857)::geometry
                   )
                   ON CONFLICT (id) DO NOTHING;
                   "#,
                   node.id,                           // $1
                   node.node_index,                   // $2
                   node.name,                         // $3
                   node.accuracy_class,               // $4
                   node.map_sheet_50k,                // $5
                   node.map_sheet_200k,               // $6
                   node.height_system,                // $7
                   node.height_determination_method,  // $8
                   node.leveling_type,                // $9
                   node.monument_mark,                // $10
                   node.center_structure_type,        // $11
                   node.surface_monument_type,        // $12
                   node.monument_height_m,            // $13
                   node.region_code,                  // $14
                   node.region_name,                  // $15
                   node.district_name,                // $16
                   node.latitude_raw,                 // $17
                   node.longitude_raw                 // $18
               )
               .execute(&mut **tx)
               .await?;
        }
        Ok(())
    }
}
