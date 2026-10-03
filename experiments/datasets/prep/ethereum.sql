-- Ethereum transactions used for the paper's Ethereum workload.
--
-- Reconstruction: the original export query was not saved. This query
-- selects the same rows as the original export, which held all
-- transactions from block 23049662 (the first block of 2025-08-02 UTC,
-- 00:00:11) through block 23129291 (2025-08-13 02:58:11 UTC; the export was
-- taken a few minutes later, so it ends there): 19,066,487 transactions.
-- The original export's last row is (block 23129291, transaction_index 227);
-- ethereum.py --order paper drops anything beyond it.
--
-- Run it in the BigQuery console (or `bq query --use_legacy_sql=false`),
-- save the result to a table, and export that table to Cloud Storage as CSV
-- with a wildcard URI (e.g. gs://<your-bucket>/eth_tx_*.csv); then feed all
-- shards to ethereum.py. The block_timestamp filter only prunes partitions
-- (the table is partitioned by day); the block_number range is what defines
-- the window.

SELECT
  block_number,
  transaction_index,
  block_timestamp,
  from_address,
  to_address
FROM `bigquery-public-data.crypto_ethereum.transactions`
WHERE block_timestamp >= TIMESTAMP('2025-08-02 00:00:00 UTC')
  AND block_timestamp <  TIMESTAMP('2025-08-13 04:00:00 UTC')
  AND block_number BETWEEN 23049662 AND 23129291
ORDER BY block_number, transaction_index;
