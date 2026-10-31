use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::info;

use crate::ModuleState;

use geo_core::infra::params::{BboxParams, PaginationParams, RadiusParams};

// Импортируем типы данных и репозитории из текущего модуля
use crate::models::{
    ChemicalElementDomain, ExportGgsParams, GgsFilterParams, GgsNodeDomain, LostFilterParams,
    LostNodeDomain, NivelerFilterParams, NivelerNodeDomain,
};
use crate::repository::{ChemistryRepository, GgsNodesRepository};

// Вспомогательная структура для экспорта
#[derive(Debug, Deserialize)]
pub struct ExportParams {
    pub map_sheet: String,
}

#[derive(Debug, Deserialize)]
pub struct ExportNivelerParams {
    pub region: String,
}

// =========================================================================
// ⛰️ 1. ХЭНДЛЕРЫ СУЩНОСТИ: ГГС ( ggs.rs )
// =========================================================================

// BULK INSERT (Пакетная вставка для Стрима)

/// **POST `/api/v1/reference/ggs/stream`**
///
/// **Асинхронный потоковый импорт пунктов ГГС из входящего сетевого потока (HTTP Body)**
///
/// Хэндлер принимает сырое тело запроса (`Body`), преобразует его в асинхронный поток байт
/// и передает в универсальный движок пакетной обработки данных `DbStreamProcessor`.
/// JSON десериализуется «на лету» без аллокации гигабайтных файлов в оперативной памяти,
/// а порции данных размером в 5000 объектов атомарно фиксируются внутри транзакций базы данных.
pub async fn stream_ggs_api(
    State(state): State<Arc<ModuleState>>,
    body: Body,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let byte_stream = body.into_data_stream();

    info!("🌐 API Стрим: Старт пакетного импорта пунктов ГГС через DbStreamProcessor...");

    let repo = GgsNodesRepository::new(&state.db_pool);

    let count = repo
        .process_ggs_json_stream(byte_stream, 2000)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка пакентного импорта: {}", err),
            )
        })?;

    info!("✅ API Стрим: Успешно залито {} пунктов ГГС.", count);

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "imported_count": count
        })),
    ))
}

// CREATE (Одиночный инсерт одной ноды ГГС напрямую из Домена)

/// **POST `/api/v1/reference/ggs`**
///
/// **Создание одиночного геодезического пункта ГГС из тела JSON-запроса**
///
/// Хэндлер принимает валидную десериализованную доменную модель `GgsNodeDomain` в формате JSON,
/// инициализирует контекст репозитория и производит точечную запись в СУБД. На стороне базы
/// данных автоматически генерируются пространственные PostGIS-поля в глобальной и локальной проекциях.
/// Благодаря логике `ON CONFLICT DO NOTHING`, хэндлер устойчив к дублированию первичных ключей.
pub async fn create_ggs_node_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<GgsNodeDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Выполняем одиночный инсерт в таблицу ggs_nodes
    repo.insert_ggs_node(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка одиночной вставки ГГС: {}", e),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "id": payload.id
        })),
    ))
}

// READ (Получить пункт по ID)

/// **GET `/api/v1/reference/ggs/:id`**
///
/// **Получить полную доменную информацию о пункте ГГС по его уникальному ID**
///
/// Извлекает целочисленный идентификатор из пути запроса (`Path`), инициализирует репозиторий
/// и производит точечный высокоэффективный поиск в базе данных.
///
/// Возвращает:
/// * `200 OK` с JSON-телом доменной модели, если объект успешно найден.
/// * `404 NOT FOUND`, если запись с таким идентификатором отсутствует в схеме `reference_geo.ggs_nodes`.
pub async fn get_ggs_by_id_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Выполняем точечный поиск в репозитории
    let node = repo.find_ggs_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка чтения из БД: {}", e),
        )
    })?;

    // Проверяем наличие сущности и формируем соответствующий HTTP-ответ
    match node {
        Some(n) => Ok((StatusCode::OK, Json(n))),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Пункт ГГС с ID {} не найден", id),
        )),
    }
}

// READ LIST (Базовая постраничная пагинация списка)

/// **GET `/api/v1/reference/ggs`**
///
/// **Получить список пунктов ГГС с поддержкой постраничной пагинации**
///
/// Хэндлер принимает опциональные Query-параметры `page` и `per_page` из веб-запроса,
/// выставляет дефолтные значения (1 страница, 20 элементов на страницу) и вызывает
/// метод репозитория `find_ggs_paginated`.
///
/// Возвращает клиенту унифицированный JSON-структурированный ответ `PaginatedResponse`
/// с массивом данных и метаинформацией страниц.
pub async fn get_ggs_list_paginated_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<PaginationParams>, // Используем зачищенную структуру параметров
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Безопасно извлекаем параметры, подставляя базовые дефолты
    let page = params.page.unwrap_or(1);
    let per_page = params.per_page.unwrap_or(20);

    // Учтен твой новый префикс метода репозитория: find_ggs_paginated
    let result = repo.find_ggs_paginated(page, per_page).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка пагинации ГГС: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(result)))
}

// FILTER (Умный поиск и текстовая фильтрация с пагинацией)

/// **GET `/api/v1/reference/ggs/search`**
///
/// **Многофакторный текстовый поиск и фильтрация пунктов ГГС с пагинацией**
///
/// Хэндлер принимает набор опциональных поисковых фильтров `GgsFilterParams` (включая текстовый запрос по имени,
/// номенклатурный лист 1:50k и название региона), делегируя выполнение оптимизированному SQL-запросу в репозитории.
///
/// Возвращает клиенту объект `PaginatedResponse`, содержащий отфильтрованный срез доменных моделей пунктов
/// и полные метаданные для построения постраничной навигации на веб-фронтенде.
pub async fn filter_ggs_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<GgsFilterParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем отрефакторенный метод динамического поиска
    let result = repo.filter_ggs(params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка фильтрации ГГС: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(result)))
}

// EXPORT TO GEOJSON (Нативная выгрузка планшета карт из PostGIS)

/// **GET `/api/v1/reference/ggs/export`**
///
/// **Экспортировать пункты ГГС определенного номенклатурного планшета в формат GeoJSON (RFC 7946)**
///
/// Хэндлер извлекает имя планшета `map_sheet` из строки запроса (`Query`), вызывает метод репозитория
/// и стримит готовую валидную ГИС-структуру `FeatureCollection`, сгенерированную на стороне СУБД Postgres.
///
/// ### Особенности отдачи контента:
/// Возвращает сырой текстовый поток со специализированным MIME-типом **`application/geo+json`**.
/// Это позволяет ГИС-системам (QGIS, ArcGIS) и картографическим библиотекам фронтенда
/// обрабатывать и рендерить этот файл напрямую без дополнительных конвертаций.
pub async fn export_ggs_geojson_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<ExportGgsParams>, // Теперь структура гарантированно видна компилятору
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем метод репозитория с учетом добавленного префикса ggs
    let geojson_string = repo
        .export_ggs_to_geojson(&params.map_sheet)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка генерации GeoJSON для ГГС: {}", e),
            )
        })?;

    // Отдаем со специальным MIME-типом геоданных, чтобы ГИС-софт (QGIS) понимал файл на лету
    axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "application/geo+json")
        .status(StatusCode::OK)
        .body(Body::from(geojson_string))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка сборки HTTP-ответа GeoJSON: {}", e),
            )
        })
}

// Spatial: find_within_radius (Пространственный поиск по радиусу в метрах)

/// **GET `/api/v1/reference/ggs/spatial/radius`**
///
/// **Пространственный PostGIS-поиск пунктов ГГС в заданном метрическом радиусе**
///
/// Хэндлер принимает входящие географические координаты центра `lon` (долгота), `lat` (широта)
/// и искомый радиус `radius_m` в метрах из Query-параметров структуры `RadiusParams`.
///
/// Метод задействует пространственный `GIST`-индекс СУБД для мгновенного отсечения объектов
/// и возвращает массив пунктов, автоматически отсортированных по возрастанию расстояния (от ближайшего к дальнему).
pub async fn find_ggs_radius_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<RadiusParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // ИСПРАВЛЕНО: Вызываем метод репозитория с учётом твоего префикса ggs
    let nodes = repo.find_ggs_within_radius(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка пространственного поиска по радиусу ГГС: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(nodes)))
}

// Spatial: find_in_bbox (Пространственный поиск по рамке экрана карты)

/// **GET `/api/v1/reference/ggs/spatial/bbox`**
///
/// **Динамический пространственный поиск пунктов ГГС внутри рамки видимости экрана (BBox)**
///
/// Хэндлер принимает четыре граничные координаты прямоугольника охвата (`min_lon`, `min_lat`, `max_lon`, `max_lat`)
/// из Query-параметров структуры `BboxParams`.
///
/// Метод оптимизирован для работы с веб-картами (Leaflet/Mapbox) и ГИС-плагинами: он мгновенно отсекает объекты
/// по плоскому пространственному 2D-индексу базы данных, подгружая только те пункты, которые видны пользователю на экране.
pub async fn find_ggs_bbox_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<BboxParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // ИСПРАВЛЕНО: Метод вызывается с учетом добавленного префикса ggs
    let nodes = repo.find_ggs_in_bbox(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка пространственного поиска по BBox ГГС: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(nodes)))
}

// UPDATE (Полное исчерпывающее обновление полей и 3D-геометрий)

/// **PUT `/api/v1/reference/ggs`**
///
/// **Полное обновление атрибутов и пространственных геометрий существующего пункта ГГС**
///
/// Хэндлер принимает доменную модель `GgsNodeDomain` из тела JSON-запроса, находит запись
/// по её уникальному идентификатору `id` и полностью перезаписывает все метаданные.
///
/// На стороне СУБД Postgres автоматически пересчитываются и обновляются PostGIS-поля глобальных
/// и локальных систем координат на основе переданных сырых значений широты и долготы.
pub async fn update_ggs_node_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<GgsNodeDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // ИСПРАВЛЕНО: Вызываем метод с учетом добавленного префикса ggs
    repo.update_ggs(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка обновления записи ГГС: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({ "status": "success", "message": "Пункт ГГС успешно обновлен" })),
    ))
}

// DELETE (Удалить ноду из системы)

/// **DELETE `/api/v1/reference/ggs/:id`**
///
/// **Физическое удаление пункта ГГС из базы данных по его ID**
///
/// Хэндлер извлекает уникальный целочисленный идентификатор из пути запроса (`Path`),
/// инициализирует репозиторий и выполняет жесткое удаление (`HARD DELETE`) записи
/// из таблицы `reference_geo.ggs_nodes`.
///
/// Метод каскадно очищает пространственные индексы `GIST` для данного `id`.
pub async fn delete_ggs_node_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // ИСПРАВЛЕНО: Вызываем метод с учетом добавленного префикса ggs
    repo.delete_ggs(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка удаления пункта ГГС из БД: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(
            json!({ "status": "success", "message": format!("Пункт ГГС с ID {} успешно удален", id) }),
        ),
    ))
}

// =========================================================================
// 📉 2. ХЭНДЛЕРЫ СУЩНОСТИ: УТРАЧЕННЫЕ ЗНАКИ ( lost.rs )
// =========================================================================

// BULK INSERT (Пакетная вставка для Стрима)

/// **POST `/api/v1/reference/lost/stream`**
///
/// **Асинхронный потоковый импорт утраченных/поврежденных маркшейдерских пунктов (HTTP Body)**
///
/// Хэндлер принимает сырое тело входящего сетевого HTTP-запроса в виде асинхронного потока байт,
/// делегируя его разбор и заливку в БД универсальному движку `DbStreamProcessor`.
/// Десериализация JSON-массива происходит «на лету» без аллокации избыточной оперативной памяти.
/// Данные фиксируются порциями по 5000 объектов в рамках независимых транзакций СУБД.
pub async fn stream_lost_api(
    State(state): State<Arc<ModuleState>>,
    body: Body,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Получаем асинхронный поток данных из входящего HTTP-тела
    let byte_stream = body.into_data_stream();

    info!("🌐 API Стрим: Старт пакетного импорта утраченных пунктов...");

    // Вызываем потоковый процессор, передавая целевую доменную модель LostNodeDomain
    let repo = GgsNodesRepository::new(&state.db_pool);

    let count = repo
        .process_lost_json_stream(byte_stream, 2000)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка пакетного чтения: {}", err),
            )
        })?;

    info!("✅ API Стрим: Успешно залито {} утраченных пунктов.", count);

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "imported_count": count
        })),
    ))
}

// CREATE (Одиночный ручной инсерт пункта в архив)

/// **POST `/api/v1/reference/lost`**
///
/// **Внести одиночную запись об утраченном/поврежденном маркшейдерском пункте в архив СУБД**
///
/// Хэндлер принимает валидированную доменную модель `LostNodeDomain` в формате JSON,
/// инициализирует репозиторий и производит точечную вставку в таблицу `reference_geo.lost_nodes`.
///
/// На стороне базы данных автоматически пересчитываются пространственные PostGIS-поля
/// в глобальной системе WGS 84 (4326) и локальной СК-42 (28415). Конструкция `ON CONFLICT DO NOTHING`
/// обеспечивает идемпотентность API-эндпоинта при случайных повторных запросах с тем же ID.
pub async fn create_lost_node_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<LostNodeDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Выполняем одиночный инсерт в архивную таблицу lost_nodes
    repo.insert_lost_node(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка одиночного создания в архиве маркшейдеров: {}", e),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "id": payload.id
        })),
    ))
}

// READ (Получить архивную карточку по ID)

/// **GET `/api/v1/reference/lost/:id`**
///
/// **Получить подробную архивную карточку утраченного маркшейдерского пункта по его уникальному ID**
///
/// Извлекает целочисленный идентификатор из пути запроса (`Path`), инициализирует репозиторий
/// и производит точечный высокоэффективный поиск в архивной таблице.
///
/// Возвращает:
/// * `200 OK` с JSON-телом доменной модели, если объект успешно найден.
/// * `404 NOT FOUND`, если запись с таким идентификатором отсутствует в таблице `reference_geo.lost_nodes`.
pub async fn get_lost_by_id_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Выполняем точечный поиск в репозитории маркшейдеров
    let node = repo.find_lost_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка чтения из архива БД: {}", e),
        )
    })?;

    // Проверяем наличие сущности и формируем соответствующий HTTP-ответ
    match node {
        Some(n) => Ok((StatusCode::OK, Json(n))),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Архивный пункт с ID {} не найден", id),
        )),
    }
}

// READ LIST (Базовая постраничная пагинация списка)

/// **GET `/api/v1/reference/lost`**
///
/// **Получить список утраченных/поврежденных маркшейдерских пунктов с поддержкой постраничной пагинации**
///
/// Хэндлер принимает опциональные Query-параметры `page` и `per_page` из веб-запроса,
/// выставляет базовые дефолтные значения (1 страница, 20 элементов на страницу) и вызывает
/// метод репозитория `find_lost_paginated`.
///
/// Возвращает клиенту объект постраничного ответа `PaginatedResponse`, содержащий массив
/// доменных моделей уничтоженных знаков и полные метаданные страниц для фронтенда.
pub async fn get_lost_list_paginated_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<PaginationParams>, // Используем зачищенную структуру параметров пагинации
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Безопасно извлекаем параметры, подставляя базовые дефолты
    let page = params.page.unwrap_or(1);
    let per_page = params.per_page.unwrap_or(20);

    // Вызываем отрефакторенный метод постраничного вывода маркшейдеров
    let result = repo
        .find_lost_paginated(page, per_page)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка пагинации архива маркшейдеров: {}", e),
            )
        })?;

    Ok((StatusCode::OK, Json(result)))
}

// FILTER (Умный поиск, фильтрация по состоянию и пагинация)

/// **GET `/api/v1/reference/lost/search`**
///
/// **Многофакторный текстовый поиск и фильтрация маркшейдерских пунктов по состоянию с пагинацией**
///
/// Хэндлер принимает набор опциональных поисковых фильтров `LostFilterParams` (включая текстовый запрос по имени,
/// номенклатурный лист 1:50k и текстовое состояние центра знака), делегируя выполнение оптимизированному SQL-запросу в репозитории.
///
/// Возвращает клиенту объект `PaginatedResponse`, содержащий отфильтрованный срез доменных моделей пунктов
/// и полные метаданные для построения постраничной навигации на веб-фронтенде.
pub async fn filter_lost_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<LostFilterParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем отрефакторенный метод динамического поиска архива
    let result = repo.filter_lost(params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка фильтрации архива маркшейдеров: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(result)))
}

// EXPORT TO GEOJSON (Нативная выгрузка архивных пунктов в GeoJSON FeatureCollection)

/// **GET `/api/v1/reference/lost/export`**
///
/// **Экспортировать архивные маркшейдерские пункты определенного планшета в формат GeoJSON (RFC 7946)**
///
/// Хэндлер извлекает имя номенклатурного планшета `map_sheet` из строки запроса (`Query`), переиспользуя
/// общее DTO `ExportGgsParams`. Возвращает валидный ГИС-контент `FeatureCollection`, сгенерированный
/// на уровне ядра PostgreSQL без лишних циклов сериализации в слое приложения.
///
/// ### Спецификация заголовков:
/// Ответ передается в текстовом потоке с MIME-типом **`application/geo+json`**, что позволяет
/// ГИС-инструментам (QGIS) и библиотекам фронтенда парсить пространственные свойства объектов налету.
pub async fn export_lost_geojson_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<ExportGgsParams>, // Упростили путь импорта, так как DTO объявлен в этом же файле
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем отрефакторенный метод экспорта архива маркшейдеров
    let geojson_string = repo
        .export_lost_to_geojson(&params.map_sheet)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка GeoJSON экспорта архива маркшейдеров: {}", e),
            )
        })?;

    // Отдаем со специальным MIME-типом геоданных
    axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "application/geo+json")
        .status(StatusCode::OK)
        .body(Body::from(geojson_string))
        // ИСПРАВЛЕНО: Приведение ошибки билдера к единому формату кортежа (StatusCode, String)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка сборки HTTP-ответа GeoJSON для архива: {}", e),
            )
        })
}

// Spatial: find_lost_within_radius (Гео-поиск уничтоженных точек по радиусу)

/// **GET `/api/v1/reference/lost/spatial/radius`**
///
/// **Пространственный PostGIS-поиск уничтоженных/поврежденных маркшейдерских пунктов по радиусу**
///
/// Хэндлер принимает входящие географические координаты центра `lon` (долгота), `lat` (широта)
/// и искомый радиус `radius_m` в метрах из Query-параметров структуры `RadiusParams`.
///
/// Метод задействует пространственный `GIST`-индекс СУБД для мгновенного отсечения объектов
/// и возвращает массив пунктов, автоматически отсортированных по возрастанию расстояния (от ближайшего к дальнему).
pub async fn find_lost_radius_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<RadiusParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем метод репозитория с учетом префикса lost
    let nodes = repo.find_lost_within_radius(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка гео-поиска архива по радиусу: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(nodes)))
}

// Spatial: find_lost_in_bbox (Гео-поиск уничтоженных точек по рамке карты)

/// **GET `/api/v1/reference/lost/spatial/bbox`**
///
/// **Динамический пространственный поиск уничтоженных/поврежденных пунктов внутри рамки экрана видимости (BBox)**
///
/// Хэндлер принимает четыре граничные координаты прямоугольника охвата (`min_lon`, `min_lat`, `max_lon`, `max_lat`)
/// из Query-параметров структуры `BboxParams`.
///
/// Метод оптимизирован для динамической подгрузки геоданных при перемещении пользователем карты на фронтенде:
/// он мгновенно отсекает объекты по плоскому пространственному 2D-индексу базы данных, возвращая только те
/// архивные маркшейдерские пункты, которые физически попадают в текущую область видимости экрана.
pub async fn find_lost_bbox_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<BboxParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем отрефакторенный метод пространственного поиска
    let nodes = repo.find_lost_in_bbox(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка гео-поиска архива по bbox: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(nodes)))
}

// UPDATE (Полное исчерпывающее обновление архивных полей и 3D-геометрий)

/// **PUT `/api/v1/reference/lost`**
///
/// **Полное обновление атрибутов и пространственных 3D-геометрий архивного маркшейдерского пункта**
///
/// Хэндлер принимает доменную модель `LostNodeDomain` из тела JSON-запроса, находит существующую запись
/// по её уникальному идентификатору `id` и полностью перезаписывает все метаданные и характеристики состояния.
///
/// На стороне СУБД Postgres автоматически пересчитываются и обновляются PostGIS-поля глобальных (4326)
/// и локальных (28415) систем координат на основе измененных сырых значений широты, долготы и высоты знака.
pub async fn update_lost_node_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<LostNodeDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем отрефакторенный метод репозитория маркшейдеров
    repo.update_lost(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка обновления архивной записи: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({ "status": "success", "message": "Архивная запись успешно обновлена" })),
    ))
}

// DELETE (Удалить пункт из архива)

/// **DELETE `/api/v1/reference/lost/:id`**
///
/// **Физическое удаление маркшейдерского пункта из архивной таблицы СУБД по его ID**
///
/// Хэндлер извлекает уникальный целочисленный идентификатор из пути запроса (`Path`),
/// инициализирует репозиторий и выполняет жесткое удаление (`HARD DELETE`) записи
/// из таблицы `reference_geo.lost_nodes`.
///
/// Метод автоматически каскадно очищает связанные пространственные индексы `GIST` для данного `id`.
pub async fn delete_lost_node_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    // Вызываем отрефакторенный метод удаления маркшейдеров
    repo.delete_lost(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка удаления из архива: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(
            json!({ "status": "success", "message": format!("Архивный пункт {} успешно удален", id) }),
        ),
    ))
}

// =========================================================================
// 🎚️ 3. ХЭНДЛЕРЫ СУЩНОСТИ: ВЫСОТНЫЕ НИВЕЛИРЫ ( niveler.rs )
// =========================================================================

// BULK INSERT (Пакетный стриминг из JSON)
/// `POST /api/v1/reference/niveler/stream`
pub async fn stream_niveler_api(
    State(state): State<Arc<ModuleState>>,
    body: Body,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);
    let byte_stream = body.into_data_stream();

    info!("🌐 API Стрим: Старт пакетного импорта нивелирных реперов...");

    let count = repo
        .process_niveler_json_stream(byte_stream, 2000)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка стрима нивелиров: {}", err),
            )
        })?;

    info!("✅ API Стрим: Успешно залито {} нивелирных пунктов.", count);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "status": "success", "imported_count": count })),
    ))
}

// CREATE (Одиночная ручная вставка репера)
/// `POST /api/v1/reference/niveler`
pub async fn create_niveler_node_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<NivelerNodeDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    repo.insert_niveler_node(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка создания нивелира: {}", e),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "status": "success", "id": payload.id })),
    ))
}

// READ (Получить карточку нивелира по ID)
/// `GET /api/v1/reference/niveler/:id`
pub async fn get_niveler_by_id_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    let node = repo.find_niveler_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка чтения нивелира: {}", e),
        )
    })?;

    match node {
        Some(n) => Ok((StatusCode::OK, Json(n))),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Нивелирный репер с ID {} не найден", id),
        )),
    }
}

// READ LIST (Базовая постраничная пагинация списка)
/// `GET /api/v1/reference/niveler`
pub async fn get_niveler_list_paginated_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);
    let page = params.page.unwrap_or(1);
    let per_page = params.per_page.unwrap_or(20);

    let result = repo
        .find_niveler_paginated(page, per_page)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка пагинации нивелиров: {}", e),
            )
        })?;

    Ok((StatusCode::OK, Json(result)))
}

// FILTER (Умная фильтрация и текстовый поиск с пагинацией)
/// `GET /api/v1/reference/niveler/search`
pub async fn filter_niveler_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<NivelerFilterParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    let result = repo.filter_niveler(params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка фильтрации нивелиров: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(result)))
}

// EXPORT TO GEOJSON (Нативная выгрузка реперов субъекта РФ из PostGIS)
/// `GET /api/v1/reference/niveler/export`
pub async fn export_niveler_geojson_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<ExportNivelerParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    let geojson_string = repo
        .export_niveler_to_geojson(&params.region)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка GeoJSON экспорта нивелиров: {}", e),
            )
        })?;

    axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "application/geo+json")
        .status(StatusCode::OK)
        .body(Body::from(geojson_string))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

// Spatial: find_niveler_within_radius (Гео-поиск реперов по радиусу в метрах)
/// `GET /api/v1/reference/niveler/spatial/radius`
pub async fn find_niveler_radius_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<RadiusParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    let nodes = repo
        .find_niveler_within_radius(&params)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Ошибка гео-поиска нивелиров по радиусу: {}", e),
            )
        })?;

    Ok((StatusCode::OK, Json(nodes)))
}

// Spatial: find_niveler_in_bbox (Гео-поиск нивелиров по рамке экрана карты)
/// `GET /api/v1/reference/niveler/spatial/bbox`
pub async fn find_niveler_bbox_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<BboxParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    let nodes = repo.find_niveler_in_bbox(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка гео-поиска нивелиров по bbox: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(nodes)))
}

// UPDATE (Полное исчерпывающее обновление полей и геометрий)
/// `PUT /api/v1/reference/niveler`
pub async fn update_niveler_node_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<NivelerNodeDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    repo.update_niveler(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка обновления нивелира: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({ "status": "success", "message": "Нивелирный репер успешно обновлен" })),
    ))
}

// DELETE (Удалить высотный репер по ID)
/// `DELETE /api/v1/reference/niveler/:id`
pub async fn delete_niveler_node_api(
    State(state): State<Arc<ModuleState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = GgsNodesRepository::new(&state.db_pool);

    repo.delete_niveler(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка удаления нивелира: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(
            json!({ "status": "success", "message": format!("Нивелирный репер {} успешно удален", id) }),
        ),
    ))
}

// =========================================================================
// 🧪 4. ХЭНДЛЕРЫ СУЩНОСТИ: ХИМИЧЕСКИЕ ЭЛЕМЕНТЫ ( chemistry )
// =========================================================================

// CREATE / UPDATE (Вставка с обновлением при конфликте — Ипсирт)
/// `POST /api/v1/reference/chemistry`
pub async fn upsert_element_api(
    State(state): State<Arc<ModuleState>>,
    Json(payload): Json<ChemicalElementDomain>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = ChemistryRepository::new(&state.db_pool);

    repo.upsert_element(&payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка СУБД химии: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({ "status": "success", "code": payload.code })),
    ))
}

// READ (Получить элемент по его коду, например 'au')
/// `GET /api/v1/reference/chemistry/:code`
pub async fn get_element_by_code_api(
    State(state): State<Arc<ModuleState>>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = ChemistryRepository::new(&state.db_pool);

    let element = repo.find_by_code(&code).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка чтения справочника: {}", e),
        )
    })?;

    match element {
        Some(el) => Ok((StatusCode::OK, Json(el))),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Элемент '{}' не найден в справочнике", code),
        )),
    }
}

// LIST & PAGINATION (Вывод всего справочника порциями)
/// `GET /api/v1/reference/chemistry`
pub async fn get_chemistry_list_api(
    State(state): State<Arc<ModuleState>>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = ChemistryRepository::new(&state.db_pool);
    let page = params.page.unwrap_or(1);
    let per_page = params.per_page.unwrap_or(20);

    let result = repo.find_paginated(page, per_page).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка пагинации справочника: {}", e),
        )
    })?;

    Ok((StatusCode::OK, Json(result)))
}

// DELETE (Удалить элемент из справочника)
/// `DELETE /api/v1/reference/chemistry/:code`
pub async fn delete_element_api(
    State(state): State<Arc<ModuleState>>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = ChemistryRepository::new(&state.db_pool);

    repo.delete_element(&code).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Ошибка удаления элемента: {}", e),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({ "status": "success", "message": format!("Элемент {} успешно удален", code) })),
    ))
}
