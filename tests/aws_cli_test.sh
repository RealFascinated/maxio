#!/usr/bin/env bash
set -euo pipefail

# Integration tests using AWS CLI against a running maxio server.
# Usage: ./tests/aws_cli_test.sh [port] [data_dir]
# Expects maxio to be running on localhost:${PORT:-9000}

PORT="${1:-9000}"
DATA_DIR="$(cd "${2:-./data}" && pwd)"
BUCKET="test-bucket-$$"
ENDPOINT="http://localhost:$PORT"
TMPDIR=$(mktemp -d)
PASS=0
FAIL=0

export AWS_ACCESS_KEY_ID=maxioadmin
export AWS_SECRET_ACCESS_KEY=maxioadmin
export AWS_DEFAULT_REGION=us-east-1

AWS="aws --endpoint-url $ENDPOINT"

cleanup() {
    rm -rf "$TMPDIR"
}
trap cleanup EXIT

red()   { printf "\033[31m%s\033[0m\n" "$1"; }
green() { printf "\033[32m%s\033[0m\n" "$1"; }

assert() {
    local name="$1"
    shift
    if "$@" > /dev/null 2>&1; then
        green "PASS: $name"
        PASS=$((PASS + 1))
    else
        red "FAIL: $name"
        FAIL=$((FAIL + 1))
    fi
}

assert_fail() {
    local name="$1"
    shift
    if "$@" > /dev/null 2>&1; then
        red "FAIL: $name (expected failure but succeeded)"
        FAIL=$((FAIL + 1))
    else
        green "PASS: $name"
        PASS=$((PASS + 1))
    fi
}

assert_eq() {
    local name="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        green "PASS: $name"
        PASS=$((PASS + 1))
    else
        red "FAIL: $name (expected '$expected', got '$actual')"
        FAIL=$((FAIL + 1))
    fi
}

assert_file_exists() {
    local name="$1" path="$2"
    if [ -e "$path" ]; then
        green "PASS: $name"
        PASS=$((PASS + 1))
    else
        red "FAIL: $name (file not found: $path)"
        FAIL=$((FAIL + 1))
    fi
}

assert_file_not_exists() {
    local name="$1" path="$2"
    if [ ! -e "$path" ]; then
        green "PASS: $name"
        PASS=$((PASS + 1))
    else
        red "FAIL: $name (file should not exist: $path)"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== Maxio AWS CLI integration tests ==="
echo "Server: localhost:$PORT"
echo "Data dir: $DATA_DIR"
echo ""

# --- Bucket operations ---
assert "create bucket" $AWS s3 mb "s3://$BUCKET"
assert_file_exists "bucket dir exists on disk" "$DATA_DIR/buckets/$BUCKET"

# List buckets
OUTPUT=$($AWS s3 ls 2>&1)
assert_eq "list buckets contains our bucket" "true" "$(echo "$OUTPUT" | grep -q "$BUCKET" && echo true || echo false)"

# Head bucket
assert "head bucket" $AWS s3api head-bucket --bucket "$BUCKET"

# --- Object operations ---
echo "hello maxio" > "$TMPDIR/test.txt"

assert "upload object" $AWS s3 cp "$TMPDIR/test.txt" "s3://$BUCKET/test.txt"
assert_file_exists "object file exists on disk" "$DATA_DIR/buckets/$BUCKET/test.txt"
assert_eq "on-disk content matches" "hello maxio" "$(cat "$DATA_DIR/buckets/$BUCKET/test.txt")"

# List objects
OUTPUT=$($AWS s3 ls "s3://$BUCKET/" 2>&1)
assert_eq "list objects contains test.txt" "true" "$(echo "$OUTPUT" | grep -q "test.txt" && echo true || echo false)"

# Download and verify
assert "download object" $AWS s3 cp "s3://$BUCKET/test.txt" "$TMPDIR/downloaded.txt"
assert_eq "content matches" "hello maxio" "$(cat "$TMPDIR/downloaded.txt")"

# Head object
OUTPUT=$($AWS s3api head-object --bucket "$BUCKET" --key "test.txt" 2>&1)
assert_eq "head object has etag" "true" "$(echo "$OUTPUT" | grep -q "ETag" && echo true || echo false)"
assert_eq "head object has content-length" "true" "$(echo "$OUTPUT" | grep -q "ContentLength" && echo true || echo false)"

# --- Nested keys ---
assert "upload nested object" $AWS s3 cp "$TMPDIR/test.txt" "s3://$BUCKET/folder/nested/file.txt"
assert_file_exists "nested object exists on disk" "$DATA_DIR/buckets/$BUCKET/folder/nested/file.txt"

OUTPUT=$($AWS s3 ls "s3://$BUCKET/folder/" 2>&1)
assert_eq "list nested prefix" "true" "$(echo "$OUTPUT" | grep -q "nested" && echo true || echo false)"

assert "download nested object" $AWS s3 cp "s3://$BUCKET/folder/nested/file.txt" "$TMPDIR/nested.txt"
assert_eq "nested content matches" "hello maxio" "$(cat "$TMPDIR/nested.txt")"

# --- Multipart upload (large file) ---
dd if=/dev/urandom of="$TMPDIR/big.bin" bs=1M count=15 status=none
assert "upload large object (multipart)" $AWS s3 cp "$TMPDIR/big.bin" "s3://$BUCKET/big.bin"
assert "download large object" $AWS s3 cp "s3://$BUCKET/big.bin" "$TMPDIR/big.download.bin"
assert_eq "large object size matches" "$(wc -c < "$TMPDIR/big.bin" | tr -d ' ')" "$(wc -c < "$TMPDIR/big.download.bin" | tr -d ' ')"
OUTPUT=$($AWS s3api head-object --bucket "$BUCKET" --key "big.bin" 2>&1)
assert_eq "multipart etag suffix present" "true" "$(echo "$OUTPUT" | grep -Eq '\"ETag\": \".*-[0-9]+.*\"' && echo true || echo false)"

# --- Multipart upload (explicit API lifecycle) ---
dd if=/dev/urandom of="$TMPDIR/mpart1.bin" bs=1M count=5 status=none
echo "tail-part" > "$TMPDIR/mpart2.bin"
UPLOAD_ID=$($AWS s3api create-multipart-upload --bucket "$BUCKET" --key "manual-multipart.bin" --query UploadId --output text 2>/dev/null || true)
assert_eq "create multipart upload id" "true" "$([ -n "$UPLOAD_ID" ] && [ "$UPLOAD_ID" != "None" ] && echo true || echo false)"

ETAG1=$($AWS s3api upload-part --bucket "$BUCKET" --key "manual-multipart.bin" --part-number 1 --body "$TMPDIR/mpart1.bin" --upload-id "$UPLOAD_ID" --query ETag --output text 2>/dev/null || true)
ETAG2=$($AWS s3api upload-part --bucket "$BUCKET" --key "manual-multipart.bin" --part-number 2 --body "$TMPDIR/mpart2.bin" --upload-id "$UPLOAD_ID" --query ETag --output text 2>/dev/null || true)
assert_eq "upload multipart part 1 etag" "true" "$([ -n "$ETAG1" ] && [ "$ETAG1" != "None" ] && echo true || echo false)"
assert_eq "upload multipart part 2 etag" "true" "$([ -n "$ETAG2" ] && [ "$ETAG2" != "None" ] && echo true || echo false)"

OUTPUT=$($AWS s3api list-parts --bucket "$BUCKET" --key "manual-multipart.bin" --upload-id "$UPLOAD_ID" 2>&1)
assert_eq "list-parts contains part 1" "true" "$(echo "$OUTPUT" | grep -q '"PartNumber": 1' && echo true || echo false)"
assert_eq "list-parts contains part 2" "true" "$(echo "$OUTPUT" | grep -q '"PartNumber": 2' && echo true || echo false)"

COMPLETE_JSON="$TMPDIR/complete.json"
cat > "$COMPLETE_JSON" <<EOF
{
  "Parts": [
    {"ETag": $ETAG1, "PartNumber": 1},
    {"ETag": $ETAG2, "PartNumber": 2}
  ]
}
EOF
assert "complete multipart upload" $AWS s3api complete-multipart-upload --bucket "$BUCKET" --key "manual-multipart.bin" --upload-id "$UPLOAD_ID" --multipart-upload "file://$COMPLETE_JSON"
assert "download completed multipart" $AWS s3 cp "s3://$BUCKET/manual-multipart.bin" "$TMPDIR/manual-multipart.download.bin"
assert_eq "completed multipart merged size" "$(($(wc -c < "$TMPDIR/mpart1.bin") + $(wc -c < "$TMPDIR/mpart2.bin")))" "$(wc -c < "$TMPDIR/manual-multipart.download.bin" | tr -d ' ')"

ABORT_ID=$($AWS s3api create-multipart-upload --bucket "$BUCKET" --key "abort-multipart.bin" --query UploadId --output text 2>/dev/null || true)
assert_eq "create abortable multipart upload id" "true" "$([ -n "$ABORT_ID" ] && [ "$ABORT_ID" != "None" ] && echo true || echo false)"
assert "abort multipart upload" $AWS s3api abort-multipart-upload --bucket "$BUCKET" --key "abort-multipart.bin" --upload-id "$ABORT_ID"
assert_fail "list-parts after abort should fail" $AWS s3api list-parts --bucket "$BUCKET" --key "abort-multipart.bin" --upload-id "$ABORT_ID"

# --- Copy object ---
assert "copy object same bucket" $AWS s3 cp "s3://$BUCKET/test.txt" "s3://$BUCKET/test-copy.txt"
assert "download copied object" $AWS s3 cp "s3://$BUCKET/test-copy.txt" "$TMPDIR/copy.txt"
assert_eq "copied content matches" "hello maxio" "$(cat "$TMPDIR/copy.txt")"
assert_file_exists "copied object on disk" "$DATA_DIR/buckets/$BUCKET/test-copy.txt"

# Copy object via s3api
OUTPUT=$($AWS s3api copy-object --bucket "$BUCKET" --key "api-copy.txt" --copy-source "$BUCKET/test.txt" 2>&1)
assert_eq "copy-object has ETag" "true" "$(echo "$OUTPUT" | grep -q "ETag" && echo true || echo false)"
assert "download api-copied object" $AWS s3 cp "s3://$BUCKET/api-copy.txt" "$TMPDIR/api-copy.txt"
assert_eq "api-copied content matches" "hello maxio" "$(cat "$TMPDIR/api-copy.txt")"

# --- UploadPartCopy ---
# Prepare a source object large enough to serve as multipart copy parts (5 MiB + 1 KiB)
dd if=/dev/urandom of="$TMPDIR/upc-source.bin" bs=1M count=5 status=none
dd if=/dev/urandom of="$TMPDIR/upc-tail.bin" bs=1024 count=1 status=none
cat "$TMPDIR/upc-source.bin" "$TMPDIR/upc-tail.bin" > "$TMPDIR/upc-full.bin"
assert "upload upc source object" $AWS s3 cp "$TMPDIR/upc-full.bin" "s3://$BUCKET/upc-source.bin"

UPC_SIZE=$(wc -c < "$TMPDIR/upc-full.bin" | tr -d ' ')
UPC_PART1_END=$((5 * 1024 * 1024 - 1))
UPC_PART2_START=$((5 * 1024 * 1024))
UPC_PART2_END=$((UPC_SIZE - 1))

UPC_UPLOAD_ID=$($AWS s3api create-multipart-upload --bucket "$BUCKET" --key "upc-dest.bin" --query UploadId --output text 2>/dev/null || true)
assert_eq "upc create multipart upload id" "true" "$([ -n "$UPC_UPLOAD_ID" ] && [ "$UPC_UPLOAD_ID" != "None" ] && echo true || echo false)"

UPC_ETAG1=$($AWS s3api upload-part-copy \
  --bucket "$BUCKET" --key "upc-dest.bin" \
  --upload-id "$UPC_UPLOAD_ID" --part-number 1 \
  --copy-source "$BUCKET/upc-source.bin" \
  --copy-source-range "bytes=0-$UPC_PART1_END" \
  --query CopyPartResult.ETag --output text 2>/dev/null || true)
assert_eq "upc part 1 etag present" "true" "$([ -n "$UPC_ETAG1" ] && [ "$UPC_ETAG1" != "None" ] && echo true || echo false)"

UPC_ETAG2=$($AWS s3api upload-part-copy \
  --bucket "$BUCKET" --key "upc-dest.bin" \
  --upload-id "$UPC_UPLOAD_ID" --part-number 2 \
  --copy-source "$BUCKET/upc-source.bin" \
  --copy-source-range "bytes=$UPC_PART2_START-$UPC_PART2_END" \
  --query CopyPartResult.ETag --output text 2>/dev/null || true)
assert_eq "upc part 2 etag present" "true" "$([ -n "$UPC_ETAG2" ] && [ "$UPC_ETAG2" != "None" ] && echo true || echo false)"

UPC_COMPLETE_JSON="$TMPDIR/upc-complete.json"
cat > "$UPC_COMPLETE_JSON" <<EOF
{
  "Parts": [
    {"ETag": $UPC_ETAG1, "PartNumber": 1},
    {"ETag": $UPC_ETAG2, "PartNumber": 2}
  ]
}
EOF
assert "upc complete multipart upload" $AWS s3api complete-multipart-upload \
  --bucket "$BUCKET" --key "upc-dest.bin" \
  --upload-id "$UPC_UPLOAD_ID" \
  --multipart-upload "file://$UPC_COMPLETE_JSON"

assert "upc download result" $AWS s3 cp "s3://$BUCKET/upc-dest.bin" "$TMPDIR/upc-download.bin"
assert_eq "upc result size matches source" "$UPC_SIZE" "$(wc -c < "$TMPDIR/upc-download.bin" | tr -d ' ')"
assert_eq "upc result content matches source" "$(md5sum "$TMPDIR/upc-full.bin" | cut -d' ' -f1)" "$(md5sum "$TMPDIR/upc-download.bin" | cut -d' ' -f1)"

# --- Overwrite object ---
echo "updated content" > "$TMPDIR/updated.txt"
assert "overwrite object" $AWS s3 cp "$TMPDIR/updated.txt" "s3://$BUCKET/test.txt"
assert "download overwritten" $AWS s3 cp "s3://$BUCKET/test.txt" "$TMPDIR/overwritten.txt"
assert_eq "overwritten content" "updated content" "$(cat "$TMPDIR/overwritten.txt")"
assert_eq "on-disk overwritten content" "updated content" "$(cat "$DATA_DIR/buckets/$BUCKET/test.txt")"

# --- Range request tests ---
echo "abcdefghijklmnopqrstuvwxyz" > "$TMPDIR/alphabet.txt"
assert "upload range-test file" $AWS s3 cp "$TMPDIR/alphabet.txt" "s3://$BUCKET/alphabet.txt"

assert "get-object with range bytes=0-4" \
    $AWS s3api get-object --bucket "$BUCKET" --key "alphabet.txt" \
    --range "bytes=0-4" "$TMPDIR/range_out.txt"
assert_eq "range first 5 bytes" "abcde" "$(cat "$TMPDIR/range_out.txt")"

assert "get-object with range bytes=-3" \
    $AWS s3api get-object --bucket "$BUCKET" --key "alphabet.txt" \
    --range "bytes=-3" "$TMPDIR/range_suffix.txt"
assert_eq "range suffix 3 bytes" "yz" "$(cat "$TMPDIR/range_suffix.txt" | tr -d '\n')"

assert "get-object with open-end range bytes=23-" \
    $AWS s3api get-object --bucket "$BUCKET" --key "alphabet.txt" \
    --range "bytes=23-" "$TMPDIR/range_open.txt"
assert_eq "range open-end" "xyz" "$(cat "$TMPDIR/range_open.txt" | tr -d '\n')"

assert_fail "get-object with invalid range bytes=9999-" \
    $AWS s3api get-object --bucket "$BUCKET" --key "alphabet.txt" \
    --range "bytes=9999-" "$TMPDIR/range_invalid.txt"

assert "delete range-test file" $AWS s3 rm "s3://$BUCKET/alphabet.txt"

# --- Folder operations ---
assert "create folder via put-object" $AWS s3api put-object --bucket "$BUCKET" --key "empty-folder/" --content-length 0
assert_file_exists "folder marker exists on disk" "$DATA_DIR/buckets/$BUCKET/empty-folder/.folder"

OUTPUT=$($AWS s3 ls "s3://$BUCKET/" 2>&1)
assert_eq "list shows folder prefix" "true" "$(echo "$OUTPUT" | grep -q "empty-folder/" && echo true || echo false)"

OUTPUT=$($AWS s3api head-object --bucket "$BUCKET" --key "empty-folder/" 2>&1)
assert_eq "head folder marker has zero size" "true" "$(echo "$OUTPUT" | grep -q '"ContentLength": 0' && echo true || echo false)"

assert "delete folder marker" $AWS s3api delete-object --bucket "$BUCKET" --key "empty-folder/"
assert_fail "head deleted folder marker" $AWS s3api head-object --bucket "$BUCKET" --key "empty-folder/"

# --- Checksum tests ---
echo "checksum test data" > "$TMPDIR/checksum.txt"

# PutObject with CRC32 checksum via s3api
CRC32_VALUE=$(python3 -c "
import binascii, base64
data = open('$TMPDIR/checksum.txt', 'rb').read()
crc = binascii.crc32(data) & 0xffffffff
print(base64.b64encode(crc.to_bytes(4, 'big')).decode())
")
OUTPUT=$($AWS s3api put-object --bucket "$BUCKET" --key "checksum.txt" \
    --body "$TMPDIR/checksum.txt" \
    --checksum-algorithm CRC32 \
    --checksum-crc32 "$CRC32_VALUE" 2>&1)
assert_eq "put-object with CRC32 checksum accepted" "true" "$(echo "$OUTPUT" | grep -q "ChecksumCRC32" && echo true || echo false)"

# HeadObject should return the checksum
OUTPUT=$($AWS s3api head-object --bucket "$BUCKET" --key "checksum.txt" --checksum-mode ENABLED 2>&1)
assert_eq "head-object returns CRC32 checksum" "true" "$(echo "$OUTPUT" | grep -q "ChecksumCRC32" && echo true || echo false)"

# PutObject with wrong checksum should fail
assert_fail "put-object with wrong CRC32 rejects" \
    $AWS s3api put-object --bucket "$BUCKET" --key "bad-checksum.txt" \
    --body "$TMPDIR/checksum.txt" \
    --checksum-algorithm CRC32 \
    --checksum-crc32 "AAAAAAAA"

# PutObject with SHA256 checksum
SHA256_VALUE=$(python3 -c "
import hashlib, base64
data = open('$TMPDIR/checksum.txt', 'rb').read()
print(base64.b64encode(hashlib.sha256(data).digest()).decode())
")
OUTPUT=$($AWS s3api put-object --bucket "$BUCKET" --key "checksum-sha256.txt" \
    --body "$TMPDIR/checksum.txt" \
    --checksum-algorithm SHA256 \
    --checksum-sha256 "$SHA256_VALUE" 2>&1)
assert_eq "put-object with SHA256 checksum accepted" "true" "$(echo "$OUTPUT" | grep -q "ChecksumSHA256" && echo true || echo false)"

# Cleanup checksum test objects
assert "delete checksum object" $AWS s3 rm "s3://$BUCKET/checksum.txt"
assert "delete sha256 checksum object" $AWS s3 rm "s3://$BUCKET/checksum-sha256.txt"

# --- Conditional request headers ---
echo "conditional test" > "$TMPDIR/cond.txt"
assert "upload conditional test object" $AWS s3 cp "$TMPDIR/cond.txt" "s3://$BUCKET/cond.txt"

# Capture ETag (strip surrounding quotes from JSON output)
COND_ETAG=$($AWS s3api head-object --bucket "$BUCKET" --key "cond.txt" --query ETag --output text 2>/dev/null)

# If-Match: matching ETag → 200
assert "get-object if-match correct etag" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-match "$COND_ETAG" "$TMPDIR/cond-out.txt"

# If-Match: wrong ETag → 412 Precondition Failed
assert_fail "get-object if-match wrong etag returns 412" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-match '"wrongetag000000000000000000000000"' "$TMPDIR/cond-out.txt"

# If-None-Match: matching ETag → 304 (AWS CLI treats this as a failure/error exit)
assert_fail "get-object if-none-match matching etag returns 304" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-none-match "$COND_ETAG" "$TMPDIR/cond-out.txt"

# If-None-Match: non-matching ETag → 200
assert "get-object if-none-match different etag succeeds" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-none-match '"wrongetag000000000000000000000000"' "$TMPDIR/cond-out.txt"

# If-Modified-Since: far future date → 304 (object was not modified since then)
assert_fail "get-object if-modified-since future returns 304" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-modified-since "Mon, 01 Jan 2099 00:00:00 GMT" "$TMPDIR/cond-out.txt"

# If-Modified-Since: past date → 200 (object was modified after that date)
assert "get-object if-modified-since past succeeds" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-modified-since "Mon, 01 Jan 2000 00:00:00 GMT" "$TMPDIR/cond-out.txt"

# If-Unmodified-Since: far future date → 200 (object has not been modified since)
assert "get-object if-unmodified-since future succeeds" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-unmodified-since "Mon, 01 Jan 2099 00:00:00 GMT" "$TMPDIR/cond-out.txt"

# If-Unmodified-Since: past date → 412 (object was modified after threshold)
assert_fail "get-object if-unmodified-since past returns 412" \
    $AWS s3api get-object --bucket "$BUCKET" --key "cond.txt" \
    --if-unmodified-since "Mon, 01 Jan 2000 00:00:00 GMT" "$TMPDIR/cond-out.txt"

# Same conditions on HeadObject
assert "head-object if-match correct etag" \
    $AWS s3api head-object --bucket "$BUCKET" --key "cond.txt" \
    --if-match "$COND_ETAG"

assert_fail "head-object if-match wrong etag returns 412" \
    $AWS s3api head-object --bucket "$BUCKET" --key "cond.txt" \
    --if-match '"wrongetag000000000000000000000000"'

assert_fail "head-object if-none-match matching etag returns 304" \
    $AWS s3api head-object --bucket "$BUCKET" --key "cond.txt" \
    --if-none-match "$COND_ETAG"

assert "delete conditional test object" $AWS s3 rm "s3://$BUCKET/cond.txt"

# --- Delete operations ---
assert "delete object" $AWS s3 rm "s3://$BUCKET/test.txt"
assert_file_not_exists "deleted object gone from disk" "$DATA_DIR/buckets/$BUCKET/test.txt"
assert_fail "get deleted object" $AWS s3 cp "s3://$BUCKET/test.txt" "$TMPDIR/should-not-exist.txt"

assert "delete copied object" $AWS s3 rm "s3://$BUCKET/test-copy.txt"
assert "delete api-copied object" $AWS s3 rm "s3://$BUCKET/api-copy.txt"
assert "delete nested object" $AWS s3 rm "s3://$BUCKET/folder/nested/file.txt"
assert_file_not_exists "deleted nested object gone from disk" "$DATA_DIR/buckets/$BUCKET/folder/nested/file.txt"
assert "delete large object" $AWS s3 rm "s3://$BUCKET/big.bin"
assert "delete manual multipart object" $AWS s3 rm "s3://$BUCKET/manual-multipart.bin"
assert "delete upc source object" $AWS s3 rm "s3://$BUCKET/upc-source.bin"
assert "delete upc dest object" $AWS s3 rm "s3://$BUCKET/upc-dest.bin"

# Delete bucket
assert "delete empty bucket" $AWS s3 rb "s3://$BUCKET"
assert_file_not_exists "bucket dir gone from disk" "$DATA_DIR/buckets/$BUCKET"
assert_fail "head deleted bucket" $AWS s3api head-bucket --bucket "$BUCKET"

# --- Reject rb on non-empty bucket ---
NON_EMPTY_BUCKET="nonempty-$$"
assert "create non-empty test bucket" $AWS s3api create-bucket --bucket "$NON_EMPTY_BUCKET"
echo "stay" > "$TMPDIR/stay.txt"
assert "put object into non-empty bucket" $AWS s3 cp "$TMPDIR/stay.txt" "s3://$NON_EMPTY_BUCKET/stay.txt"
assert_fail "rb on non-empty bucket rejected" \
    $AWS s3api delete-bucket --bucket "$NON_EMPTY_BUCKET"
assert "head-bucket still succeeds after failed rb" \
    $AWS s3api head-bucket --bucket "$NON_EMPTY_BUCKET"
$AWS s3 rm "s3://$NON_EMPTY_BUCKET/stay.txt" > /dev/null
assert "rb succeeds after emptying" $AWS s3api delete-bucket --bucket "$NON_EMPTY_BUCKET"

# --- CORS tests ---
CORS_BUCKET="cors-test-$$"
assert "create cors test bucket" $AWS s3api create-bucket --bucket "$CORS_BUCKET"

# GetBucketCors on bucket with no CORS config should return an error
assert_fail "get-bucket-cors on unconfigured bucket fails" \
    $AWS s3api get-bucket-cors --bucket "$CORS_BUCKET"

# PutBucketCors
cat > "$TMPDIR/cors.json" <<'EOF'
{
  "CORSRules": [
    {
      "AllowedOrigins": ["*"],
      "AllowedMethods": ["GET", "PUT"],
      "AllowedHeaders": ["*"],
      "MaxAgeSeconds": 3600
    }
  ]
}
EOF
assert "put-bucket-cors" \
    $AWS s3api put-bucket-cors --bucket "$CORS_BUCKET" --cors-configuration file://"$TMPDIR/cors.json"

# GetBucketCors — should succeed now
assert "get-bucket-cors succeeds after put" \
    $AWS s3api get-bucket-cors --bucket "$CORS_BUCKET"

# Verify content via curl preflight
PREFLIGHT_STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X OPTIONS \
    -H "Origin: http://example.com" \
    -H "Access-Control-Request-Method: GET" \
    "$ENDPOINT/$CORS_BUCKET/test-object.txt")
if [ "$PREFLIGHT_STATUS" = "200" ]; then
    green "PASS: CORS preflight returns 200"
    PASS=$((PASS + 1))
else
    red "FAIL: CORS preflight returned $PREFLIGHT_STATUS (expected 200)"
    FAIL=$((FAIL + 1))
fi

# Preflight without CORS config should return 403
NOCORS_BUCKET="no-cors-$$"
assert "create no-cors bucket" $AWS s3api create-bucket --bucket "$NOCORS_BUCKET"
NOCORS_PREFLIGHT=$(curl -s -o /dev/null -w "%{http_code}" -X OPTIONS \
    -H "Origin: http://example.com" \
    -H "Access-Control-Request-Method: GET" \
    "$ENDPOINT/$NOCORS_BUCKET/test.txt")
if [ "$NOCORS_PREFLIGHT" = "403" ]; then
    green "PASS: CORS preflight without config returns 403"
    PASS=$((PASS + 1))
else
    red "FAIL: CORS preflight without config returned $NOCORS_PREFLIGHT (expected 403)"
    FAIL=$((FAIL + 1))
fi

# DeleteBucketCors
assert "delete-bucket-cors" \
    $AWS s3api delete-bucket-cors --bucket "$CORS_BUCKET"

# GetBucketCors after delete should fail again
assert_fail "get-bucket-cors fails after delete" \
    $AWS s3api get-bucket-cors --bucket "$CORS_BUCKET"

# Cleanup CORS test buckets
assert "delete cors test bucket" $AWS s3api delete-bucket --bucket "$CORS_BUCKET"
assert "delete no-cors test bucket" $AWS s3api delete-bucket --bucket "$NOCORS_BUCKET"
# --- Object tagging ---
echo ""
echo "--- Object tagging tests ---"
TAG_BUCKET="tag-$$"
assert "create tag bucket" $AWS s3api create-bucket --bucket "$TAG_BUCKET"
echo "tagged" > "$TMPDIR/tag.txt"
assert "put object for tagging" $AWS s3api put-object \
    --bucket "$TAG_BUCKET" --key tag.txt --body "$TMPDIR/tag.txt"

# put-object-tagging
assert "put-object-tagging 2 tags" $AWS s3api put-object-tagging \
    --bucket "$TAG_BUCKET" --key tag.txt \
    --tagging 'TagSet=[{Key=env,Value=prod},{Key=app,Value=maxio}]'

# get-object-tagging roundtrip
TAG_GET=$($AWS s3api get-object-tagging --bucket "$TAG_BUCKET" --key tag.txt 2>&1)
assert_eq "get-object-tagging sees env=prod" "prod" \
    "$(echo "$TAG_GET" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(next((t["Value"] for t in d.get("TagSet",[]) if t["Key"]=="env"), ""))' 2>/dev/null)"
assert_eq "get-object-tagging sees app=maxio" "maxio" \
    "$(echo "$TAG_GET" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(next((t["Value"] for t in d.get("TagSet",[]) if t["Key"]=="app"), ""))' 2>/dev/null)"

# delete-object-tagging → empty tag set
assert "delete-object-tagging" $AWS s3api delete-object-tagging \
    --bucket "$TAG_BUCKET" --key tag.txt
TAG_AFTER=$($AWS s3api get-object-tagging --bucket "$TAG_BUCKET" --key tag.txt 2>&1)
TAG_AFTER_COUNT=$(echo "$TAG_AFTER" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("TagSet",[])))' 2>/dev/null)
assert_eq "get-object-tagging empty after delete" "0" "$TAG_AFTER_COUNT"

$AWS s3 cp "$TMPDIR/tag.txt" "s3://$TAG_BUCKET/inline-tag.txt" \
    --metadata-directive REPLACE \
    --tagging 'env=prod&team=platform' > /dev/null
INLINE_TAGS=$($AWS s3api get-object-tagging --bucket "$TAG_BUCKET" --key "inline-tag.txt" --output json 2>/dev/null)
assert_contains "put-object x-amz-tagging sets tags" "env" "$INLINE_TAGS"
assert_contains "put-object x-amz-tagging team tag" "platform" "$INLINE_TAGS"

$AWS s3 rm "s3://$TAG_BUCKET/tag.txt" > /dev/null || true
$AWS s3 rm "s3://$TAG_BUCKET/inline-tag.txt" > /dev/null || true
assert "delete tag bucket" $AWS s3api delete-bucket --bucket "$TAG_BUCKET"

# --- Batch DeleteObjects ---
echo ""
echo "--- Batch DeleteObjects tests ---"
BATCH_BUCKET="batch-$$"
assert "create batch bucket" $AWS s3api create-bucket --bucket "$BATCH_BUCKET"
echo "a" > "$TMPDIR/a.txt"
echo "b" > "$TMPDIR/b.txt"
echo "c" > "$TMPDIR/c.txt"
$AWS s3 cp "$TMPDIR/a.txt" "s3://$BATCH_BUCKET/a.txt" > /dev/null
$AWS s3 cp "$TMPDIR/b.txt" "s3://$BATCH_BUCKET/b.txt" > /dev/null
$AWS s3 cp "$TMPDIR/c.txt" "s3://$BATCH_BUCKET/c.txt" > /dev/null

DEL_OUT=$($AWS s3api delete-objects --bucket "$BATCH_BUCKET" \
    --delete 'Objects=[{Key=a.txt},{Key=b.txt},{Key=c.txt}]' 2>&1)
DEL_COUNT=$(echo "$DEL_OUT" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("Deleted",[])))' 2>/dev/null)
assert_eq "delete-objects removed 3" "3" "$DEL_COUNT"
assert_file_not_exists "a.txt gone" "$DATA_DIR/buckets/$BATCH_BUCKET/a.txt"
assert_file_not_exists "b.txt gone" "$DATA_DIR/buckets/$BATCH_BUCKET/b.txt"
assert_file_not_exists "c.txt gone" "$DATA_DIR/buckets/$BATCH_BUCKET/c.txt"

# Batch delete with one missing key — still returns 200; S3 treats missing as deleted.
echo "x" > "$TMPDIR/x.txt"
$AWS s3 cp "$TMPDIR/x.txt" "s3://$BATCH_BUCKET/x.txt" > /dev/null
DEL_MIX=$($AWS s3api delete-objects --bucket "$BATCH_BUCKET" \
    --delete 'Objects=[{Key=x.txt},{Key=does-not-exist.txt}]' 2>&1)
DEL_MIX_COUNT=$(echo "$DEL_MIX" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(len(d.get("Deleted",[])))' 2>/dev/null)
if [ "$DEL_MIX_COUNT" -ge 1 ]; then
    green "PASS: delete-objects handles mixed existing + missing keys"
    PASS=$((PASS + 1))
else
    red "FAIL: delete-objects mixed gave Deleted=$DEL_MIX_COUNT"
    FAIL=$((FAIL + 1))
fi

assert "delete batch bucket" $AWS s3api delete-bucket --bucket "$BATCH_BUCKET"

# --- Versioning ---
echo ""
echo "--- Versioning tests ---"
VER_BUCKET="ver-$$"
assert "create version bucket" $AWS s3api create-bucket --bucket "$VER_BUCKET"
assert "put-bucket-versioning Enabled" $AWS s3api put-bucket-versioning \
    --bucket "$VER_BUCKET" --versioning-configuration 'Status=Enabled'

VER_STATUS=$($AWS s3api get-bucket-versioning --bucket "$VER_BUCKET" 2>&1)
assert_eq "get-bucket-versioning returns Enabled" "Enabled" \
    "$(echo "$VER_STATUS" | grep -o '"Status": "[^"]*"' | cut -d'"' -f4)"

# Three PUTs of same key → three versions
echo "v1" > "$TMPDIR/v1.txt"
echo "v2" > "$TMPDIR/v2.txt"
echo "v3" > "$TMPDIR/v3.txt"
V1_VID=$($AWS s3api put-object --bucket "$VER_BUCKET" --key obj --body "$TMPDIR/v1.txt" 2>&1 | grep -o '"VersionId": "[^"]*"' | cut -d'"' -f4)
V2_VID=$($AWS s3api put-object --bucket "$VER_BUCKET" --key obj --body "$TMPDIR/v2.txt" 2>&1 | grep -o '"VersionId": "[^"]*"' | cut -d'"' -f4)
V3_VID=$($AWS s3api put-object --bucket "$VER_BUCKET" --key obj --body "$TMPDIR/v3.txt" 2>&1 | grep -o '"VersionId": "[^"]*"' | cut -d'"' -f4)
if [ -n "$V1_VID" ] && [ -n "$V2_VID" ] && [ -n "$V3_VID" ] \
    && [ "$V1_VID" != "$V2_VID" ] && [ "$V2_VID" != "$V3_VID" ]; then
    green "PASS: three distinct VersionIds returned"
    PASS=$((PASS + 1))
else
    red "FAIL: VersionIds v1=$V1_VID v2=$V2_VID v3=$V3_VID"
    FAIL=$((FAIL + 1))
fi

# list-object-versions shows all 3
VER_LIST=$($AWS s3api list-object-versions --bucket "$VER_BUCKET" 2>&1)
VER_LIST_COUNT=$(echo "$VER_LIST" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("Versions",[])))' 2>/dev/null)
assert_eq "list-object-versions returns 3 versions" "3" "$VER_LIST_COUNT"

# get-object --version-id returns historical content
$AWS s3api get-object --bucket "$VER_BUCKET" --key obj --version-id "$V1_VID" \
    "$TMPDIR/v1-out.txt" > /dev/null 2>&1
if [ "$(cat "$TMPDIR/v1-out.txt" 2>/dev/null)" = "v1" ]; then
    green "PASS: get-object --version-id retrieves old v1 content"
    PASS=$((PASS + 1))
else
    red "FAIL: get-object --version-id returned $(cat "$TMPDIR/v1-out.txt" 2>/dev/null)"
    FAIL=$((FAIL + 1))
fi

# delete-object (no version-id) creates a delete marker
DEL_MARKER=$($AWS s3api delete-object --bucket "$VER_BUCKET" --key obj 2>&1)
assert_eq "delete-object returns DeleteMarker=true" "true" \
    "$(echo "$DEL_MARKER" | grep -o '"DeleteMarker": [a-z]*' | awk '{print $2}')"

VER_LIST_AFTER=$($AWS s3api list-object-versions --bucket "$VER_BUCKET" 2>&1)
DM_COUNT=$(echo "$VER_LIST_AFTER" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("DeleteMarkers",[])))' 2>/dev/null)
assert_eq "list-object-versions shows 1 DeleteMarker" "1" "$DM_COUNT"

# delete-object --version-id hard-deletes a specific version
assert "delete-object --version-id hard-delete v1" $AWS s3api delete-object \
    --bucket "$VER_BUCKET" --key obj --version-id "$V1_VID"
VER_LIST_FINAL=$($AWS s3api list-object-versions --bucket "$VER_BUCKET" 2>&1)
VER_FINAL_COUNT=$(echo "$VER_LIST_FINAL" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("Versions",[])))' 2>/dev/null)
assert_eq "post hard-delete: 2 versions remain" "2" "$VER_FINAL_COUNT"

# Suspend versioning
assert "put-bucket-versioning Suspended" $AWS s3api put-bucket-versioning \
    --bucket "$VER_BUCKET" --versioning-configuration 'Status=Suspended'

VER_LIST_SUSPENDED=$($AWS s3api list-object-versions --bucket "$VER_BUCKET" 2>&1)
VER_SUSPENDED_COUNT=$(echo "$VER_LIST_SUSPENDED" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("Versions",[])))' 2>/dev/null)
assert_eq "suspending versioning preserves existing versions" "2" "$VER_SUSPENDED_COUNT"

echo "v-null" > "$TMPDIR/v-null.txt"
NULL_PUT=$($AWS s3api put-object --bucket "$VER_BUCKET" --key obj --body "$TMPDIR/v-null.txt" 2>&1)
assert_eq "suspended PUT does not return a new VersionId" "false" "$(echo "$NULL_PUT" | grep -q '"VersionId"' && echo true || echo false)"

VER_LIST_WITH_NULL=$($AWS s3api list-object-versions --bucket "$VER_BUCKET" 2>&1)
NULL_COUNT=$(echo "$VER_LIST_WITH_NULL" | python3 -c 'import sys,json; print(sum(1 for v in json.load(sys.stdin).get("Versions",[]) if v.get("VersionId")=="null"))' 2>/dev/null)
assert_eq "suspended PUT creates/updates null version" "1" "$NULL_COUNT"

$AWS s3api get-object --bucket "$VER_BUCKET" --key obj --version-id null \
    "$TMPDIR/v-null-out.txt" > /dev/null 2>&1
if [ "$(cat "$TMPDIR/v-null-out.txt" 2>/dev/null)" = "v-null" ]; then
    green "PASS: get-object --version-id null retrieves suspended current"
    PASS=$((PASS + 1))
else
    red "FAIL: get-object --version-id null returned $(cat "$TMPDIR/v-null-out.txt" 2>/dev/null)"
    FAIL=$((FAIL + 1))
fi

# Cleanup all remaining versions
for vid in $(echo "$VER_LIST_FINAL" | python3 -c 'import sys,json; [print(v["VersionId"]) for v in json.load(sys.stdin).get("Versions",[])+json.load(open("/dev/stdin")).get("DeleteMarkers",[])]' 2>/dev/null || echo ""); do
    $AWS s3api delete-object --bucket "$VER_BUCKET" --key obj --version-id "$vid" > /dev/null 2>&1 || true
done
# Fallback — delete everything
$AWS s3 rm "s3://$VER_BUCKET" --recursive > /dev/null 2>&1 || true
# remove any lingering version files on disk (versioning tests leave .versions/ dirs)
rm -rf "$DATA_DIR/buckets/$VER_BUCKET"
$AWS s3api delete-bucket --bucket "$VER_BUCKET" > /dev/null 2>&1 || true

# --- ListObjectsV1 pagination ---
echo ""
echo "--- ListObjectsV1 pagination tests ---"
PAG_BUCKET="pag-$$"
assert "create pagination bucket" $AWS s3api create-bucket --bucket "$PAG_BUCKET"
for i in 0 1 2 3 4 5 6; do
    echo "obj$i" > "$TMPDIR/obj$i.txt"
    $AWS s3 cp "$TMPDIR/obj$i.txt" "s3://$PAG_BUCKET/obj$i" > /dev/null
done

PAGE1=$($AWS s3api list-objects --bucket "$PAG_BUCKET" --max-keys 3 2>&1)
PAGE1_COUNT=$(echo "$PAGE1" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("Contents",[])))' 2>/dev/null)
assert_eq "list-objects v1 page 1 returns 3" "3" "$PAGE1_COUNT"
PAGE1_TRUNC=$(echo "$PAGE1" | grep -o '"IsTruncated": [a-z]*' | awk '{print $2}')
assert_eq "list-objects v1 page 1 IsTruncated=true" "true" "$PAGE1_TRUNC"
NEXT_MARKER=$(echo "$PAGE1" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("NextMarker") or d["Contents"][-1]["Key"])' 2>/dev/null)

PAGE2=$($AWS s3api list-objects --bucket "$PAG_BUCKET" --max-keys 3 --marker "$NEXT_MARKER" 2>&1)
PAGE2_COUNT=$(echo "$PAGE2" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("Contents",[])))' 2>/dev/null)
assert_eq "list-objects v1 page 2 returns 3" "3" "$PAGE2_COUNT"

$AWS s3 rm "s3://$PAG_BUCKET" --recursive > /dev/null
assert "delete pagination bucket" $AWS s3api delete-bucket --bucket "$PAG_BUCKET"

# --- GetBucketLocation ---
echo ""
echo "--- GetBucketLocation test ---"
LOC_BUCKET="loc-$$"
assert "create loc bucket" $AWS s3api create-bucket --bucket "$LOC_BUCKET"
LOC=$($AWS s3api get-bucket-location --bucket "$LOC_BUCKET" 2>&1)
# LocationConstraint is "us-east-1" or null-equivalent
LOC_VAL=$(echo "$LOC" | grep -o '"LocationConstraint": "[^"]*"' | cut -d'"' -f4)
if [ "$LOC_VAL" = "us-east-1" ] || [ -z "$LOC_VAL" ]; then
    green "PASS: get-bucket-location returned '$LOC_VAL'"
    PASS=$((PASS + 1))
else
    red "FAIL: get-bucket-location returned '$LOC_VAL'"
    FAIL=$((FAIL + 1))
fi
assert "delete loc bucket" $AWS s3api delete-bucket --bucket "$LOC_BUCKET"

# --- Cross-bucket CopyObject ---
echo ""
echo "--- Cross-bucket CopyObject test ---"
SRC_BUCKET="src-$$"
DST_BUCKET="dst-$$"
assert "create src bucket" $AWS s3api create-bucket --bucket "$SRC_BUCKET"
assert "create dst bucket" $AWS s3api create-bucket --bucket "$DST_BUCKET"
echo "cross-bucket content" > "$TMPDIR/xcopy.txt"
$AWS s3 cp "$TMPDIR/xcopy.txt" "s3://$SRC_BUCKET/src-key" > /dev/null
assert "cross-bucket copy-object" $AWS s3api copy-object \
    --bucket "$DST_BUCKET" --key dst-key \
    --copy-source "$SRC_BUCKET/src-key"
$AWS s3api get-object --bucket "$DST_BUCKET" --key dst-key "$TMPDIR/xcopy-out.txt" > /dev/null
if cmp -s "$TMPDIR/xcopy.txt" "$TMPDIR/xcopy-out.txt"; then
    green "PASS: cross-bucket copy preserves content"
    PASS=$((PASS + 1))
else
    red "FAIL: cross-bucket copy differs"
    FAIL=$((FAIL + 1))
fi
$AWS s3 rm "s3://$SRC_BUCKET/src-key" > /dev/null
$AWS s3 rm "s3://$DST_BUCKET/dst-key" > /dev/null
assert "delete src bucket" $AWS s3api delete-bucket --bucket "$SRC_BUCKET"
assert "delete dst bucket" $AWS s3api delete-bucket --bucket "$DST_BUCKET"
# --- Presigned URLs ---
echo ""
echo "--- Presigned URL tests ---"
PRE_BUCKET="presign-$$"
assert "create presign bucket" $AWS s3api create-bucket --bucket "$PRE_BUCKET"
echo "presigned content" > "$TMPDIR/presign.txt"
$AWS s3 cp "$TMPDIR/presign.txt" "s3://$PRE_BUCKET/obj" > /dev/null

PRESIGN_URL=$($AWS s3 presign "s3://$PRE_BUCKET/obj" --expires-in 3600 2>&1)
if echo "$PRESIGN_URL" | grep -q "X-Amz-Signature"; then
    green "PASS: s3 presign returns signed URL"
    PASS=$((PASS + 1))
else
    red "FAIL: s3 presign output missing signature: $PRESIGN_URL"
    FAIL=$((FAIL + 1))
fi

# Curl the presigned URL without any AWS creds → should succeed
(unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY; curl -s "$PRESIGN_URL" -o "$TMPDIR/presign-out.txt")
if cmp -s "$TMPDIR/presign.txt" "$TMPDIR/presign-out.txt"; then
    green "PASS: presigned URL GET works without credentials"
    PASS=$((PASS + 1))
else
    red "FAIL: presigned URL GET content mismatch"
    FAIL=$((FAIL + 1))
fi

# Expired presigned URL → 403
EXPIRED_URL=$($AWS s3 presign "s3://$PRE_BUCKET/obj" --expires-in 1 2>&1)
sleep 2
EXPIRED_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$EXPIRED_URL")
if [ "$EXPIRED_STATUS" = "403" ]; then
    green "PASS: expired presigned URL returns 403"
    PASS=$((PASS + 1))
else
    red "FAIL: expired presigned URL returned $EXPIRED_STATUS (expected 403)"
    FAIL=$((FAIL + 1))
fi

$AWS s3 rm "s3://$PRE_BUCKET/obj" > /dev/null
assert "delete presign bucket" $AWS s3api delete-bucket --bucket "$PRE_BUCKET"
# --- Object key edge cases ---
echo ""
echo "--- Object key edge case tests ---"
KEY_BUCKET="keys-$$"
assert "create keys bucket" $AWS s3api create-bucket --bucket "$KEY_BUCKET"
echo "edge" > "$TMPDIR/edge.txt"

# NOTE: Object bytes are stored as on-disk paths under `{data_dir}/buckets/{bucket}/`,
# so key length is ultimately bounded by OS filesystem limits (NAME_MAX per
# component, PATH_MAX for the full path). The S3 spec allows 1024-byte UTF-8
# keys; MaxIO enforces that upper bound in `validate_key`.

# 240-byte single-component key (comfortably under NAME_MAX on common filesystems)
MAX_KEY_SINGLE=$(python3 -c 'print("a"*240)')
assert "PUT with 240-byte single-component key" $AWS s3api put-object \
    --bucket "$KEY_BUCKET" --key "$MAX_KEY_SINGLE" --body "$TMPDIR/edge.txt"
assert "GET with 240-byte single-component key" $AWS s3api get-object \
    --bucket "$KEY_BUCKET" --key "$MAX_KEY_SINGLE" "$TMPDIR/maxkey-out.txt"

# 1025-byte key — rejected by MaxIO's validate_key (>1024) regardless of FS
OVER_KEY=$(python3 -c 'k="/".join(["a"*200]*5 + ["a"*25]); print(k[:1025])')
assert_fail "PUT with 1025-byte key rejected" \
    $AWS s3api put-object --bucket "$KEY_BUCKET" --key "$OVER_KEY" --body "$TMPDIR/edge.txt"

# Unicode + space key
UNICODE_KEY="日本語 file.txt"
assert "PUT unicode+space key" $AWS s3api put-object \
    --bucket "$KEY_BUCKET" --key "$UNICODE_KEY" --body "$TMPDIR/edge.txt"
$AWS s3api get-object --bucket "$KEY_BUCKET" --key "$UNICODE_KEY" "$TMPDIR/unicode-out.txt" > /dev/null
if cmp -s "$TMPDIR/edge.txt" "$TMPDIR/unicode-out.txt"; then
    green "PASS: unicode+space key roundtrip"
    PASS=$((PASS + 1))
else
    red "FAIL: unicode+space key roundtrip mismatch"
    FAIL=$((FAIL + 1))
fi

$AWS s3 rm "s3://$KEY_BUCKET" --recursive > /dev/null 2>&1 || true
assert "delete keys bucket" $AWS s3api delete-bucket --bucket "$KEY_BUCKET"

# --- Summary ---
echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
