# Moving our own projects from GitHub to Layr Cloud (lead plan, 2026-10-25)

Goal: igor moves `layr`, `layr-cloud` and `layr-studio` from GitHub to Layr Cloud and stops using GitHub for daily work.
GitHub stays only as an optional Git mirror (`layr git push github main`).

## What we use on GitHub today

| GitHub | Use today | Layr Cloud replacement |
|---|---|---|
| Repositories | 3 active repos | Layr projects in a Layr Cloud store (M1) |
| Pull requests | 13 in `layr`, review and merge | Changes, versions, approvals, comments, queue (M2) |
| Actions | `ci.yml`, `overlayfs.yml` on push and PR | Executor runs checks on each new change version and on main (M3) |
| Releases | `v0.1.0`, `v0.2.0` with binaries | `release/*` refs with built files to download (M4) |
| Web UI | browse code, review | Layr Cloud site with actions (M2, M5) |
| `gh` in agents | Studio agents open PRs | Agents use `layr` changes and the queue (M6) |
| Public repo | `layr` is public | Public projects that anyone can read (M7) |

## Acceptance criteria

- **M1 Hosting.** The three projects live in the deployed store. Clone, push, sync and the site work from igor's machines through the public gateway (docs/public-gateway-design.md). A Git mirror to GitHub still works.
- **M2 Review in the browser.** A person can approve, withdraw an approval, comment on a change (on a file line too), add to and remove from the queue, and move a ref. Each action is a request signed by the person's key; the executor applies it or writes a signed refusal (design section 6).
- **M3 Checks.** Each new change version and each new main state gets the configured checks on the executor. Results are attestations and show on the site. The two GitHub workflows have Layr equivalents (btrfs suite, OverlayFS ext4 and xfs).
- **M4 Releases.** `release/<v>` refs show on the Releases page with their notes (changes since the previous release) and downloadable built files (docs/release-files-design.md).
- **M5 Site parity.** Every page of the prototype (layr.ws) that has data exists on the real site, including Releases and the Lines filters.
- **M6 Agents.** Studio agents can open a change, see checks and comments, and merge through the queue with `layr` only.
- **M7 Public read.** A project can be public: anyone can read it without a key. Writes still need access.
- **M8 Deployed.** Gateway, executor and store run in AWS account `ifdotpy` (134250228509), reachable over public IPv6 HTTPS at `cloud.layr.ws` (no tailnet). The prototype stays at `layr.ws`.

## Order

1. Wave 1, in parallel: M5 Releases and Lines on the site; M2 core in `layr` (signing keys, requests, apply and refusal, comments); M3 checks on the executor; M8 deploy.
2. Wave 2: M2 in the browser; M4 release files; `store-index` (large stores, blame).
3. Wave 3: M7 public projects; M6 Studio agents; move the three projects; GitHub becomes a mirror.
