CREATE TABLE IF NOT EXISTS backtest_closed_trades (
  id INTEGER NOT NULL PRIMARY KEY,
  backtest_id BIGINT NOT NULL,
  open_trade_metadata JSONB NOT NULL,
  close_trade_metadata JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS backtest_closed_trades__backtest_id ON backtest_closed_trades (backtest_id);
