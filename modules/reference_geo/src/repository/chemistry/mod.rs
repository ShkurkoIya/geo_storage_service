use crate::models::ChemicalElementDomain;
use geo_core::infra::params::PaginatedResponse;
use sqlx::{Pool, Postgres};

pub struct ChemistryRepository<'a> {
    pub pool: &'a Pool<Postgres>,
}

impl<'a> ChemistryRepository<'a> {
    pub fn new(pool: &'a Pool<Postgres>) -> Self {
        Self { pool }
    }

    // --- CREATE / UPDATE (Вставка с обновлением при конфликте - Ипсирт) ---
    pub async fn upsert_element(&self, element: &ChemicalElementDomain) -> Result<(), sqlx::Error> {
        sqlx::query!(
                r#"
                INSERT INTO reference_geo.chemical_elements (code, display_name, measurement_unit, display_color)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (code) DO UPDATE
                SET display_name = EXCLUDED.display_name,
                    measurement_unit = EXCLUDED.measurement_unit,
                    display_color = EXCLUDED.display_color;
                "#,
                element.code.trim(),
                element.display_name.trim(),
                element.measurement_unit.trim(),
                element.display_color.trim()
            )
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // --- READ (Получить элемент по его коду, например 'au') ---
    pub async fn find_by_code(
        &self,
        code: &str,
    ) -> Result<Option<ChemicalElementDomain>, sqlx::Error> {
        let row = sqlx::query_as!(
            ChemicalElementDomain,
            r#"
               SELECT code, display_name, measurement_unit, display_color
               FROM reference_geo.chemical_elements
               WHERE code = $1
               "#,
            code.trim()
        )
        .fetch_optional(self.pool)
        .await?;

        Ok(row)
    }

    // --- DELETE (Удалить элемент из справочника) ---
    pub async fn delete_element(&self, code: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "DELETE FROM reference_geo.chemical_elements WHERE code = $1",
            code.trim()
        )
        .execute(self.pool)
        .await?;
        Ok(())
    }

    // --- LIST & PAGINATION (Вывод всего справочника порциями) ---
    pub async fn find_paginated(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedResponse<ChemicalElementDomain>, sqlx::Error> {
        let limit = per_page.clamp(1, 100);
        let offset = (page - 1).max(0) * limit;

        // Считаем сколько всего элементов в справочнике
        let total_records =
            sqlx::query_scalar!("SELECT COUNT(*) FROM reference_geo.chemical_elements")
                .fetch_one(self.pool)
                .await?
                .unwrap_or(0);

        // Тянем нужную страницу
        let data = sqlx::query_as!(
            ChemicalElementDomain,
            r#"
               SELECT code, display_name, measurement_unit, display_color
               FROM reference_geo.chemical_elements
               ORDER BY code ASC
               LIMIT $1 OFFSET $2
               "#,
            limit,
            offset
        )
        .fetch_all(self.pool)
        .await?;

        let total_pages = (total_records as f64 / limit as f64).ceil() as i64;

        Ok(PaginatedResponse {
            data,
            current_page: page,
            per_page: limit,
            total_records,
            total_pages,
        })
    }
}
