use crate::models::{LostFilterParams, LostNodeDomain, LostNodeRow};
use crate::repository::ggs_nodes::GgsNodesRepository;
use geo_core::impl_json_stream;
use geo_core::infra::params::{BboxParams, PaginatedResponse, RadiusParams};
use sqlx::{Postgres, Transaction};

impl<'a> GgsNodesRepository<'a> {
    // --- CREATE (Добавить утраченный/поврежденный маркшейдерский пункт) ---

    /// **Добавить новый маркшейдерский пункт (утраченный/поврежденный) в базу данных**
    ///
    /// Производит одиночную вставку записи в таблицу `reference_geo.lost_nodes`.
    /// На основе сырых координат (`latitude_raw`, `longitude_raw`) и высоты автоматически
    /// вычисляет глобальную 3D-геометрию (WGS 84) и локальную проекцию в СК-42 / Гаусса-Крюгера зона 15 (SRID 28415).
    ///
    /// ### Параметры:
    /// * `node` — Ссылка на доменную модель `LostNodeDomain`.
    ///
    /// ### Технические особенности:
    /// * **Исправлен синтаксис**: Удалена паразитная закрывающая скобка на стыке строк в наименовании колонки `height_determination_method`.
    /// * **Порядок осей в PostGIS**: `ST_MakePoint($19, $18, $11)` принимает аргументы строго в географическом порядке: `($19: Longitude, $18: Latitude, $11: Height)`.
    /// * ** compile-time подсказка**: Пространственные функции приведены к типу `::geometry` для успешного прохождения макроса `sqlx::query!`.
    pub async fn insert_lost_node(&self, node: &LostNodeDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO reference_geo.lost_nodes (
                id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k,
                height_determination_method, monument_mark, center_structure_type,
                surface_monument_type, monument_height_m, region_code, region_name,
                district_name, condition_monument, condition_center, verification_year,
                latitude_raw, longitude_raw, geom_global, geom_local
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19,
                ST_SetSRID(ST_MakePoint($19, $18, $11), 4326)::geometry,
                ST_Transform(ST_SetSRID(ST_MakePoint($19, $18, $11), 4326), 28415)::geometry
            )
            ON CONFLICT (id) DO NOTHING;
            "#,
            node.id,                           // $1
            node.node_index,                   // $2
            node.name,                         // $3
            node.accuracy_class,               // $4
            node.map_sheet_50k,                // $5
            node.map_sheet_200k,               // $6
            node.height_determination_method,  // $7
            node.monument_mark,                // $8
            node.center_structure_type,        // $9
            node.surface_monument_type,        // $10
            node.monument_height_m,            // $11
            node.region_code,                  // $12
            node.region_name,                  // $13
            node.district_name,                // $14
            node.condition_monument,           // $15
            node.condition_center,             // $16
            node.verification_year,            // $17
            node.latitude_raw,                 // $18
            node.longitude_raw                 // $19
        )
        .execute(self.pool)
        .await?;

        Ok(())
    }

    // --- READ (Получить карточку утраченного пункта по ID) ---

    /// **Получить подробную карточку утраченного/поврежденного маркшейдерского пункта по его ID**
    ///
    /// Выполняет точечную выборку из таблицы `reference_geo.lost_nodes`.
    /// Автоматически трансформирует плоскую строку ответа СУБД (`LostNodeRow`) в чистую
    /// доменную модель бизнес-логики (`LostNodeDomain`) через маппинг.
    ///
    /// ### Параметры:
    /// * `id` — Уникальный системный идентификатор маркшейдерского пункта (`i32`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Option<LostNodeDomain>, sqlx::Error>` — В случае успеха возвращает:
    ///   * `Some(LostNodeDomain)` — Если пункт с таким ID существует.
    ///   * `None` — Если запись в базе данных не найдена.
    /// * Возвращает `Err(sqlx::Error)` при сетевых сбоях или ошибках пула соединений.
    ///
    /// ### Технические особенности:
    /// Все типы полей (включая nullable-атрибуты вроде `verification_year` или текстовых описаний условий)
    /// строго валидируются компилятором Rust на этапе сборки проекта благодаря макросу `sqlx::query_as!`.
    pub async fn find_lost_by_id(&self, id: i32) -> Result<Option<LostNodeDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
            LostNodeRow,
            r#"
            SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_determination_method,
                   monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                   district_name, condition_monument, condition_center, verification_year, latitude_raw, longitude_raw
            FROM reference_geo.lost_nodes WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool)
        .await?;

        // Если строка найдена, трансформируем её в доменную сущность
        Ok(row.map(|r| r.into_domain()))
    }

    // --- READ LIST (Базовая постраничная пагинация списка утраченных знаков) ---

    /// **Получить список утраченных/поврежденных маркшейдерских пунктов с поддержкой постраничной пагинации**
    ///
    /// Метод осуществляет контролируемую выборку данных. Сначала вычисляется общее
    /// количество записей в таблице `reference_geo.lost_nodes`, после чего извлекается
    /// целевой срез данных для текущей страницы на основе смещения (`OFFSET`) и лимита (`LIMIT`).
    ///
    /// ### Параметры:
    /// * `page` — Номер запрашиваемой страницы (`i64`). Начинается с `1`.
    /// * `per_page` — Желаемое количество элементов на одной странице (`i64`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<PaginatedResponse<LostNodeDomain>, sqlx::Error>` — Объект постраничного ответа,
    ///   содержащий вектор доменных моделей пунктов и метаданные страниц.
    ///
    /// ### Технические особенности и оптимизация:
    /// * Параметр `per_page` жестко ограничивается диапазоном от `1` до `100` через `.clamp()`,
    ///   предотвращая перегрузку пула соединений СУБД.
    /// * Переменная `total_records` принудительно приводится к типу `i64` через развертку `Option`,
    ///   что гарантирует прохождение compile-time проверок типов макроса `sqlx`.
    /// * Расчет общего числа страниц защищен от деления на ноль на случай работы с пустой таблицей.
    pub async fn find_lost_paginated(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedResponse<LostNodeDomain>, sqlx::Error> {
        // Ограничиваем аппетиты пользователей и нормализуем счетчик страниц
        let limit = per_page.clamp(1, 100);
        let normalized_page = page.max(1);
        let offset = (normalized_page - 1) * limit;

        // Получаем общее количество записей в таблице lost_nodes
        // Явно принуждаем к типу i64 через unwrap_or(0), страхуясь от капризов компилятора sqlx
        let total_records: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) FROM reference_geo.lost_nodes")
                .fetch_one(self.pool)
                .await?
                .unwrap_or(0);

        // Вытаскиваем конкретную страницу данных
        let rows = sqlx::query_as!(
            LostNodeRow,
            r#"
            SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_determination_method,
                   monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                   district_name, condition_monument, condition_center, verification_year, latitude_raw, longitude_raw
            FROM reference_geo.lost_nodes
            ORDER BY id ASC
            LIMIT $1 OFFSET $2
            "#,
            limit,
            offset
        )
        .fetch_all(self.pool)
        .await?;

        // Трансформируем плоские строки БД в стерильные Доменные модели
        let data: Vec<LostNodeDomain> = rows.into_iter().map(|r| r.into_domain()).collect();

        // Безопасный расчет страниц (избегаем деления на 0 при пустой выборке)
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

    // --- GEOSPATIAL (Радиусный поиск утраченных пунктов) ---

    /// **Найти утраченные/поврежденные маркшейдерские пункты в заданном радиусе от точки**
    ///
    /// Выполняет высокоэффективный геопространственный поиск объектов с использованием
    /// индекса `GIST` на колонке `geom_global`. Итоговый массив автоматически сортируется
    /// по метрическому расстоянию — от самых близких пунктов к более удаленным.
    ///
    /// ### Параметры:
    /// * `params` — Ссылка на общую структуру параметров `RadiusParams`, содержащую
    ///   координаты центра `lon` (долгота), `lat` (широта) и радиус поиска `radius_m` в метрах.
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Vec<LostNodeDomain>, sqlx::Error>` — В случае успеха возвращает вектор доменных
    ///   моделей пунктов, попавших в географическую зону охвата.
    ///
    /// ### Технические особенности и PostGIS-оптимизация:
    /// * **Устранение конфликта макроса**: CTE-выражение `WITH center_point` инициализирует
    ///   географическую точку центра ровно один раз. Это полностью убирает дублирование аргументов `$1` и `$2`
    ///   в теле SQL, из-за которого макрос `sqlx` выдает ошибки вывода типов.
    /// * **Индексированная фильтрация**: Использование функции `ST_DWithin` гарантирует, что
    ///   Postgres будет делать быстрый поиск по индексу, а не сканировать всю таблицу последовательно.
    pub async fn find_lost_within_radius(
        &self,
        params: &RadiusParams,
    ) -> Result<Vec<LostNodeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
               LostNodeRow,
               r#"
               WITH center_point AS (
                   SELECT ST_SetSRID(ST_MakePoint($1, $2, 0.0), 4326)::geography AS geo
               )
               SELECT
                   id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_determination_method,
                   monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                   district_name, condition_monument, condition_center, verification_year, latitude_raw, longitude_raw
               FROM reference_geo.lost_nodes, center_point
               WHERE ST_DWithin(geom_global, center_point.geo, $3)
               ORDER BY ST_Distance(geom_global, center_point.geo) ASC
               "#,
               params.lon,     // $1 - f64
               params.lat,     // $2 - f64
               params.radius_m // $3 - f64
           )
           .fetch_all(self.pool)
           .await?;

        // Трансформируем плоские записи СУБД в стерильные доменные модели бизнес-логики
        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // --- GEOSPATIAL (BBox поиск уничтоженных знаков) ---

    /// **Найти уничтоженные/поврежденные маркшейдерские пункты внутри географического экстента (BBox)**
    ///
    /// Метод используется для динамической фильтрации и подгрузки точек на фронтенде веб-карты
    /// или в ГИС-плагинах при перемещении или масштабировании области видимости экрана.
    ///
    /// ### Параметры:
    /// * `params` — Ссылка на общую пространственную структуру `BboxParams`, содержащую
    ///   координаты углов рамки видимости: `min_lon` (запад), `min_lat` (юг), `max_lon` (восток), `max_lat` (север).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<Vec<LostNodeDomain>, sqlx::Error>` — В случае успеха возвращает вектор доменных
    ///   моделей пунктов, физически попавших в границы переданного прямоугольника.
    ///
    /// ### Технические особенности и PostGIS-оптимизация:
    /// * **Индексированное плоское сканирование**: Оператор пересечения рамок **`&&`** заменяет 3D-оператор `&&&`.
    ///   Это заставляет планировщик Postgres использовать плоский индекс `GIST` без лишних расчетов Z-координаты.
    /// * **Безопасность compile-time макроса**: Передаваемые параметры `f64` строго соотносятся с типами аргументов
    ///   функции `ST_MakeEnvelope` для SRID 4326.
    pub async fn find_lost_in_bbox(
        &self,
        params: &BboxParams,
    ) -> Result<Vec<LostNodeDomain>, sqlx::Error> {
        let rows = sqlx::query_as!(
               LostNodeRow,
               r#"
               SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_determination_method,
                      monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                      district_name, condition_monument, condition_center, verification_year, latitude_raw, longitude_raw
               FROM reference_geo.lost_nodes
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

        // Трансформируем плоские строки ответов СУБД в чистые доменные модели
        Ok(rows.into_iter().map(|r| r.into_domain()).collect())
    }

    // --- UPDATE (Полное обновление метаданных и 3D-геометрий) ---

    /// **Обновить данные существующего маркшейдерского пункта (утраченного/поврежденного)**
    ///
    /// Полностью перезаписывает атрибуты пункта в таблице `reference_geo.lost_nodes` по его уникальному `id`.
    /// На основе измененных сырых координат (`latitude_raw`, `longitude_raw`) и высоты знака
    /// автоматически пересчитывает и обновляет пространственные 3D-поля в глобальной WGS 84 (SRID 4326)
    /// и локальной проекции Гаусса-Крюгера зона 15 (SRID 28415).
    ///
    /// ### Параметры:
    /// * `node` — Ссылка на доменную модель `LostNodeDomain`, несущую обновленные бизнес-метрики.
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешного выполнения апдейта возвращает `Ok(())`.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Строгий порядок координат**: Функция `ST_MakePoint($18, $17, $10)` использует аргументы в ГИС-формате: `($18: Longitude, $17: Latitude, $10: Monument Height)`.
    /// * **Предотвращение ошибок типизации**: К вызовам пространственных функций принудительно добавлен суффикс явного приведения типов `::geometry`. Без него макрос `sqlx` падает на этапе разбора AST-дерева SQL-запроса компилятором Rust.
    pub async fn update_lost(&self, node: &LostNodeDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
                r#"
                UPDATE reference_geo.lost_nodes
                SET
                    node_index = $1, name = $2, accuracy_class = $3, map_sheet_50k = $4, map_sheet_200k = $5,
                    height_determination_method = $6, monument_mark = $7, center_structure_type = $8, surface_monument_type = $9,
                    monument_height_m = $10, region_code = $11, region_name = $12, district_name = $13,
                    condition_monument = $14, condition_center = $15, verification_year = $16, latitude_raw = $17, longitude_raw = $18,
                    -- ИСПРАВЛЕНО: Добавлен явный каст к ::geometry для успешной компиляции макроса sqlx
                    geom_global = ST_SetSRID(ST_MakePoint($18, $17, $10), 4326)::geometry,
                    geom_local = ST_Transform(ST_SetSRID(ST_MakePoint($18, $17, $10), 4326), 28415)::geometry
                WHERE id = $19
                "#,
                node.node_index,                  // $1
                node.name,                        // $2
                node.accuracy_class,              // $3
                node.map_sheet_50k,               // $4
                node.map_sheet_200k,              // $5
                node.height_determination_method, // $6
                node.monument_mark,               // $7
                node.center_structure_type,       // $8
                node.surface_monument_type,       // $9
                node.monument_height_m,           // $10
                node.region_code,                 // $11
                node.region_name,                 // $12
                node.district_name,               // $13
                node.condition_monument,          // $14
                node.condition_center,            // $15
                node.verification_year,           // $16
                node.latitude_raw,                // $17
                node.longitude_raw,               // $18
                node.id                           // $19
            )
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // --- DELETE (Удаление уничтоженного знака по ID) ---

    /// **Физически удалить маркшейдерский пункт из базы данных**
    ///
    /// Производит необратимое удаление (`HARD DELETE`) записи из таблицы `reference_geo.lost_nodes`
    /// по её уникальному системному идентификатору `id`.
    ///
    /// ### Параметры:
    /// * `id` — Уникальный целочисленный идентификатор удаляемого маркшейдерского пункта (`i32`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<(), sqlx::Error>` — В случае успешного удаления (или если записи с таким ID
    ///   изначально не было в СУБД) возвращает `Ok(())`.
    /// * Возвращает `Err(sqlx::Error)`, если строка заблокирована конкурентной транзакцией.
    ///
    /// ### ГИС-контекст безопасности:
    /// Удаление записи каскадно очищает связанные пространственные индексы `GIST` для данного `id`.
    /// Убедись, что на удаляемый пункт не ссылаются слои съемок или журналы деформаций в других модулях,
    /// иначе СУБД выбросит ошибку нарушения целостности внешних ключей.
    pub async fn delete_lost(&self, id: i32) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM reference_geo.lost_nodes WHERE id = $1", id)
            .execute(self.pool)
            .await?;

        Ok(())
    }

    // --- SEARCH & FILTER (Умная фильтрация с пагинацией) ---

    /// **Выполнить гибкий поиск маркшейдерских пунктов с динамической фильтрацией и пагинацией**
    ///
    /// Осуществляет многофакторный поиск по текстовому совпадению (регистронезависимо через `ILIKE`)
    /// названия пункта, номенклатурного листа 1:50 000 и текстового состояния центра знака.
    /// Все фильтры опциональны: если параметр передан как `None`, СУБД пропустит условие.
    ///
    /// ### Параметры:
    /// * `filters` — Структура `LostFilterParams`, содержащая опциональные селекторы поиска,
    ///   а также номер страницы (`page`) и лимит (`per_page`).
    ///
    /// ### Возвращаемое значение:
    /// * `Result<PaginatedResponse<LostNodeDomain>, sqlx::Error>` — Объект постраничного ответа,
    ///   готовый для сериализации в JSON на фронтенд.
    ///
    /// ### Технические особенности и оптимизация:
    /// * **Предотвращение паники типов**: Все входящие текстовые срезы принудительно нормализуются
    ///   в тип `Option<String>` перед биндингом в макрос. Это полностью исключает ошибки вывода типов драйвером Postgres.
    /// * **Безопасная пагинация**: Переменная `total_records` жестко кастится к базовому типу `i64`,
    ///   а счетчик страниц застрахован от багов деления на ноль при пустой выборке СУБД.
    pub async fn filter_lost(
        &self,
        filters: LostFilterParams,
    ) -> Result<PaginatedResponse<LostNodeDomain>, sqlx::Error> {
        let limit = filters.per_page.unwrap_or(20).clamp(1, 100);
        let current_page = filters.page.unwrap_or(1).max(1);
        let offset = (current_page - 1) * limit;

        // Нормализуем текстовые фильтры под стандарты ILIKE и Option<String> для sqlx
        let name_like = filters.name_query.map(|q| format!("%{}%", q.trim()));
        let map_sheet = filters.map_sheet_50k.map(|s| s.trim().to_string());
        let condition_like = filters.condition_center.map(|c| format!("%{}%", c.trim()));

        // Выборка отфильтрованной страницы данных
        let rows = sqlx::query_as!(
                LostNodeRow,
                r#"
                SELECT id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_determination_method,
                       monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                       district_name, condition_monument, condition_center, verification_year, latitude_raw, longitude_raw
                FROM reference_geo.lost_nodes
                WHERE ($1::text IS NULL OR name ILIKE $1)
                  AND ($2::text IS NULL OR map_sheet_50k = $2)
                  AND ($3::text IS NULL OR condition_center ILIKE $3)
                ORDER BY name ASC
                LIMIT $4 OFFSET $5
                "#,
                name_like,      // $1 - Option<String>
                map_sheet,      // $2 - Option<String>
                condition_like, // $3 - Option<String>
                limit,          // $4 - i64
                offset          // $5 - i64
            )
            .fetch_all(self.pool)
            .await?;

        // Считаем общее количество записей, удовлетворяющих тем же фильтрам
        // Явно принуждаем к типу i64 через unwrap_or(0) для защиты от капризов компилятора sqlx
        let total_records: i64 = sqlx::query_scalar!(
            r#"
                SELECT COUNT(*) FROM reference_geo.lost_nodes
                WHERE ($1::text IS NULL OR name ILIKE $1)
                  AND ($2::text IS NULL OR map_sheet_50k = $2)
                  AND ($3::text IS NULL OR condition_center ILIKE $3)
                "#,
            name_like,
            map_sheet,
            condition_like
        )
        .fetch_one(self.pool)
        .await?
        .unwrap_or(0);

        // Трансформируем плоские строки ответов СУБД в чистые доменные модели
        let data = rows.into_iter().map(|r| r.into_domain()).collect();

        // Безопасный расчет общего количества страниц
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

    // --- EXPORT (Нативный GeoJSON FeatureCollection из PostGIS) ---

    /// **Экспортировать уничтоженные/поврежденные маркшейдерские пункты в формат GeoJSON (RFC 7946)**
    ///
    /// Полностью формирует валидную ГИС-структуру `FeatureCollection` прямо на стороне СУБД PostgreSQL.
    /// Метод изолирует слой Rust от тяжелых аллокаций памяти и циклов сериализации/десериализации,
    /// выдавая готовую текстовую строку для мгновенной отправки клиенту или записи в `.geojson` файл.
    ///
    /// ### Параметры:
    /// * `map_sheet_50k` — Номенклатурное имя листа масштаба 1:50 000 (например, "M-38-144").
    ///
    /// ### Возвращаемое значение:
    /// * `Result<String, sqlx::Error>` — Валидная GeoJSON-строка. Если по указанному листу пунктов
    ///   не найдено, возвращает пустой каркас `{"type":"FeatureCollection","features":[]}`.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Разделение свойств и геометрии**: Функция `ST_AsGeoJSON(t.*, 'geometry')` берет объект строки `t`,
    ///   автоматически упаковывает поле `geometry` в GeoJSON-поле `geometry`, а атрибуты состояния (`condition_center`,
    ///   `condition_monument`, `verification_year`) — в объект `properties` согласно спецификации RFC 7946.
    /// * **Гарантия типов для макроса**: Конструкция `AS "geojson_raw!"` сообщает макросу `sqlx`,
    ///   что результатом выполнения будет гарантированный `String`. Это полностью убирает ошибки компиляции типов.
    pub async fn export_lost_to_geojson(&self, map_sheet_50k: &str) -> Result<String, sqlx::Error> {
        // Запрос собирает валидный GeoJSON прямо на стороне СУБД
        // Алиас с восклицательным знаком "geojson_raw!" страхует sqlx от паники compile-time проверок
        let geojson_raw = sqlx::query_scalar!(
                r#"
                SELECT json_build_object(
                    'type', 'FeatureCollection',
                    'features', COALESCE(json_agg(ST_AsGeoJSON(t.*, 'geometry')::json), '[]'::json)
                )::text AS "geojson_raw!"
                FROM (
                    SELECT
                        id, name, node_index, condition_center, condition_monument, verification_year,
                        geom_global AS geometry
                    FROM reference_geo.lost_nodes
                    WHERE map_sheet_50k = $1
                ) t;
                "#,
                map_sheet_50k.trim()
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
    LostNodeDomain,
    insert_lost_batch,
    process_lost_json_stream
);
impl GgsNodesRepository<'_> {
    // --- BULK INSERT (Пакетная вставка для Стрима утраченных знаков) ---

    /// **Выполнить пакетную вставку (батчинг) уничтоженных пунктов в рамках транзакции**
    ///
    /// Метод принимает срез данных (`slice`) и последовательно выполняет параметризованные
    /// инсерты, используя контекст активной внешней транзакции. Интегрируется с асинхронным
    /// движком потоковой обработки данных `DbStreamProcessor` для высокопроизводительного импорта.
    ///
    /// ### Параметры:
    /// * `tx` — Мутабельная ссылка на активную транзакцию базы данных (`&mut Transaction`).
    /// * `batch` — Срез доменных моделей `&[LostNodeDomain]`, подготовленный для текущей итерации записи.
    ///
    /// ### Технические особенности и PostGIS-логика:
    /// * **Безопасность типов в циклах**: Передача `&mut ***tx` необходима макросу `sqlx` для
    ///   повторного использования соединения внутри итератора без потери контекста владения.
    /// * **Порядок осей**: Переменные в `ST_MakePoint($19, $18, $11)` строго соответствуют
    ///   географическому порядку: `($19: Longitude, $18: Latitude, $11: Monument Height)`.
    /// * ** compile-time валидация**: Пространственные функции снабжены явным приведением к типу `::geometry`,
    ///   что защищает макрос от ошибок разбора схемы БД компилятором Rust.
    pub async fn insert_lost_batch(
        tx: &mut Transaction<'_, Postgres>,
        batch: &[LostNodeDomain],
    ) -> Result<(), sqlx::Error> {
        for node in batch {
            sqlx::query!(
                   r#"
                   INSERT INTO reference_geo.lost_nodes (
                       id, node_index, name, accuracy_class, map_sheet_50k, map_sheet_200k, height_determination_method,
                       monument_mark, center_structure_type, surface_monument_type, monument_height_m, region_code, region_name,
                       district_name, condition_monument, condition_center, verification_year, latitude_raw, longitude_raw, geom_global, geom_local
                   )
                   VALUES (
                       $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19,
                       ST_SetSRID(ST_MakePoint($19, $18, $11), 4326)::geometry,
                       ST_Transform(ST_SetSRID(ST_MakePoint($19, $18, $11), 4326), 28415)::geometry
                   )
                   ON CONFLICT (id) DO NOTHING;"#,
                   node.id, node.node_index, node.name, node.accuracy_class, node.map_sheet_50k, node.map_sheet_200k,
                   node.height_determination_method, node.monument_mark, node.center_structure_type, node.surface_monument_type,
                   node.monument_height_m, node.region_code, node.region_name, node.district_name, node.condition_monument,
                   node.condition_center, node.verification_year, node.latitude_raw, node.longitude_raw
               )
               .execute(&mut **tx)
               .await?;
        }
        Ok(())
    }
}
