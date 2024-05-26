-- Params: [backtest_id]

SELECT 
  pair_address,
  count(*) as trade_count
FROM
  backtest_closed_trades
WHERE
  backtest_id = :backtest_id
GROUP BY
  pair_address;
