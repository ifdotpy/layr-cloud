#!/bin/bash
# Streamed built-file objects and real CLI caller through TLS gateway and MinIO.
. "$(dirname "$0")/lib.sh"
set -e
: "${LAYR_MINIO_ENDPOINT:?Set the local MinIO endpoint}"
: "${LAYR_MINIO_ACCESS_KEY:?Set the local MinIO test access key}"
: "${LAYR_MINIO_SECRET_KEY:?Set the local MinIO test secret key}"
export LAYR_GATEWAY_BINARY="${LAYR_GATEWAY_BINARY:-$(cd "$(dirname "$0")/.." && pwd)/target/debug/layr-gateway}"
export LAYR_GATEWAY_TEST_STORE="$LAYR_ROOT"
export LAYR_RELEASE_ONLY=1
cd "$(dirname "$0")/.."
cargo test --locked minio::actual_cli_minio_round_trip -- --exact --ignored --nocapture
PASS=$((PASS+1))
finish
