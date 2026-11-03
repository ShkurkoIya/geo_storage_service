ALTER TABLE nasa.spatial_granules 
    -- 1. Расширяем тип геометрии до общего, чтобы не падать на MultiPolygon
    ALTER COLUMN footprint TYPE geometry(Geometry, 4326),
    -- 2. Разрешаем отсутствие облачности для радаров
    ALTER COLUMN cloud_cover DROP NOT NULL;

ALTER TABLE nasa.spatial_granules
    -- 3. Переделываем уникальность на составную
    DROP CONSTRAINT IF EXISTS spatial_granules_nasa_granule_id_key,
    ADD CONSTRAINT unique_granule_in_collection UNIQUE (nasa_granule_id, collection_concept_id);

-- 4. Добавляем поля отслеживания версий на стороне NASA
ALTER TABLE nasa.spatial_granules 
    ADD COLUMN IF NOT EXISTS nasa_updated_at TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS revision_id INT DEFAULT 1;

-- 5. Прокачиваем очередь задач под параллельных воркеров
ALTER TABLE nasa.download_tasks 
    ADD COLUMN IF NOT EXISTS worker_id UUID,
    ADD COLUMN IF NOT EXISTS error_count INT DEFAULT 0 NOT NULL;

-- 6. Добавляем GIN-индекс для моментального поиска по бэндам внутри JSONB
-- Это позволит делать запросы вида: WHERE band_urls ?? 'B05' (есть ли канал B05 у гранулы)
CREATE INDEX IF NOT EXISTS idx_granules_band_urls_gin 
ON nasa.spatial_granules USING gin (band_urls);

