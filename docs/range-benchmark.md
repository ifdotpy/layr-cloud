# Native range read measurements

Date: 2026-10-14. Local release build of the modified `desk_core::Reader`.
The baseline calls `read_full`; the selective path calls `read`.
Both use the same encrypted object and an instrumented memory store.
The fixture has 1,024 random files of 16,000 bytes each.
The encrypted part has 16,398,528 bytes. Each result is the median of five runs.

| Case | Path | Bytes fetched | Requests | Local time, ms |
| --- | --- | ---: | ---: | ---: |
| One middle file | Full | 16,398,528 | 1 | 74.45 |
| One middle file | Selective | 393,216 | 3 | 0.96 |
| 32 scattered files | Full | 16,398,528 | 1 | 60.12 |
| 32 scattered files | Selective | 17,447,104 | 9 | 64.54 |
| Full restore | Full | 16,398,528 | 1 | 147.01 |
| Full restore | New bulk path | 16,398,528 | 1 | 133.06 |

The middle-file case fetches 41.7 times fewer bytes and makes two extra requests.
The scattered case reaches the eight-range limit, then fetches the full object.
It fetches 1 MiB more data than the full path and makes eight extra requests.
Both full-restore cases use `read_full`. Their time difference is not evidence of an improvement.

These times exclude network latency, TLS, S3, and the gateway.
They do not predict end-to-end latency. Extra sequential requests can offset the byte reduction on a slow connection.
The test does not measure a real vault or production request cost.
Local HTTP tests separately cover authentication, signed S3 ranges, and the native client.
The browser reader still fetches complete objects.

The temporary harness and raw output are in `/tmp/layr-format-bench/production.rs` and
`/tmp/layr-format-bench/production-results.jsonl` on the development machine.
