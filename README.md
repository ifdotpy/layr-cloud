# layr-desk

End-to-end encrypted copies of projects in your own AWS accounts, with a web view (Desk) and a
read-only network disk. Only your devices decrypt: storage, gateway and AWS see ciphertext.

- **Writer:** `layr-desk push` sends each new project state to S3, encrypted to the vault's `age`
  recipient. It sends only the changes: file deltas against the previous version and tree deltas.
- **Vault:** one S3 bucket per project, in the AWS account of the project's owner. A passkey
  (WebAuthn PRF) or a paper key of 24 words opens it.
- **Gateway:** one small server in a tailnet that serves the Desk site and every vault. It reads
  the buckets through roles that each account grants it.
- **Desk:** a Finder-like site that decrypts in the browser.
- **Network disk:** `layr-desk mount` shows every vault as a read-only disk in Finder and decrypts
  on the Mac.

The design, the key model and the AWS setup are in [docs/design.md](docs/design.md). A person
who does not want to trust a shared gateway runs their own: [docs/self-hosted.md](docs/self-hosted.md).

## Layout

| Folder             | What                                                                       |
| ------------------ | -------------------------------------------------------------------------- |
| `crates/desk-core` | Rust library: keys, the format, the reader, the writer, AWS signing        |
| `crates/layr-desk` | the `layr-desk` command: the gateway, the writer; read, verify, mount      |
| `js/vault`         | keys, reader, gateway, CloudFormation templates and the `vault-*` commands |
| `web`              | the Desk site                                                              |
| `python`           | the first writer and its S3 client, replaced by `layr-desk push`           |
| `docs`             | design                                                                     |

Status: the logic moves to Rust (one library for the writer, the gateway, the network disk and,
through WebAssembly, the browser). The JavaScript and Python code stays until each part has a
Rust replacement.

## Writer binary

The host of a project is arm64 Linux; the writer for it is one static binary:

```sh
docker run --rm -v "$PWD":/src -w /src rust:1-alpine sh -c 'apk add musl-dev &&
  cargo build --release --locked -p layr-desk --target aarch64-unknown-linux-musl'
```

The binary is `target/aarch64-unknown-linux-musl/release/layr-desk`.

## Tests

```sh
pnpm install
cargo test
pnpm test                                   # library and gateway tests, then the site e2e
cargo build -p layr-desk && pnpm --filter layr-desk-web run test:e2e:rust   # e2e on layr-desk serve
python3 -m unittest discover -s python -p 'test_*.py'
python3 -m unittest discover -s js/vault/test -p 'test_*.py'
```

## Browser index for large states

`push` adds an encrypted directory index when a state has at least 100,000 files.
The browser reads at most 256 direct children per page. It does not need the full
tree to open a directory. Filenames and file entries remain encrypted. Directory
object names contain only the state identity, a path hash and a page number.
The objects live under `states/browse/`. The gateway excludes this folder with an
S3 delimiter, so the state list does not enumerate directory pages.
Older states and readers continue to use the original tree format.

To add the index to an existing state, use its trusted local writer folder:

```sh
layr-desk reindex --config /path/to/vault.json --state-dir /path/to/writer
```

Run this command after the writer stops. Keep the original `writer.json` and
`tree.json` together. The command uses the existing writer credentials and public
recipient. It needs no paper key or private vault key. It does not download or
replace file data, tree objects, or the writer record.

The command uploads eight pages at a time. It publishes the encrypted index marker
only after every page succeeds. It keeps encrypted pages and success receipts in
`browse-<seq>.<attempt>` inside the writer folder. Preserve this folder for retries.
A retry skips only writes that the local journal confirms. The journal binds these
receipts to the bucket, recipient, state and tree hash.

If a response is lost, the command stops with an uncertain-outcome error. An
existing remote object without a local success receipt also stops the command.
Do not delete the journal or overwrite remote objects to force a retry. Compare
the pending ciphertext with the remote object before recovery. The original
backup remains available if the optional index fails.
