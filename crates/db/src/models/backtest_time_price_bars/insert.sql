INSERT OR REPLACE INTO backtest_time_price_bars
  (pair_address, resolution, resolution_ts, data)
VALUES
  (:pair_address, :resolution, :resolution_ts, :data);
