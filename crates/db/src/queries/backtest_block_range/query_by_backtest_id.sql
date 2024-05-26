-- Params: [backtest_id]

SELECT
    start_block.timestamp AS start_block_timestamp,
    end_block.timestamp AS end_block_timestamp
FROM
  backtests
JOIN
  blocks AS start_block ON start_block.number = backtests.start_block_number
JOIN
  blocks AS end_block ON end_block.number = backtests.end_block_number
WHERE
  id = :backtest_id;
