CREATE TABLE IF NOT EXISTS backtest_time_price_bars (
  pair_address BLOB NOT NULL,
  resolution BIGINT NOT NULL,
  resolution_ts BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (pair_address, resolution_ts)
);
