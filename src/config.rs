use std::env;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server_address: String,
    pub database_url: String,
    pub db_max_connections: u32,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        if let Err(e) = dotenvy::dotenv() {
            warn!(
                "Файл .env не найден, используем системные переменные среды: {}",
                e
            );
        }

        let server_address =
            env::var("SERVER_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

        let database_url = env::var("DATABASE_URL").map_err(|_| {
            "Критическая ошибка: Переменная DATABASE_URL не задана в окружении! Задайте её в .env"
        })?;

        let db_max_connections = env::var("DATABASE_MAX_CONNECTIONS")
            .unwrap_or_else(|_| "5".to_string())
            .parse::<u32>()
            .unwrap_or(5);

        Ok(AppConfig {
            server_address,
            database_url,
            db_max_connections,
        })
    }
}
