# Store index (lead decision, 2026-10-25)

Problem: the browser verifies every record before the first page. A store with 100,000 records or more is slow to open. The browser also has no blame and no file history.

Rule: the index is a cache. Signed records always win. The browser never shows index data that it cannot check against records, and it drops the whole index on any mismatch.

## Objects

All index objects are encrypted to the project recipients, like states, and signed by the machine that writes them (a trusted machine key, the same check as for records).

1. `index/heads/<machine>/<seq:020>.age`: a snapshot for the first page.
   - Watermark: for each machine, the last record sequence number and the hash of that record line.
   - Lines (name, id, owner, machine, head), refs (name, target), open changes (id, latest version state, title, author, time), counts (states, records, overlays).
   - The newest snapshot by watermark wins. A writer makes a new one after each push that adds records.
2. `index/paths/<state>.age`: for one state, the paths that changed against its first parent: path, old and new blob hash, mode. It is small, and it never changes, because a state never changes.
3. `index/graph/<state>.age`: for one state, skip pointers: ancestors at distance 1, 2, 4, 8, and so on, along the first parent. It never changes.

Blame and file history use the `paths` objects along the first-parent chain (skip pointers find the range), then load only the blob versions of that one file. The diff code in layr-model computes the blame lines. The same function serves the CLI `layr blame`, so there is one implementation.

## Browser flow

1. Unlock, load the newest `heads` snapshot, check its signature, render Lines, Code and Changes at once.
2. In the background, load records from the newest backwards and verify them. For each watermark entry, check that the record exists and its line hash matches.
3. On a mismatch, drop the index, show a short note, and fall back to the full verified load.
4. Pages that need full history (Activity, all states) wait for the records they show, and page through them.

## Writer

`layr index write` on a trusted machine, run by the executor after each sync and by `layr push --index`. It is idempotent. Missing `paths` and `graph` objects are filled in for states that do not have them.

## Checks

- Golden: blame and file history from the index equal `layr blame` and `layr log -- <path>` on the CLI.
- Tamper: a changed snapshot (wrong head, wrong watermark hash, unknown signer) is refused and the full load is used.
- Benchmark: open time for the first page at 1,000, 100,000 and 1,000,000 records, before and after. Report numbers.
