-- =========================================================================
-- МИГРАЦИЯ 0002: СИМУЛЯЦИОННАЯ ПЕСОЧНИЦЫ ПОД ГИНТЕЛЛ
-- =========================================================================

CREATE SCHEMA IF NOT EXISTS sandbox_simulation;

-- 1. Синтетические 3D-линзы рудных тел
CREATE TABLE IF NOT EXISTS sandbox_simulation.synthetic_ore_bodies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL,
    center_east FLOAT8 NOT NULL,
    center_north FLOAT8 NOT NULL,
    center_elevation FLOAT8 NOT NULL,
    length_m FLOAT8 NOT NULL,
    width_m FLOAT8 NOT NULL,
    dip_angle FLOAT8 NOT NULL,
    geom_poly geometry(PolygonZ, 4326),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 2. Скважины
CREATE TABLE IF NOT EXISTS sandbox_simulation.synthetic_drillholes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    body_id UUID REFERENCES sandbox_simulation.synthetic_ore_bodies(id) ON DELETE CASCADE,
    hole_code VARCHAR(50) NOT NULL,
    easting FLOAT8 NOT NULL,
    northing FLOAT8 NOT NULL,
    elevation FLOAT8 NOT NULL,
    max_depth FLOAT8 NOT NULL,
    is_barren BOOLEAN NOT NULL DEFAULT FALSE,
    geom_line geometry(LineStringZ, 4326)
);

-- 3. Рекомендованные точки бурения ГКЗ
CREATE TABLE IF NOT EXISTS sandbox_simulation.recommended_gkz_proposals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    body_id UUID REFERENCES sandbox_simulation.synthetic_ore_bodies(id) ON DELETE CASCADE,
    proposed_code VARCHAR(50) NOT NULL,
    easting FLOAT8 NOT NULL,
    northing FLOAT8 NOT NULL,
    target_depth FLOAT8 NOT NULL,
    priority_weight FLOAT8 NOT NULL,
    geom_point geometry(PointZ, 4326)
);

CREATE INDEX IF NOT EXISTS idx_sim_ore_geom ON sandbox_simulation.synthetic_ore_bodies USING gist(geom_poly);
CREATE INDEX IF NOT EXISTS idx_sim_dh_geom ON sandbox_simulation.synthetic_drillholes USING gist(geom_line);
CREATE INDEX IF NOT EXISTS idx_sim_prop_geom ON sandbox_simulation.recommended_gkz_proposals USING gist(geom_point);

-- 4. БЛОЧНАЯ МОДЕЛЬ ЗАПАСОВ ГКЗ (3D КУБИКИ ДЛЯ ВИЗУАЛИЗАЦИИ)
CREATE TABLE IF NOT EXISTS sandbox_simulation.synthetic_voxels (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    body_id UUID REFERENCES sandbox_simulation.synthetic_ore_bodies(id) ON DELETE CASCADE,
    voxel_east FLOAT8 NOT NULL,       -- Центр кубика X
    voxel_north FLOAT8 NOT NULL,      -- Центр кубика Y
    voxel_elevation FLOAT8 NOT NULL,  -- Центр кубика Z
    voxel_size_m FLOAT8 NOT NULL,     -- Размер ребра (например, 10 метров)
    grade_au FLOAT8 NOT NULL,         -- Сгенерированное содержание золота (г/т)
    gkz_category VARCHAR(10) NOT NULL, -- Расчитанная категория ('C1', 'C2', 'Брак')
    geom_cube geometry(PolygonZ, 4326) -- 3D-контур кубика для неонового рендера в QGIS
);

CREATE INDEX IF NOT EXISTS idx_sim_voxel_geom ON sandbox_simulation.synthetic_voxels USING gist(geom_cube);
