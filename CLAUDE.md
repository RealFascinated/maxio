# MaxIO

S3-compatible object storage server written in Rust. Single-binary replacement for MinIO.

## Agent Guidelines

### Behavior

**Think before coding, but don't over-plan. Just do the task.**

- State assumptions explicitly. If uncertain about something critical, ask — don't guess silently.
- If multiple valid approaches exist, pick the simplest and say why.
- Don't produce step-by-step plans or loop structures unless the task is genuinely complex and multi-phase. Most tasks are not.

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

**No backward compatibility unless asked.**

- Do not add migration paths, dual-format loaders, deprecated-key fallbacks, or "legacy" shims when changing configs, database schemas, serialized fields, or APIs — unless the user explicitly requests it or confirms it is needed.
- Default to the new shape only. Many features here are not yet in production; there is usually no existing data to preserve.
- If a change could break something already live (renamed config key, restructured DB column, removed field), **ask the user** whether backward compatibility or a migration is needed before implementing it. Do not assume it is required.

**Never commit automatically. Let the user commit their own changes.**

After completing changes, suggest a short commit message the user can use — but don't run `git commit` yourself. No conventional commit prefixes (e.g. `refactor:`, `feat:`, `fix:`). Just the message itself.

**Touch only what you must.**

- Don't "improve" adjacent code, formatting, or comments.
- Don't refactor things that aren't broken.
- Remove imports/variables/functions that *your* changes made unused — leave pre-existing dead code alone unless asked.

### Project Conventions

**Follow existing project conventions exactly.** Before writing new code, read nearby modules that solve a similar problem and mirror how they do it. This is non-negotiable — do not introduce your own patterns, reorganize structure, or "clean up" code to match a different style.

This applies to everything:

- **Code style** — naming, formatting, module layout, error handling, async patterns, documentation level.
- **Logic style** — how problems are solved in this codebase (Axum handlers, diesel repos, `MetadataStore`/`BlobStorage` traits, console API routes, Svelte components, TanStack Query hooks, etc.). Reuse the same abstractions and call patterns the surrounding code uses.
- **Project layout** — new S3 handlers go in `src/api/`; DB access goes through `src/db/repos/`; storage traits in `src/storage/`; console endpoints in `src/api/console.rs`; UI pages and components follow existing `ui/src/` structure.

**Do not deviate** unless:

1. **Explicit user request** — the user has directly asked for a different approach, structure, or style.

If something in the project looks inconsistent, match the local convention for that area — don't pick a "better" alternative on your own.

### Type-Driven Design

**Model variation with types, not branching.** When behaviour differs by kind, category, or role, express that difference through enums, traits, and polymorphic dispatch — not long `match`/`if` chains, string discriminators, or flags that need comments to interpret.

- **Extend existing abstractions** — storage backends implement `MetadataStore`/`BlobStorage`; S3 handlers follow the patterns in sibling `src/api/` modules; console routes register through the console router in `console.rs`.
- **Shared logic belongs at the right layer** — pull common behaviour into traits, parent modules, or shared helpers; implementations override only what actually varies.
- **Trait methods document the contract** — a well-named trait method on `MetadataStore` or `S3Error` variant replaces prose explaining "when X happens, do Y."

**Only where it earns its keep.** A single implementation with no realistic second variant stays concrete. Don't add a trait for one implementor — that contradicts minimum-code principles. Use traits and enums when the codebase already has (or clearly needs) multiple implementations of the same contract.

```rust
// bad — behaviour encoded in branches; reader must trace conditions
if op == "put" { ... } else if op == "get" { ... }

// good — each operation owns its handler
async fn put_object(...) -> Result<Response, S3Error> { ... }
async fn get_object(...) -> Result<Response, S3Error> { ... }
```

### Surrounding Context

**Read the subsystem before you write code.** A change is never isolated — it sits inside a router, repo, trait, or UI feature area. Before implementing, explore how that area already works: its traits, registration paths, middleware, migrations, and existing implementations. Your change should plug into those mechanisms, not bypass them.

- **Follow the integration points** — new S3 operations wire through `src/api/router.rs`; metadata changes go through `MetadataStore` and diesel repos; console features extend `console.rs` and mirror existing `/api/` patterns; UI features use TanStack Query and the Coolify design system.
- **No loose workarounds** — don't reach around a trait with one-off DB calls, duplicated logic that an existing abstraction already handles, or ad-hoc state outside `AppState`. If the framework doesn't support what you need, extend it at the right layer — don't glue around it from the call site.
- **Mirror a nearby example** — pick an existing implementation closest to what you're adding and trace it end to end: handler, storage call, error mapping, test, UI component. That path is the template.

### Code Style

The rules below are part of the project conventions above. They are not suggestions — new and changed code must follow them.

#### Rust

- Use `tracing` (`info`, `warn`, `debug`) — never declare a logger manually.
- **Log when the event is infrequent and worth an audit trail** — auth failures, admin actions, lifecycle events, security-sensitive operations. **Do not log hot paths** — per-request object reads/writes, signature verification debug on every call, etc. Reserve `tracing::info` for deliberate audit events; use `tracing::warn` for unexpected but handled conditions; use `tracing::debug` sparingly and only for development diagnostics.
- Methods that may not return a value use `Option<T>`, not nullable pointers or sentinel values.
- Return `Result<T, E>` with the project's error types (`S3Error`, `StorageError`) — don't panic on expected failure paths.
- Use `///` doc comments when something non-obvious is happening (side effects, preconditions, return semantics). Skip docs for boilerplate whose behaviour is obvious from the name and signature.
- Match naming style of nearby code. Clear, full words — not terse abbreviations or overly verbose names.
- Don't assign a value to a variable if it's only used once immediately after.
- Don't extract a function if it's only called from one place and the extraction adds no clarity. Inline it.
- Prefer `HashMap` over ordered maps when order doesn't matter; avoid unnecessary allocation in hot paths; don't iterate a collection multiple times when one pass will do.

#### Frontend (`ui/`)

- Svelte 5 runes, TanStack Query for server state, shadcn-svelte components.
- Follow [`ui/DESIGN_SYSTEM.md`](ui/DESIGN_SYSTEM.md) — Coolify theme, inset inputs, button variants, 2px border radius.
- All `fetch` catch blocks log via `console.error` with context (e.g. `'fetchBuckets failed:'`).
- Use **bun** (not npm).

### Performance

MaxIO handles object storage workloads where throughput matters. Performance is not an afterthought.

- Prefer efficient data structures and avoid unnecessary clones/allocations in request handlers and storage paths.
- Don't iterate a collection multiple times when one pass will do.
- Cache lookups that are repeated across the same operation rather than re-fetching.
- If two approaches are otherwise equal, pick the faster one.

### Testing

**Test-Driven Development (TDD)**: Before implementing any new function or feature, write a failing test first. Then implement until the test passes.

**After every code change**, re-run the full test suite to catch regressions (see [Development Workflow](#development-workflow) below).

Only add tests if requested or they add meaningful coverage of real behavior. Do not add tests that trivially assert the obvious.

## Naming Convention

Always spell the product name **MaxIO** (capital M, capital I, capital O). Never use "Maxio", "maxio", or "MAXIO" in prose. Lowercase `maxio` is acceptable only for CLI binary names, environment variable prefixes (`MAXIO_`), mc aliases, and code identifiers.

## User Preferences

- Use **bun** (not npm) for the `ui/` frontend

## Build & Run

```bash
# Start Postgres (or use docker compose up postgres -d)
export MAXIO_DATABASE_URL=postgres://maxio:maxio@localhost:5432/maxio

# Build binary (build.rs runs the UI build and embeds it)
cargo build --release
./target/release/maxio --data-dir ./data --database-url "$MAXIO_DATABASE_URL" --port 9000
```

**Prerequisites**: `libpq` dev package for building (`postgresql-libs` on Arch, `libpq-dev` on Debian).

Environment variables: `MAXIO_DATABASE_URL` (required), `MAXIO_PORT`, `MAXIO_ADDRESS`, `MAXIO_DATA_DIR`, `MAXIO_ACCESS_KEY` (aliases: `MINIO_ROOT_USER`, `MINIO_ACCESS_KEY`), `MAXIO_SECRET_KEY` (aliases: `MINIO_ROOT_PASSWORD`, `MINIO_SECRET_KEY`), `MAXIO_REGION` (aliases: `MINIO_REGION_NAME`, `MINIO_REGION`)

**Docker Compose** (Postgres + MaxIO): `docker compose up -d`

## Production Build

The release binary is fully self-contained — the frontend UI is embedded at compile time via `rust-embed`. No external files needed.

```bash
# 1. Install frontend dependencies
cd ui && bun install

# 2. Build frontend (outputs to ui/build/; cargo build also does this automatically)
bun run build && cd ..

# 3. Build optimized binary
cargo build --release

# Result: single binary at ./target/release/maxio
# Copy it anywhere — no ui/build/ or other files needed at runtime
```

The binary serves the web console at `/ui/` with proper MIME types, ETags, and cache headers (immutable for hashed assets, no-store for `200.html` / HTML shell).

Defaults: port 9000, access/secret `maxioadmin`/`maxioadmin`, region `us-east-1`

## Development Workflow

**After every code change**, re-run the full test suite to catch regressions:

```bash
# 1. Unit + integration tests (always run first, no server needed)
cargo test

# 2. AWS CLI integration tests (start server, run tests, stop server)
docker compose up postgres -d  # or local Postgres
export MAXIO_DATABASE_URL=postgres://maxio:maxio@localhost:5432/maxio
cargo build && RUST_LOG=info ./target/debug/maxio --data-dir /tmp/maxio-test --database-url "$MAXIO_DATABASE_URL" --port 9876 &
./tests/aws_cli_test.sh 9876 /tmp/maxio-test
kill %1 && rm -rf /tmp/maxio-test
```

**Hot-reload dev server** (for manual testing):

```bash
bun run dev
```

This runs both processes concurrently (Ctrl+C kills both):
- `cargo watch` — rebuilds and restarts the Rust server on backend changes
- Vite dev server — serves the UI with HMR at `http://127.0.0.1:5173/ui/` and proxies `/api` to the Rust server

## Architecture

### Module Layout

- `src/main.rs` — entry point, config, server start, graceful shutdown
- `src/config.rs` — CLI args + env vars via clap derive
- `src/server.rs` — Axum router construction, AppState, middleware wiring
- `src/error.rs` — S3Error with XML error response rendering
- `src/auth/` — AWS Signature V4 verification + Axum middleware
- `src/api/` — S3 API handlers (bucket.rs, object.rs, multipart.rs, list.rs, router.rs, console.rs)
- `src/db/` — Postgres schema, migrations, diesel-async repos
- `src/storage/` — `BlobStorage` (bytes on disk) + `PgMetadataStore` (Postgres metadata) composed by `ObjectStorage`
- `src/iam/` — `PgIamStore` (IAM users/policies in Postgres)
- `src/xml/` — S3 XML response types (serde + quick-xml)

### Key Design Decisions

- **Split storage**: Object **bytes** on filesystem (`data_dir`); all **metadata** (buckets, objects, IAM, multipart sessions) in **PostgreSQL** via diesel-async
- **Storage layout**: `{data_dir}/buckets/{bucket-name}/{key-path}` for object/part/version bytes and EC manifests only — no `.meta.json` sidecars
- **Unraid tip**: Run Postgres on the cache pool (fast SSD); point `MAXIO_DATA_DIR` at the array for bulk object storage
- **Path-style only**: `/{bucket}/{key}` routing. No virtual-hosted-style yet
- **UNSIGNED-PAYLOAD accepted**: Skips body hashing for PutObject (AWS CLI default)
- **Embedded UI assets**: Frontend is compiled into the binary via `rust-embed`. In debug builds, assets are read from the SvelteKit static build (`ui/build/`) when embedded; dev uses Vite/SvelteKit HMR. In release builds, assets are baked in — single binary, no external files needed
- **Web console**: SPA at `/ui/`, API at `/api/`. Cookie-based auth (HMAC tokens, not SigV4). Presigned URL generation with configurable expiry (1h/6h/24h/7d picker in UI)

### Data Layout

```
{data_dir}/                          # object bytes only
└── buckets/
    └── my-bucket/
        ├── .uploads/{uploadId}/1    # multipart part bytes
        ├── photos/vacation.jpg        # object data
        └── large.bin.ec/              # erasure-coded shards + manifest.json

Postgres (MAXIO_DATABASE_URL)        # all metadata
├── buckets, objects, object_versions
├── multipart_uploads, multipart_parts
└── iam_users, iam_access_keys, policies, ACLs, tags
```

### S3 Operations Implemented

| Operation | Method | Path |
|---|---|---|
| ListBuckets | GET | `/` |
| CreateBucket | PUT | `/{bucket}` |
| HeadBucket | HEAD | `/{bucket}` |
| DeleteBucket | DELETE | `/{bucket}` |
| GetBucketLocation | GET | `/{bucket}?location` |
| ListObjectsV1 | GET | `/{bucket}?prefix=&marker=&max-keys=&delimiter=` |
| ListObjectsV2 | GET | `/{bucket}?list-type=2` |
| ListObjectVersions | GET | `/{bucket}?versions` |
| GetBucketVersioning | GET | `/{bucket}?versioning` |
| PutBucketVersioning | PUT | `/{bucket}?versioning` |
| DeleteObjects | POST | `/{bucket}?delete` |
| PutObject | PUT | `/{bucket}/{key}` |
| GetObject | GET | `/{bucket}/{key}` |
| HeadObject | HEAD | `/{bucket}/{key}` |
| DeleteObject | DELETE | `/{bucket}/{key}` |
| CopyObject | PUT | `/{bucket}/{key}` (with `x-amz-copy-source` header) |
| GetObjectTagging | GET | `/{bucket}/{key}?tagging` |
| PutObjectTagging | PUT | `/{bucket}/{key}?tagging` |
| DeleteObjectTagging | DELETE | `/{bucket}/{key}?tagging` |
| GetBucketCors | GET | `/{bucket}?cors` |
| PutBucketCors | PUT | `/{bucket}?cors` |
| DeleteBucketCors | DELETE | `/{bucket}?cors` |
| CreateMultipartUpload | POST | `/{bucket}/{key}?uploads` |
| UploadPart | PUT | `/{bucket}/{key}?partNumber=N&uploadId=X` |
| UploadPartCopy | PUT | `/{bucket}/{key}?partNumber=N&uploadId=X` (with `x-amz-copy-source` header) |
| CompleteMultipartUpload | POST | `/{bucket}/{key}?uploadId=X` |
| AbortMultipartUpload | DELETE | `/{bucket}/{key}?uploadId=X` |
| ListParts | GET | `/{bucket}/{key}?uploadId=X` |
| ListMultipartUploads | GET | `/{bucket}?uploads` |

### Console API (`/api/`)

| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/api/auth/login` | POST | none | Login with accessKey/secretKey, sets session cookie |
| `/api/auth/check` | GET | none | Check if session cookie is valid |
| `/api/auth/logout` | POST | cookie | Clear session cookie |
| `/api/buckets` | GET | cookie | List all buckets |
| `/api/buckets` | POST | cookie | Create bucket (`{ name }`) |
| `/api/buckets/{bucket}` | DELETE | cookie | Delete bucket |
| `/api/buckets/{bucket}/objects` | GET | cookie | List objects (`?prefix=&delimiter=`) |
| `/api/buckets/{bucket}/objects/{key}` | DELETE | cookie | Delete object |
| `/api/buckets/{bucket}/upload/{key}` | PUT | cookie | Upload object |
| `/api/buckets/{bucket}/download/{key}` | GET | cookie | Download object |
| `/api/buckets/{bucket}/presign/{key}` | GET | cookie | Generate presigned URL (`?expires=SECONDS`, default 3600, max 604800) |

### Frontend Error Logging

All `fetch` catch blocks in UI components log errors via `console.error` with context (e.g. `'fetchBuckets failed:'`, `'shareObject failed:'`). Check browser DevTools console for debugging.

### Testing with MinIO Client (mc)

```bash
# Install mc
brew install minio/stable/mc

# Configure alias
mc alias set maxio http://localhost:9000 maxioadmin maxioadmin

# Bucket operations
mc mb maxio/test-bucket
mc ls maxio/

# Upload / download
echo "hello maxio" > /tmp/test.txt
mc cp /tmp/test.txt maxio/test-bucket/test.txt
mc ls maxio/test-bucket/
mc cat maxio/test-bucket/test.txt
mc cp maxio/test-bucket/test.txt /tmp/downloaded.txt

# Nested keys
mc cp /tmp/test.txt maxio/test-bucket/folder/nested/file.txt
mc ls maxio/test-bucket/folder/

# Cleanup
mc rm maxio/test-bucket/test.txt
mc rm maxio/test-bucket/folder/nested/file.txt
mc rb maxio/test-bucket
```

### Testing with AWS CLI

```bash
export AWS_ACCESS_KEY_ID=maxioadmin
export AWS_SECRET_ACCESS_KEY=maxioadmin
aws --endpoint-url http://localhost:9000 s3 mb s3://test-bucket
aws --endpoint-url http://localhost:9000 s3 cp file.txt s3://test-bucket/file.txt
aws --endpoint-url http://localhost:9000 s3 ls s3://test-bucket/
aws --endpoint-url http://localhost:9000 s3 cp s3://test-bucket/file.txt downloaded.txt
aws --endpoint-url http://localhost:9000 s3 rm s3://test-bucket/file.txt
aws --endpoint-url http://localhost:9000 s3 rb s3://test-bucket
```

### Running Tests

```bash
# Unit + integration tests (no server needed)
cargo test

# AWS CLI integration tests (requires running server)
./tests/aws_cli_test.sh
```

### Benchmarking (MaxIO vs MinIO)

Uses [WARP](https://github.com/minio/warp) to compare MaxIO against MinIO across 7 scenarios: PUT (4KiB/1MiB/64MiB), GET (4KiB/1MiB), mixed workload, and multipart uploads. Prerequisites: `brew install minio-warp` and `brew install minio/stable/minio`.

```bash
# Full benchmark (starts both servers automatically)
cargo build --release
./tests/bench.sh

# Quick benchmark (small objects + mixed only, 10s each)
./tests/bench.sh --duration=10s --scenarios=put-small,get-small,mixed

# Custom duration
./tests/bench.sh --duration=60s

# Against external servers (skip automatic server management)
./tests/bench.sh --maxio-host=server1:9000 --minio-host=server2:9000

# Via root package scripts
bun run bench        # full (30s per scenario)
bun run bench:quick  # quick smoke test
```

**Remote server benchmark** (single command — cross-compiles, copies binary, auto-downloads warp + minio on the server, runs, streams results):

```bash
./tests/bench-remote.sh user@host
./tests/bench-remote.sh user@host --duration=60s --scenarios=put-small,mixed
```

## UI Design System

The web console (`ui/`) follows the Coolify design system. The full specification is in [`ui/DESIGN_SYSTEM.md`](ui/DESIGN_SYSTEM.md). Key points:

- **Stack**: SvelteKit static SPA, Svelte 5, Vite, Tailwind CSS v4, shadcn-svelte components, TanStack Query
- **Theme**: Class-based dark mode (`.dark` on `<html>`), with light/dark CSS variable swap in `ui/src/app.css`
- **Accent colors**: Coollabs purple `#6b16ed` (light) / warning yellow `#fcd452` (dark). Brand purple (`--color-brand`) is always `#6b16ed` regardless of theme
- **Font**: Geist Sans + Geist Mono via `@fontsource/geist-sans` / `@fontsource/geist-mono` (Inter fallback)
- **Inputs**: Inset box-shadow system (4px colored left bar on focus), no standard borders — see `.input-cool` in `app.css`
- **Buttons**: `border-2`, `h-8`, `rounded-sm`. Variants: `default`, `highlighted`, `destructive`, `outline`, `secondary`, `ghost`, `link`, `brand`
- **Border radius**: `0.125rem` (2px) everywhere — set via `--radius` in `@theme inline`
- **Sidebar**: Collapsible 224px → 56px icon-only, uses `--cool-sidebar-*` CSS variables

## Roadmap

- **Phase 2**: ~~Multipart upload~~, ~~presigned URLs~~, ~~CopyObject~~, ~~DeleteObjects batch~~, ~~CORS~~, ~~Range headers~~
- **Phase 3**: ~~Web console (SPA at `/ui/`)~~, ~~versioning~~, lifecycle rules, multi-user, metrics
- **Phase 4**: Distributed mode, ~~erasure coding~~, replication
