CREATE INDEX objects_bucket_stats_idx ON objects (bucket_id) INCLUDE (size, compressed_size)
    WHERE is_delete_marker = false AND is_folder_marker = false;
