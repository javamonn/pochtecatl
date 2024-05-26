-- Params: [backtest_id, pair_address, start_close_trade_block_timestamp, end_close_trade_block_timestamp]
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
  AND pair_address = :pair_address
  AND close_trade_block_timestamp >= :start_close_trade_block_timestamp
  AND close_trade_block_timestamp <= :end_close_trade_block_timestamp;
ORDER BY close_trade_block_timestamp ASC;
