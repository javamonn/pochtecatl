-- Params: [backtest_id]
SELECT
  id,
  backtest_id,
  pair_address,
  close_trade_block_timestamp,
  open_trade_metadata,
  close_trade_metadata
FROM backtest_closed_trades
WHERE
  backtest_id = :backtest_id
