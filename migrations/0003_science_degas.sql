CREATE SCHEMA IF NOT EXISTS science_degas;

-- ТАБЛИЦА ТЕРМАЛЬНЫХ АНОМАЛИЙ (Точечные выходы глубинного тепла / Разломы)
CREATE TABLE IF NOT EXISTS science_degas.thermal_hotspots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Если ссылка на NASA обязательна, ставим NOT NULL
    source_id UUID NOT NULL REFERENCES core_storage.data_sources(id) ON DELETE CASCADE,
    acquisition_time TIMESTAMP NOT NULL,
    temp_kelvin FLOAT8 NOT NULL,
    temp_celsius FLOAT8 GENERATED ALWAYS AS (temp_kelvin - 273.15) STORED,
    is_night_scene BOOLEAN NOT NULL DEFAULT TRUE,

    geom geometry(Point, 4326) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_thermal_hotspots_geom ON science_degas.thermal_hotspots USING gist(geom);

-- ТАБЛИЦА ГАЗОВЫХ АНОМАЛИЙ (Полигоны шлейфов метана и водорода высокого разрешения)
CREATE TABLE IF NOT EXISTS science_degas.gas_plumes(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES core_storage.data_sources(id) ON DELETE CASCADE,
    gas_type VARCHAR(10) NOT NULL, -- 'CH4', 'H2', 'CO2'
    pixel_value_max FLOAT8,
    pixel_value_avg FLOAT8,
    confidence_score INT CHECK (confidence_score BETWEEN 0 AND 100),

    geom geometry(MultiPolygon, 4326) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_gas_plumes_geom ON science_degas.gas_plumes USING gist(geom);

-- ТАБЛИЦА ВОДОРОДНЫХ СТРУКТУР (Геоботаника / Кольцевые депрессии "Ведьмины кольца")
CREATE TABLE IF NOT EXISTS science_degas.hydrogen_rings(
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Оставляем NULL, так как привязано к лицензии опционально через SET NULL
    deposit_id UUID REFERENCES core_storage.deposits(id) ON DELETE SET NULL,
    structure_type VARCHAR(50) NOT NULL DEFAULT 'Sub-circular depression', -- Ставим NOT NULL
    ring_diameter_meters FLOAT8,
    ndvi_drop_percentage FLOAT8,
    geological_setting TEXT,
    geom geometry(Polygon, 4326) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_hydrogen_rings_geom ON science_degas.hydrogen_rings USING gist(geom);

-- ============================================================================
-- АНАЛИТИЧЕСКОЕ ПРЕДСТАВЛЕНИЕ (PostGIS View) С ХАРДКОРНЫМ ХЕКОМ ТИПОВ
-- ============================================================================
-- Использованием функции COALESCE мы принудительно заставляем планировщик Postgres
-- прописать флаг NOT NULL в метаданных вьюхи. Теперь sqlx увидит строгие типы!

CREATE OR REPLACE VIEW science_degas.v_hydrogen_prime_targets AS
SELECT
    -- ХЕК: COALESCE говорит базе: "Тут гарантированно не-null UUID". СУБД фиксирует NOT NULL во View!
    COALESCE(r.id, '00000000-0000-0000-0000-000000000000'::uuid) AS ring_id,
    r.deposit_id, -- Оставляем nullable, так как он может быть пустой по бизнес-логике
    r.ring_diameter_meters,
    r.ndvi_drop_percentage,
    t.temp_celsius AS hotspot_temp_c,
    COALESCE(p.gas_type, 'UNKNOWN') AS gas_type, -- Гарантированная строка
    p.pixel_value_max AS gas_concentration_max,
    r.geom AS geom
FROM science_degas.hydrogen_rings r
JOIN science_degas.thermal_hotspots t ON ST_Intersects(r.geom, t.geom)
JOIN science_degas.gas_plumes p ON ST_Intersects(r.geom, p.geom)
WHERE p.gas_type = 'CH4' AND p.pixel_value_max > 200.0;
