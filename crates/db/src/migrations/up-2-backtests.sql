CREATE TABLE IF NOT EXISTS backtests (
  id INTEGER NOT NULL PRIMARY KEY,
  start_block_number INTEGER NOT NULL,
  end_block_number INTEGER NOT NULL,
  created_at BIGINT NOT NULL
);
