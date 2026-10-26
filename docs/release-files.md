# Release files

A maintainer publishes built files with `layr release attach release/1.2.0 <path>...`.
The signed `release.files` record binds the reference, exact state, filename,
plaintext size, SHA-256, and an opaque `files/<uuid>.age` object key. See the
shared CLI repository's `docs/release-files.md` for command usage and GC rules.

The Releases list shows a file count when at least one release has files. A
release page shows each filename, size, and Download button. The worker obtains
metadata from the authenticated model, decrypts with the session's project
identity for the fetched object's epoch, and checks the size and SHA-256 before returning a Blob. A failure
shows an error and creates no download. Public projects use their public opener.
Files over 256 MiB show `layr release get` instead of a Download button.

## Gateway transfers

Only UUID `.age` objects under `files/` use the streaming path. GET requires the
existing read proof or public access. PUT requires the write grant and publishes
with create-once semantics. The maximum ciphertext size is the 512 MiB plaintext
limit plus the shared age overhead allowance. Other objects retain the 32 MiB
buffered path.

GET forwards S3 bytes through a one-element channel of 64 KiB blocks, so client
backpressure reaches S3. PUT feeds S3 multipart uploads in 8 MiB parts. A limit,
length, transport, or completion failure aborts the multipart upload. Dropping a
client connection also aborts it. A GC grant can delete release file objects.

## Verification

The shared CLI suite `tests/t47-release-files.sh` covers file and MinIO stores,
protection, moved references, public reads, current and historical epoch reads,
corrupt ciphertext, and release deletion and orphan grace periods.

Cloud checks:

```sh
cargo test --workspace
cd web
npm test
npm run test:e2e
npm run build
```

For the TLS gateway and MinIO checks, provide `LAYR_MINIO_ENDPOINT`,
`LAYR_MINIO_ACCESS_KEY`, `LAYR_MINIO_SECRET_KEY`, and the real `LAYR_BIN`. Run
`tests/t48-release-files-gateway.sh` as root on a Linux machine with btrfs. It
checks a 128 MiB streamed transfer, process RSS, create-once writes, read proof,
the ciphertext limit, interrupted multipart cleanup, and a real CLI attach/get.
Run the full `tests/t27-gateway.sh` separately for the existing gateway boundary
suite.

For the real browser flow, export a disposable public fixture with the shared
CLI suite's `LAYR_RELEASE_EXPORT` option, or use
`tests/make-release-cli-fixture.sh` with `LAYR_CLI_SOURCE`, `LAYR_BIN`, and
`LAYR_RELEASE_EXPORT` to export after rotation to a current public epoch. Build
the cloud workspace binaries and examples with `layr-web/test-tools`, then run:

```sh
cd web
LAYR_RELEASE_FIXTURE=/tmp/layr-release-browser-fixture node tests/release-files-live.mjs
```

This local harness uses an isolated MinIO in `layr-rf` (override with
`LAYR_RELEASE_VM`), a real gateway and the
production WASM worker. It verifies a 10 MiB download and its digest, SHA-256
failure, truncated age failure, and the over-limit command without fetching the
large object. For an epoch export, it additionally verifies the canonical
current-epoch source and a retained root-epoch release download. It also writes
desktop and mobile screenshots. Credentials are
synthetic, generated for the run, and passed only through the environment.

The worker resolves current objects first and tries retained historical sources
only on HTTP 404. It passes the exact canonical source to
`Session::open_release_file`. The session selects that epoch's identity and
compares the logical file key with the signed attachment metadata before
checking the plaintext. A failed fetch or authentication never selects another
identity by guessing.

The store IAM template grants the writer PutObject and AbortMultipartUpload on
root and epoch `files/*` resources. The GC role can delete those resources.
Listing still requires a read proof. The deployment owner must apply the store
IAM change before a live streaming check.
