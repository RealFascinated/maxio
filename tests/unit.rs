#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::redundant_pattern_matching
)]

#[path = "unit/checksum.rs"]
mod checksum;
#[path = "unit/config.rs"]
mod config;
#[path = "unit/console_objects.rs"]
mod console_objects;
#[path = "unit/cors.rs"]
mod cors;
#[path = "unit/db_caches.rs"]
mod db_caches;
#[path = "unit/hashing.rs"]
mod hashing;
#[path = "unit/iam.rs"]
mod iam;
#[path = "unit/listing.rs"]
mod listing;
#[path = "unit/metrics.rs"]
mod metrics;
#[path = "unit/object.rs"]
mod object;
#[path = "unit/signature_v4.rs"]
mod signature_v4;
#[path = "unit/storage.rs"]
mod storage;
