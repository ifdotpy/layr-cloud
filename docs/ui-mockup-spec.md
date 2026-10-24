# Layr Cloud UI mockup: specification (lead)

Status: decided 2026-10-23. The lead owns every design decision in this file. The worker builds
exactly this. A decision that this file does not make goes to the lead as a question before it is
built. Do not invent features, names, colors, or layouts.

## 1. Purpose and output

- A static, clickable mockup of the full Layr Cloud web UI. It shows every feature of Layr and
  Layr Cloud in its final form, with fake data. No backend, no real data.
- Folder: `mockup/` at the repository root. One HTML file per page (list in section 6).
  Shared files: `mockup/styles.css`, `mockup/app.js` (optional, under 200 lines), `mockup/README.md`.
- Plain HTML, CSS and vanilla JS. No frameworks, no build step, no npm, no external JS, no icon
  fonts, no images except inline SVG for the few icons in section 4.7.
- Fonts: Geist and Geist Mono from Google Fonts
  (`https://fonts.googleapis.com/css2?family=Geist:wght@400..600&family=Geist+Mono:wght@400..600&display=swap`),
  fallback `system-ui, sans-serif` and `ui-monospace, monospace`.
- Every page opens from `file://` with relative links. Every link in the navigation goes to a
  real mockup page. A link to a page that is out of scope goes to `#` and has `aria-disabled`.
- Do not touch `web/`, `crates/`, `js/`, `python/` or any other folder.

## 2. Product vocabulary (strict)

Layr does not use Git words. Use only these names in the UI copy:

| Concept | UI word | Never write |
|---|---|---|
| a writable working folder | **line** | branch, worktree |
| a frozen snapshot | **state** | commit, revision, snapshot (as a noun in copy) |
| make a state | **Save** (`layr save -m`) | commit |
| the current state of a line | **head** | HEAD, tip |
| a reviewable unit with versions | **change** | pull request, PR, merge request |
| a version of a change | **version** (v1, v2, v3) | patchset, iteration |
| a signed note about one state | **attestation** (kinds: check, approval, origin, import, quarantine, revoke) | status check, CI status |
| a name for a state | **ref** (`env/prod`, `release/1.4.2`) | tag (except OCI image tags) |
| private files with own readers | **overlay** | secret repo, vault |
| a stack of layers | **view** | |
| another place with the same states | **remote** (kinds: peer, store) | origin (as a word for remote) |
| an encrypted copy target | **store** | backup, bucket (except the S3 URL itself) |
| the merge queue | **queue** | merge train |
| a behavior comparison | **behavior diff**, from **probes** | |
| a signed operation record | **operation** (`layr op log`) | event, reflog |
| shelve work | **park** | stash |
| other line operations | **pick**, **replay**, **invert** | cherry-pick, rebase, revert |

The word "backup" never appears. The words "PR", "pull request", "commit", "branch" never
appear. Exception: the "Git mirror" settings panel may say "Git commits" when it describes the
Git side.

## 3. Fake dataset (use the same data on every page)

- Organization `acme`. Projects: `shop` (main example), `payments`, `infra`, `docs-site`.
- Project `shop`: Rust and TypeScript web shop. Default line `main` (protected).
- People: `alice` (Alice Chen, owner), `bob` (Bob Novak, maintainer), `carol` (Carol Diaz, reader),
  `dan` (Dan Ito, invited, not yet accepted).
- Machines: `laptop-alice` (macOS, APFS), `laptop-bob` (Linux, btrfs), `executor-eu1`
  (Layr Cloud executor, Linux btrfs), `ci-runner-2` (Linux, OverlayFS).
- Agents: `agent-1` … `agent-16` lines, made by `alice` with `layr line new agent main --count 16`.
  Their sessions run on `executor-eu1`.
- Lines: `main`, `topic-checkout` (bob), `feature-coupons` (alice), `fix-tax-rounding` (carol has
  no write right, so this one is bob), `agent-1` … `agent-16`.
- States: short ids are 4 to 12 hex characters, mono font, for example `7d3e9a1c40b2`. Use about 30
  different ids. A state has: id, message (or "auto" for automatic states), author (person or
  agent), machine, time, parents, attestations.
- Changes: `#812` "Checkout: support saved cards" (open, v3, bob), `#815` "Fix tax rounding for
  EUR" (in queue, v2, bob), `#820` "Coupons: percent and fixed" (open, v1, alice, made by
  `agent-7`), `#806` "Bump tokio to 1.48" (merged), `#799` "Remove legacy cart API" (closed).
- Refs: `env/prod` → `release/1.4.2` state, `env/staging` → newer state, `release/1.4.2`,
  `release/1.4.1`, `pins/audit-2026-10`, `holds/legal-hold-17`, `leases/agent-3` (TTL 2 h).
- Overlays: `secrets` (readers: alice, bob, executor-eu1; files `.env`, `.env.prod`,
  `config/stripe.key`), `local-tools` (reader: alice only).
- Views: `views/9f2c41` = `main` + overlay `secrets`.
- Remotes: `cloud` (store, `layr+https://cloud.acme.dev/shop`), `vault` (store,
  `s3://acme-layr/shop`), `laptop-bob` (peer, `ssh://laptop-bob/shop`), `github` (Git mirror,
  `git@github.com:acme/shop.git`).
- Runnables in `layr.toml`: `app` (image), `worker` (image), `chart` (Helm chart).
- Probes: `api-catalog` (HTTP answers), `bench-checkout` (metric), `bundle-size` (file size),
  `sql-plan` (file).
- Checks: `unit`, `lint`, `e2e`, `build-image`.

## 4. Visual system (Vercel Geist)

### 4.1 Tokens

Put these in `:root` in `styles.css`, with `color-scheme: light dark` and `light-dark()` values.
Light and dark come only from `prefers-color-scheme`. No theme toggle.

- Background: `--bg-100: light-dark(oklch(1 0 0), oklch(0 0 0))` (page);
  `--bg-200: light-dark(oklch(0.984 0 0), oklch(0.027 0 0))` (side panels, table header, code).
- Grays: use the official Geist gray scale 100 to 1000 and the gray-alpha scale from
  `vercel-brand.css`. Text primary = gray-1000, text secondary = gray-900. Borders: subtle =
  gray-alpha-300, default = gray-alpha-400, strong = gray-alpha-700.
- Meaning colors only: blue-700 (`oklch(57.61% 0.2508 258.23)`) for links on hover, focus ring and
  the one primary button per page; green-900 for passed, merged, success; amber-900 for pending,
  warning; red-700 for failed, refused, conflict, deleted lines in diffs. Diff added uses
  green-900 text on a 10% green background; diff removed uses red-700 text on a 10% red background.
- Color never stands alone: each colored status also has a word or a symbol (✓ passed,
  ✕ failed, ● pending, ⟳ running).
- Font sizes: 12 (meta, badges), 13 (tables, code), 14 (body, default), 16 (section heading),
  20 (page heading), 24 (only the project name on the Code page and the change title).
  Line height 1.5 for body, 1.3 for headings.
- Weights: 400 body, 450 headings, 500 labels and buttons, 600 only for the active tab and
  numbers in summary rows.
- Radius: 6px for buttons, inputs, badges; 8px for cards, tables, dialogs. Nothing larger.
- Spacing scale only: 4, 8, 12, 16, 20, 24, 32, 40, 48, 64 px.
- Geist Mono only for ids, refs, paths, commands, file contents, digests, and numbers in tables.

### 4.2 Rules

- Monochrome first. Hierarchy comes from type size, weight and spacing, not from color.
- No gradients, glows, blur, ornamental shadows, decorative pills, emoji, or icons inside cards.
  A dialog and a menu may have one shadow: `0 4px 12px gray-alpha-200`.
- Borders are 1px. Tables have row borders only, no vertical lines. Numbers right-aligned.
- Buttons: height 32px (small 28px). Primary = gray-1000 background with bg-100 text (the
  Vercel black button). Secondary = bg-100 with a default border. Danger = red-700 text with a
  default border. Only one primary button per view.
- Max content width 1280px, centered. Page side padding 24px (16px under 768px).
- Focus ring: 2px blue-700 outline with 2px offset on every interactive element.
- WCAG AA contrast in both schemes. All controls reachable by keyboard.

### 4.3 Status badge

Height 20px, radius 6px, 12px text, weight 500, 1px border in the status color at 40% alpha,
text in the status color, symbol first: `✓ passed`, `✕ failed`, `● pending`, `⟳ running`,
`○ skipped`, `merged`, `open`, `in queue`, `closed`, `protected`. "open" and "closed" are gray.

### 4.4 Tables

Header row bg-200, 12px uppercase-free labels in text-secondary, weight 500. Row height 40px.
Row hover bg-200. The first column is the link. Ids in mono.

### 4.5 Code and diffs

Mono 13px, line height 20px, line numbers in gray-700, bg-200 background, 8px radius frame with
a header row (file path in mono, right side: "+12 −3" in green and red, and buttons). Unified view
by default; a "Split" toggle switches to two columns.

### 4.6 Commands in the UI

Every action that has a CLI equivalent shows it: a small mono line under the action button, in
text-secondary, with a copy button. Example under "Merge": `layr merge topic-checkout --expect 7d3e9a1c`.
This teaches the CLI. It is a core part of the design; do not drop it.

### 4.7 Icons

Inline SVG, 16px, stroke 1.5px, currentColor, only in: top bar (search, avatar), file tree
(folder, file), line switcher (chevron), copy buttons, overflow menus (three dots). Nothing else.

## 5. Layout and navigation (GitHub-like)

### 5.1 Top bar (every page)

Height 56px, bottom border subtle, bg-100. Left to right:
1. Wordmark "Layr" (weight 600, 16px) linking to `index.html` (org overview).
2. Breadcrumb: `acme` / `shop` (each a link; `/` in gray-700). On org pages only `acme`.
3. Search field, 280px, placeholder "Search or jump to…", hint key `/` on the right.
4. Right side: "Machines" link with a small count of online machines ("3 online"),
   a "+" menu (New project, New line, New change, Invite person), and the avatar of alice
   with a menu (Your person key, Your machines, Sign out).

### 5.2 Project header and tabs (every project page)

Under the top bar: project name `acme / shop` (20px), a "protected main" badge, and on the right:
"Clone" (secondary, opens the clone dialog, section 7.1) and "Watch" is not used (do not add it).

Tabs, GitHub style (text tabs, active tab has a 2px gray-1000 underline and weight 600, counts
as gray numbers in a 20px rounded-6 box):

`Code` · `Changes 3` · `Queue 1` · `Checks` · `Behavior` · `Environments` · `Registry` ·
`Activity` · `Settings`

Under 768px the tabs scroll horizontally. No hamburger.

### 5.3 Org level

Org pages have the top bar and this tab row instead: `Projects` · `People` · `Stores` ·
`Machines` · `Settings`.

### 5.4 Settings layout

Settings pages (project and org) use a left side nav, 220px, GitHub style, grouped with small
group labels, and the content on the right.

## 6. Pages

File names in parentheses. Each page lists its sections in order. "Cmd:" is the CLI line from 4.6.

### 6.1 Org: Projects (`index.html`)

- Heading "acme". Right: primary "New project".
- Filter field and a sort select (Last activity, Name).
- Project list (rows, not cards): name (link), one-line description, default line, lines count,
  open changes, `env/prod` state id and age, store status ("Encrypted copy: 4 min ago").
- Under the list, a secondary action "Import from Layr Desk" with a dialog: S3 URL of the Desk
  vault, "Signatures are checked. Unsigned old states import as unsigned." Cmd:
  `layr clone s3://desk-bucket/project`.

### 6.2 Project: Code (`code.html`)

Two columns: main (fluid) and a right sidebar (296px).

Main:
1. Toolbar row: line switcher button with the text "line: main ▾" (no icon), then links
   "20 lines" (`lines.html`), "14 refs" (`environments.html`), "312 states" (`states.html`);
   right: "Go to file". No "Save" button: saves come from the CLI.
2. Line switcher (dropdown on click): tabs "Lines" and "Refs" and "Views"; filter field; list.
   Views tab shows `views/9f2c41  main + secrets`.
3. Head row (bg-200, 8px radius): author avatar letter, author, message, attestation summary
   (`✓ 4/4`), state id (mono), age, link "312 states".
4. File tree table: name, last state message, age. Rows: `.layr/`, `src/`, `web/`, `migrations/`,
   `layr.toml`, `.layrattributes`, `Cargo.toml`, `package-lock.json` (with a gray "derived" mark),
   `README.md`. Overlay files `.env` and `config/stripe.key` show with a lock-free mark: the word
   "overlay: secrets" in a gray badge, visible because alice is a reader. Add a note row:
   "2 overlay files hidden from readers without access".
5. README preview of the project (short fake README with headings and a code block).

Sidebar:
- About: description, link, "MIT".
- Overlays: `secrets` (3 files, 3 readers), `local-tools` (only you).
- Environments: `env/prod` → `release/1.4.2` ✓, `env/staging` → `a91c…` ✓.
- Remotes: `cloud` store (last push 4 min), `vault` store, `laptop-bob` peer, `github` Git mirror.
- Runnables: `app`, `worker`, `chart` with link to Registry.

### 6.3 Lines (`lines.html`)

- Heading "Lines". Right: primary "New line" (dialog: name, from (select), count (number,
  "Make several lines at once, for agents"), Cmd: `layr line new agent main --count 16`).
- Filters: segmented "All · Yours · Agents · Stale". Filter field.
- Table: line (link), owner, head state (mono) and message, ahead/behind main ("3 ↑ 1 ↓"),
  cache size ("128 MiB, shared"), conflict radar ("overlaps 2 lines" as an amber link, opens a
  popover listing `agent-4: src/cart.rs`, `topic-checkout: src/cart.rs`), last activity, overflow
  menu (Rename, Park, Replay onto main, Delete). Show `main` first with "protected".
- Show all 16 agent lines grouped under a collapsible row "agent-1 … agent-16 (16 lines, made
  together)".
- Footer note: "Disk use for all 20 lines: 214 MiB. As separate copies: 3.1 GiB."

### 6.4 States (`states.html`), the history list

- Line switcher at top, plus a toggle "Show automatic states" (off by default; when on, auto
  states show indented in gray under the saved state, e.g. "auto · agent-7 · after tool call
  `apply_patch`").
- Grouped by day ("Saved on Oct 22, 2026"), GitHub commit list style. Each row: message, author
  and machine, attestation badges (`✓ unit`, `✓ lint`, `● e2e`, `✓ approved by alice`),
  refs at this state as gray mono badges (`env/prod`, `release/1.4.2`), state id (mono, copy
  button), "Browse files" link.
- Top right: "Find the state that broke it" (secondary) → `bisect.html`.

### 6.5 State detail (`state.html`)

- Heading: message. Meta: author, machine `laptop-bob`, time, parents (two ids for a merge),
  "Signed by laptop-bob (Ed25519)" with the key fingerprint in mono.
- Attestations panel: table of kind, name, result, by (person or machine), exact argv for checks
  (`cargo test --workspace`), time. A quarantine row example in amber.
- Files changed: diff list as in 4.5. A derived file (`package-lock.json`) is collapsed with
  "Derived by `npm`. Rebuilt, not merged."
- Actions (secondary): "Restore folder to this state" (Cmd: `layr restore --source 7d3e9a1c .`),
  "Pick onto line…", "Invert", "New line from here".

### 6.6 Bisect (`bisect.html`)

- Heading "Find the state that broke it". Form: good state, bad state, command
  (`cargo test -p checkout`), checkbox "Search automatic states too" (on). Cmd:
  `layr bisect run --auto -- cargo test -p checkout`.
- Result card: "First bad: auto state `c41e…` by agent-7 after tool call `apply_patch src/tax.rs`",
  steps table (state, result, reused from attestation yes/no), "9 runs, 4 reused".

### 6.7 Changes list (`changes.html`)

GitHub PR list pattern with Layr words.
- Heading "Changes". Right: primary "New change".
- Segmented: "3 open · 41 merged · 6 closed". Filters: Author, Line, Attestation, Queue.
- Rows: status mark, title, `#812`, "v3", author (person or agent label "agent-7 for alice"),
  line `topic-checkout → main`, attestation summary, approvals ("1 of 2"), behavior summary
  ("2 behavior changes"), comment count, updated.

### 6.8 Change detail (`change.html`, `change-files.html`, `change-behavior.html`,
`change-checks.html`, `change-steps.html`)

Shared header: title (24px) and `#812`, status badge "open", "bob wants to merge
`topic-checkout` into `main`", version selector "v3 ▾" (v1, v2, v3 with times). Tabs:
`Overview` · `Files 4` · `Behavior 2` · `Checks 4` · `Steps 38`.

Right sidebar on all five tabs:
- Reviewers: one row per path owner rule ("src/payments/ → alice ✓ approved v3",
  "web/ → bob ● waiting"). Note: "Approvals bind to an exact state. A new version needs new
  approvals for changed paths only."
- Required for `main`: list from protection (`✓ unit`, `✓ lint`, `● e2e`, approvals 1 of 2).
- Queue: "Not in queue" + primary "Add to queue" (Cmd: `layr queue add 812`).
- Preview: image tag `app:change-812` (mono, copy) and "Pull: `docker pull cloud.acme.dev/shop/app:change-812`".
- Conflict radar: "`src/cart.rs` also changes in `agent-4`".
- Secrets: "No secret values in shared files ✓". On `#820` show instead an amber finding:
  "Possible secret in `src/coupons/test.rs:14`" with actions "Move to overlay secrets" and
  "Mark as not a secret" (Cmd: `layr secrets declassify --value …`).

Overview tab: description (markdown), versions timeline (v1, v2, v3 with "replayed onto main
`9a0c…`" events separated from content changes), attestations timeline, conversation with
comments on lines, and the merge box at the bottom: required list, merge methods
"Merge" / "Merge as one state" (primary only when all required pass; else disabled with the
reason), Cmd: `layr merge topic-checkout --expect 7d3e9a1c`.

Files tab: version compare bar "Compare v2 → v3" (selects) with a toggle "Hide base moves"
(on). File list on the left (tree, 240px), diffs on the right. Derived files collapsed. One
inline comment thread. Cmd: `layr change diff 812 --from v2 --to v3`.

Behavior tab (the "behavior review"): heading "Behavior diff: base `9a0c…` → v3 `7d3e…`".
One card per probe:
- `api-catalog`: "GET /api/products?page=2: 200 → 200, body changed": a JSON diff of the
  answer (2 fields).
- `bench-checkout`: table metric, base, v3, change: "p95 latency 41 ms → 52 ms (+27%)" in amber,
  "throughput 1,920 → 1,905 req/s (−0.8%)" neutral.
- `bundle-size`: "web/dist/app.js 214 KiB → 221 KiB (+3%)".
- `sql-plan`: "No change ✓".
Each card shows the probe's command in mono and "Ran on executor-eu1, exact states". Top right:
"Rerun probes" (Cmd: `layr probe diff 9a0c 7d3e`).

Checks tab: table check, state, result, duration, runner machine, argv, "reused from earlier
run" mark when the exact argv and state already passed. Log viewer for a failed `e2e` run.

Steps tab: for a change made by an agent (show on `#820`): a vertical timeline of automatic
states, one per tool call ("agent-7 ran `apply_patch src/coupons/mod.rs`", "ran `cargo test`
→ exit 101"), each with "+12 −3" and a "View diff" link and "Restore line to this step".
Header: "38 steps by agent-7 in 14 min. Every step is a state you can open or restore."

### 6.9 Queue (`queue.html`)

- Heading "Queue for `main`". Explanation line: "Each entry merges only after required checks
  pass on the exact result."
- Table: position, change, version, result state being tested (mono), checks progress
  (`⟳ e2e 3/5 min`), probes, added by, waiting time. Row 1 `#815` running, row 2 `#806` merged
  (recent, gray). Actions: Remove, Move up. Cmd: `layr queue run`.
- Settings summary link: "Required: unit, lint, e2e. Probes: bench-checkout (fail over +30%)".

### 6.10 Checks (`checks.html`)

Project-wide attestation feed: filter by check, line, result. Table as in 6.8 Checks. Right
panel "Check definitions" from `layr config check.*` (name, argv, runner).

### 6.11 Behavior (`behavior.html`)

Probe list: name, kind (HTTP answers, metric, file), command, last run, trend of the last 10
states as plain numbers (no charts with color decoration; a small mono sparkline of ▁▂▃ is
allowed in gray). "Compare any two states" form → shows the same cards as 6.8 Behavior.

### 6.12 Environments (`environments.html`, `environment.html`)

List: ref (`env/prod`, `env/staging`), current state, release ref, protection summary
("✓ unit · ✓ e2e · 1 approval"), last moved by and when, image tag (`app:prod`).
Detail (`environment.html` for `env/prod`): position history table (`env/prod@{0}` … `@{5}`:
state, moved by, time, expect value), primary "Move env/prod…" dialog (target state, required
attestations listed with status, Cmd: `layr ref set env/prod 7d3e --expect a91c`), and
"Roll back to `env/prod@{1}`" (secondary, danger style not needed).
Section "Other refs": `release/*`, `pins/*`, `holds/*` (reason "legal hold"), `leases/*` with TTL
countdown.

### 6.13 Registry (`registry.html`)

- Runnables from `layr.toml` (show the toml in a code frame).
- Images: name, tag, maps to (ref or state), digest (mono, short), size, "deterministic: same
  state gives the same digest". Tags: `prod` → `env/prod`, `1.4.2` → `release/1.4.2`,
  `change-812`, `line-topic-checkout`, `view-9f2c41`.
- Helm chart `chart` with versions.
- Pull instructions: `docker login cloud.acme.dev -u alice --password-stdin`,
  `docker pull cloud.acme.dev/shop/app:prod`, `helm install shop oci://cloud.acme.dev/shop/chart --version 1.4.2`.
- Note: "Images are built on demand from the exact state when a tag is first pulled."

### 6.14 Activity (`activity.html`)

The signed operation log: table of operation (save, merge, ref set, line new, overlay readers,
push, undo), who, machine, target, time, signature mark "✓ signed". Each row has "Undo" when the
operation can be undone (Cmd: `layr undo <op>`). Filter by person, agent, machine, kind.

### 6.15 Project settings (`settings.html` and sub pages)

Side nav groups and pages (each a file `settings-<name>.html`):
- **Access**: `settings-access.html` people and roles (owner, maintainer, writer, reader), path
  owners for review.
- **Protection**: `settings-protection.html` per line and ref: required checks, approvals,
  queue required, who may move. Cmd: `layr protect env/prod --check unit --approvals 1`.
- **Overlays**: `settings-overlays.html` each overlay: files patterns, readers (people and
  machines), peers that may hold it, key epoch ("epoch 3, started when dan was removed"),
  "Remove reader" warns: "Files dan already copied stay readable to dan."
- **Secrets**: `settings-secrets.html` detector paths, declassified values (listed by HMAC
  fingerprint only, never the value), recent refusals.
- **Merge rules**: `settings-merge.html` `.layrattributes` derive rules table (`package-lock.json
  derive=npm`, `Cargo.lock derive=cargo`).
- **Checks and probes**: `settings-checks.html` check and probe definitions.
- **Remotes**: `settings-remotes.html` stores (URL, recipient pin `age1…` mono, last push,
  state count, size), peers (`ssh://`), Git mirror, "Trust a changed store recipient" flow with
  both values shown. Cmd: `layr remote add vault s3://acme-layr/shop`.
- **Registry tokens**: `settings-tokens.html` token names, scope (pull), created, last used,
  Cmd: `layr access token`.
- **Caches**: `settings-caches.html` regenerable caches per line (`target/`, `node_modules/`),
  size, shared yes/no.

### 6.16 Org: People (`people.html`)

Table: person, login, role in org, person key (recipient fingerprint, mono), projects, last
seen. Invite flow (dialog: login, projects, role). Row for `dan`: "invited · waiting for key".
Section "Your person key": "One key opens every project you may read. Layr Cloud never holds
it." Buttons "Show recovery words" (paper key) and Cmd: `layr person keys`.
Grant dialog: Cmd: `layr remote grant cloud --person carol --recipient age1…`.

### 6.17 Org: Stores (`stores.html`)

Per project store: URL, region, encryption "age, end to end", size, objects, last write, GC
status. Note: "Storage and Layr Cloud see ciphertext only."

### 6.18 Org: Machines (`machines.html`)

Table: machine, OS and backend (APFS / btrfs / OverlayFS, all shown the same way), role
(person laptop, executor, CI runner), signing key fingerprint, trust state, last seen, online
mark. Section "Gateway": URL, TLS, write tokens count. Section "Executors": `executor-eu1`
running queue, checks, registry, probes; current jobs.

### 6.19 Org settings (`org-settings.html`)

Billing is out of scope. Show: org name, AWS account and region (from CloudFormation), member
default role, "Delete organization" danger zone.

## 7. Shared components

### 7.1 Clone dialog

Tabs: "Clone", "Mount", "Git". Clone: `layr clone layr+https://cloud.acme.dev/shop`. Mount:
`layr mount cloud ~/shop` and "Opens the project as a folder, read only, decrypted on your
machine." Git: `git clone https://cloud.acme.dev/shop.git` is NOT offered; instead
"Mirror to Git: `layr git push github main`".

### 7.2 Empty states

Lines page for `docs-site` is not needed. Show one empty state only on the Queue page variant
`queue-empty.html`: "The queue is empty" plus the Cmd.

### 7.3 Interactions in `app.js`

Only: dropdowns (line switcher, + menu, avatar menu), dialogs (open, close with Esc and a close
button, focus trap), tabs inside a page (clone dialog), the "Show automatic states" toggle, the
agent lines collapse, copy buttons (navigator.clipboard), Split/Unified diff toggle. Without JS
every page still reads correctly.

## 8. Responsive

- 1280px: layouts as described.
- Under 1012px: the right sidebars move under the main content.
- Under 768px: tables become stacked rows (label: value) only for the Lines, Changes and States
  lists; other tables scroll horizontally inside their frame. Tab rows scroll.

## 9. Acceptance (the worker gives this evidence)

1. All pages in section 6 exist and every nav link resolves (a link check script output,
   0 broken).
2. Screenshots with headless Chrome of every page at 1280×900 light, 1280×900 dark (use
   `--force-dark-mode` or an emulated media query), and 390×844 light, saved under
   `mockup/screenshots/` (not committed; list paths in the report).
3. `grep -riE 'backup|pull request|\bPR\b|commit|branch|stash|cherry|rebase|revert|HEAD'
   mockup/*.html` prints only the allowed Git mirror lines.
4. No external resources except the Google Fonts CSS and font files.
5. HTML validity: no unclosed tags (run a parser check, e.g. `python3 -m html.parser`-based script).
6. Commit on branch `ui/mockup` with messages in plain words. No tool-advertisement lines.
   Commit messages end with the co-author line the harness gives.

## 10. Questions

Ask the lead before any of these: adding or removing a page, a tab, a feature or a name;
changing a token, a size or the layout; using a pattern not in this file. Do not ask about
fake text that only fills space; write it in the same tone as the examples.

## 11. Additions (2026-10-24)

- Behavior shows UI diffs: a probe of kind `screenshots` renders routes of each exact state in its
  try copy and compares them. Views: Side by side, Swipe, Onion skin, Difference (changed regions
  outlined in amber, unchanged pixels dimmed). Each screen lists its route, viewport and the share
  of changed pixels.
- Overlay settings show reader kinds (`person:<login>`, `machine:<id>`) and key managers, with a
  warning when an overlay has only one manager.
- Org Stores shows "Your disk" (`layr mount --all`, lock state per store, stores with no address)
  and "AWS access" (profile, SSO token).
- Org People shows "Your invites" with Accept.
- Org Settings shows "Deployment" (the four stacks and `layr-cloud deploy`).

## 12. Simplification (2026-10-24, lead)

Same features, fewer places. Every removed element has a named new home.

| Before | After | Why |
|---|---|---|
| Project tabs: Code, Changes, Queue, Checks, Behavior, Environments, Registry, Activity, Settings (9) | Code, Changes, Checks, Deploy, Activity, Settings (6) | Related views share a tab with a sub navigation |
| Queue tab | Changes, segment "In queue" | Queue entries are changes in a state |
| Checks tab and Behavior tab | Checks tab with "Tests" and "Behavior"; check and probe definitions shown there | Probes are checks; definitions live next to results |
| Environments tab and Registry tab | Deploy tab with "Environments" and "Images" | Image tags map to refs; one place for what runs |
| Bisect page | Dialog on the States page | It is one action on the history |
| Change tabs: Overview, Files, Behavior, Checks, Steps | Overview, Files, Behavior, Steps; Steps only when a change has agent steps | Checks table and logs move into Overview |
| Change sidebar: Reviewers, Required, Queue, Preview, Conflict radar, Secrets (6 blocks) | "Merge" checklist with one action, and Preview | One list answers "can this merge, and if not, why"; radar and secrets show only as checklist lines |
| Merge box at the bottom of Overview and "Add to queue" in the sidebar | One button in the Merge block: "Merge" (through the queue when main requires it) | No duplicate controls |
| A CLI line under every action | A small "CLI" copy chip with the command in its tooltip; "Show CLI commands" in the account menu expands all lines | Same teaching, less noise |
| Project settings: 9 pages in 4 groups | Access (people, roles, path owners, registry tokens), Protection, Private files (overlays and the secret detector), Files (merge rules and caches), Remotes | Group by what people manage |
| Org tabs: Projects, People, Stores, Machines, Settings | Projects, People, Infrastructure (stores, machines, gateway, executors, deployment), Settings | Stores and machines are one system |
| Personal items on org pages (person key, invites, your disk, AWS access) and "Machines 3 online" in the top bar | One "You" page from the account menu | Personal and shared settings are apart |
