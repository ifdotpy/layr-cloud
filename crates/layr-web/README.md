# Browser store reader

`layr-web` uses `layr-store` for age keys, slots, signatures, trees, and files.
It uses `layr-model` for the project read model.
The worker owns the person identity and store identities.
Lock terminates the worker and releases its WebAssembly memory.
The gateway cache contains age ciphertext only.

Install the `wasm32-unknown-unknown` Rust target and `wasm-pack`.
Build the browser package from the repository root:

```sh
wasm-pack build crates/layr-web --target web --out-dir ../../web/wasm --release --no-opt
```

The web build can run the same command through `node scripts/build-model-wasm.mjs`.
Do not commit the generated `web/wasm` package.
`Cargo.toml` and `Cargo.lock` pin the model and store to the exact Layr revision used by this build.

Run the native checks:

```sh
cargo test -p layr-web --lib
```

The real store test needs a store from `layr/tests/t37-model.sh`.
Set `LAYR_MODEL_STORE_OUT` when that script runs.
Use its exported `store` and `paper` paths:

```sh
cargo run -p layr-web --example prepare_gateway -- /path/store /path/paper /path/person
cp /path/cli/lines.json /path/person/lines.json
LAYR_WEB_TEST_STORE=/path/store LAYR_WEB_TEST_PERSON=/path/person cargo test -p layr-web cli_store_unlocks -- --ignored
```

Set `LAYR_WEB_TEST_INTERDIFF` to the golden `interdiff.json` to compare the canonical file output too.

The example creates disposable person objects and a recovery file for tests.
Keep its output in a private test folder.
Do not use production keys for these tests.

The MinIO browser check runs on macOS with the `layr-dev` VM.
It creates one VM folder and one MinIO process for each run.
It starts the real gateway and a test whois socket on macOS.
The test sends no recovery words or person identity to either server.

Build the gateway and the fixture tools:

```sh
cargo build -p layr-gateway --bin layr-gateway
cargo build -p layr-web --example prepare_gateway
cargo build -p layr-web --features test-tools --example upload_gateway
```

Make the browser test store with `tests/make-cli-fixture.sh` in this crate.
Run the script as root in `layr-dev`.
Set `LAYR_BIN` to a native CLI binary built from the tested Layr revision.
The script prints its new folder under `/root/work`.
Copy `archive`, `paper`, and `lines.json` from that folder to a private folder on macOS.
The browser check uses this fixture's exact README and file bytes.

Run the browser check from `web`:

```sh
LAYR_WEB_TEST_STORE=/path/store LAYR_WEB_TEST_PAPER=/path/paper LAYR_WEB_TEST_LINES=/path/lines.json node --experimental-strip-types tests/model-gateway.e2e.mjs
```

The test requires the MinIO binary at `/root/work/remote-store-tools/minio` in `layr-dev`.
All test keys are disposable.
The test stops only its own processes and deletes only its own folders.

Screenshot comparisons use `layr_model::probes::diff` and its shared screenshot implementation.
The adapter adds PNG data URLs from verified output bytes and retains the canonical structure diff and node regions.
The worker loads outputs through the same authenticated tree and part path as other probes.

To capture `screens.json`, use the t37 store and its `store_golden` JSON directory.
Prepare the disposable person, then run from the repository root with absolute paths:

```sh
cargo run -p layr-web --example prepare_gateway -- /path/store /path/paper /path/person
cp /path/fixtures/lines.json /path/person/lines.json
LAYR_WEB_TEST_STORE=/path/store \
LAYR_WEB_TEST_PERSON=/path/person \
LAYR_WEB_TEST_FIXTURES=/path/fixtures \
LAYR_WEB_TEST_INTERDIFF=/path/fixtures/interdiff.json \
LAYR_WEB_TEST_SCREENS_OUT=/path/fixtures/screens.json \
cargo test -p layr-web cli_store_unlocks -- --ignored
```

The test compares probe JSON, copy records and file comparisons with the golden capture.
It also checks five screen rows, verified PNGs, structure changes and stored bisect runs.

To run the new model paths through MinIO, use the t37 store and add `LAYR_WEB_TEST_GOLDEN`:

```sh
cd web
LAYR_WEB_TEST_STORE=/path/store \
LAYR_WEB_TEST_PAPER=/path/paper \
LAYR_WEB_TEST_LINES=/path/fixtures/lines.json \
LAYR_WEB_TEST_GOLDEN=/path/fixtures \
node --experimental-strip-types tests/model-gateway.e2e.mjs
```

This run compares screenshots, probes, transfers, bisects and version provenance through the real worker.
It also opens the structure diff page and checks that a parent-only reader cannot discover a hidden overlay.

The hidden reader browser case uses two people created by the real CLI.
Run `tests/make-hidden-cli-fixture.sh` as root in `layr-dev`, with `LAYR_BIN` set to the pinned CLI binary.
Copy its `archive`, `reader-view.json`, `reader-paper`, and `lines.json` into a private local test folder.
Then run:

```sh
cd web
LAYR_WEB_TEST_HIDDEN_FIXTURE=/path/hidden-fixture \
LAYR_WEB_TEST_STORE=/path/hidden-fixture/archive \
LAYR_WEB_TEST_PAPER=/path/hidden-fixture/reader-paper \
LAYR_WEB_TEST_LINES=/path/hidden-fixture/lines.json \
node --experimental-strip-types tests/model-gateway.e2e.mjs
```

The reader opens the real encrypted hidden manifest through its person slot.
The test reads the hidden file and checks its metadata, signed create/save history, and first key epoch.
It opens the list and detail pages and checks one neutral hidden badge on each.
It writes six screenshots and runs axe for all three display modes.
`LAYR_WEB_TEST_SCREENSHOT_DIR` selects the screenshot folder.

Epoch ordinals come directly from `layr_model::overlays::epochs`.
The function follows canonical signed record order (time, machine, sequence), including repeated key publications.
No supplemental export patch is needed.

The native overlay detail oracles use `layr overlay show <name> --json` and
`layr overlay log <name> --ops --json`.
`t37-model.sh` exports both contracts for visible and hidden readers with `LAYR_MODEL_FIXTURE_OUT`.
Run `tests/make-overlay-details-oracle.sh` in Linux as root, with that directory, `LAYR_ORACLE_OUT`,
`LAYR_BIN`, `LAYR_ORACLE_ARCHIVE` (the t37 store) and `LAYR_ORACLE_PAPER`.
It also captures independent file paths with `overlay ls-files` from a disposable clone.
Use the resulting parent directory for `LAYR_WEB_VISIBLE_ORACLE`.
The native test compares both t37 overlays, including state, readers, managers, patterns, epoch numbers,
and the operation ID, display name, actor and time. Record signatures remain verified by Session.
The separate hidden person fixture captures the same CLI commands before its archive is frozen;
use that fixture directory for `LAYR_WEB_HIDDEN_ORACLE`.

Run all native reader checks, including their optional fixtures, with absolute paths:

```sh
LAYR_WEB_TEST_STORE=/path/golden/store \
LAYR_WEB_TEST_PERSON=/path/golden/person \
LAYR_WEB_TEST_FIXTURES=/path/golden/fixtures \
LAYR_WEB_TEST_INTERDIFF=/path/golden/fixtures/interdiff.json \
LAYR_WEB_VISIBLE_STORE=/path/golden/store \
LAYR_WEB_VISIBLE_PAPER=/path/golden/paper \
LAYR_WEB_VISIBLE_ORACLE=/path/oracles/visible \
LAYR_WEB_HIDDEN_FIXTURE=/path/hidden-fixture \
LAYR_WEB_HIDDEN_ORACLE=/path/oracles/hidden \
cargo test -p layr-web --lib --test hidden-cli-store -- --include-ignored
```

## Rotated store verification

Run `tests/make-epoch-cli-fixture.sh` in this crate as root in the disposable VM.
Set `LAYR_BIN`, `LAYR_PREPARE_PERSON`, a unique `LAYR_FIXTURE_LOGIN`, and a unique
`LAYR_FIXTURE_ALIAS`. Set `LAYR_FIXTURE_ROTATE=1` for a local rotated fixture.
The script keeps all fixture keys, recovery words, and store files under `/run`.
It prints the owned fixture path after successful creation.

Set `LAYR_WEB_EPOCH_FIXTURE` to that path and run:

```sh
cargo test -p layr-web --test epoch-cli-store -- --ignored
```

The native caller authenticates discovery, sealed root history, current records,
and file parts from both epochs. It compares file bytes with native CLI output.
It also checks the access mode and epoch list. The deployed browser caller uses
`browser.json` from the same fixture to check Settings > Access and both files
before its single comment. Keep the fixture until all checks finish. Then unmount
its store, detach its loop device, and remove only that fixture folder.
