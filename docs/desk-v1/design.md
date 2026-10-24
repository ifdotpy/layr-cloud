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

| Key                                                    | Where                                             | Can                                                    |
| ------------------------------------------------------ | ------------------------------------------------- | ------------------------------------------------------ |
| Vault identity (`age` X25519)                          | nowhere in plain form; inside each slot           | decrypt everything                                     |
| Vault recipient (`age1…`)                              | VM vault configuration, `vault.json`, gateway     | encrypt new data                                       |
| Paper key (`age` X25519 identity, `AGE-SECRET-KEY-1…`) | on paper, two copies                              | open the paper slot                                    |
| Passkey (WebAuthn PRF)                                 | the user's passkey provider                       | open its passkey slot                                  |
| Writer AWS key                                         | VM, root only (`layr-admin/vault/<project>.json`) | put new objects only                                   |
| Writer signing key (Ed25519)                           | writer configuration (`signingKey`)               | sign new states                                        |
| Person identity (`age` X25519)                         | nowhere in plain form; inside each person slot    | open the person slots of vaults, the index and invites |
| Person recipient (`age1…`)                             | `person.json` at the gateway; vault owners        | seal person slots, invites and the index               |
| Person paper key (24 words)                            | on paper, two copies                              | open the person's paper slot                           |
| Person passkey (WebAuthn PRF)                          | the person's passkey provider                     | open its person passkey slot                           |

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

- `vault.json`: `{"format": 1, "recipient": "age1…", "created": <epoch>, "writer": "<base64>",
  "signedFrom": <seq>}` (plain); `writer` and `signedFrom` name the writer's public key and the
  first signed state (see "Signatures").
- `slots/*.age`: see above (each is encrypted). `slots/person-<login>.age` is the vault identity
  sealed to the person key of the tailnet login `<login>` (see "Person key").
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
 "tree": "full" | "delta", "base": [11, "0a9b8c7d"], "parts": 2, "attempt": "1f2e3d4c",
 "sig": "<base64>"}
```

`sig` is the writer's signature (see "Signatures"); a tree of a writer without a signing key has
none.

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
`secretAccessKey`, `signingKey` (optional, see "Signatures"), and `endpoint` for a test store.
With `signingKey` every tree carries `sig`; without it the trees are unsigned. The command prints
a JSON summary.

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
reading a file fetches the parts of its delta chain. Every reader checks the vault identity, the
signature of each tree that `vault.json` asks for, and each file against the size and SHA-256 of
its tree entry.

## Signatures

The writer signs each tree with an Ed25519 key, so that a reader can tell the writer's states
from states that someone else put into the bucket.

- The signing key is `signingKey` in the writer configuration: standard base64 of a 32-byte
  Ed25519 seed. Only the writer has it. `layr-desk deploy` makes one with each new writer access
  key (the first configuration, or `--new-writer-key`) and sends it through the pipe of
  `--writer`; it never goes to a file or to the output.
- `vault.json` names the public key as `writer` (standard base64 of 32 bytes) and the first `seq`
  that must be signed as `signedFrom`. `layr-desk deploy` writes both after the `--writer`
  command took the key: `signedFrom` is then the highest `seq` in the bucket plus 1. The comment
  in `crates/layr-desk/src/setup.rs` explains why this order leaves no gap. A new writer key
  starts again from the `seq` after the last state, so the states that the old key signed then
  show as unsigned.
- The tree header field `sig` is standard base64 of the 64-byte Ed25519 signature of this UTF-8
  message (lines joined with `\n`, no line break at the end):

  ```
  layr-desk tree v1
  {prefix}
  {seq}
  {attempt}
  {state}
  {time}
  {tree}
  {base}
  {sha256}
  ```

  `tree` is `full` or `delta`; `base` is `{seq}.{attempt}` of the base, or `-`; `sha256` is the
  lowercase hex SHA-256 of the canonical tree bytes after delta decoding. The tree holds the size
  and SHA-256 of each file, so the signature also covers the content of every file. A reader
  that does not know `sig` ignores it.

The policy, in every reader (desk-core for `layr-desk ls`, `verify`, `states`, `pull` and
`mount`; `reader.mjs` for the site, `vault-restore` and `vault-view`):

- A `vault.json` without `writer` has no signature checks, and every state shows as unsigned.
- With `writer`, a state with `seq` from `signedFrom` on (every state when `signedFrom` is
  missing) must carry a valid signature. Reading it fails otherwise, with an error that names the
  state. A state before `signedFrom` reads as before and shows as unsigned.
- A slot that opens to a vault identity whose recipient is not the `recipient` of `vault.json`
  is refused.
- The gateway gives the public part of `vault.json` (`recipient`, `writer`, `signedFrom`) at
  `GET /api/p/<p>/vault`, without a token, read through the reader role. The site and the
  `layr-desk` commands take it from there; `vault-restore` and `vault-view` read it from the
  bucket.
- The site checks with WebCrypto (`crypto.subtle.verify("Ed25519", …)`), which Chrome and Edge
  137, Firefox 129 and Safari 17 have. A browser without it shows an error and reads no signed
  state; there is no JavaScript fallback.
- The Python reader (`python/vault_reader.py`) does not check signatures or the vault identity.
  The Python writer does not sign: a state that it pushes into a vault with `writer` is refused.

What it protects against: a forged state or slot from anyone who can put objects into the bucket
but does not hold the signing key, as long as the `vault.json` that the reader gets stays as
`layr-desk deploy` wrote it. A stolen writer access key (it puts only below `states/`) and a
person with write access to the bucket can no longer add a state that readers accept, change a
signed state or one of its files, or give a reader another vault identity through a slot.

What it does not protect against yet:

- A reader without a pin trusts the `vault.json` that the gateway serves (or that the bucket
  holds, for `vault-restore` and `vault-view`). The gateway, and anyone who can change
  `vault.json` in the bucket, can remove `writer` or name another key and recipient. The site
  pins the recipient, the writer key and `signedFrom` of each vault in the person index (see
  "Person key"); the `layr-desk` readers do not read the index yet.
- States before `signedFrom`, such as a second tree for an old `seq`.
- Removed states: a reader cannot tell that newer states existed.
- A stolen signing key, until `--new-writer-key` replaces it.

## Person key

One key for each person opens every vault that the person may read. On the site, one passkey
touch opens all of them. The vaults keep their own paper and passkey slots; a vault without
person slots opens as before.

### Keys and commands

- The person identity is an `age` X25519 identity. Its openers are the person's passkeys and a
  paper key of 24 words, with slots as for a vault. The PRF salt of a person passkey is
  `layr-person/v1/<login>`, so the same passkey gives the person key and each vault a different
  secret.
- `layr-desk person keys --site <site> [--clear]` makes the person identity and its paper key,
  and shows the 24 words once. Run it in a terminal of your own: never through an agent, and not
  in a Studio terminal, because Studio keeps terminal output in SQLite. The site never makes or
  shows the words; it only takes them to unlock. `~/.config/studio-vault/.person/` keeps only the
  recipient and the encrypted paper slot. With `--site`, the command registers the recipient and
  the paper slot at that gateway. A later run with another `--site` registers the same key there.
- `layr-desk person show` prints the person recipient (`age1…`). It is public. The person gives
  it to each vault owner through a channel that they trust, because a recipient from someone
  else could be an attacker's.
- On the site, the person unlocks the person key once with the words and adds a passkey
  ("Passkeys…" in the person box). After that, one passkey touch unlocks it.
- `layr-desk grant --project <p> --person <login> --recipient age1… [--site <site>]` runs in the
  terminal of the vault owner:
  1. It opens the vault identity with the vault's paper key, or with the owner's person paper key
     and the owner's person slot in the vault.
  2. It puts `<p>/slots/person-<login>.age` (the vault identity sealed to the recipient) into the
     vault bucket with the owner's AWS profile (`cloud.json` of `deploy`, or `--profile`).
  3. It drops an invite into the person's inbox at the gateway: the pin of the vault (see below),
     sealed to the recipient.

  No key passes through anyone else: the owner needs only the person's public recipient. Removing
  the slot object revokes the person, as for any slot. A new person key needs a new grant.

  The first person slot changes who sees the vault (see "Gateway side"): from then on only the
  logins with a person slot see it. Other tailnet members, tagged devices, and the `layr-desk`
  disk and lists of a Mac whose login has no slot no longer see it in the list and cannot open
  it through the gateway. So the owner grants their own login first. `grant` refuses, before it
  writes anything, to give the first person slot to another login than the caller's (as the
  gateway names it), unless the owner passes `--hide-from-others`.
- `layr-desk person vaults --site <site>` unlocks the person key with the words, reads the index,
  and checks the `vault.json` of each pinned vault against its pin.

### Objects at the gateway

The prefix `people/<login>/` in the gateway's artifacts bucket (the gateway's own account, which
has versioning) holds the records of one tailnet login:

- `person.json`: `{"format": 1, "recipient": "age1…", "created": <epoch>}` (plain).
- `slots/paper.age`, `slots/passkey-<credential id>.age`: the person identity sealed to each
  opener.
- `index.age`: the person index, sealed to the person recipient (at most 48 KiB).
- `inbox/<ms>-<8 hex>.json`: `{"from": "<login>", "time": <ms>, "data": "<base64>"}`. `data` is
  one invite sealed to the person recipient; `from` is the login that the gateway saw.

The plaintext of the index is a first line `layr-desk person index v1`, a second line with the
MAC in lowercase hex, and then the body: `{"vaults": [{"project": "<p>", "recipient": "age1…",
"writer": "<base64>" | null, "signedFrom": <seq> | null}]}`. The MAC is HMAC-SHA256 of the body
bytes with the key HMAC-SHA256(secret, `layr-desk person index v1`); `secret` is the 32-byte
X25519 secret of the person identity. Anyone can encrypt to a recipient, so the MAC shows that
the holder of the person key wrote the index.

The plaintext of an invite is `{"invite": 1, "project": …, "recipient": …, "writer": …,
"signedFrom": …}`.

### Pins

Each index entry pins three values of the vault's `vault.json`: `recipient`, `writer` and
`signedFrom`. The site refuses a vault whose `vault.json` differs from its pin in any of them,
and opens the vault identity from the person slot only when it belongs to the pinned recipient.
`signedFrom` is in the pin because a larger value would let unsigned states through. A new writer
key (`deploy --new-writer-key`) changes `writer` and `signedFrom`: the owner runs `grant` again,
and the person accepts the new invite, which replaces the pin.

### Gateway side

The gateway listens on its tailnet address with its own certificate; nothing stands in front of
it. A `Tailscale-User-Login` header would therefore come from the caller, so the gateway does not
read it. It asks the local tailscaled who the caller is, once per connection:
`GET /localapi/v0/whois?addr=<ip>:<port>` on `/var/run/tailscale/tailscaled.sock`
(`--tailscale-socket`). A tagged device, or an address that tailscaled does not know, has no
login. There is no password database.

API, for the caller's own login:

- `GET /api/person`: the login and the records: `recipient` (null before registration), `slots`,
  `index`, `inbox`.
- `PUT /api/person` `{recipient, paper}`: register once. The same recipient again is fine; another
  one gets 409.
- `GET /api/person/challenge`: a token sealed to the person recipient, as for a vault.
- With `Authorization: Bearer <person token>`: `PUT /api/person/index` `{data, previous}`
  (`previous` is the SHA-256 of the index that the site read, or null; another one gets 409),
  `PUT|DELETE /api/person/slots/passkey-<id>`, and `DELETE /api/person/inbox/<id>`.
- `POST /api/people/<login>/inbox` `{data}`: any tailnet person drops an invite. An inbox holds
  at most 64 invites of at most 4096 bytes each.

Every API write (`PUT`, `POST`, `DELETE`) must send `content-type: application/json`, and the
gateway refuses a write whose `Origin` is not its own or whose `Sec-Fetch-Site` is not
`same-origin` or `none`. A page of another site, opened on a tailnet device, can send a form or a
text/plain POST without a CORS preflight; JSON needs a preflight, which the gateway never grants.
The `layr-desk` commands send no `Origin`.

A vault with person slots shows only to the logins that have one: `GET /api/projects` lists it
only for them, and every route of it gives others the 404 of a vault that does not exist (a
valid vault token still reads the states, as it proves the vault identity). A vault without
person slots shows to every tailnet member, as before. The `slots` route gives the caller's own
person slot and no other.

Storage: `gateway.yaml` lets the gateway role get, put and delete `people/*` in the artifacts
bucket and list that prefix. `sync.py` writes `people` (`bucket`, `region`, `prefix`) into the
configuration. The vault buckets do not change: the owner puts person slots with the vault
account's profile, and the reader role already reads `slots/*`.

### Site

- The site reads `GET /api/person` first. Without a login or a person record, it works as before.
- The person box in the sidebar unlocks with a passkey (asked without a click when the person has
  one) or with the words. The site refuses a person slot whose identity does not belong to the
  recipient in `person.json`.
- After the unlock, the sidebar lists the vaults of the index with the vaults that the gateway
  shows. A pinned vault opens without a reload and without another touch.
- Invites show the vault, the sender and the pins. Before Accept writes the new index, the site
  checks the invite against the `vault.json` that the gateway serves and against the person slot.
- Lock forgets the person identity and every vault identity of the tab.

### What it protects, and what not

It protects against:

- The gateway, AWS and other tailnet users reading data: they see ciphertext, public keys and
  login names only.
- Other tailnet users seeing a vault in the list, or its slots, without a person slot in it.
- Anyone who can write to the people prefix or to a vault bucket, but does not hold the person
  key. Such a person cannot plant an index (MAC), cannot make the site open a pinned vault with
  another `vault.json` (pins), cannot give the person another identity through a person slot
  (recipient check), and cannot change person records through the API (token).
- An invite that adds a vault by itself: the person must confirm it.

It does not protect against:

- JavaScript that the operator serves. The site code comes from the gateway. The operator, or
  anyone who changes the release, can serve code that sends the person identity and every vault
  identity elsewhere. Pins, MACs and encryption do not help against that. A person who does not
  trust the operator runs a gateway of their own (docs/self-hosted.md), or reads with `layr-desk`
  on the Mac.
- A false invite that the person accepts. The sender name comes from the gateway, and an invite
  to a vault that an attacker made passes the checks.
- Rollback and hiding: the gateway can serve an older index or none. The site then shows fewer
  vaults or older pins, and an older pin refuses a vault after a key change.
- Metadata: the operator sees which logins have slots in which vaults (object names), and the
  times of invites.
- A person who loses the person key and all its openers. The operator then removes
  `people/<login>/`, the person makes a new key, and the owners grant again.

## AWS (`js/vault/cloud/` and `crates/layr-desk/cloud/`)

One stack per project, in the project's AWS account (`crates/layr-desk/cloud/vault.yaml`, which
`layr-desk deploy` carries):

- Bucket with versioning; public access blocked; no CORS, no public path.
- Writer user: `s3:PutObject` on `<project>/states/*` with `If-None-Match` required,
  `s3:ListBucket` on `<project>/states/`; nothing else.
- Reader role `studio-vault-reader-<project>`: read states, slots and `vault.json`, put and
  delete `slots/passkey-*`. It trusts the gateway role by ARN (`aws:PrincipalArn`), so the gateway can
  be in another account and its role can be made again.
- What stays out of CloudFormation: `vault.json`, the paper slot, and the writer's access key,
  which would otherwise sit in the stack outputs; IAM Roles Anywhere is to replace that key.

One gateway for all projects, in the user's account (`vault-gateway.mjs`):

- `dns.yaml`: the Route 53 zone of the domain; the registrar names its servers.
- `bootstrap.yaml`: the artifacts bucket: `gateway/releases/<sha256>.tgz`, `gateway/release.json`,
  `gateway/gateway.json` (host and zone) and `gateway/projects/<project>.json` (bucket, region,
  reader role, recipient), which `layr-desk deploy` writes.
- `gateway.yaml`: an IPv6-only VPC and one `t4g.nano` (Debian 13, standard CPU credits, IMDSv2,
  8 GB encrypted disk). No public IPv4 address; the security group lets in only WireGuard (UDP
  41641). Its role reads `gateway/*`, reads and writes `people/*` (the person records), assumes
  `studio-vault-reader-*`, reads the tailnet auth key
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

API, per vault `<p>`: `GET /api/projects`, `GET /api/p/<p>/slots`, `GET /api/p/<p>/vault`,
`GET /api/p/<p>/challenge`, and with `Authorization: Bearer <token>`: `GET /api/p/<p>/states`, `GET /api/p/<p>/states/<name>`,
`PUT|DELETE /api/p/<p>/slots/passkey-<id>`. Every other path is the site; `/<p>/` is the page of
one vault. The host header must be the configured host. The person routes are in "Person key".

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

Codex Studio runs these steps from the LAYR menu of a project ("Set up vault…"). By hand:

1. The person, in a terminal of their own (never through an agent, so the paper key stays out of
   transcripts and logs): `layr-desk keys --project <prefix> --clear`. It shows the paper key
   once; write it down twice. Enter then clears the terminal and its scrollback.
   `~/.config/studio-vault/<prefix>/` keeps only the recipient and the encrypted paper slot.
2. `layr-desk deploy --project <prefix> --profile <profile> --region eu-central-1 --writer
   '<command>'`. The profile names the vault's account, which may be a client's. It makes the
   stack, uploads the paper slot and `vault.json`, registers the vault with the gateway, and
   pipes a write-only key and a signing key to the writer command (for Codex Studio:
   `codex_vault.py configure <project folder>`). It exits with 3 when the profile's IAM Identity
   Center sign-in has ended: `layr-desk aws-login --profile <profile>` signs in again (the
   device flow in the browser), and `layr-desk aws-profiles` shows which profiles need it. Both
   keep the token in `~/.aws/sso/cache`, as `aws sso login` does.
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
