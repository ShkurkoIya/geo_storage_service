-- =========================================================================
-- 🚨 СКРИПТ ОТКАТА МИГРАЦИИ (0001_init.down.sql)
-- =========================================================================

-- 1. Сносим индексы оптимизации
DROP INDEX IF EXISTS reference_geo.idx_niveler_nodes_district_search;
DROP INDEX IF EXISTS reference_geo.idx_niveler_nodes_spatial_local;
DROP INDEX IF EXISTS reference_geo.idx_niveler_nodes_spatial_global;
DROP INDEX IF EXISTS reference_geo.idx_lost_nodes_condition;
DROP INDEX IF EXISTS reference_geo.idx_lost_nodes_geom_spatial;
DROP INDEX IF EXISTS reference_geo.idx_ggs_nodes_analytics;
DROP INDEX IF EXISTS reference_geo.idx_ggs_nodes_geom_local;
DROP INDEX IF EXISTS reference_geo.idx_ggs_nodes_geom_global;
DROP INDEX IF EXISTS core_storage.idx_blocks_attributes;
DROP INDEX IF EXISTS core_storage.idx_blocks_search;
DROP INDEX IF EXISTS core_storage.idx_drillhole_geom_world;

-- 2. Выкашиваем таблицы справочников и геодезической опоры (reference_geo)
DROP TABLE IF EXISTS reference_geo.chemical_elements CASCADE;
DROP TABLE IF EXISTS reference_geo.niveler_nodes CASCADE;
DROP TABLE IF EXISTS reference_geo.lost_nodes CASCADE;
DROP TABLE IF EXISTS reference_geo.ggs_nodes CASCADE;

-- 3. Выкашиваем таблицы недр и ТЭО (core_storage)
DROP TABLE IF EXISTS core_storage.geological_blocks CASCADE;
DROP TABLE IF EXISTS core_storage.drillhole_intervals CASCADE;
DROP TABLE IF EXISTS core_storage.drillhole_collars CASCADE;
DROP TABLE IF EXISTS core_storage.data_sources CASCADE;
DROP TABLE IF EXISTS core_storage.deposits CASCADE;

-- 4. Аннулируем изолированные доменные схемы и пространственные расширения
DROP SCHEMA IF EXISTS reference_geo CASCADE;
DROP SCHEMA IF EXISTS core_storage CASCADE;
-- Постгис не удаляем, пускай висит глобально, он нам пригодится
