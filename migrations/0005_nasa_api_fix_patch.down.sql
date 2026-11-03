-- 1. Удаляем GIN-индекс для band_urls
DROP INDEX IF EXISTS nasa.idx_granules_band_urls_gin;

-- 2. Удаляем добавленные поля из таблицы очередей задач download_tasks
ALTER TABLE nasa.download_tasks 
    DROP COLUMN IF EXISTS worker_id,
    DROP COLUMN IF EXISTS error_count;

-- 3. Удаляем поля версионирования NASA из таблицы гранул
ALTER TABLE nasa.spatial_granules 
    DROP COLUMN IF EXISTS nasa_updated_at,
    DROP COLUMN IF EXISTS revision_id;

-- 4. Возвращаем обратно ограничение уникальности только по nasa_granule_id
ALTER TABLE nasa.spatial_granules
    DROP CONSTRAINT IF EXISTS unique_granule_in_collection,
    ADD CONSTRAINT spatial_granules_nasa_granule_id_key UNIQUE (nasa_granule_id);

-- 5. Возвращаем жесткое ограничение NOT NULL на cloud_cover
-- ⚠️ ВНИМАНИЕ: Перед выполнением этой команды убедитесь, что в базе нет строк с NULL в cloud_cover, 
-- иначе Postgres выдаст ошибку. Если такие строки есть, их нужно сначала заполнить дефолтным значением (например, -1.0)
-- UPDATE nasa.spatial_granules SET cloud_cover = -1.0 WHERE cloud_cover IS NULL;
ALTER TABLE nasa.spatial_granules 
    ALTER COLUMN cloud_cover SET NOT NULL;

-- 6. Сужаем тип геометрии обратно строго до Polygon
-- ⚠️ ВНИМАНИЕ: Если во время тестов в базу успели залететь MultiPolygon или другие типы геометрий, 
-- эта команда упадет. Базу придется сначала очистить от невалидных типов.
ALTER TABLE nasa.spatial_granules 
    ALTER COLUMN footprint TYPE geometry(Polygon, 4326);

