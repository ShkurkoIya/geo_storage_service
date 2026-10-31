-- Удаляем зависимую таблицу-очередь задач (индексы по ней удалятся автоматически)
DROP TABLE IF EXISTS nasa.download_tasks CASCADE;

-- Удаляем родительскую таблицу метаданных гранул
DROP TABLE IF EXISTS nasa.spatial_granules CASCADE;

-- Удаляем кастомный тип ENUM для статусов задач
DROP TYPE IF EXISTS nasa.task_status CASCADE;

-- Удаляем схему nasa (только если она пустая, либо каскадом)
DROP SCHEMA IF EXISTS nasa CASCADE;
