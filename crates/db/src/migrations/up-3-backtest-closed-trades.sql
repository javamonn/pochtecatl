CREATE TABLE IF NOT EXISTS backtest_closed_trades (
  id INTEGER NOT NULL PRIMARY KEY,
  pair_address BLOB NOT NULL,
  backtest_id BIGINT NOT NULL,
  close_trade_block_timestamp BIGINT NOT NULL,
  open_trade_metadata JSONB NOT NULL,
  close_trade_metadata JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS backtest_closed_trades__backtest_id 
  ON backtest_closed_trades (backtest_id);
CREATE INDEX IF NOT EXISTS backtest_closed_trades__pair_address 
  ON backtest_closed_trades (pair_address);
CREATE INDEX IF NOT EXISTS backtest_closed_trades__close_trade_block_timestamp
  ON backtest_closed_trades (close_trade_block_timestamp);
