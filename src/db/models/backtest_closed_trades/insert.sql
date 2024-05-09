INSERT INTO backtest_closed_trades
  (backtest_id, open_trade_metadata, close_trade_metadata)
VALUES
  (:backtest_id, :open_trade_metadata, :close_trade_metadata);
