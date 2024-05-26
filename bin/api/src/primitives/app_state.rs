use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

#[derive(Clone)]
pub struct AppState {
    db: Pool<SqliteConnectionManager>
}

impl AppState {
    pub fn new(db: Pool<SqliteConnectionManager>) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &Pool<SqliteConnectionManager> {
        &self.db
    }
}
