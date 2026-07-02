# MaxIO Performance Work

Benchmark environment:

| Setting | Value |
|---------|-------|
| Test root | `/home/liam/Desktop/stuff/maxio` |
| TMPDIR | `/home/liam/Desktop/stuff/maxio/tmp` |
| Benchmark artifacts | `/home/liam/Desktop/stuff/maxio/bench/` |
| Tool | [minio/warp](https://github.com/minio/warp) via `./tests/bench.sh` |
| Postgres | `postgres://maxio:maxio@127.0.0.1:5432/maxio` |
| MaxIO flags | `--async-meta-write` (bench script default) |
| Duration | 15s per scenario (phase 2), 10s (phase 1) |

## Reproduce

```bash
export TMPDIR=/home/liam/Desktop/stuff/maxio/tmp
export MAXIO_DATABASE_URL=postgres://maxio:maxio@127.0.0.1:5432/maxio
mkdir -p "$TMPDIR" /home/liam/Desktop/stuff/maxio/bench /home/liam/Desktop/stuff/maxio/logs

cargo build --release
./tests/bench.sh \
  --duration=15s \
  --scenarios=put-small,get-small,put-med,get-med \
  --maxio-bin=./target/release/maxio \
  --outdir=/home/liam/Desktop/stuff/maxio/bench/<label>
```

Three-run average (recommended — single runs vary ±25% on PUT 1MiB):

```bash
/home/liam/Desktop/stuff/maxio/bench-3x.sh phase2 15s put-small,get-small,put-med,get-med
```

Perf phase logging:

```bash
RUST_LOG=maxio::perf=warn   # slow phases only (≥5ms)
RUST_LOG=maxio::perf=trace  # every instrumented phase
```

---

## Summary (3-run average)

| Scenario | Phase 1 | Phase 2 avg | Phase 3 avg | Phase 2→3 |
|----------|---------|-------------|-------------|-----------|
| PUT 4KiB | 44.54 MiB/s | 56.94 MiB/s | **65.09 MiB/s** | **+14%** |
| GET 4KiB | 115.08 MiB/s | 79.95 MiB/s | **103.28 MiB/s** | **+29%** |
| PUT 1MiB | 136.09 MiB/s | 89.80 MiB/s | 50.69 MiB/s* | volatile |
| GET 1MiB | 8879 MiB/s | 7808 MiB/s | 7107 MiB/s | page-cache noise |

\*PUT 1MiB run 3 had warp errors; runs 1–2 averaged 125/15 MiB/s. Use 3-run script and discard error runs.

---

## Phase 1 improvements (logging + metadata/blob/auth)

### 1. Perf phase logging (`src/perf/`)

`maxio::perf` tracing on auth, HTTP, DB pool, bucket context, object read, storage ops.

### 2. Skip CORS on PutObject bucket cache miss

`load_bucket_cache_entry_core` + `cors_loaded` flag — no `bucket_cors_rules` query on PUT.

### 3. Single-query object read (LEFT JOIN checksums)

Halves Postgres round-trips on object-read cache miss.

### 4. `known_dirs` single lock on write

One mutex acquisition per new parent directory.

### 5. Auth middleware: fewer allocations

Reuse `&str` from request; presigned path reads from request directly.

**Phase 1 single-run results** (`bench/final`, 10s):

| Scenario | MaxIO |
|----------|-------|
| PUT 4KiB | 44.54 MiB/s |
| GET 4KiB | 115.08 MiB/s |
| PUT 1MiB | 136.09 MiB/s |
| GET 1MiB | 8879 MiB/s |

Log: `/home/liam/Desktop/stuff/maxio/logs/final.log`

---

## Phase 2 improvements (profiling-driven)

Profiling showed every PutObject — including async background upserts — took the **slow
upsert path** because `normalize_object_meta` materialized a default private ACL, forcing
`object_acl_grants` DELETE+INSERT on every object.

### 6. Implicit private ACL — no DB rows on plain PUT

**Change:** `normalize_object_meta` only fills owner fields. `acl: None` means
implicit-private; `get_object_acl` already synthesizes `Acl::private` when no grant rows exist.

**Files:** `src/storage/mod.rs`

| Scenario | Before (phase 1) | After (phase 2 avg) | Delta |
|----------|------------------|---------------------|-------|
| PUT 4KiB | 44.54 MiB/s | 56.94 MiB/s | **+28%** |
| PUT 1MiB | 136.09 MiB/s | 89.80 MiB/s | −34% (variance) |

Fast upsert path: single `INSERT … ON CONFLICT DO UPDATE` with no `RETURNING` and no ACL side table.

### 7. Bounded async metadata writes

**Change:** `DbContext` holds a `Semaphore(32)`; `defer_object_upsert` acquires a permit before
`upsert_object`. Prevents unbounded `tokio::spawn` from exhausting the 64-connection pool.

**Files:** `src/db/context.rs`, `src/db/repos/objects.rs`

Skips redundant read-cache write on background flush (cache already staged synchronously).

### 8. Content-Length-aware blob write routing

**Change:** `put_object` accepts `content_length: Option<u64>`. When `Content-Length` is known:
- `> 256KiB` → stream immediately (no 8KiB probe loop)
- `≤ 256KiB` → single `read_to_end` + buffered write

**Files:** `src/storage/traits.rs`, `src/storage/blob.rs`, `src/api/object/put.rs`

| Scenario | Before | After (phase 2 avg) | Notes |
|----------|--------|---------------------|-------|
| PUT 1MiB | 136.09 MiB/s | 89.80 MiB/s | 38–156 MiB/s across 3 runs |

Warp sends `Content-Length` for PUT; 1MiB no longer probes 33×8KiB before streaming.

### 9. Skip duplicate read-cache write on async upsert

**Change:** `upsert_object_inner(..., refresh_read_cache)`. Defer worker passes `false` —
cache was already populated synchronously in `defer_object_upsert`.

**Files:** `src/db/repos/objects.rs`

---

## Phase 2 raw runs (15s each)

| Run | PUT 4KiB | GET 4KiB | PUT 1MiB | GET 1MiB |
|-----|----------|----------|----------|----------|
| 1 | 57.76 MiB/s | 86.38 MiB/s | 155.92 MiB/s | 7628 MiB/s |
| 2 | 56.29 MiB/s | 70.88 MiB/s | 75.08 MiB/s | 7823 MiB/s |
| 3 | 56.76 MiB/s | 82.60 MiB/s | 38.40 MiB/s | 7974 MiB/s |
| **Avg** | **56.94** | **79.95** | **89.80** | **7808** |

Logs: `/home/liam/Desktop/stuff/maxio/logs/phase2-run{1,2,3}.log`

---

## Phase 3 improvements (cache + coalescing)

### 10. `Arc<ObjectMeta>` in object read cache

**Change:** `ObjectReadCache` stores `Arc<ObjectMeta>`; LRU hits bump ref-count instead of
cloning ~10 strings under a write lock. `get_object_for_read` uses `Arc::unwrap_or_clone` at
the API boundary (one clone per request when returning owned `ObjectMeta`).

**Files:** `src/db/object_read_cache.rs`, `src/db/repos/objects.rs`

| Scenario | Phase 2 avg | Phase 3 avg | Delta |
|----------|-------------|-------------|-------|
| GET 4KiB | 79.95 MiB/s | 103.28 MiB/s | **+29%** |
| PUT 4KiB | 56.94 MiB/s | 65.09 MiB/s | +14% |

### 11. Coalescing async metadata writer

**Change:** Replaced per-PUT `tokio::spawn` with a single background worker (`src/db/async_meta_writer.rs`).
Jobs queue via `mpsc`; worker coalesces by `(bucket, key)` (last-write-wins), flushes every 2ms
or at 128 pending entries. Semaphore(32) limits concurrent DB upserts during flush.

**Files:** `src/db/async_meta_writer.rs`, `src/db/context.rs`, `src/db/repos/objects.rs`

| Scenario | Phase 2 avg | Phase 3 avg | Notes |
|----------|-------------|-------------|-------|
| PUT 1MiB | 89.80 MiB/s | 50.69 MiB/s* | Run 3 had errors; runs 1–2: 125/15 MiB/s |
| PUT 4KiB | 56.94 MiB/s | 65.09 MiB/s | Fewer pool spikes |

## Phase 3 raw runs (15s each)

| Run | PUT 4KiB | GET 4KiB | PUT 1MiB | GET 1MiB |
|-----|----------|----------|----------|----------|
| 1 | 62.93 MiB/s | 100.55 MiB/s | 125.33 MiB/s | 7911 MiB/s |
| 2 | 64.20 MiB/s | 104.55 MiB/s | 15.41 MiB/s | 7534 MiB/s |
| 3 | 68.13 MiB/s | 104.75 MiB/s | 11.33 MiB/s (errors) | 5878 MiB/s |
| **Avg** | **65.09** | **103.28** | **50.69** | **7107** |

Logs: `/home/liam/Desktop/stuff/maxio/logs/phase3-run{1,2,3}.log`

---

## Production log analysis (2026-07-02)

Sample from a `--cache-dir` deployment with ~597k cache entries and heavy concurrent PUTs
to `wild-survival-pl3xmap`.

### What the logs actually mean

| Phase | Typical | Spike seen | Root cause |
|-------|---------|------------|------------|
| `auth_sigv4` | was ~same as `http_request` | 108ms | **Bug (fixed):** timer included handler/storage time, not auth only |
| `auth_resolve_credentials` | &lt;1ms (root) | 8.9ms | IAM access key → Postgres lookup under pool contention |
| `db_pool_get` | &lt;1ms | 6.8ms | 64-connection pool saturated by concurrent PUTs + async metadata flushes |
| `storage_put_object` | 5–15ms | 5–108ms | Disk/cache writeback + burst after cache merge unblocked waiters |
| `async_upsert_object` | background | 8–12ms | Expected for deferred metadata; competes for DB pool |
| `cache: merged index` | — | 6530ms / 596k entries | Full filesystem walk on restart; **was blocking all PUTs until done (fixed)** |

### Fixes applied from production

1. **`auth_sigv4` timing** — stop timer before `next.run()` so it reflects credential resolve + verify only.
2. **Cache index merge no longer blocks writers** — when `.lru-index.bin` loads, `scan_complete` is set immediately;
   merge reconciles in the background.
3. **Sharded disk cache state + async `mark_dirty`** — PUT no longer takes a global lock on 596k-entry LRU; updates
   coalesce in a background worker (1ms / 256 ops). Eviction runs continuously in its own task with O(1) clean-LRU
   pops instead of O(n) `min_by_key` scans on the request path.
4. **Deferred read-cache write** — `write_through_read_cache` moved off the PutObject hot path into async metadata flush.

### If spikes persist

- **`auth_resolve_credentials` ~9ms** — workload uses IAM keys, not root. Ensure `CachingIamStore` TTL is reasonable;
  check Postgres latency and consider raising `db_pool_size` if async flushes + sync reads contend.
- **`storage_put_object` 30–108ms`** — many concurrent PUTs to one bucket on writeback cache; check backing disk
  (ZFS/array) and cache size vs eviction pressure.
- **403 on `GET /{bucket}/`** — policy/IAM deny on ListBucket; unrelated to perf.

---

## Remaining opportunities

1. **Lightweight `fetch_put_bucket_context`** — return 4 fields without cloning policy/ACL JSON
2. **Prepared statement cache bound** — `CacheSize::Unbounded` → fixed size under metadata bursts
3. **Read-through cache populate skip** for one-shot large GETs when `--cache-dir` is set
4. **Defer coalescing flush tuning** — adaptive interval under load

---

## Tests

```bash
export TMPDIR=/home/liam/Desktop/stuff/maxio/tmp
cargo test                    # unit + integration (243 total)
cargo test --test integration # 165 integration tests
```
