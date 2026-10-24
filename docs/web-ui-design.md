# Layr Cloud web UI: design (lead)

Status: decided 2026-10-24. The mockup (`mockup/`, branch `ui/mockup`, spec
`docs/ui-mockup-spec.md`) is the visual and structural source of truth. This file decides how the
real UI gets its data, how it stays end to end encrypted, and the order of work.

## 1. Principles

1. **End to end stays true.** The gateway, the storage and the static site never see plaintext.
   The browser decrypts. A person opens their person key in the browser (passkey with the PRF
   extension, or the 24 recovery words), and the person key opens the store slots, as in Layr.
2. **One implementation of the formats.** The browser does not get a second reader written in
   JavaScript. It runs Layr's own Rust code compiled to WebAssembly (WASM): `layr-store` for the
   store format and keys, and a new crate `layr-model` for the project model.
3. **The store is the API.** Everything the UI shows comes from signed records and states in the
   store: lines, refs, states, changes, attestations, the queue, probe results, the operation log.
   The executor writes its results there (as it does today). The gateway stays an object and
   people server; it gets no project API.
4. **Actions are signed by the person.** In phase 2 the browser writes signed requests; a
   trusted Layr machine applies them under Layr's access rules. Nobody acts for a person without
   that person's signature.

## 2. Parts

| Part | Repository | What |
|---|---|---|
| `layr-store` (existing) | layr | Gets a `wasm` feature: no network, no file system, zstd decode with `ruzstd`, `age` as today. Native features (S3, AWS, HTTP client, AWS profiles) stay behind the default `native` feature. |
| `layr-model` (new crate) | layr | Pure Rust, no I/O. Folds a verified record log into a read model: projects, lines and heads, states and parents, refs and ref history, changes and versions, attestations, queue entries and outcomes, probe results, the operation log, access policy. Same rules as `layr`'s `Db`. |
| `layr-web` (new crate) | layr-cloud | `wasm-bindgen` wrapper: unlock (person key, slots), fetch through the gateway, verify signatures, build the model, read files of a state, diff two states, read probe outputs. Runs in a Web Worker so the page stays responsive. |
| `web/` (rewritten) | layr-cloud | The site: TypeScript, Preact, Vite. Styles from the mockup's `styles.css` tokens. Pages and names exactly as the mockup. |
| Gateway | layr-cloud | Serves the built site; existing object, people and inbox routes. Phase 2: the requests route (section 6). |

The old Desk site code in `web/` is replaced, not ported. Its passkey flow (`client.js`) is the
reference for unlock and is moved into the new site.

## 3. Consistency with the CLI

`layr-model` must give the same answers as `layr`. Golden tests: a Linux test builds a project
with the real `layr` binary (lines, saves, merges, changes with versions, approvals, queue, probes,
refs, overlays), pushes it to a `file://` store, and compares `layr-model` output with the CLI's
`--json` output (`line list`, `log`, `change show`, `ref list`, `ref log`, `queue list`,
`attestations`, `op log`, `probe diff`). A difference fails the test.

## 4. What the browser can do in phase 1 (read only)

All pages of the mockup that read data: Code (tree, file view, README), Lines, States (with auto
states), State, Bisect results (read), Changes and the five change tabs, Queue, Checks, Behavior
(including UI diffs, section 7), Environments, Registry (tags and digests from records; pulling
stays with Docker), Activity, project Settings (read), org Projects, People, Stores, Machines,
Settings (read).

Overlays: shown when the person's key opens the overlay's slot; otherwise the page says which
overlay is hidden, as `layr` does.

Large data: file contents and diffs load on demand per path; the model keeps record metadata only.
A store with more than 100 000 records shows lines and refs first and loads history in pages.

## 5. Unlock and sessions

- Unlock with a passkey (PRF) or the recovery words. The opened person key lives only in the Web
  Worker's memory. Closing the tab or 30 minutes without use locks it.
- Nothing secret goes to `localStorage`, `IndexedDB` or cookies. Cached ciphertext may go to the
  browser cache (it is ciphertext).
- Content Security Policy: no inline scripts, no remote code, `connect-src` only the gateway.

## 6. Phase 2: actions

- Each person gets a signing key (Ed25519), made with `layr person keys`, sealed to the person
  recipient and kept in `people/<login>/signing.age`; its public key is in `person.json` and an
  owner pins it into the project like a machine key (`layr access person-key <login> <key>`).
- The browser writes a signed request to `requests/<request id>.json` (create only): approve,
  withdraw approval, add to queue, remove from queue, move a ref, comment, declassify.
- Any trusted Layr machine that syncs the store (normally the executor) verifies the person's
  signature and Layr's access rules, applies the request as a normal operation, and records the
  person as the actor. An approval becomes an attestation signed by the person's key.
- Refused requests get a signed refusal record with the reason; the UI shows it.

## 7. UI diffs (screenshots probe)

- New probe kind `screenshots` in Layr: the probe command writes PNG files under its output
  folder (one per route and viewport, named `<route>@<width>.png`). `layr probe diff` compares
  them pixel by pixel (the `image` crate), with a small tolerance for antialiasing, and stores per
  screen: changed pixel share, and changed regions as boxes. A probe may set `fail_over` on the
  changed share, like other probes.
- The UI shows Side by side, Swipe, Onion skin and Difference from the stored PNGs and boxes.
- Structure too: next to each PNG the probe may write `<route>@<width>.html`, a DOM snapshot with
  inlined CSS. Layr compares it as text after normalization (pretty-print, sorted attributes,
  generated ids and nonces removed) and reports text changes and added or removed elements.
  Pixels show what a person sees; structure explains why it changed and is exact. The UI shows
  both per screen: Visual (the four views) and Structure (an HTML diff with a one-line summary).
  A DOM snapshot can be rendered again later at another width without running the probe again.
- Native apps work the same way. Screens are named `<screen>@<device>` (`Checkout@iPhone-15`).
  The structure file is the app's view or accessibility tree: `.xml` (Android uiautomator),
  `.json` (an iOS accessibility dump from XCUITest), or `.txt` (any text dump). Layr normalizes
  each format and reports the same fields. iOS probes need a macOS runner with Xcode: an
  executor on macOS (an AWS EC2 Mac host or the team's own Mac registered as a machine).
- A view or accessibility tree cannot be rendered again (it has no styles, colors or images), so
  a native probe must capture a PNG per device when it runs. Node frames are not part of the text
  diff, but Layr keeps them and reports the box of each changed node, so the UI outlines on the
  screenshot exactly the element whose text, role or presence changed, and can draw a wireframe
  from the frames. Only web screens can be rendered again later, from their HTML.

## 8. Order of work

1. `layr`: `layr-store` `wasm` feature, `layr-model` crate, golden tests.
2. `layr`: screenshots probe kind (section 7).
3. `layr-cloud`: `layr-web` (WASM, Worker) and the phase 1 site, page by page in the mockup's
   order, each page with a Playwright test against a store made by the real `layr`.
4. Phase 2 actions (`layr` and `layr-cloud` together).

Steps 1 and 2 run in parallel. Step 3 starts when step 1 has the model API, using its golden
stores as fixtures.
