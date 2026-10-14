-- =========================================================================
-- 🗂️ ИНИЦИАЛИЗАЦИЯ ИЗОЛИРОВАННЫХ ДОМЕННЫХ СХЕМ И POSTGIS ПРОСТРАНСТВА
-- =========================================================================
CREATE EXTENSION IF NOT EXISTS postgis;

CREATE SCHEMA IF NOT EXISTS core_storage;
CREATE SCHEMA IF NOT EXISTS reference_geo;

-- =========================================================================
-- ⛰️ ДОМЕН CORE_STORAGE: РЕСУРСЫ НЕДР (МЕСТОРОЖДЕНИЯ, СКВАЖИНЫ, БЛОКИ ТЭО)
-- =========================================================================

-- 1. Месторождения / Лицензионные отводы
CREATE TABLE IF NOT EXISTS core_storage.deposits(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL UNIQUE,
    polygon_boundary geometry(Polygon, 4326),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 2. Таблица метаданных загруженных файлов (Bronze / Silver слои Data Lake)
CREATE TABLE IF NOT EXISTS core_storage.data_sources(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    deposit_id UUID REFERENCES core_storage.deposits(id) ON DELETE SET NULL,
    file_name VARCHAR(255) NOT NULL,
    sha256 VARCHAR(64) NOT NULL UNIQUE,
    deposit_name VARCHAR(100) NOT NULL DEFAULT 'Неизвестно',
    geological_year INT,
    status VARCHAR(50) NOT NULL, -- 'ingested', 'processed', 'error'
    ai_passport JSONB NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 3. Таблица устьев скважин (Gold ГИС-слой)
CREATE TABLE IF NOT EXISTS core_storage.drillhole_collars(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID REFERENCES core_storage.data_sources(id) ON DELETE CASCADE,
    hole_id VARCHAR(100) NOT NULL,
    line_id VARCHAR(100),
    easting_raw FLOAT8 NOT NULL,
    northing_raw FLOAT8 NOT NULL,
    collar_elevation FLOAT8 NOT NULL DEFAULT 0.0, -- ЧИСТЫЙ МЕЖДУНАРОДНЫЙ СТАНДАРТ
    max_depth FLOAT8 NOT NULL DEFAULT 0.0,
    geom_world geometry(LineString, 4326),          -- 3D траектория ствола
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT unique_hole_per_source UNIQUE (source_id, hole_id)
);

-- 4. Таблица интервалов опробования (Химический артхаус — ТЭО и ML слой)
CREATE TABLE IF NOT EXISTS core_storage.drillhole_intervals(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collar_id UUID REFERENCES core_storage.drillhole_collars(id) ON DELETE CASCADE,
    sample_id VARCHAR(100),
    depth_from FLOAT8 NOT NULL,
    depth_to FLOAT8 NOT NULL,
    thickness FLOAT8 NOT NULL,
    quality_attributes JSONB NOT NULL DEFAULT '{}'::jsonb, -- Карта элементов "au", "ag"
    geom_poly geometry(LineString, 4326),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 5. Поблочная оценка балансовых запасов ГКЗ (Экономический ML-слой недр)
CREATE TABLE IF NOT EXISTS core_storage.geological_blocks(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    deposit_id UUID REFERENCES core_storage.deposits(id) ON DELETE CASCADE,
    block_code VARCHAR(100) NOT NULL,              -- ЧИСТЫЙ МЕЖДУНАРОДНЫЙ СТАНДАРТ (block_number -> code)
    ore_body_name VARCHAR(100) NOT NULL,           -- Название жилы ('Жила Параллельная')
    horizon_elevation FLOAT8 NOT NULL,             -- ЧИСТЫЙ МЕЖДУНАРОДНЫЙ СТАНДАРТ (horizon_level -> elevation)
    resource_category VARCHAR(10) NOT NULL,        -- ЧИСТЫЙ МЕЖДУНАРОДНЫЙ СТАНДАРТ (reserve_category -> resource)
    area_m2 FLOAT8,
    thickness_m FLOAT8,
    volume_m3 FLOAT8,
    ore_tonnage FLOAT8,
    dip_angle FLOAT8,
    grade_and_economics_attributes JSONB NOT NULL DEFAULT '{}'::jsonb, -- Динамический ТЭО контейнер ценности
    geom_poly geometry(Polygon, 4326),             -- Пространственный 3D контур блока ГКЗ
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT uniq_block_per_deposit UNIQUE (deposit_id, block_code, ore_body_name)
);

-- =========================================================================
-- 📐 ДОМЕН REFERENCE_GEO: ГОСУДАРСТВЕННАЯ ГЕОДЕЗИЧЕСКАЯ ОПОРА И СПРАВОЧНИКИ
-- =========================================================================

-- 6. Пункты Государственной геодезической сети (ГГС / ggs_nodes.json)
CREATE TABLE IF NOT EXISTS reference_geo.ggs_nodes (
    id INT PRIMARY KEY,
    node_index VARCHAR(20) NOT NULL UNIQUE,
    name VARCHAR(100) NOT NULL,
    accuracy_class VARCHAR(10),
    map_sheet_50k VARCHAR(50),
    map_sheet_200k VARCHAR(50),
    height_system VARCHAR(10),
    height_determination_method VARCHAR(10),
    leveling_type VARCHAR(50),
    monument_mark VARCHAR(20),
    center_structure_type VARCHAR(20),
    surface_monument_type VARCHAR(20),
    monument_height_m FLOAT8 NOT NULL DEFAULT 0.0,
    region_code VARCHAR(20),
    region_name VARCHAR(100),
    district_name VARCHAR(100),
    latitude_raw FLOAT8 NOT NULL,
    longitude_raw FLOAT8 NOT NULL,
    geom_global geometry(PointZ, 4326),
    geom_local geometry(PointZ, 3857),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 7. Утраченные/Поврежденные маркшейдерские пункты (lost_nodes.json)
CREATE TABLE IF NOT EXISTS reference_geo.lost_nodes (
    id INT PRIMARY KEY,
    node_index VARCHAR(20) NOT NULL UNIQUE,
    name VARCHAR(100) NOT NULL,
    accuracy_class VARCHAR(10),
    map_sheet_50k VARCHAR(50),
    map_sheet_200k VARCHAR(50),
    height_determination_method VARCHAR(10),
    monument_mark VARCHAR(20),
    center_structure_type VARCHAR(50),
    surface_monument_type VARCHAR(20),
    monument_height_m FLOAT8 NOT NULL DEFAULT 0.0,
    region_code VARCHAR(20),
    region_name VARCHAR(100),
    district_name VARCHAR(100),
    condition_monument VARCHAR(100),
    condition_center VARCHAR(100),
    verification_year INT DEFAULT NULL,
    latitude_raw FLOAT8 NOT NULL,
    longitude_raw FLOAT8 NOT NULL,
    geom_global geometry(PointZ, 4326),
    geom_local geometry(PointZ, 28415),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 8. Нивелирные реперы высотных сетей (Запорожье / niveler_nodes.json)
CREATE TABLE IF NOT EXISTS reference_geo.niveler_nodes (
    id INT PRIMARY KEY,
    node_index VARCHAR(20) NOT NULL UNIQUE,
    name VARCHAR(100) NOT NULL,
    region_name VARCHAR(100),
    district_name VARCHAR(255),
    latitude_raw FLOAT8 NOT NULL,
    longitude_raw FLOAT8 NOT NULL,
    geom_global geometry(PointZ, 4326),
    geom_local geometry(PointZ, 3857),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 9. Официальный справочник химических элементов
CREATE TABLE IF NOT EXISTS reference_geo.chemical_elements (
    code VARCHAR(10) PRIMARY KEY,
    display_name VARCHAR(100) NOT NULL,
    measurement_unit VARCHAR(20) NOT NULL,
    display_color VARCHAR(7) NOT NULL DEFAULT '#FFFFFF',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- =========================================================================
-- ⚡ ИНДЕКСЫ
-- =========================================================================
CREATE INDEX IF NOT EXISTS idx_drillhole_geom_world ON core_storage.drillhole_collars USING gist(geom_world);
CREATE INDEX IF NOT EXISTS idx_blocks_search ON core_storage.geological_blocks(ore_body_name, resource_category);
CREATE INDEX IF NOT EXISTS idx_blocks_attributes ON core_storage.geological_blocks USING gin(grade_and_economics_attributes);
CREATE INDEX IF NOT EXISTS idx_ggs_nodes_geom_global ON reference_geo.ggs_nodes USING gist(geom_global);
CREATE INDEX IF NOT EXISTS idx_ggs_nodes_geom_local ON reference_geo.ggs_nodes USING gist(geom_local);
CREATE INDEX IF NOT EXISTS idx_ggs_nodes_analytics ON reference_geo.ggs_nodes(accuracy_class, region_name);
CREATE INDEX IF NOT EXISTS idx_lost_nodes_geom_spatial ON reference_geo.lost_nodes USING gist(geom_local);
CREATE INDEX IF NOT EXISTS idx_lost_nodes_condition ON reference_geo.lost_nodes(condition_center, condition_monument);
CREATE INDEX IF NOT EXISTS idx_niveler_nodes_spatial_global ON reference_geo.niveler_nodes USING gist(geom_global);
CREATE INDEX IF NOT EXISTS idx_niveler_nodes_spatial_local ON reference_geo.niveler_nodes USING gist(geom_local);
CREATE INDEX IF NOT EXISTS idx_niveler_nodes_district_search ON reference_geo.niveler_nodes(district_name);
