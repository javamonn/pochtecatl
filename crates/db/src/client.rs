use eyre::Result;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

#[cfg(test)]
const INIT_PRAGMAS: &'static str = concat!(
    // Pragmas
    // better write-concurrency
    "PRAGMA journal_mode = WAL;",
    // fsync only in critical moments
    "PRAGMA synchronous = NORMAL;",
    // write WAL changes back every 1000 pages, for an in average 1MB WAL file.
    "PRAGMA wal_autocheckpoint = 1000;",
    // free some space by truncating possibly massive WAL files from the last run.
    "PRAGMA wal_checkpoint(TRUNCATE);",
    // sleep if the database is busy
    "PRAGMA busy_timeout = 250;",
    // enforce foreign keys
    "PRAGMA foreign_keys = ON;",
);
#[cfg(not(test))]
const INIT_PRAGMAS: &'static str = concat!(
    // Pragmas
    // better write-concurrency
    "PRAGMA journal_mode = WAL;",
    // fsync only in critical moments
    "PRAGMA synchronous = NORMAL;",
    // write WAL changes back every 1000 pages, for an in average 1MB WAL file.
    "PRAGMA wal_autocheckpoint = 1000;",
    // free some space by truncating possibly massive WAL files from the last run.
    "PRAGMA wal_checkpoint(TRUNCATE);",
    // sleep if the database is busy
    "PRAGMA busy_timeout = 250;",
    // enforce foreign keys
    "PRAGMA foreign_keys = ON;",
    // increase cache size to ~200mb
    "PRAGMA cache_size = -200000;",
);

const INIT_MIGRATIONS: &'static str = concat!(
    // Idempotent up migrations
    include_str!("migrations/up-1-blocks.sql"),
    include_str!("migrations/up-2-backtests.sql"),
    include_str!("migrations/up-3-backtest-closed-trades.sql"),
);

pub fn connect(url: &String) -> Result<Pool<SqliteConnectionManager>> {
    let pool =
        Pool::new(SqliteConnectionManager::file(url).with_init(|c| c.execute_batch(INIT_PRAGMAS)))?;

    // Run the up migrations
    {
        let mut conn = pool.get()?;
        let tx = conn.transaction()?;
        tx.execute_batch(INIT_MIGRATIONS)?;
        tx.commit()?;
    }

    Ok(pool)
}
