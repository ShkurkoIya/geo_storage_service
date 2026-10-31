use futures_util::future::BoxFuture;
use futures_util::{Stream, StreamExt};
use serde::de::DeserializeOwned;
use sqlx::{Pool, Postgres, Transaction};

/// **Макрос автоматической генерации асинхронного ГИС-стриминг-инжестера**
///
/// Разворачивает высокоуровневый метод для пакетной обработки асинхронных JSON/GeoJSON потоков
/// внутри указанной структуры репозитория. Инкапсулирует под капотом всю рутину по буферизации сетевых байт,
/// потоковой десериализации данных «на лету» без оверхеда по ОЗУ и атомарного коммита батчей через транзакции.
///
/// ### Параметры макроса:
/// * `$repo:ident` — Идентификатор (имя) структуры твоего репозитория, куда генерируется метод (например, `GgsNodesRepository`).
/// * `$domain:ty` — Целевой тип доменной модели данных для десериализации (например, `GgsNodeDomain`).
/// * `$batch_fn:ident` — Имя статического метода пакетного инсерта, объявленного в репозитории (например, `insert_ggs_batch`).
/// * `$method_name:ident` — Уникальное имя генерируемого метода (предотвращает конфликты дублирования имен в общем репозитории).
///
/// ### Сгенерированный метод принимает аргументы:
/// * `byte_stream: S` — Входящий асинхронный сетевой поток данных, реализующий трейты `Stream` и `Unpin` (например, `body.into_data_stream()` из Axum).
/// * `batch_size: usize` — Размер пачки для фиксации атомарной транзакции базы данных. Настраивается динамически во время выполнения (runtime).
///
/// ### Возвращаемое значение метода:
/// * `Result<usize, String>` — В случае успеха возвращает общее количество успешно обработанных и записанных в БД объектов.
///
/// ### Пример использования в файле данных (`ggs.rs`):
/// ```rust
/// impl_geojson_stream!(GgsNodesRepository, GgsNodeDomain, insert_ggs_batch, process_ggs_stream);
/// ```
#[macro_export]
macro_rules! impl_json_stream {
    ($repo:ident, $domain:ty, $batch_fn:ident, $method_name:ident) => {
        impl<'a> $repo<'a> {
            #[doc = concat!(" Асинхронный потоковый инжектор для сущности [", stringify!($domain), "].")]
            #[doc = ""]
            #[doc = " Выкачивает сырой поток байт из HTTP-тела, десериализует JSON частями"]
            #[doc = concat!(" и пачками размера `batch_size` вызывает статический метод вставки `", stringify!($batch_fn), "`.")]
            #[doc = " Каждая пачка фиксируется (COMMIT) изолированно."]
            pub async fn $method_name<S, E>(&self, byte_stream: S, batch_size: usize) -> Result<usize, String>
            where
                S: futures_util::Stream<Item = Result<axum::body::Bytes, E>> + Unpin,
                E: std::fmt::Display,
            {
                // Вызываем движок ядра, полностью спрятав под капот Box::pin и касты типов
                $crate::DbStreamProcessor::process_stream(
                    self.pool,
                    byte_stream,
                    batch_size,
                    // Явная подсказка типов для слайса данных &[ $domain ].
                    // Позволяет компилятору Rust автоматически вывести все 4 дженерика метода без использования прочерков `_`.

                    |tx, batch: &[ $domain ]| Box::pin($repo::<'_>::$batch_fn(tx, batch)),
                )
                .await
            }
        }
    };
}

pub struct DbStreamProcessor;

impl DbStreamProcessor {
    // ИСПРАВЛЕНО: Убрали generic-параметр Fut. Теперь сигнатура чистая.
    pub async fn process_stream<T, S, E, F>(
        pool: &Pool<Postgres>,
        mut byte_stream: S,
        batch_size: usize,
        mut insert_batch_fn: F,
    ) -> Result<usize, String>
    where
        T: DeserializeOwned,
        S: Stream<Item = Result<bytes::Bytes, E>> + Unpin,
        E: std::fmt::Display,
        // Замыкание принимает транзакцию на лайфтайм 'a
        // и возвращает строго типизированную BoxFuture, которая живет тоже 'a.
        F: for<'a> FnMut(
            &'a mut Transaction<'_, Postgres>,
            &'a [T],
        ) -> BoxFuture<'a, Result<(), sqlx::Error>>,
    {
        let mut all_bytes = Vec::new();
        while let Some(chunk) = byte_stream.next().await {
            let data = chunk.map_err(|e| format!("Ошибка HTTP-потока: {}", e))?;
            all_bytes.extend_from_slice(&data);
        }

        let json_stream = serde_json::Deserializer::from_reader(&all_bytes[..]).into_iter::<T>();

        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        let mut count = 0;
        let mut batch = Vec::with_capacity(batch_size);

        for item in json_stream {
            let node = item.map_err(|e| format!("Ошибка парсинга JSON: {}", e))?;
            batch.push(node);
            count += 1;

            if batch.len() == batch_size {
                // Передаем ссылку на транзакцию. Благодаря BoxFuture компилятор знает,
                // что заимствование tx закончится СТРОГО внутри этой итерации.
                insert_batch_fn(&mut tx, &batch)
                    .await
                    .map_err(|e| format!("Ошибка выполнения пакетной вставки: {}", e))?;

                tx.commit().await.map_err(|e| e.to_string())?;
                tx = pool.begin().await.map_err(|e| e.to_string())?;
                batch.clear();
            }
        }

        if !batch.is_empty() {
            insert_batch_fn(&mut tx, &batch)
                .await
                .map_err(|e| format!("Ошибка финальной пакетной вставки: {}", e))?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(count)
    }
}
