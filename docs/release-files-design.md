# Release files (lead decision, 2026-10-26)

Goal (M4): a `release/<v>` ref shows its built files on the Releases page, and anyone who can read the project can download them. This replaces GitHub release assets.

## Mechanism

No new cryptography. Files are encrypted like states, to the project vault identity of the current epoch.

1. `layr release attach release/<v> <path>... [--name <name>]` on a trusted machine.
   - Each file becomes one object `files/<uuid>.age`. The name is a random UUID v7, not a content hash, so equal files in two projects cannot be linked.
   - One signed record `release.files` holds: the ref name, the exact state the ref points to, and for each file its name, size, plaintext SHA-256 and object key.
   - The command refuses when the ref does not point to that state any more, or when the protection rule for `release/*` does not allow the machine or person.
   - A later `release.files` record for the same ref and state replaces the list. The objects stay until GC.
2. `layr release files release/<v>` lists the files. `layr release get release/<v> [<name>] [-o <dir>]` downloads, decrypts and checks the SHA-256.
3. GC keeps every object that a `release.files` record of an existing `release/*` ref names. A deleted ref frees its files.
4. Limits: 512 MiB per file, 64 files per release. The CLI streams. The site refuses files over 256 MiB and shows the CLI command instead.

## Gateway

- `files/<uuid>.age` joins the object allowlist: read with a read token, create-once PUT with the write grant, public read for a public project.
- No other route changes.

## Site

- Release page: a Files section with name, size and a Download button. The worker downloads, decrypts and checks the SHA-256, then offers the file as a Blob. On a mismatch it shows an error and saves nothing.
- Releases list: a file count column only when at least one release has files.
- A public project downloads with the public identity.

## Checks

- Attach, list, get round trip with SHA-256 check, through file://, s3:// with MinIO, and the gateway.
- A changed or truncated object is refused by `get` and by the site.
- Attach refuses a moved ref and a ref outside the protection rule.
- GC keeps attached files and frees them after `layr ref delete release/<v>`.
- A public reader downloads a file of a public project; a reader without access gets nothing.
- Browser: download of a 10 MiB file, the SHA-256 error path, and the over-limit note.
