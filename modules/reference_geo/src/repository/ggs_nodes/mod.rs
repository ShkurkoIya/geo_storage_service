pub mod ggs;
pub mod lost;
pub mod niveler;

use sqlx::{Pool, Postgres};

pub struct GgsNodesRepository<'a> {
    pub pool: &'a Pool<Postgres>,
}

impl<'a> GgsNodesRepository<'a> {
    pub fn new(pool: &'a Pool<Postgres>) -> Self {
        Self { pool }
    }
}
