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

## Browser check on the deployed service

Date: 2026-10-14. Release `e7641fb3e1099996d5ecd5ac70b1de40d2ca20cf42bcdfe551dc2d0140d5b4db`, source commit `e78a216`.
The built Worker ran in Chromium against `https://desk.layr.ws` and the real S3 archive.
The file was `.agents/skills/codex-orchestrator/SKILL.md` in `codex-agents`: 11,051 bytes, one full frame.
The Reader verified the reconstructed file hash in every run.

Each mode ran three times with a new Worker and an empty data cache.
The full mode removed the Range request header through the test harness, so the gateway returned the complete object.
Both modes used the same deployed Worker and file entry.
The measurements exclude vault unlock, tree retrieval, and WebAssembly initialization.

| Mode | Median first read | Bytes fetched | Requests |
| --- | ---: | ---: | ---: |
| Full object | 6,439 ms | 16,773,311 | 1 |
| Range | 170 ms | 131,072 | 1 |

For this file, the Range path took about 38 times less time and fetched about 128 times fewer bytes.
The frame fits in the first ciphertext block. Files elsewhere in a part can require more requests.
Repeat reads took 0.7 to 1.4 ms and fetched no bytes.
These results describe this file and connection, not every file or network.

An earlier Node run of the old JavaScript reader took 7,036 ms, including 6,915 ms of network work.
Its output size and transfer size matched the browser comparison.
The browser now uses the Rust age reader, two concurrent frame fetches at most, and a dedicated Worker.
It shows the download byte count, decryption, and file restoration separately.

Raw browser samples: [browser-range-2026-10-14.json](measurements/browser-range-2026-10-14.json).
