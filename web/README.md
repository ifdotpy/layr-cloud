# Layr Cloud web

The site uses TypeScript, Preact and Vite. The stylesheet comes from the approved mockup. No CSS framework or external runtime library is used. Geist and Geist Mono load from the site. Their OFL license and source revision are in `public/fonts/`.

## Run

Prerequisites: Rust with the `wasm32-unknown-unknown` target and wasm-pack 0.15.0.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
```

The Rust model and store dependencies use Layr revision `2007480315e6bb48a06a686e8badaa4dc5266cbd`.

```sh
cd web
npm ci
npm run build:wasm
npm run dev
```

The production build regenerates WASM. The dev command checks for generated WASM and does not rebuild it.

Open `http://127.0.0.1:5173/`. The default source opens a person key in a Web Worker. Enter your login, then unlock with a passkey or recovery words. A new account needs an invite token from a gateway owner; save the displayed recovery words before registration. Use `?source=fixture#/projects` to inspect the captured test project without a gateway.

```sh
npm run build
npm test
npx playwright install chromium
npm run test:e2e
```

Playwright writes the page screenshots and axe reports to `web/test-results/`. These outputs are not committed. Screenshots use 1280 pixels in light and dark, and 390 pixels in light.

`npm test` checks the test types before the unit tests. Run `npm run test:types` to check them separately.

## Data boundary

Every page reads through `src/data/source.ts`. CLI twins retain the model JSON field names and full IDs. `FixtureSource` only reshapes JSON from the real store recipe. `ModelSource` uses the verified Rust model in a Web Worker. No secret material enters browser storage. The worker opens the padded public unlock response, signs the gateway challenge through `layr_store::session::sign`, and keeps its person session bearer in memory. It retries a person route once after a 401 with a fresh session. Lock discards the bearer, signing key and pending delivery material.

Unknown facts stay unknown. The site does not invent disk use, live jobs, organization settings, image digests, readers, approval results, or copy completeness.

The old passkey code is preserved at `src/data/legacy-passkey.js` as a reference. The site does not import this file.

## Fixture source

The committed capture comes from Layr revision `1a91cd32555a878e87d63d6311520ecca3be7173`.
It was generated as root in the `layr-dev` Linux VM, from a fresh Git archive in `/root/work/web3-layr-1a91cd3`.
`tests/t37-model.sh` passed 47 checks, including 18 CLI/model comparisons.

The recipe creates a disposable btrfs project with the real CLI and pushes an encrypted store.
`crates/layr-model/examples/store_golden.rs` verifies the store and exports the page JSON.
Use `LAYR_MODEL_FIXTURE_OUT` and `LAYR_MODEL_STORE_OUT` to retain the JSON and encrypted store.
Start the VM run with `setsid nohup bash -c '...' </dev/null >run.log 2>&1 & disown`.
A plain `nohup` process can stop when the OrbStack session closes.

The 37 JSON files contain two lines, 33 states, 13 changes, attestations, queue outcomes, probes, refs and access rules.
They include two recorded bisect runs, push and sync transfers, explicit amend/replay/pick provenance, and two readable overlays.
One overlay is hidden. The parent-only capture contains no hidden overlay name or descriptor.
The capture contains public test keys and test content. It contains no recovery words or private keys.

The screenshot probe compares five screens with PNG, HTML, XML, JSON and text output.
The PNG is a 2 × 2 test image. Other screens test structure-only output and unchanged structure.
`screens.json` adds verified PNG data URLs to the canonical model comparison rows.
The native real-store test generates it through the same adapter that serves `ModelSource.screens()`.
The epoch `number` field comes from `layr_model::overlays::epochs`.
It follows canonical signed record order (time, machine, sequence), retaining repeated key publications.
See `crates/layr-web/README.md` for the capture commands.

The `interdiff*.json` files come from real native version folders.
The store also contains authenticated trees for all three versions, which the availability test checks separately.
Amend retains the earlier states. The recipe uses no version pins.

The recipe has no README, agent sessions, image registry data, live jobs or disk sharing measurements.
Organization and personal gateway facts have no JSON capture.
Transfer records prove the recorded counts and successful operation. They do not prove current copy completeness.

## Limits

The **Yours** filter matches your session login to an exact recorded actor name on the line's machine and owner UID.
The model does not bind a person key to a local UID. If the names do not match, the filter shows no matching lines.

Lines share a group when at least two names have the same prefix before the first hyphen, for example `exp-one` and `exp-two`.
The group label is `exp-… (2 lines)`. A prefix with one line stays ungrouped.
The default line appears first, then other single lines by last activity, then groups.
Groups start closed. A filter or search opens the matching groups.
The **Made together** note requires a verified operation that creates all lines in the group.
The pinned CLI records each line separately, including lines from `line new --count`. These records do not prove a shared command.

Unlock authenticates the newest heads index after verifying the writers' initial record prefixes. Lines, Code and open Changes render provisionally while the remaining records load backwards. Reconciliation uses the exact watermark and the shared model; any mismatch discards the index and shows one muted note. Missing indexes use the full verified load. History pages wait for reconciliation.

The initial record segment must be opened to establish writer trust. A store with a single large initial segment pays that verification cost before its first page. File history reconciles path and ancestry objects against signed states and trees. Blame loads only versions of the selected file, within the shared model's size limits.

Open a public project with `/?project=<gateway-project-name>`. The worker uses the root public manifest and slot, and checks its signer against authenticated writer history before exposing data. Invalid or absent public manifests show “This project is not public” and offer sign-in. Signing in adds personal access and preserves the Public badge. Epoch discovery is a later integration with the shared readers and gateway epoch routes.

Passkey tests check the salt and PRF bytes. A live authenticator has not been tested.

## Person actions and comments

The worker opens `people/<login>/signing.age` from the person response and checks the public key.
The project must pin the same key with `layr access person-key`.
Approve, withdraw, queue changes, ref moves, comments and replies all use `DataSource.submitRequest`.
The shared `layr_store::request::seal` encrypts the signed request to the current project recipient.
Only the v2 ciphertext envelope reaches the gateway and store.
Each submission has one UUIDv7 ID. The UI waits for the verified operation or signed refusal record.
An uncertain delivery can retry the same envelope and ID. It does not create another action.
Comments use the model's folded records; line comments name an exact change state, path and line.

The executor smoke test uses a disposable btrfs project in `layr-dev` and a disposable MinIO bucket.
Build the gateway and fixture tools as described in `crates/layr-web/README.md`, then run:

```sh
cd web
node tests/setup-actions-fixture.mjs
```

Use the printed folder as `ACTION_FIXTURE`:

```sh
LAYR_WEB_ACTION_FIXTURE="$ACTION_FIXTURE" \
LAYR_WEB_ACTION_EXECUTOR="$ACTION_FIXTURE/apply.sh" \
cargo test --manifest-path ../Cargo.toml -p layr-web real_executor_applies_browser_signatures -- --ignored --nocapture

LAYR_WEB_ACTION_FIXTURE="$ACTION_FIXTURE" \
LAYR_WEB_TEST_STORE="$ACTION_FIXTURE/store" \
LAYR_WEB_TEST_PAPER="$ACTION_FIXTURE/paper" \
LAYR_WEB_TEST_LINES="$ACTION_FIXTURE/lines.json" \
node --experimental-strip-types tests/model-gateway.e2e.mjs
```

The browser check writes approval before and after, withdrawal, queue add, thread, inline comment, refusal and queue screenshots,
with axe reports, to `web/test-results/actions-live/` in three display modes.
After both checks, run `$ACTION_FIXTURE/cleanup.sh` to unmount and remove its Linux project.

## Index and public checks

`crates/layr-web/tests/make-index-cli-fixture.sh` creates a disposable real CLI store with two record segments, encrypted heads/paths/graph indexes, public access, and independent CLI history/blame output. Run it as root in `layr-dev` using the detached launch pattern above. Copy the emitted archive and oracle files into a private local test folder.

```sh
cargo run -p layr-web --example prepare_gateway -- /path/fixture/archive /path/fixture/paper /path/fixture/person
LAYR_WEB_INDEX_FIXTURE=/path/fixture LAYR_WEB_INDEX_PERSON=/path/fixture/person cargo test -p layr-web --test index-cli-store -- --ignored
cd web
LAYR_WEB_TEST_STORE=/path/fixture/archive LAYR_WEB_TEST_PAPER=/path/fixture/paper LAYR_WEB_TEST_LINES=/path/fixture/lines.json LAYR_WEB_TEST_INDEX=/path/fixture node --experimental-strip-types tests/model-gateway.e2e.mjs
```

The live check uses MinIO, the real gateway and WASM worker. It withholds a later record segment while opening Lines, Code and Changes, checks corrupted-index fallback, compares history/blame with the CLI, and opens the public project in a fresh anonymous browser session. It also checks missing/invalid public manifests and personal sign-in. Screenshots cover provisional first page, blame, file history and public project at all three display settings, with axe and AA badge contrast checks.

`cargo run --release -p layr-web --example index_first_page -- 1000 100000 1000000` measures the native Session adapter, including manifest/slot opening, writer-prefix verification and index authentication. It excludes network and listing time. Generation and encryption are outside each measured interval.

## Signature sign-in checks

`tests/signin.spec.ts` covers the sign-in and invite forms in all three screenshot views. Recovery words are masked in screenshots. `tests/signin-live.mjs` adds a real gateway, native Session proof, passkey PRF caller, invite registration, recovery sign-in and one session renewal after a 401 to the disposable MinIO run.

For the deployed gateway, run `tests/signin-deployed.mjs` in layr-dev. Keep the live fixture, owner store image and recovery files under `/run` tmpfs. Set `LAYR_WEB_LIVE_FIXTURE` to a JSON file with `login`, `words_file`, `project` and `change`; set `LAYR_WEB_LIVE_DIST` to the reviewed browser build. `LAYR_WEB_LIVE_PROXY` selects the restricted CONNECT proxy through the AWS executor. The page uses `https://cloud.layr.ws`, with its real Host, SNI and certificate verification. The test uses the published site by default. `LAYR_WEB_LIVE_STAGED_CLIENT=1` serves the reviewed static client files inside the VM while API calls still use the deployed gateway. It does not establish public IPv4 reachability.

The deployed check waits for the executor's signed comment record, saves three screenshots and axe reports, checks storage isolation, then locks the key. Only encrypted request bytes and public proofs cross the relay.
