use axum::Router;
use reference_geo::{ModuleState, routes};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

pub struct TestContext {
    pub app: Router,
    pub db_pool: sqlx::PgPool,
}

impl TestContext {
    pub async fn new() -> Self {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres::postgres@5436/geo_db".into());

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("❌ Не удалось подключиться к тестовой БД PostGIS");

        // Очищаем каскадом таблицы перед запуском каждого теста, обеспечивая воспроизводимость
        sqlx::query("TRUNCATE TABLE reference_geo.ggs_nodes CASCADE;")
            .execute(&pool)
            .await
            .expect("❌ Ошибка зачистки таблицы ggs_nodes перед тестом");

        // Создаем доменное состояние
        let state = Arc::new(ModuleState {
            db_pool: pool.clone(),
        });

        let app = routes(state);

        Self { app, db_pool: pool }
    }
}
