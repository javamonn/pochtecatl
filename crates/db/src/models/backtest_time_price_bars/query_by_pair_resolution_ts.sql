-- Params [pair_address, start_resolution_ts, end_resolution_ts]
SELECT
  pair_address,
  resolution,
  resolution_ts,
  data
FROM
  backtest_time_price_bars
WHERE
  pair_address = :pair_address
  AND resolution_ts >= :start_resolution_ts
  AND resolution_ts <= :end_resolution_ts;
ORDER BY
  resolution_ts ASC;
