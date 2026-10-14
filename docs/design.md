# Project vault: encrypted copies in AWS

The vault keeps the latest state of a Studio project, and its history, in an S3 bucket
that the user owns. Only the user can read it: the data is encrypted in the VM with the
public key of the vault, and only the user's passkeys and paper key open the private key.
The Mac and the VM are working copies; the vault is the copy that stays.

## What it gives

- Desk, a Finder-like web page on the gateway (one small server in the user's tailnet), shows
  the latest state of every vault and any earlier one, previews files, and downloads them. The
  browser decrypts them. Outside the tailnet the page does not exist.
- `vault-restore` rebuilds any state into a folder with the paper key.
- Each project can use its own AWS account and bucket (one per client).

## Keys

| Key                                                    | Where                                             | Can                   |
| ------------------------------------------------------ | ------------------------------------------------- | --------------------- |
| Vault identity (`age` X25519)                          | nowhere in plain form; inside each slot           | decrypt everything    |
| Vault recipient (`age1…`)                              | VM vault configuration, `vault.json`, gateway     | encrypt new data      |
| Paper key (`age` X25519 identity, `AGE-SECRET-KEY-1…`) | on paper, two copies                              | open the paper slot   |
| Passkey (WebAuthn PRF)                                 | the user's passkey provider                       | open its passkey slot |
| Writer AWS key                                         | VM, root only (`layr-admin/vault/<project>.json`) | put new objects only  |

- A slot is the vault identity encrypted with `age` to the recipient of one opener:
  `slots/paper.age` for the paper key, `slots/passkey-<credential id>.age` for each passkey.
  The PRF output of a passkey (32 bytes, salt `layr-vault/v1/<project>`) is the X25519 secret
  of its recipient, so one passkey of the gateway host can open several vaults with different
  secrets. Any number of slots can exist; removing a slot object revokes that opener.
- The paper key is 24 words of the English BIP39 list: the 32 bytes of an X25519 secret with an
  8-bit checksum, as crypto wallets write their keys. The site and the Mac tools also take the
  same key in its `age` form (`AGE-SECRET-KEY-1…`), the bech32 encoding of those 32 bytes with
  the prefix `age-secret-key-`.
- Without this software: any BIP39 tool turns the words into the 32 bytes (the entropy, not a
  BIP39 seed), bech32 turns them into the `age` form, and the `age` tool decrypts every object:
  `age -d -i paper.txt slots/paper.age > vault.txt`, then `age -d -i vault.txt <object>`.
- The VM has the recipient only, so agents and a stolen VM disk cannot read the vault.
- The gateway proves that a caller holds the vault identity: it returns a token encrypted to
  the vault recipient, and accepts the decrypted token (an HMAC with a time and the project) for
  10 minutes. There is no user database; the signing key lives in the gateway's memory.

## Objects

Bucket prefix `<project>/`:

- `vault.json`: `{"format": 1, "recipient": "age1…", "created": <epoch>}` (plain).
- `slots/*.age`: see above (open to every tailnet device through the gateway; each is encrypted).
- `states/<seq>.<attempt>.tree.age`: the tree of one archived state (the commit point).
- `states/<seq>.<attempt>.<part>.data.age`: data frames of that state.

`seq` is a 10-digit counter per project; `attempt` is 8 random hex digits, so a retried push
never collides with the parts of a failed one. Every put uses `If-None-Match: *`; objects
are never changed or deleted by the writer. The latest state is the highest `seq` that has a
tree object; every reference names its `seq` and `attempt`, so a second tree for one `seq`
(after a lost reply) is harmless.

### Tree object

Plaintext: one header line of JSON, a newline, then one zstd frame.

```json
{"v": 1, "seq": 12, "state": "<layr state id>", "time": 1791700000,
 "tree": "full" | "delta", "base": [11, "0a9b8c7d"], "parts": 2, "attempt": "1f2e3d4c"}
```

The frame holds the canonical tree JSON: `{"files": {"<path>": [mode, size, sha256, chain]}}`
with sorted keys, as Python's `json.dumps(tree, sort_keys=True, separators=(",", ":"))` writes it
(non-ASCII as `\uXXXX`). `tree: "delta"` means the frame is compressed with the canonical tree of
`base` (`[seq, attempt]` of the previous push) as its zstd prefix (`zstd --patch-from`). A full tree is written at
least every 32 seq.

`chain` lists the frames of the file, newest first: each is `[seq, attempt, part, offset,
length, kind]`, and the bytes at `offset` and `length` of the decrypted data part are one zstd
frame. The last frame is `"full"`; a `"delta"` frame is compressed with the content of the next
frame's version as its prefix. A chain has at most 16 deltas. Mode is octal text (`"100644"`,
`"100755"`, `"120000"` for a symbolic link, whose content is its target).

### Data part

The concatenated frames. A part holds up to 16 MiB of frames; a larger frame has its own part.

## Writer

`layr-desk push --project <p> --state-dir <folder> [--config <file>]` pushes the main head of one
layr project; the host of the project runs it as the owner of the layr store. The configuration
(a file, or standard input) is JSON: `bucket`, `region`, `prefix`, `recipient`, `accessKeyId`,
`secretAccessKey`, and `endpoint` for a test store. The command prints a JSON summary.

1. `layr rev-parse main` names the head. When the record names this state, stop.
2. The export folders `a` and `b` take turns: `layr export <head>` fills one; the other holds
   the last pushed state. `layr diff --name-status <last> <head>` names the changed paths. The
   writer also looks at every added or removed path and at every path with a tab or a line
   break, which a diff line cannot carry. The first push takes every file.
3. A changed file gets a delta frame against its version in the other folder when that is the
   version of the last push, its chain has fewer than 16 deltas, the file has at most 32 MiB,
   and the delta is smaller; else a full frame. Frames are zstd level 3. A delta frame is made
   as `zstd --patch-from` makes it: the window covers the larger input (at most 2^27 bytes,
   which every decoder takes by default) and long-distance matching is on. Without that, a file
   above 2 MiB gains almost nothing from its base.
4. Each object is encrypted with `age` to the vault recipient. The parts go first, each when it
   is full, then the tree. A push that stops before its tree leaves no state, and the next push
   uses a new attempt.
5. The state folder keeps the canonical tree (`tree.json`) and the record (`writer.json`:
   `seq`, `attempt`, `state`, `side`, `since_full`). The old record goes first and the new one
   comes last, so a stop between them leaves no record.

Without a record, or with a record that does not match `tree.json`, the push is full: a full
tree, full files, and the `seq` after the highest tree that `ListObjectsV2` (`states/`, names
only) shows.

The Python writer (`python/vault_writer.py`) keeps the same state folder, so each writer
continues a folder of the other. The S3 client is a small one of its own, with no SDK:
`desk-core` signs (SigV4) and finds the dual-stack endpoint
`<bucket>.s3.dualstack.<region>.amazonaws.com`, as for the gateway, so a host with IPv6 only
reaches S3; `crates/layr-desk/src/s3.rs` sends the two requests over rustls with the key of the
configuration. The time limit of a put grows with its size (2 minutes plus 1 second for each
100 kB), so a large part on a slow line is not cut off.

## Reader

The browser and `vault-restore` share one implementation (`workspaces/runtime/apps/vault`):
`age-encryption` (typage) and `@bokuweb/zstd-wasm` (`decompressUsingDict` takes the base as a
raw dictionary). Reading the latest tree fetches the tree objects back to the last full tree;
reading a file fetches the parts of its delta chain.

## AWS (`studio-vault/cloud/`)

One stack per project, in the project's AWS account (`template.yaml`):

- Bucket with versioning; public access blocked; no CORS, no public path.
- Writer user: `s3:PutObject` on `<project>/states/*` with `If-None-Match` required,
  `s3:ListBucket` on `<project>/states/`; nothing else.
- Reader role `studio-vault-reader-<project>`: read states and slots, put and delete
  `slots/passkey-*`. It trusts the gateway role by ARN (`aws:PrincipalArn`), so the gateway can
  be in another account and its role can be made again.
- What stays out of CloudFormation: `vault.json`, the paper slot, and the writer's access key,
  which would otherwise sit in the stack outputs; IAM Roles Anywhere is to replace that key.

One gateway for all projects, in the user's account (`vault-gateway.mjs`):

- `dns.yaml`: the Route 53 zone of the domain; the registrar names its servers.
- `bootstrap.yaml`: the artifacts bucket: `gateway/releases/<sha256>.tgz`, `gateway/release.json`,
  `gateway/gateway.json` (host and zone) and `gateway/projects/<project>.json` (bucket, region,
  reader role, recipient), which `vault-deploy.mjs` writes.
- `gateway.yaml`: an IPv6-only VPC and one `t4g.nano` (Debian 13, standard CPU credits, IMDSv2,
  8 GB encrypted disk). No public IPv4 address; the security group lets in only WireGuard (UDP
  41641). Its role reads `gateway/*`, assumes `studio-vault-reader-*`, reads the tailnet auth key
  in SSM, and changes only the host's `A` and `_acme-challenge` records.

## Gateway

`layr-desk serve` (Rust) is the server. A release (`js/vault/gateway/build.mjs`) holds the
program (static, `aarch64-unknown-linux-musl`, built with `cargo zigbuild`), the built site and
`sync.py`, which runs as root at first boot and every 5 minutes. `sync.py` joins the tailnet with
the auth key from SSM (`tag:vault`, Tailscale SSH on), installs the release that `release.json`
names, writes `/etc/vault-gateway/config.json` from `gateway/projects/`, points the host's public
`A` record at the tailnet address (100.x, which only the tailnet can reach), gets a Let's Encrypt
certificate by DNS challenge, and keeps `vault-gateway.service` running (`DynamicUser`, read-only
system). Unattended upgrades, Tailscale's included, reboot at 04:30 when needed.

The server reads the configuration again when its modification time changes, and listens on its
`listen` address. It has no AWS SDK: it takes the instance role from IMDSv2 (over IPv6 with
`AWS_EC2_METADATA_SERVICE_ENDPOINT_MODE=IPv6`), assumes each vault's reader role through STS
(session `vault-gateway`), and calls the dual-stack STS and S3 endpoints of the vault's region;
`desk-core` signs (SigV4).

The certificate: `--tls files` (the default) reads `tls.crt` and `tls.key` from the systemd
credentials (certbot's files; certbot's deploy hook restarts the service), or the files of
`--cert` and `--key`, and reads them again when they change. A host without a domain of its own
uses its tailnet name with `--tls tailscale`: the server asks `tailscale status --json` for the
name (`Self.DNSName`) and reads `<name>.crt` and `<name>.key` in `--tls-dir`
(`/etc/vault-gateway/tls`). HTTPS certificates must be on in the tailnet; then, as root, once and
then daily:

    tailscale cert --cert-file /etc/vault-gateway/tls/<name>.crt \
      --key-file /etc/vault-gateway/tls/<name>.key <name>

The server serves a renewed certificate from the next connection on. Its user must be able to
read the key, and `host` in the configuration is then `<name>`. `--tls none` serves plain HTTP, on a loopback address only (the
site e2e).

API, per vault `<p>`: `GET /api/projects`, `GET /api/p/<p>/slots`, `GET /api/p/<p>/challenge`,
and with `Authorization: Bearer <token>`: `GET /api/p/<p>/states`, `GET /api/p/<p>/states/<name>`,
`PUT|DELETE /api/p/<p>/slots/passkey-<id>`. Every other path is the site; `/<p>/` is the page of
one vault. The host header must be the configured host.

Tailnet policy: `tag:vault` belongs to the admins; access rules take `autogroup:member` as the
source, so the gateway answers but cannot reach the user's devices; one SSH rule lets members
reach `root@tag:vault`.

Cost in eu-central-1: about $3.50 a month for the instance, $0.80 for the disk and $0.50 for the
zone. From a network without IPv6, the tailnet relays the traffic through a Tailscale DERP server.

## Measurements behind the format

On the codex-agents history (1,095 states), one run each: btrfs send streams with zstd took
100.8 MB; content-defined chunks 92 to 116 MB in 17,000 to 21,000 objects; file deltas against
the previous version with tree deltas, one object per state, 25.7 MB (zstd -3) and 23.9 MB
(zstd -19). `git gc --aggressive` took 23.9 MB.

## Network disk

`layr-desk mount` (Rust) shows every vault of a gateway as one read-only disk on the Mac: a
folder per vault, each with the latest state of its project. It reads through the gateway like
the site (challenge, token, state objects), so the Mac needs no AWS profile, and it decrypts on
the Mac. It looks for new states every 2 minutes.

- `layr-desk unlock --site <site> --project <p>` opens a vault once with its paper key and keeps
  the vault identity in the login Keychain (service `layr-desk`, account `<host>/<p>`), through
  the `security` tool so that a new build reads it without a prompt. A vault without a key
  shows a `LOCKED.txt` with this command. `layr-desk lock` removes the key.
- `layr-desk status --site <site> --json` lists the vaults of a gateway and whether this Mac
  keeps the key of each; `unlock --paper-file -` reads the words from stdin, for a program that
  asks for them itself. `layr-desk service status` prints the installed agent as JSON.
- `layr-desk service install --site <site>` writes the launchd agent `ws.layr.desk`: it starts
  at login, serves WebDAV on `127.0.0.1:8095` and mounts it at `~/Desk` with `mount_webdav`
  (the volume "Desk" in Finder, read only). `service remove` stops it.
- `layr-desk states` lists the states of a vault, and `layr-desk pull --to <new folder>
[--state <seq>]` writes one state into a new folder: files with their executable bit, links
  as links. The folder appears only when every file is written and checked; an existing folder
  is refused. Both use the kept key, else ask for the paper key.
- A link shows the file it points to inside the state; a link that leaves the state shows its
  target text. Files are checked against their tree entry and stay in
  `~/Library/Caches/layr-desk`, named by their SHA-256 (plain files, protected by FileVault).

## Set up the gateway, once

1. Buy the domain; `vault-gateway.mjs` prints the Route 53 name servers for the registrar.
2. Tailnet policy as above; then an auth key: not reusable, not ephemeral, pre-approved, tag
   `tag:vault`. The user stores it in SSM `/studio-vault/gateway/tailscale-auth-key`
   (`SecureString`) from their own terminal.
3. `node workspaces/runtime/apps/vault/bin/vault-gateway.mjs --host <desk.example.com> --profile
<profile>`. It builds the site and the server, uploads the release, and makes the stacks. A
   later run with the same arguments installs a new release within 5 minutes; `--new-image`
   replaces the instance, which then needs a new auth key.

## Set up a vault for a project

1. The user, in a terminal of their own (never through an agent, so the paper key stays out of
   transcripts): `node workspaces/runtime/apps/vault/bin/vault-keys.mjs <prefix>`. It shows the
   paper key once; write it down twice. `~/.config/studio-vault/<prefix>/` keeps only the
   recipient and the encrypted paper slot.
2. `aws sso login --profile <profile>`, then, with the Studio Python first on `PATH`:
   `node workspaces/runtime/apps/vault/bin/vault-deploy.mjs --project <prefix> --profile <profile>
--region eu-central-1 --folder <project folder>`. The profile names the vault's account, which
   may be a client's. It makes the stack, uploads the paper slot and `vault.json`, registers the
   vault with the gateway, and pipes a write-only key to the VM (`codex_vault.py configure`).
3. The root broker pushes every configured project every 10 minutes; `codex_vault.py push` pushes
   at once, and `codex_vault.py status` shows the last push and the last error.
4. On `https://<host>/<prefix>/`, unlock with the paper key and add a passkey, or use an existing
   passkey of another vault on the same host.
5. Mac: `vault-view.mjs --project <prefix> --mount` mounts the latest state in Finder;
   `vault-restore.mjs --project <prefix> --out <folder> [--seq <n>]` rebuilds a state. Both read
   S3 directly with the AWS profile, so they work without the gateway and the tailnet.

`tests/site.e2e.mjs` in `studio-vault-web` checks the built site behind the real gateway code
in headless Chromium (paper key, columns and list views, Quick Look, download, passkeys for two
vaults with a virtual PRF authenticator); `tests/live.e2e.mjs` checks a deployed gateway.

## Not in the first slice

A company key slot, Object Lock defaults, garbage collection, Node.js 22 on the gateway (Debian 13
ships 20; AWS SDK releases after January 2027 need 22).

## Native range reads

The Rust reader can request parts of existing encrypted objects. The archive format and writer do not change.
The gateway forwards one bounded `Range: bytes=start-end` request to S3 after the normal vault authorization.
It returns `206` with `Content-Range`, or a complete `200` response when the store does not support ranges.
Malformed ranges return `400`. A range outside the object returns `416`.

The native client caches ciphertext in 128 KiB blocks. The age library decrypts and authenticates the required chunks.
The reader still checks the reconstructed file size and SHA-256.
It rejects a wrong range, a short response, and a change in object length.

Each part permits eight range fetches before another cache miss uses a complete-object read.
Compressed frames of at least 512 KiB also use complete-object reads.
`pull` and `verify` use complete-object reads from the start.
The combined cache of data parts and ciphertext blocks has a 512 MiB limit.
Tree caches remain separate.

An older gateway can return the complete object. The client reads it without another download.
The browser uses the same Rust age reader through WebAssembly in a dedicated Worker.
It caches up to 128 MiB of data parts and ciphertext blocks, apart from tree data and temporary buffers.
It uses the same block size, request budget, full-frame threshold, and full-response fallback as the native client.
At most two frame fetches run together. The browser restores deltas in order and checks the final size and SHA-256.
The page shows download bytes, decryption, and file restoration separately.
Locking the vault terminates its Worker and pending reads.

The browser build requires the `wasm32-unknown-unknown` Rust target and `wasm-bindgen-cli` version 0.2.129.
`pnpm --filter layr-desk-web run build` builds the Rust module before Vite packages the site.
The browser still uses JavaScript for tree traversal and zstd WebAssembly for decompression.
No key change or archive migration is required.

The proposed next steps for shared writes and review are in [collaboration.md](collaboration.md).
