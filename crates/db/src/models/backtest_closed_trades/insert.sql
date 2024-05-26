INSERT INTO backtest_closed_trades (
  backtest_id,
  pair_address,
  close_trade_block_timestamp,
  open_trade_metadata,
  close_trade_metadata
)
VALUES (
  :backtest_id, 
  :pair_address, 
  :close_trade_block_timestamp,
  :open_trade_metadata, 
  :close_trade_metadata
);
