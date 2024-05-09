SELECT
  number,
  timestamp,
  uniswap_v2_pairs
FROM blocks
WHERE
  number >= :min_number AND number <= :max_number
