# Review: layr cloud design and code against the owner goals (2026-10-31)

Read only review of docs, code, branches cloud/laptop-runner and cloud/cdn-client, and repo
ifdotpy/layr-cloud. Nothing was executed. Numbers come from the docs. "Unverified" marks my inference.

Goals used: G1 speed, G2 own forge + CI/CD, G3 warm lines, G4 shared test results (layr-retest),
G5 disk, G6 generic layr.

Severity: BLOCKS (goal needs a redesign), SLOWS, COSTS DISK.
## Top 5 (details below)

1. P1 Every check runs in a cold, empty copy and cannot save anything back. BLOCKS G3, G4.
2. P2 Sync is pull only, scans the whole history, runs on 60 to 120 s timers. No events, no job claims. BLOCKS G1, G2.
3. P3 A 20 to 30 GB cache moves as one serial, single thread pipeline and stays forever in the store. SLOWS G1, COSTS DISK G5.
4. P4 Cache access is tied to local uids and per machine slots. Autoscaled or ephemeral runners cannot join. BLOCKS G3, G4.
5. P5 layr-retest records are keyed by auto state ids, need the layr CLI, and double the target cache. BLOCKS G4, COSTS DISK G5.

## P1. Checks and queue run in a cold copy with no write back
What:
- `src/cmd/checks.rs:~315` and `src/cmd/admin.rs:810` (`with_try_copy`) make the work copy with
  `repo.materialize(...)`. `materialize` (`src/repo.rs:1485`, `prepare_at` ~1540) fills only the
  legacy `StateRec.caches` and `regenerable` placeholders. It never calls `take_caches`
  (`src/cache_overlay.rs:321`), which is called only from `line new` (`repo.rs:1453`, `lineops.rs:122`).
  So a check copy has no `target/`, no `.layr-retest`.
- `docs/laptop-runner.md`: "Checks compile in private copies deleted after each run; they share only
  the read only toolchain". A `unit` check on a runner is a full cold build every time.
- The result of a check is only `check.log` as a derived state plus a signed attestation
  (`checks.rs:~330`). There is no way to name a folder of the work copy as an output to keep.
- The merge queue is no better: `mergecmd.rs:~520` runs `admin::run_check` on the queue host when no
  attestation exists for the candidate. Same cold copy, same serial loop.
Conflict: G3 (every line starts warm) is not true for the place where most runs happen. G4 cannot
work: retest has nothing to read and nowhere to write.
How bad: BLOCKS. The biggest single time cost of a gate is not addressed by any cache design in the docs.
Smallest fix:
- In `with_try_copy`, call `take_caches` for the state (nearest ancestor cache state, as `line new X <state>`).
  One function call on the existing rule.
- Add a check setting `checks.keep` (list of cache paths). After the command exits, the runner process
  (outside the sandbox) saves those folders with `save_cache_overlays` into the overlay line of the
  checked line, then deletes the copy. Layr stays generic: it knows folders, not tests.

## P2. Pull only sync, full history scans, timers, no events, no claims
What:
- `src/store_remote/reader.rs:351` (`fetch`): each fetch exports the whole local log
  (`export_jsonl`), lists `records/`, downloads and decrypts every segment, verifies every record
  line (`records_with_local`, line 26), then lists all of `states/` (`attempts`, line 269).
  The cdn-client branch batches the GETs but keeps the full scan. Cost is O(history) per poll.
- `DEPLOYED.md` measured a trivial 5 commit project: one runner cycle = 366 requests (88 record objects
  per fetch, 178 on push). Runner timers: `layr-sync@.timer` 120 s + 30 s jitter; cloud
  `layr-checks.timer` 60 s after the last run; `layr-transfer.timer` 1 min.
- `src/cmd/checks.rs:~187`: the watcher rescans the local db every 5 s and, for protected lines, reads
  all records each scan (`repo.p.db.records(usize::MAX >> 1)`, ~line 228).
- `infra/aws/checks.py` `tick_project`: fetch, requests apply, `checks watch --once` (runs ALL pending jobs
  one by one, timeout 900 s each, `TimeoutStartSec=0`), only then `push`, `index write`. Results and
  UI actions wait for the whole job set. While a check runs the tick holds `active.lock`, so no fetch
  and no request apply happens.
- `checks.lock` and `queue.lock` are per project flocks. `docs/checks.md`: "The lock is local, not a
  distributed lease: run one active runner per project and platform". Two runners would run the
  same job twice. There is no claim object.
- Gateway: no event or long poll route (`docs/cloud-design.md` decision 3 lists only GET/PUT/DELETE of objects).
Conflict: G1 (latency of gates and releases) and G2 (a forge that reacts at once to an approval or a push).
Floor today for one change: push, up to 120 s wait for the runner fetch, job time, up to 120 s wait for
the push of the result, then the queue host fetch. Roughly 4 to 5 minutes of pure waiting.
How bad: BLOCKS the speed goal for gates. Each extra runner adds duplicate work, not speed.
Smallest fix (three small pieces, none changes the store format):
1. Gateway event feed. Every write passes through the gateway PUT (write tokens). It already sees the
   object name. Keep an in memory counter and last N names per project; add
   `GET /v1/p/<project>/events?after=<n>` as a long poll (or SSE). No decryption. Clients and runners
   wake in under a second. Keep the timer as a fallback.
2. Incremental fetch. Use the S3 and gateway listing `start-after` per machine from the local verified
   prefix. Records are immutable and named by sequence range, so "list after my last segment" is
   exact. The cdn-client batching then pays off. O(new) instead of O(history).
3. Job claim as a create only object: `claims/<hash(state,argv,platform)>` written with
   `If-None-Match: *` (the store already has this primitive). First writer runs the job; others skip or
   take it after a TTL. Also push each result immediately after the job (call `push` per job, not per
   tick). This is also the base for several runners and for P6.

## P3. Moving and keeping a 20 to 30 GB cache
What:
- Upload (`src/store_remote/writer.rs:~600 to 750`, `upload_state_journal`): one thread. Per file:
  read whole file into memory, SHA-256, zstd, age encrypt per 16 MiB part (`PART_LIMIT`), upload in line
  (`put_part`), no pipelining. The delta path needs `is_subvolume` on both trees, so on APFS it is a
  full read and hash of every byte each push. Untested guess: 20 GB at one core is minutes before
  network time. (Test spawn of 2 threads at writer.rs:1589 is a test only.)
- Download (`reader.rs:400 to 460`): good when a parent state exists locally (snapshot the parent, patch
  changed files). A new runner has no parent: full 20 to 30 GB download, decrypt, decompress, write
  per file. `docs/cloud-rfc.md` 6.6 itself says 20 GB at 250 MB/s = 80 s of pure transfer.
- Per line cache states are never removed from the store. `history=last` (cache-overlays.md section 2) is
  local. Remote `gc` (`reader.rs:543`) deletes only orphan parts of incomplete attempts older than 24 h
  and releases. `remote-design.md` decision 8: "a store keeps everything by default". Each agent line
  pushing its `target/` adds a new state (deltas help only against the parent).
- APFS: `save_cache_overlays` and `line new` use one `clonefile` of the folder. The kernel work is per file;
  this is the measured 3 to 4 min save. A warm line start on APFS pays the same order (unverified for
  start, measured for save). Docs say btrfs is O(1) (snapshot).
- No scheduler or locality: RFC 5.1 lists "Scheduler near warm state" and "Cache Registry";
  `cloud-design.md` item 10 and RFC stage 3 put them out of scope. No record says which machine
  holds which cache state, so work cannot be sent to a warm machine.
Conflict: G1 (hydration time), G3 (warm line costs minutes on APFS), G5 (store grows without bound).
How bad: SLOWS (cold runner, APFS) and COSTS DISK (remote and every machine).
Smallest fix:
- Do not move big caches by default: push `target` only to a machine with no ancestor cache state; make
  cache push opt in per overlay (keep small `.layr-retest` and source states travelling).
- Pipeline the uploader: N worker threads for hash+zstd+encrypt, one ordered part writer. This is a local
  change inside `upload_state_journal`; the format is unchanged.
- Add remote retention for cache states: `gc --remote` honours the overlay `history` (keep the head
  and last N per overlay line). Safe because cache states are "speed cache, not proof".
- Machine presence record: a small signed record `machine.warm {overlay, state}` after `save --caches`
  or fetch. The queue or gateway scheduler picks the machine with the nearest ancestor cache state.
- APFS (idea, not tested): make a folder overlay on APFS one sparse disk image file mounted at the path.
  `clonefile` of one file is O(1). `laptop-runner.md` already uses sparse APFS images for test volumes.
  Check whether `hdiutil attach` is allowed in the check sandbox before relying on it.

## P4. Cache access follows uids and per machine slots
What:
- `cache_overlay.rs:~272` `usable_cache`: `uid == 0 || st.cache_shared || st.cache_by == Some(uid)`.
  `cache_by` is a Linux uid. A uid on machine A means nothing on machine B. After a fetch, only states
  saved on a protected line (`cache_shared`) are usable on another machine. A cache made by a runner
  while checking a change version is never shared, so a developer line never gets runner caches.
  Also, `cache cache_shared` only exists after a merge met protection (cache-overlays.md section 10.1),
  so warm main caches appear only at merge time.
- `platform` filter: records and caches from `linux-aarch64` are never imported on `macos-aarch64`
  (reader.rs fetch, "other_platform"). Linux runner results cannot warm a Mac line.
- Readers = trusted machines at the time of save (`cache_overlay.rs:160`, `overlay-sync-design.md`).
  A new machine needs: owner `remote trust`, then a push by a machine that holds a slot, before it can
  open anything. Removing a reader starts a new epoch and new objects: cached 20 GB is encrypted again.
  Ephemeral or autoscaled runners (RFC stage 3C: EC2 pool, SQS, Spot) would each be a trusted machine
  entry and each removal an epoch change.
Conflict: G3 and G4 across machines and with autoscaled runners; goal "records travel between runners
and developer machines".
How bad: BLOCKS for more than the owner's fixed machines.
Smallest fix:
- Replace `cache_by uid` with a signed cache class on the state: `class: private | team`, set by the
  saving machine, and let the project policy say which trusted machines may save `team` caches
  (`runner` role). Usability on a machine = class `team` + platform match, no uid. This is the
  same trust rule the docs already use for `cache_shared`, minus the protected line condition.
- One shared "runner group" layer key for caches. Runners get a slot by a short lived enrolment token
  instead of per machine `trust` + epoch change. Leaving machines rotate the group epoch only for new
  cache states; old cache states are expendable.
- Allow cross platform import for overlays that declare `platform=any` (retest records, source indexes).

## P5. layr-retest cannot live inside `layr checks` as designed
What blocks it, each with place:
1. Needs the `layr` CLI. `retest.md` "Flow": `layr save`, `layr log --auto`, `layr diff -U0`, `layr show`.
   `checks.rs execute` (bwrap, `--unshare-all --clearenv`, `ro-bind /usr`, no socket, no store)
   cannot call layr. macOS `sandbox-exec` denies network, so the daemon socket is also blocked (unverified
   for unix sockets). The work copy `.try-xxxx` is not a registered line, so `layr save` has nothing to save.
2. Baseline is "newest state in `layr log --auto` with a record" and records are
   `.layr-retest/<state id>.json`. Auto states of a developer line are not pushed (only with `--auto`).
   Another machine never has that baseline id. Only named states and main heads travel, so
   cross machine reuse works only from main.
3. Instrumented build goes to a separate `target/retest` (retest.md "Flow", measurements: 19.6 GiB `target`
   plus the instrumented one). Two caches per line; a cold runner builds twice. COSTS DISK (G5).
4. Evidence mix. If the check command is `layr-retest run`, its pass becomes a signed `check`
   attestation (`checks.rs:~330`) that protected lines accept (`merge_met_rule`, `mergecmd.rs:~466`).
   `retest.md` says saved passes are not evidence, the environment is not in the key. Records written by
   a developer machine (readers and writers = trusted machines) can then skip a test on a gate. Poison
   by mistake or by a compromised machine is possible.
5. No write path and cold start: see P1 and P4.
Conflict: G4, G5, and the stated rule "records are a speed cache, not proof".
Smallest fix:
- Split retest into three steps run by the runner, with no layr CLI inside the sandbox:
  (host) `layr-retest plan` reads baseline and hunks from layr and writes `/work/.layr-retest/plan.json`
  (filter, stale tests); (sandbox) `layr-retest exec` runs only the plan, writes coverage profiles;
  (host) `layr-retest record` reduces profiles to the record and saves the folder through P1's `checks.keep`.
- Key records by the Merkle tree id of the tested named state, and record `parent_tree` ids; no auto
  state ids. Baseline = nearest ancestor with a record in `layr log` of named states and main heads.
- Share one `target`: build the normal profile instrumented only for the selected crates, or accept
  coverage from `-C instrument-coverage` in the one `target` (measure first). Otherwise keep the
  retest target as a separate cache with `history=last` and `gc=lru`, never pushed (P3).
- Name it apart: `unit-fast` (retest, can gate a change version) and `unit` (full). Protected main
  requires `unit` on the merge candidate, or a periodic `--full` attestation (retest.md "Contradiction").
  Records get a signed writer; only trusted `runner` machines may write them (P4 class).

## P6. Merge queue is serial, inline and restarts from scratch
What (`src/cmd/queue.rs:226`, `mergecmd.rs:704 to 1010`):
- One flock `queue.lock` for the project. `run_entries` loops entries one by one. Each entry builds a
  candidate `head + change` (new state id), then runs required checks inline in `run_line_checks`
  via `admin::run_check` on the queue host.
- If the head moved during the checks, it retries once (`attempts == 1`) and then fails the entry.
  A slow check plus any other merge into the line means a failed entry.
- It never looks for check results made by other runners for the candidate (the candidate is new, so
  none exists), and it does not wait for them.
- RFC 2D says "start with a sequential queue, parallel by measurement". No measurement hook exists.
- Fast forward case is good: if the change version is already checked by runners, the candidate is the
  same state and `check_exit` skips the run. This is the one real reuse today.
Conflict: G1 (gate and release latency), G4 (no reuse across candidates).
How bad: SLOWS. Throughput is 1 / (sum of check times) per project.
Smallest fix:
- Queue speculation: build candidates for positions 1..k as a chain (`head+c1`, `head+c1+c2`, ...), publish
  the candidate states with `merge.candidate` (already a record) and let runners take the jobs
  through claims (P2). The queue then only waits for attestations and moves the ref. A failed
  position drops itself and recomputes the later ones.
- Fix the retry: on a moved head, rebase the candidate and reuse the retest records of the previous
  candidate as baseline (P5) instead of failing.

## P7. Forge gaps and things in the way (G2)
Present: changes with versions, approvals, encrypted comments, queue, release files, public projects,
people and invites, person signed requests, WASM read model, index objects.
Missing or in the way:
- Roles are Unix uid and gid (`access::Who::of(uid, gid)`, `mergecmd.rs:~450`), plus a "pinned person"
  path. Web users, teams and orgs have no uid. Fix: person and team principals in `access.rs` (new `Who`
  variants, same rule table, no format change).
- Server work needs plaintext (apply requests, merge, index, notifications), so only the executor can do it,
  on a 60 s tick (`executor.yaml`) that checks can hold (P2). Approve and merge answer in minutes.
  With P2 events and claims, several executors can share the work.
- No outbound events: no webhooks, no status API. The P2 event feed is the base for them.
- No search or cross project query (browser only). CLI fetch has no index (P2).
- External contributors: Git mirror only. Smallest: a signed "propose" request, create only under
  `requests/` (the shape of requests v2 already).
- Rate limit per credential 3 000/min (`DEPLOYED.md`): a runner at 15 s uses 1 464/min. P2 incremental fetch fixes it.
- Gateway `t4g.nano` carries bytes. The cdn-client branch fixes reads; cache and release uploads still pass
  through it. Fix: after the write token check, hand out presigned multipart PUTs.

## P8. Disk (G5)
Encryption does not block dedup where it matters:
- Local states are plain subvolumes or APFS clones after fetch, so `layr gc` dedup works on runners and
  developer machines. Remote objects are zstd + age with random keys: no server side dedup, none promised.
- Fetch with no local parent writes files one by one; two lines with different parents hold equal files twice
  until `gc`. Fix: in `restore_over` look up a local file with the same SHA-256 and reflink it. No format change.
- btrfs runner mount uses `compress=zstd` (`executor.yaml`). FIDEDUPERANGE gain on compressed extents is
  unverified; measure first.
What grows:
- `union-theirs` never deletes (cache-overlays.md section 12): protected main cache is the union of all merged
  `target/` folders. Set `gc=lru:<size>` by default when `save --caches` creates the overlay (default is `none`).
- `target/retest` (P5.3), and per line cache states in the store (P3).
- One derived state + ref per check result, forever (`checks.rs:~325`).

## P9. Generic (G6)
- `src/` and `crates/` have no `studio`, `codex` or `agent` (grep). Docs do (below).
- Docs do: `retest.md` (Studio measurements, `STUDIO_RETEST`), `cloud-rfc.md` 1.2, 4.5, 5.5 (tool call saves,
  `LAYR_SESSION`, `refs/agents/{agent_id}`), `studio-compatibility-audit.md`. Doc only. Move Studio parts to
  Studio's repo and keep neutral words ("session", "actor").

## What fits the goals and should stay
- Create only objects (`If-None-Match: *`), per machine record segments, no CAS refs (`remote-design.md` 1, 2).
  Safe on lost responses. Base for claims, events and CDN reads.
- Cache as an overlay with properties (`history`, `merge`, `gc`, `platform`, `folder`). Fix defaults and
  authority, not the model.
- Fetch by local parent snapshot plus patch (`reader.rs:~415`). O(1) folder snapshots on btrfs.
- Check job key (state, argv, privileged) and "results from other trusted machines count".
- Gateway never decrypts; CDN signed URLs for immutable classes; `store-index` objects; WASM model with golden tests.
- Retest idea (line records, no build fast path, `--full`, `CONTRADICTION`), with the P5 split.

## Suggested order
1. P1 `take_caches` in `with_try_copy` + `checks.keep` (unlocks G3, G4 on runners).
2. P2 incremental fetch, per job push, claims; then events.
3. P3 uploader threads, remote cache retention, per overlay push opt in.
4. P4 cache class instead of uid; runner group slot.
5. P5 plan/exec/record split; name `unit-fast`.
6. P6 queue speculation (needs claims).
7. P7 principals in `access.rs`, webhooks from the event feed.
