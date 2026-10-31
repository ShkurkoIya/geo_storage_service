// use crate::handlers::simulation::SyntheticVoxel;
use crate::models::SyntheticVoxel;
use crate::models::{
    DataSourceDomain, DepositDomain, DrillholeCollarDomain, DrillholeIntervalDomain,
    GeologicalBlockDomain,
};
use sqlx::{Pool, Postgres};
use uuid::Uuid;

pub struct GeoRepository<'a> {
    pool: &'a Pool<Postgres>,
}

impl<'a> GeoRepository<'a> {
    pub fn new(pool: &'a Pool<Postgres>) -> Self {
        Self { pool }
    }

    // =========================================================================
    // ДОМЕН: CORE_STORAGE (Ресурсы недр — Месторождения, Скважины, Блоки ТЭО)
    // =========================================================================
    pub async fn insert_deposit(&self, node: &DepositDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO core_storage.deposits (id, name, polygon_boundary)
            VALUES ($1, $2, CASE WHEN $3::text IS NOT NULL THEN ST_GeomFromText($3, 4326) ELSE NULL END)
            ON CONFLICT (name) DO UPDATE SET polygon_boundary = EXCLUDED.polygon_boundary;
            "#,
            node.id,
            node.name.trim(),
            node.polygon_boundary
        )
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Метаданные загруженного файла
    pub async fn insert_data_source(&self, node: &DataSourceDomain) -> Result<(), sqlx::Error> {
        // Вытягиваем паспорт, так как он обёрнут в Option внутри структуры
        let passport_json = node
            .ai_passport
            .as_ref()
            .map(|p| serde_json::to_value(p).unwrap());

        sqlx::query!(
            r#"
            INSERT INTO core_storage.data_sources (id, deposit_id, file_name, sha256, deposit_name, geological_year, status, ai_passport)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (sha256) DO UPDATE SET status = $7, ai_passport = $8;
            "#,
            node.id,
            node.deposit_id,
            node.file_name,
            node.sha256,
            node.deposit_name.trim(),
            node.geological_year,
            node.status,
            passport_json
        )
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Устья скважин
    pub async fn insert_drillhole_collar(
        &self,
        node: &DrillholeCollarDomain,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO core_storage.drillhole_collars (id, source_id, hole_id, line_id, easting_raw, northing_raw, collar_elevation, max_depth, geom_world)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                    ST_SetSRID(ST_MakeLine(ST_MakePoint($5, $6, $7), ST_MakePoint($5, $6, $7 - $8)), 4326))
            ON CONFLICT (source_id, hole_id) DO NOTHING;
            "#,
            node.id,
            node.source_id,
            node.hole_id,
            node.line_id,
            node.easting_raw,
            node.northing_raw,
            node.collar_elevation,
            node.max_depth
        )
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Интервалы опробования (Внутреннее содержание скважины — ТЭО и динамический JSONB)
    pub async fn insert_drillhole_interval(
        &self,
        node: &DrillholeIntervalDomain,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
              r#"
              INSERT INTO core_storage.drillhole_intervals (id, collar_id, sample_id, depth_from, depth_to, thickness, quality_attributes, geom_poly)
              VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)
              "#,
              node.id,
              node.collar_id,
              node.sample_id,
              node.depth_from,
              node.depth_to,
              node.thickness,
              serde_json::to_value(&node.quality_attributes).unwrap()
          )
          .execute(self.pool)
          .await?;

        Ok(())
    }

    /// Поблочная оценка балансовых запасов ГКЗ
    pub async fn insert_geological_block(
        &self,
        node: &GeologicalBlockDomain,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO core_storage.geological_blocks (
                id, deposit_id, block_code, ore_body_name, horizon_elevation, resource_category,
                area_m2, thickness_m, volume_m3, ore_tonnage, dip_angle, grade_and_economics_attributes, geom_poly
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                    CASE WHEN $13::text IS NOT NULL THEN ST_GeomFromText($13, 4326) ELSE NULL END)
            ON CONFLICT (deposit_id, block_code, ore_body_name) DO NOTHING;
            "#,
            node.id,
            node.deposit_id,
            node.block_code,
            node.ore_body_name,
            node.horizon_elevation,
            node.resource_category,
            node.area_m2,
            node.thickness_m,
            node.volume_m3,
            node.ore_tonnage,
            node.dip_angle,
            node.grade_and_economics_attributes,
            node.geom_poly
        )
        .execute(self.pool)
        .await?;

        Ok(())
    }
}

impl<'a> GeoRepository<'a> {
    pub async fn insert_synthetic_ore_body(
        &self,
        id: Uuid,
        name: &str,
        east: f64,
        north: f64,
        elev: f64,
        len: f64,
        width: f64,
        dip: f64,
        wkt_poly: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO sandbox_simulation.synthetic_ore_bodies (
                id, name, center_east, center_north, center_elevation,
                length_m, width_m, dip_angle, geom_poly
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, ST_GeomFromText($9, 4326))
            ON CONFLICT (id) DO NOTHING;
            "#,
            id,
            name.trim(),
            east,
            north,
            elev,
            len,
            width,
            dip,
            wkt_poly
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_synthetic_drillhole(
        &self,
        id: Uuid,
        body_id: Uuid,
        code: &str,
        east: f64,
        north: f64,
        elev: f64,
        depth: f64,
        is_barren: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO sandbox_simulation.synthetic_drillholes (
                id, body_id, hole_code, easting, northing, elevation, max_depth, is_barren, geom_line
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                    ST_SetSRID(ST_MakeLine(ST_MakePoint($4, $5, $6), ST_MakePoint($4, $5, $6 - $7)), 4326))
            ON CONFLICT (id) DO NOTHING;
            "#,
            id, body_id, code.trim(), east, north, elev, depth, is_barren
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }

    /// 📦 ПАКЕТНАЯ ЗАЛИВКА ВОКСЕЛЕЙ: Нативный 3D-конструктор PostGIS
    pub async fn insert_synthetic_voxel(&self, voxel: &SyntheticVoxel) -> Result<(), sqlx::Error> {
        // Микро-шаг сетки в градусах для формирования ребра кубика (около 11 метров)
        let d = 0.00005;

        sqlx::query!(
            r#"
            INSERT INTO sandbox_simulation.synthetic_voxels (
                id, body_id, voxel_east, voxel_north, voxel_elevation, voxel_size_m, grade_au, gkz_category, geom_cube
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                -- 🚀 ЯВНОЕ ПРИВЕДЕНИЕ ТИПОВ: Подсказываем sqlx, что мы вычитаем FLOAT8!
                ST_Force3D(
                    ST_SetSRID(
                        ST_MakeEnvelope(
                            ($3::float8 - $9::float8),
                            ($4::float8 - $9::float8),
                            ($3::float8 + $9::float8),
                            ($4::float8 + $9::float8)
                        ),
                        4326
                    )
                )
            )
            ON CONFLICT (id) DO NOTHING;
            "#,
            voxel.id,
            voxel.body_id,
            voxel.voxel_east,       // $3 - Долгота X (89.15)
            voxel.voxel_north,      // $4 - Широта Y (54.55)
            voxel.voxel_elevation,  // $5 - Высота Z (200.0)
            voxel.voxel_size_m,     // $6 - Размер 10м
            voxel.grade_au,         // $7 - Содержание золота
            voxel.gkz_category,     // $8 - Категория ГКЗ
            d                       // $9 - Наш дельта-шаг в градусах!
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }
}
