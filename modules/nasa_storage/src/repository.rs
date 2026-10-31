use crate::models::{DownloadTaskRow, NewGranule, QueueStats};
use sqlx::{types::Json, Pool, Postgres, Transaction};
use uuid::Uuid;

pub struct NasaRepository<'a> {
    pub pool: &'a Pool<Postgres>,
}

impl<'a> NasaRepository<'a> {
    pub fn new(pool: &'a Pool<Postgres>) -> Self {
        Self { pool }
    }

    // ============================================================================
    // 🤖 CRAWLER OPERATIONS (МЕТАДАННЫЕ NASA CMR)
    // ============================================================================

    /// **CREATE**: Атомарно сохраняет метаданные насовского снимка и создает таску на скачивание.
    ///
    /// # Механика PostGIS
    /// Принимает footprint полигона в формате WKT (Well-Known Text) и строит геометрию через `ST_GeomFromText($5, 4326)`.
    /// Весь процесс обернут в транзакцию базы данных.
    pub async fn insert_granule_and_create_task(
        &self,
        granule: NewGranule,
    ) -> Result<Uuid, sqlx::Error> {
        let mut tx: Transaction<'_, Postgres> = self.pool.begin().await?;

        // 1. Сохраняем гранулу. Если прилетел дубль от NASA — просто обновляем таймстамп
        let granule_row = sqlx::query!(
            r#"
            INSERT INTO nasa.spatial_granules 
                (nasa_granule_id, collection_id, acquisition_date, cloud_cover, footprint, band_urls)
            VALUES 
                ($1, $2, $3, $4, ST_GeomFromText($5, 4326), $6)
            ON CONFLICT (nasa_granule_id) 
            DO UPDATE SET created_at = NOW()
            RETURNING id;
            "#,
            granule.nasa_granule_id,
            granule.collection_id,
            granule.acquisition_date,
            granule.cloud_cover,
            granule.footprint_wkt,
            Json(granule.band_urls) as _
        )
        .fetch_one(&mut *tx)
        .await?;

        let created_granule_id = granule_row.id;

        // 2. Создаем задачу в очереди для downloader воркера
        sqlx::query!(
            r#"
            INSERT INTO nasa.download_tasks (granule_id, status)
            VALUES ($1, 'pending'::nasa.task_status)
            ON CONFLICT DO NOTHING;
            "#,
            created_granule_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(created_granule_id)
    }

    // ============================================================================
    // 📥 DOWNLOADER WORKER OPERATIONS (ОЧЕРЕДЬ СКАЧИВАНИЯ COG)
    // ============================================================================

    /// **LOCK & FETCH**: Атомарно захватывает одну свободную задачу из очереди.
    ///
    /// # Механика Highload Очередей
    /// Использует паттерн `FOR UPDATE SKIP LOCKED`. Воркер блокирует строку, пропуская уже
    /// занятые другими потоками задачи, переводя её в статус `processing` и выставляя `locked_at`.
    pub async fn pick_next_download_task(&self) -> Result<Option<DownloadTaskRow>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Ищем одну таску со статусом 'pending'
        let task = sqlx::query_as!(
            DownloadTaskRow,
            r#"
            UPDATE nasa.download_tasks
            SET 
                status = 'processing'::nasa.task_status,
                locked_at = NOW()
            WHERE id = (
                SELECT id 
                FROM nasa.download_tasks 
                WHERE status = 'pending'::nasa.task_status
                ORDER BY created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, granule_id, status as "status: _", attempts;
            "#
        )
        .fetch_optional(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(task)
    }

    /// **UPDATE**: Переводит задачу в статус успешного завершения.
    pub async fn complete_task(&self, task_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE nasa.download_tasks
            SET status = 'complete'::nasa.task_status, updated_at = NOW()
            WHERE id = $1;
            "#,
            task_id
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }

    /// **UPDATE**: Логирует падение задачи, увеличивает счетчик попыток.
    pub async fn fail_task(&self, task_id: i64, error: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE nasa.download_tasks
            SET 
                status = CASE WHEN attempts >= 3 THEN 'failed'::nasa.task_status ELSE 'pending'::nasa.task_status END,
                attempts = attempts + 1,
                error_message = $2,
                updated_at = NOW()
            WHERE id = $1;
            "#,
            task_id,
            error
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }

    // ============================================================================
    // 📊 MANAGEMENT & CONTROL (КОНТРОЛЬ ОЧЕРЕДИ / РУЧКИ АДМИНКИ)
    // ============================================================================

    /// **READ**: Собирает агрегированную метрику по состоянию очереди для Axum-ручки `/queue/stats`.
    pub async fn get_queue_statistics(&self) -> Result<QueueStats, sqlx::Error> {
        let stats = sqlx::query!(
            r#"
            SELECT 
                COUNT(*) FILTER (WHERE status = 'pending'::nasa.task_status) as pending_count,
                COUNT(*) FILTER (WHERE status = 'processing'::nasa.task_status) as processing_count,
                COUNT(*) FILTER (WHERE status = 'complete'::nasa.task_status) as complete_count,
                COUNT(*) FILTER (WHERE status = 'failed'::nasa.task_status) as failed_count
            FROM nasa.download_tasks;
            "#
        )
        .fetch_one(self.pool)
        .await?;

        Ok(QueueStats {
            pending: stats.pending_count.unwrap_or(0),
            processing: stats.processing_count.unwrap_or(0),
            complete: stats.complete_count.unwrap_or(0),
            failed: stats.failed_count.unwrap_or(0),
        })
    }

    /// **UPDATE**: Тот самый "пинок" из роутера — принудительный сброс зависшего или упавшего таска обратно в `pending`.
    pub async fn force_retry_task(&self, task_uuid: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE nasa.download_tasks
            SET 
                status = 'pending'::nasa.task_status,
                attempts = 0,
                error_message = NULL,
                locked_at = NULL,
                updated_at = NOW()
            WHERE granule_id = $1;
            "#,
            task_uuid
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }
}
