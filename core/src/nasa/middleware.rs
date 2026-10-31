use async_trait::async_trait;
use http::Extensions;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result as MiddlewareResult};

pub struct NasaAuthMiddleware {
    pub token: String,
}

#[async_trait]
impl Middleware for NasaAuthMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> MiddlewareResult<Response> {
        if let Some(host) = req.url().host_str() {
            // Защита: инжектируем заголовки только на домены NASA
            if host.contains("nasa.gov") {
                // 1. Формируем строку авторизации.
                // Автоматически страхуемся на случай, если в .env токен записан с префиксом или без.
                let auth_string = if self.token.starts_with("Bearer ") {
                    self.token.clone()
                } else {
                    format!("Bearer {}", self.token)
                };

                if let Ok(mut auth_header) = HeaderValue::from_str(&auth_string) {
                    auth_header.set_sensitive(true); // Защита от утечки ключа в дебаг-логи
                    req.headers_mut().insert(AUTHORIZATION, auth_header);
                }

                // 2. Добавляем Client-Id в правильном регистре (используем HeaderName::from_static)
                let client_id_name = reqwest::header::HeaderName::from_static("client-id");
                let client_id_val = HeaderValue::from_static("HydrogenGateway/1.0");
                req.headers_mut().insert(client_id_name, client_id_val);

                // 3. Гарантируем, что NASA вернет JSON (используем встроенную константу ACCEPT)
                let accept_val = HeaderValue::from_static("application/json");
                req.headers_mut().insert(ACCEPT, accept_val);

                // 4. Добавляем системный USER_AGENT (помогает при аудите логов на стороне NASA)
                let ua_val = HeaderValue::from_static("HydrogenGateway/1.0");
                req.headers_mut().insert(USER_AGENT, ua_val);
            }
        }

        next.run(req, extensions).await
    }
}
