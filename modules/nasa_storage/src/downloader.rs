use aws_sdk_s3::primitives::ByteStream;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use crate::{models::DownloadTaskRow, repository::NasaRepository, DownloaderSignal, ModuleState};

/// Бесконечный фоновый цикл обработки очереди скачивания COG растров
pub async fn run_background_loop(
    state: Arc<ModuleState>,
    mut rx: mpsc::Receiver<DownloaderSignal>,
) {
    let repo = NasaRepository::new(&state.db_pool);
    tracing::info!("📥 Фоновый асинхронный загрузчик COG-файлов успешно развернут");

    loop {
        // Атомарно захватываем свободную задачу по паттерну FOR UPDATE SKIP LOCKED
        match repo.pick_next_download_task().await {
            Ok(Some(task)) => {
                // Изолируем обработку таски в отдельный контекст логирования (Span)
                if let Err(e) = process_single_task(&state, &repo, task).await {
                    tracing::error!(error = %e, "Критический сбой внутри пайплайна обработки таски");
                }
                // Сразу уходим на следующий круг без сна, пока очередь не опустеет
                continue;
            }
            Ok(None) => {
                // Задач нет, воркер лениво уходит спать в блок tokio::select! ниже
            }
            Err(e) => {
                tracing::error!(error = %e, "Ошибка обращения к СУБД в цикле ожидания очереди");
            }
        }

        // Логика сна и мгновенного пробуждения по сигналу из API
        tokio::select! {
            _ = sleep(Duration::from_secs(state.config.workers.downloader_sleep_secs)) => {
                // Просто просыпаемся по таймеру и идем проверять Postgres заново
            }
            Some(signal) = rx.recv() => {
                match signal {
                    DownloaderSignal::RetryTask(uuid) => {
                        tracing::info!(granule_uuid = %uuid, "Получен принудительный сигнал /retry из API. Прерываю сон загрузчика.");
                    }
                }
            }
        }
    }
}

/// Оркестрация скачивания конкретной задачи внутри изолированного Tracing Span'а
#[tracing::instrument(
    name = "cog_download_pipeline",
    skip(state, repo, task),
    fields(task_id = task.id, granule_id = %task.granule_id)
)]
async fn process_single_task(
    state: &ModuleState,
    repo: &NasaRepository<'_>,
    task: DownloadTaskRow,
) -> Result<(), sqlx::Error> {
    tracing::info!("Начало выкачивания целевых спектральных COG-каналов из NASA Earthdata...");

    // 1. Извлекаем из базы данных URLs спектральных каналов для этой гранулы
    // (Используем сырой запрос к нашему репозиторию)
    let band_urls_json = match sqlx::query!(
        r#"SELECT band_urls FROM nasa.spatial_granules WHERE id = $1;"#,
        task.granule_id
    )
    .fetch_one(&state.db_pool)
    .await
    {
        Ok(row) => row.band_urls,
        Err(e) => {
            let err_msg = format!("Не удалось извлечь band_urls из СУБД: {}", e);
            repo.fail_task(task.id, &err_msg).await?;
            return Err(e);
        }
    };

    let bands: crate::models::BandUrls = match serde_json::from_value(band_urls_json) {
        Ok(b) => b,
        Err(e) => {
            let err_msg = format!("Ошибка десериализации JSONB мапы каналов: {}", e);
            repo.fail_task(task.id, &err_msg).await?;
            return Ok(());
        }
    };

    // 2. Формируем вектор задач на скачивание (Канал, Внешний URL, Имя файла в MinIO)
    let download_targets = vec![
        ("B05", bands.b5),
        ("B06", bands.b6),
        ("B07", bands.b7),
        ("B10", bands.b10),
        ("Fmask", bands.qa),
    ];

    // Инициализируем S3 клиент на базе нашего сеньорского конфига
    let s3_client = init_s3_client(&state.config.s3).await;
    let http_client = reqwest::Client::new();

    // 3. Скачиваем каналы последовательно, чтобы не забить гигабитный канал воркспейса
    for (band_name, remote_url) in download_targets {
        if remote_url.is_empty() {
            tracing::warn!(band = %band_name, "URL канала пуст, пропускаю...");
            continue;
        }

        tracing::info!(band = %band_name, source_url = %remote_url, "Запуск асинхронного стриминга канала...");

        // Генерируем уникальный ключ объекта в бакете, например: "scenes/UUID/B05.tif"
        let s3_object_key = format!("scenes/{}/{}.tif", task.granule_id, band_name);

        match stream_nasa_cog_to_s3(
            &http_client,
            &s3_client,
            &state.config.s3.bucket_name,
            &remote_url,
            &s3_object_key,
        )
        .await
        {
            Ok(_) => {
                tracing::info!(band = %band_name, "Канал успешно залит в MinIO S3.");
            }
            Err(err) => {
                let err_msg = format!("Пайплайн упал на канале [{}]: {}", band_name, err);
                repo.fail_task(task.id, &err_msg).await?;
                return Ok(());
            }
        }
    }

    // 4. Если все каналы гранулы скачались без ошибок — закрываем задачу
    tracing::info!("🎉 Все спектральные каналы успешно закешированы. Перевожу таску в [complete].");
    repo.complete_task(task.id).await?;

    Ok(())
}

/// Потоковый конвейер: reqwest (Stream bytes) -> AWS SDK S3 (Upload) без выделения RAM
async fn stream_nasa_cog_to_s3(
    http_client: &reqwest::Client,
    s3_client: &aws_sdk_s3::Client,
    bucket: &str,
    url: &str,
    s3_key: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Выполняем HTTP запрос к NASA. Включаем Range или обычный стрим
    let response = http_client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(format!("NASA API вернул статус-код ошибки: {}", response.status()).into());
    }

    // Превращаем тело HTTP ответа в асинхронный поток байт (Stream)
    let body_stream = response.bytes_stream();

    // Оборачиваем reqwest-стрим в ByteStream, который нативно понимает AWS SDK S3
    let s3_body = ByteStream::from_core_stream(body_stream);

    // Стреляем асинхронной загрузкой в MinIO
    s3_client
        .put_object()
        .bucket(bucket)
        .key(s3_key)
        .body(s3_body)
        .content_type("image/tiff")
        .send()
        .await?;

    Ok(())
}

/// Инициализация AWS S3 клиента, перенаправленного на локальный MinIO эндпоинт
async fn init_s3_client(cfg: &crate::config::S3StorageConfig) -> aws_sdk_s3::Client {
    let credentials = aws_sdk_s3::config::Credentials::new(
        &cfg.access_key_id,
        &cfg.secret_access_key,
        None,
        None,
        "Static",
    );

    let s3_config = aws_sdk_s3::config::Builder::new()
        .endpoint_url(&cfg.endpoint_url)
        .credentials_provider(credentials)
        .region(aws_sdk_s3::config::Region::new(cfg.region.clone()))
        // Критически важно для MinIO: включаем адресацию путей в стиле http://endpoint/bucket
        .force_path_style(true)
        .build();

    aws_sdk_s3::Client::from_conf(s3_config)
}
