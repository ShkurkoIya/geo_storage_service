CREATE SCHEMA IF NOT EXISTS nasa;

-- Создаем перечисление (ENUM) для статусов задач
CREATE TYPE nasa.task_status AS ENUM (
    'pending',
    'processing',
    'complete',
    'failed'
);

-- Создаем родительскую таблицу НАУЧНЫХ КОЛЛЕКЦИЙ
CREATE TABLE nasa.collections (
    concept_id VARCHAR(100) PRIMARY KEY, -- Например: 'C1748058432-LPCLOUD'
    short_name VARCHAR(50) NOT NULL,     -- Например: 'HLSL30'
    version_id VARCHAR(20) NOT NULL,     -- Например: '061' или '2.0'
    title TEXT NOT NULL,                 -- Полное название (Landsat 8 Harmonized...)
    
    -- Абстрактная мапа спектральных каналов, которую поддерживает эта коллекция
    -- Например: {"B05": "NIR", "B06": "SWIR1", "B10": "TIRS"}
    -- Это избавит аналитику от хардкода бэндов!
    band_aliases JSONB NOT NULL DEFAULT '{}'::jsonb, 
    
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- Таблица для хранения дампированных метаданных гранул (Обновленная)
CREATE TABLE nasa.spatial_granules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    nasa_granule_id VARCHAR(255) UNIQUE NOT NULL,
    collection_concept_id VARCHAR(100) NOT NULL REFERENCES nasa.collections(concept_id),
    acquisition_date TIMESTAMP WITH TIME ZONE NOT NULL,
    cloud_cover REAL NOT NULL,
    footprint geometry(Polygon, 4326) NOT NULL,

    -- Сюда пишем все HTTPS ссылки: 
    -- Для HLS:    {"B01": "https://...", "B02": "https://...", "Fmask": "https://..."}
    -- Для MODIS:  {"hdf": "https://...", "opendap": "https://...", "xml": "https://..."}
    band_urls JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- То же самое, но для протокола S3 (на случай если воркер в AWS us-west-2):
    -- {"hdf": "s3://...", "B01": "s3://..."}
    s3_urls JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- Ссылка на дефолтную JPG-превьюшку (из rel: .../browse#)
    browse_url TEXT,

    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE INDEX idx_granules_collection ON nasa.spatial_granules(collection_concept_id);

-- Геопространственный GiST-индекс для моментального поиска по координатам
CREATE INDEX idx_granules_footprint
ON nasa.spatial_granules USING gist(footprint);

-- Индекс по дате для быстрой фильтрации временных рядов (таймсерий)
CREATE INDEX idx_granules_acquisition_date
ON nasa.spatial_granules (acquisition_date DESC);

-- Таблица-очередь задач для воркера-загрузчика (downloader)
CREATE TABLE nasa.download_tasks (
    id BIGSERIAL PRIMARY KEY,
    granule_id UUID NOT NULL REFERENCES nasa.spatial_granules(id) ON DELETE CASCADE,
    status nasa.task_status DEFAULT 'pending' NOT NULL,
    attempts INT DEFAULT 0 NOT NULL,
    locked_at TIMESTAMP WITH TIME ZONE,
    error_message TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- Частичный индекс: воркер будет сканировать только те задачи, которые реально нужно выполнить
CREATE INDEX idx_download_tasks_pending_status
ON nasa.download_tasks(status)
WHERE status = 'pending';

-- Индекс для фонового клинера, который будет сбрасывать зависшие таски по таймауту
CREATE INDEX idx_download_tasks_stuck_processing
ON nasa.download_tasks(status, locked_at)
WHERE status = 'processing';
