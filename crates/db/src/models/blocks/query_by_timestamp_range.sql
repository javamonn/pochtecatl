-- Params: [min_timestamp, max_timestamp]
SELECT
  number,
  timestamp,
  pair_ticks 
FROM blocks
WHERE
  timestamp >= :min_timestamp AND timestamp <= :max_timestamp;
