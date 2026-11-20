ALTER TABLE object_versions DROP COLUMN compressed_size;
ALTER TABLE objects DROP COLUMN compressed_size;
ALTER TABLE buckets DROP COLUMN compression;
