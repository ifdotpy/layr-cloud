# Public projects (lead decision, 2026-10-25)

Goal: anyone can read a public project (like a public GitHub repository) without an account or a key. Writes still need access. Private overlays of a public project stay private.

## Mechanism

No new cryptography and no unencrypted objects. A public project has one extra reader slot whose identity is published.

1. `layr remote public <remote> on` (owner only, asks to type the project name) makes a new age identity, seals the project vault identity to it as a slot `slots/public.age` (existing `seal_slot`), and writes that public identity into `public.json` in the store (create once, signed by the owner machine). Anyone can then open the vault, so all current and future data of the project is readable. The command says so in one line.
2. There is no `off` in version 1. Making a project private again needs a new vault key for new data, and the store has no project key rotation yet. `layr remote public <remote> off` refuses and names that limit. Old data stays readable to anyone who copied it.
3. Overlays never get the public slot. Their files stay with their readers. A hidden overlay stays hidden.
4. A reader with only the public identity can clone, browse, diff and build, but cannot push: the writer role and the record signatures still decide writes.

## Gateway

- `GET /p/<project>/...` serves the objects of a project with `public.json` without a session: read only, the same object name allowlist, rate limits per client address.
- Write, GC and token routes never accept an anonymous caller.

## Site

- A public project opens without unlock: the worker loads `public.json` and uses that identity.
- A "Public" badge in the project header. Signing in adds the person's own access (write actions, private overlays).

## Checks

- A clone with only the public identity reads lines, states and files, and cannot push (the refusal is tested).
- Overlay files are absent for the public reader, and a hidden overlay leaves no trace (the rule 8 test with the public identity).
- `off` refuses without any change, names the missing key rotation, and the project stays public.
