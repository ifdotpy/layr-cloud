# Release files verification, 2026-10-26

Cloud branch `cloud/release-files` is rebased onto layr-cloud main `0c3807f`.
All shared dependencies pin layr main
`b41642ab1f4ba4591072886f7ea8153ffced2c57`, which contains the merged CLI work.

| Final cloud check | Result |
| --- | --- |
| Native workspace on macOS | 67 passed, 0 failed, 9 ignored |
| Web unit | 89 passed, 0 failed |
| Playwright | 46 passed, 0 failed |
| Production WASM, TypeScript, Vite | passed |
| Infrastructure unittest discovery | 36 run, 33 passed, 3 skipped |
| cfn-lint, all AWS templates | passed |
| t48 TLS gateway and MinIO | 1 passed, 0 failed |
| Full t27 gateway boundary suite | 1 passed, 0 failed |
| t47 current-epoch fixture export | 38 passed, 0 failed |
| Real current/historical browser checks | 6 passed |

The final Linux suites ran sequentially as root in the dedicated `layr-rf`
Ubuntu 26.04 VM. It has limits of 4 CPUs, 4 GiB RAM and 24 GiB disk. Builds used
two cargo jobs and about 2.1 GiB of target output. No suite ran in `layr-dev`
during this follow-up. The VM remains until task acceptance, then is deleted.

## Actual callers

`t48` attaches, lists, and gets a signed 10 MiB file through the real TLS gateway
and MinIO. It then rotates the project, attaches another file in the current
epoch, verifies current and retained historical downloads, and verifies a public
epoch download. The disposable store's paper opener is supplied only for the
rotation that retains that reader.

The real browser uses the production WASM worker and the signed CLI fixture
exported after rotation. It verifies:

1. A 10 MiB download, its expected plaintext SHA-256, and filename.
2. Valid age ciphertext with wrong plaintext: SHA-256 error, no saved file.
3. Truncated age ciphertext: error, no saved file.
4. A file above 256 MiB: CLI command and no object fetch.
5. The current file is fetched from its canonical epoch source.
6. A retained root-epoch release downloads with its sealed historical identity.

The worker passes the actual fetched source to `Session::open_release_file`,
which uses `Store::source` for identity selection and compares the logical key
against signed metadata. The public gateway streams canonical epoch file keys.
Main's read-proof-only `files/` listing is retained without a duplicate rule.
Desktop and mobile screenshot artifacts were inspected.

## Streaming and limits

The real TLS gateway streamed 128 MiB into and out of MinIO, verified every byte,
and checked create-once writes, read proof, the ciphertext size limit, and cleanup
of a disconnected multipart upload. The final t48 process RSS delta was
8,466,432 bytes, below 16 MiB. Earlier runs measured 8,454,144 to 14,909,440 bytes.
These are sampled process measurements for one active request. PUT uses 8 MiB
parts; GET uses a bounded channel of 64 KiB blocks. Other object transfers keep
the existing 32 MiB buffered path. Shared store unit tests additionally cover
stream overflow and abort behavior.

## IAM and deployment

The template adds root and epoch `files/*` resources to Writer PutObject and
AbortMultipartUpload, and to GC DeleteObject. Tests check all four resource
additions and preserve the writer's read/list restrictions. No AWS changes were
made by this task.

The deployment owner must apply this IAM change to `layr-store-m8-final` before
the live 128 MiB CloudFront check. `layr-store-layr` needs the same change before
the project move. That live check follows CloudFront availability and is not
claimed by the local MinIO evidence.

## Logs and reproduction

- Native: `/tmp/cloud-release-epoch-cargo-3.log`.
- Web: `/tmp/cloud-release-epoch-unit-final.log`, `/tmp/cloud-release-epoch-playwright.log`.
- Infrastructure: `/tmp/layr-rf-infra-tests.log`, `/tmp/layr-rf-cfn-lint.log`.
- VM: `/root/work/t48-2.log`, `/root/work/t27.log`, `/root/work/t47-export-3.log`.
- Browser: `/tmp/cloud-release-current-epoch-browser-final.log`.

The fixture archive crossed the VM boundary through `/mnt/mac/tmp`, with matching
size 342,118,400 bytes and SHA-256
`98e576e6dd1b9f9e57aed22eeb25915f0dc0aa8696f31505c4d33641e0111705`.
Copied MinIO and rustc binaries, and the final copied fixture/test scripts, also
matched by size and SHA-256. Archives and binaries did not cross OrbStack stdio.
See `docs/release-files.md` for the reproducible fixture and browser commands.

The pre-merge shared CLI implementation also passed its workspace checks (225 on
macOS, 221 on Linux), t47 file/S3 (37/28), t41 (59), t42 (99), and t40 (24).
Those results belong to the original shared feature revision `54af64c`; the
final gateway and epoch export checks above use the merged `b41642a` pin.
