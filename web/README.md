# Layr Cloud web

The read-only site uses TypeScript, Preact and Vite. The stylesheet comes from the approved mockup. No CSS framework or external runtime library is used. Geist and Geist Mono load from the site. Their OFL license and source revision are in `public/fonts/`.

## Run

Prerequisites: Rust with the `wasm32-unknown-unknown` target and wasm-pack 0.15.0.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
```

The Rust model and store dependencies use Layr revision `153a239c9963fd1b0aa140cef51d11037a557ab8`.

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

The committed capture comes from layr revision `7b0a165`, generated in the `layr-dev` Linux virtual machine. `tests/t37-model.sh` passes 32 checks. The earlier runs at `3f407a7` and `882f0a2` passed 25 and 26 checks. Each run used a new folder under `/root/work/`.

The recipe creates a disposable btrfs project with the real CLI. It pushes an encrypted store. `store_golden` verifies that store and exports the page JSON. The test compares 13 CLI outputs with the model outputs.

The 33 JSON files contain two lines, named and automatic states, change versions, exact-state attestations, queue outcomes, numeric probes, ref history, access rules, and one readable overlay with history. The capture contains public test keys and test contents. It contains no recovery words or private keys.

The `interdiff*.json` files are native comparison fixtures from the real local version folders. They do not prove that the store has older version trees. The separate availability test checks `trees.json`. All three versions have authenticated trees in this capture. The recipe's only push follows v3 (`t37-model.sh:201`). Push sends every present state (`writer.rs:190`, `db.rs:595`). Amend keeps v1 and v2. No version pins are used.

The recipe has no README, agent sessions, stored bisect result, screenshot probes, image data, live jobs, or disk sharing measurements. Organization and personal gateway facts have no JSON capture. Copy configuration proves an address. It does not prove a completed push or copy completeness.

## Limits

Unlock verifies all record metadata before the first page. The lines and refs first load for stores above 100,000 records (design section 4) is not built and not benchmarked. The lead accepted this limit for phase 1. A follow-up task will add this load path and a store index.

Passkey tests check the salt and PRF bytes. A live authenticator has not been tested. UI diffs need the screenshot model API, which is not available to `ModelSource` yet.
