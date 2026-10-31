pub mod handlers;
pub mod models;
pub mod repository;

use axum::{routing::post, Router};
use sqlx::{Pool, Postgres};
use std::sync::Arc;

pub struct ModuleState {
    pub db_pool: Pool<Postgres>,
}

/// **Маршрутизатор домена Core Storage**
///
/// Собирает все геологические эндпоинты в единый роутер,
/// локально внедряет в них `Arc<ModuleState>` и отдает готовый
/// закрытый `Router<()>` в корневой оркестратор приложения.
pub fn routes(state: Arc<ModuleState>) -> Router {
    Router::new()
        // Группируем ручки, связанные с симуляцией и генерацией 3D-вокселей ГКЗ.
        // Корневой оркестратор примонтирует этот роутер к префиксу "/api/v1/geology",
        // поэтому итоговый URL для вызова будет: POST "/api/v1/geology/simulation/generate"
        .nest(
            "/simulation",
            Router::new().route("/generate", post(handlers::handle_generate_simulation)),
        )
        // Сюда в будущем добавятся ручки для загрузки реальных баз скважин и ML-анализа запасов
        // .nest("/drillholes", Router::new().route("/", post(handlers::import_drillholes_api)))
        // Канонично вшиваем локальный стейт модуля прямо здесь,
        // превращая возвращаемый тип в Router<()> для оркестратора
        .with_state(state)
}
