# layr-desk

End-to-end encrypted copies of projects in your own AWS accounts, with a web view (Desk) and a
read-only network disk. Only your devices decrypt: storage, gateway and AWS see ciphertext.

- **Writer:** sends each new project state to S3, encrypted to the vault's `age` recipient. It
  sends only the changes: file deltas against the previous version and tree deltas.
- **Vault:** one S3 bucket per project, in the AWS account of the project's owner. A passkey
  (WebAuthn PRF) or a paper key of 24 words opens it.
- **Gateway:** one small server in a tailnet that serves the Desk site and every vault. It reads
  the buckets through roles that each account grants it.
- **Desk:** a Finder-like site that decrypts in the browser.
- **Network disk:** `layr-desk mount` shows every vault as a read-only disk in Finder and decrypts
  on the Mac.

The design, the key model and the AWS setup are in [docs/design.md](docs/design.md).

## Layout

| Folder             | What                                                                       |
| ------------------ | -------------------------------------------------------------------------- |
| `crates/desk-core` | Rust library: keys, slots, paper words, the object format and the reader   |
| `crates/layr-desk` | the `layr-desk` command: the gateway, and read, verify and mount vaults    |
| `js/vault`         | keys, reader, gateway, CloudFormation templates and the `vault-*` commands |
| `web`              | the Desk site                                                              |
| `python`           | the writer and its S3 client                                               |
| `docs`             | design                                                                     |

Status: the logic moves to Rust (one library for the writer, the gateway, the network disk and,
through WebAssembly, the browser). The JavaScript and Python code stays until each part has a
Rust replacement.

## Tests

```sh
pnpm install
cargo test
pnpm test                                   # library and gateway tests, then the site e2e
cargo build -p layr-desk && pnpm --filter layr-desk-web run test:e2e:rust   # e2e on layr-desk serve
python3 -m unittest discover -s python -p 'test_*.py'
```
