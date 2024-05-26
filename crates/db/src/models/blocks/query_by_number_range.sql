SELECT
  number,
  timestamp,
  pair_ticks 
FROM blocks
WHERE
  number >= :min_number AND number <= :max_number;
