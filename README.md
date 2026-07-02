<div align="center">

# MaxIO

S3-compatible object storage server — single-binary replacement for MinIO.

Rust · Axum · PostgreSQL · SvelteKit · Svelte 5 · Tailwind CSS v4 · shadcn-svelte

</div>

## About the Project

> **Warning:** MaxIO is under active development. Do not use it in production yet.

MaxIO is a lightweight, single-binary S3-compatible object storage server written in Rust. Metadata lives in PostgreSQL; object bytes are stored on a local filesystem path (`--data-dir`). You need Postgres and a data directory — back up both.

## Features

- **Single Binary** — Frontend assets are compiled into the binary via `rust-embed`. Nothing extra to deploy
- **Postgres + Filesystem Storage** — Metadata in PostgreSQL (indexed listing, IAM, multipart state); object bytes on disk under `--data-dir`
- **AWS Signature V4** — Compatible with `mc`, AWS CLI, and any S3 SDK
- **IAM Users** — Multi-user access with policy-based permissions; manage users in the console or via CLI
- **Web Console** — SvelteKit SPA at `/ui/` for browsing, uploading, and managing buckets and objects
- **S3 API Coverage** — ListBuckets, CreateBucket, HeadBucket, DeleteBucket, GetBucketLocation, ListObjectsV1/V2, ListObjectVersions, PutObject, GetObject, HeadObject, DeleteObject, DeleteObjects (batch), CopyObject, Multipart Upload (including UploadPartCopy), Object Tagging, CORS, Versioning, Presigned URLs
- **Conditional Requests** — `If-Match`, `If-None-Match`, `If-Modified-Since`, `If-Unmodified-Since` headers (RFC 7232)
- **Range Requests** — HTTP 206 Partial Content support via `Range` header on GetObject
- **Checksum Verification** — CRC32, CRC32C, SHA-1, and SHA-256 checksums on upload with automatic validation and persistent storage
- **Optional SSD Cache** — Write-through or writeback cache layer for object bytes on a fast local disk
- **Metrics** — Prometheus endpoint (`GET /metrics`) and a live throughput dashboard in the console (root users)
- **Console UX** — Object search, infinite scroll, bulk select/delete, drag-and-drop upload, file preview, folder deletion, bucket settings (versioning, CORS, public read), and orphan-metadata scan/repair


## Installation

### Prerequisites

- **PostgreSQL** — required for all metadata (buckets, objects, IAM, multipart state)
- **libpq** dev package for building from source (`postgresql-libs` on Arch, `libpq-dev` on Debian)

### Build from Source

```bash
export MAXIO_DATABASE_URL=postgres://maxio:maxio@localhost:5432/maxio

# Build binary (build.rs runs the UI build and embeds it)
cargo build --release

# Run
./target/release/maxio --data-dir ./data --database-url "$MAXIO_DATABASE_URL" --port 9000
```

### Docker

Release images are published to [GHCR](https://github.com/coollabsio/maxio/pkgs/container/maxio) only (`ghcr.io/coollabsio/maxio`, with `:main` on every push to `main`).

**Docker Compose** (recommended — includes Postgres):

```bash
docker compose up -d
```

Or manually:

```yaml
services:
  postgres:
    image: docker.io/library/postgres:18-alpine
    environment:
      POSTGRES_USER: maxio
      POSTGRES_PASSWORD: maxio
      POSTGRES_DB: maxio
    volumes:
      - maxio-pg:/var/lib/postgresql

  maxio:
    image: ghcr.io/realfascinated/maxio:main
    ports:
      - "9000:9000"
    environment:
      MAXIO_DATABASE_URL: postgres://maxio:maxio@postgres:5432/maxio
      MAXIO_DATA_DIR: /data
      MAXIO_ACCESS_KEY: setme
      MAXIO_SECRET_KEY: setme
    volumes:
      - maxio-data:/data
    depends_on:
      - postgres

volumes:
  maxio-pg:
  maxio-data:
```

Standalone container (external Postgres required):

```bash
docker run -d \
  -p 9000:9000 \
  -v $(pwd)/data:/data \
  -e MAXIO_DATABASE_URL=postgres://user:pass@host:5432/maxio \
  -e MAXIO_ACCESS_KEY=myadmin \
  -e MAXIO_SECRET_KEY=mysecret \
  -e MAXIO_DEFAULT_BUCKETS=my-bucket,logs,backups \
  ghcr.io/coollabsio/maxio
```

Open `http://localhost:9000/ui/` in your browser. Default credentials: `maxioadmin` / `maxioadmin`

## Configuration

| Variable | CLI Flag | Default | Description |
|---|---|---|---|
| `MAXIO_DATABASE_URL` | `--database-url` | _(required)_ | PostgreSQL connection URL for metadata |
| `MAXIO_PORT` | `--port` | `9000` | Listen port |
| `MAXIO_ADDRESS` | `--address` | `0.0.0.0` | Bind address |
| `MAXIO_DATA_DIR` | `--data-dir` | `./data` | Object bytes storage directory |
| `MAXIO_ACCESS_KEY` | `--access-key` | `maxioadmin` | Root access key (aliases: `MINIO_ROOT_USER`, `MINIO_ACCESS_KEY`) |
| `MAXIO_SECRET_KEY` | `--secret-key` | `maxioadmin` | Root secret key (aliases: `MINIO_ROOT_PASSWORD`, `MINIO_SECRET_KEY`) |
| `MAXIO_ALLOW_INSECURE_DEV` | `--allow-insecure-dev` | `false` | Allow insecure development defaults, including default credentials and HTTP console cookies |
| `MAXIO_SECURE_COOKIES` | `--secure-cookies` | `true` | Force `Secure` on console session cookies; keep enabled for public consoles |
| `MAXIO_DEFAULT_BUCKETS` | `--default-buckets` | _(none)_ | Comma-separated list of bucket names to create during startup (aliases: `MINIO_DEFAULT_BUCKETS`) |
| `MAXIO_MAX_CONSOLE_BODY_BYTES` | `--max-console-body-bytes` | `1048576` | Max request body size for console JSON/form API routes; object uploads are streaming and not covered by this limit |
| `MAXIO_METRICS_TOKEN` | `--metrics-token` | _(empty)_ | Bearer token for `GET /metrics`; when empty the endpoint returns 403 |
| `MAXIO_PUBLIC_URL` | `--public-url` | _(none)_ | Public S3 base URL for presigned links behind a reverse proxy, e.g. `https://s3.example.com` |
| `MAXIO_DB_POOL_SIZE` | `--db-pool-size` | `64` | Max Postgres connection pool size |
| `MAXIO_DB_PREPARED_STATEMENT_CACHE` | `--db-prepared-statement-cache` | `true` | Cache prepared SQL statements on each pool connection |
| `MAXIO_CACHE_DIR` | `--cache-dir` | _(none)_ | Optional SSD cache directory for object bytes |
| `MAXIO_CACHE_MAX_SIZE` | `--cache-max-size` | `10737418240` (10 GiB) | Maximum cache size in bytes |
| `MAXIO_CACHE_WRITEBACK` | `--cache-writeback` | `false` | Write to cache first and flush to `data_dir` in the background |
| `MAXIO_CACHE_FLUSH_INTERVAL` | `--cache-flush-interval` | `30` | Writeback flush interval in seconds |
| `MAXIO_OBJECT_READ_CACHE_MAX_ENTRIES` | `--object-read-cache-max-entries` | `262144` | Max object read-metadata cache entries |
| `MAXIO_BUCKET_CACHE_MAX_ENTRIES` | `--bucket-cache-max-entries` | `10000` | Max bucket metadata cache entries |
| `MAXIO_SIGNING_KEY_CACHE_MAX_ENTRIES` | `--signing-key-cache-max-entries` | `10000` | Max signing key cache entries |
| `MAXIO_IAM_CACHE_MAX_ENTRIES` | `--iam-cache-max-entries` | `10000` | Max entries per IAM metadata sub-cache |
| `MAXIO_HEALTHCHECK_URL` | `healthcheck --url` | `http://127.0.0.1:9000/healthz` | Healthcheck endpoint URL; default port follows `MAXIO_PORT` when set |
| `MAXIO_HEALTHCHECK_TIMEOUT_MS` | `healthcheck --timeout-ms` | `2000` | Healthcheck connect/read timeout in milliseconds |

## Usage

### MinIO Client (mc)

```bash
mc alias set maxio http://localhost:9000 maxioadmin maxioadmin

mc mb maxio/my-bucket
mc cp file.txt maxio/my-bucket/file.txt
mc ls maxio/my-bucket/
mc cat maxio/my-bucket/file.txt
mc rm maxio/my-bucket/file.txt
mc rb maxio/my-bucket
```

### AWS CLI

```bash
export AWS_ACCESS_KEY_ID=maxioadmin
export AWS_SECRET_ACCESS_KEY=maxioadmin

aws --endpoint-url http://localhost:9000 s3 mb s3://my-bucket
aws --endpoint-url http://localhost:9000 s3 cp file.txt s3://my-bucket/file.txt
aws --endpoint-url http://localhost:9000 s3 ls s3://my-bucket/
aws --endpoint-url http://localhost:9000 s3 rm s3://my-bucket/file.txt
aws --endpoint-url http://localhost:9000 s3 rb s3://my-bucket
```

### CLI Commands

Beyond `serve` (the default), MaxIO ships subcommands for operations and maintenance:

```bash
# Health check (also used by the Docker HEALTHCHECK)
maxio healthcheck

# IAM user management
maxio user add --username alice --database-url "$MAXIO_DATABASE_URL"
maxio user list --database-url "$MAXIO_DATABASE_URL"
maxio user create-key --username alice --database-url "$MAXIO_DATABASE_URL"
maxio user put-policy --username alice --policy-name read-only \
  --document '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject","s3:ListBucket"],"Resource":["arn:aws:s3:::my-bucket/*","arn:aws:s3:::my-bucket"]}]}' \
  --database-url "$MAXIO_DATABASE_URL"

# IAM policy management
maxio policy list --database-url "$MAXIO_DATABASE_URL"

# Find metadata rows whose object bytes are missing on disk
maxio orphan-meta --database-url "$MAXIO_DATABASE_URL" --data-dir ./data
maxio orphan-meta --delete --database-url "$MAXIO_DATABASE_URL" --data-dir ./data
```

Orphan metadata can also be scanned and repaired from the console **Settings** page.

### Prometheus Metrics

Set `MAXIO_METRICS_TOKEN` and scrape `GET /metrics` with the bearer token:

```bash
curl -H "Authorization: Bearer $MAXIO_METRICS_TOKEN" http://localhost:9000/metrics
```

The console **Metrics** page (root users only) shows live throughput, cache stats, Postgres latency, and per-bucket totals.


## Contributing

See [CLAUDE.md](CLAUDE.md) for the full development workflow, architecture details, and testing instructions.

## License

[Apache-2.0](LICENSE)
