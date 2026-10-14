use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use std::time::Duration;
use tracing::info;

pub struct DbManager;

impl DbManager {
    pub async fn create_pool(database_url: &str) -> Result<Pool<Postgres>, sqlx::Error> {
        info!("[СУБД] Подключение к Posgres кластеру (порт 5434)...");

        let pool = PgPoolOptions::new()
            .max_connections(15)
            .min_connections(3)
            .acquire_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(30))
            .connect(database_url)
            .await?;

        info!("[СУБД] Пул соединений Postgres успешно запущен!");
        Ok(pool)
    }

    pub async fn run_migrations(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
        info!("[СУБД] Проверка и накат SQL-миграций схемы данных...");

        sqlx::migrate!("./migrations").run(pool).await?;

        info!("[СУБД] Все ГИС-таблицы в актуальном состоянии!");

        Ok(())
    }
}
