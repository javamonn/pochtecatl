SELECT
  id,
  backtest_id,
  open_trade_metadata,
  close_trade_metadata
FROM backtest_closed_trades
WHERE
  backtest_id = :backtest_id
