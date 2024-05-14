CREATE TABLE IF NOT EXISTS blocks (
  number BIGINT NOT NULL PRIMARY KEY,  
  timestamp BIGINT NOT NULL,
  pair_ticks JSONB
);

CREATE UNIQUE INDEX IF NOT EXISTS blocks__number ON blocks (number);
CREATE INDEX IF NOT EXISTS blocks__timestamp on blocks (timestamp);
