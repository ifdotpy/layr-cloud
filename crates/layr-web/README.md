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
`Cargo.toml` pins the model and store to Layr revision `153a239c9963fd1b0aa140cef51d11037a557ab8`.

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

Screenshot probe comparison remains unavailable until `layr-model` supplies the shared implementation.
`ModelSource.screens()` returns an error with that reason.
Other probe kinds remain available.
