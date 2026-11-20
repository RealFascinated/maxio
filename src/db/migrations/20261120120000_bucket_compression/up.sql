ALTER TABLE buckets ADD COLUMN compression TEXT NOT NULL DEFAULT 'none';
ALTER TABLE objects ADD COLUMN compressed_size BIGINT;
ALTER TABLE object_versions ADD COLUMN compressed_size BIGINT;
