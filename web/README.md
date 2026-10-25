# Layr Cloud web

The read-only site uses TypeScript, Preact and Vite. The stylesheet comes from the approved mockup. No CSS framework or external runtime library is used. Geist and Geist Mono load from the site. Their OFL license and source revision are in `public/fonts/`.

## Run

Prerequisites: Rust with the `wasm32-unknown-unknown` target and wasm-pack 0.15.0.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
```

The Rust model and store dependencies use Layr revision `1a91cd32555a878e87d63d6311520ecca3be7173`.

```sh
cd web
npm ci
npm run build:wasm
npm run dev
```

The production build regenerates WASM. The dev command checks for generated WASM and does not rebuild it.

Open `http://127.0.0.1:5173/`. The default source opens a person key in a Web Worker. Use `?source=fixture#/projects` to inspect the captured test project without a gateway.

```sh
npm run build
npm test
npx playwright install chromium
npm run test:e2e
```

Playwright writes the page screenshots and axe reports to `web/test-results/`. These outputs are not committed. Screenshots use 1280 pixels in light and dark, and 390 pixels in light.

`npm test` checks the test types before the unit tests. Run `npm run test:types` to check them separately.

## Data boundary

Every page reads through `src/data/source.ts`. CLI twins retain the model JSON field names and full IDs. `FixtureSource` only reshapes JSON from the real store recipe. `ModelSource` uses the verified Rust model in a Web Worker. No secret material enters browser storage.

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
The **Made together** note requires a verified operation that creates all lines in the group.
The pinned CLI records each line separately, including lines from `line new --count`. These records do not prove a shared command.

Unlock verifies all record metadata before the first page. The lines and refs first load for stores above 100,000 records (design section 4) is not built and not benchmarked. The lead accepted this limit for phase 1. A follow-up task will add this load path and a store index.

Passkey tests check the salt and PRF bytes. A live authenticator has not been tested.
