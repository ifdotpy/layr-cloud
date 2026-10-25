# Access modes and project key rotation (lead decision, 2026-10-25, required)

Every project and every overlay has one access mode. The mode says who can decrypt it.
All three modes use the same mechanism: reader slots that seal the vault identity. No mode adds cryptography.

## Modes, from strict to open

| Mode | Who reads | How |
|---|---|---|
| `private` | Only the people and machines that hold a slot | Slots for readers only. The service has no slot. |
| `service` | Readers, and the Layr Cloud service | Also a slot for the service identity. Server features become possible: search, mail with diffs, integrations, server review. |
| `public` | Anyone | Also a published slot (`public.json`, docs/public-projects-design.md). |

Order: `private` is stricter than `service`, and `service` is stricter than `public`.

## Overlay rule

An overlay has its own mode. **It can be the same as the project mode or stricter, never more open.**

| Project mode | Allowed overlay modes |
|---|---|
| `public` | `public`, `service`, `private` |
| `service` | `service`, `private` |
| `private` | `private` |

- The default overlay mode is `private` in every project. Secrets stay with their readers unless someone chooses otherwise.
- `layr overlay mode <name> <mode>` refuses a mode that is more open than the project mode, and names the project mode.
- When a project becomes stricter, each overlay that is now more open than the project is made stricter too, in the same operation. The command lists them.
- A hidden overlay (docs/overlay-sync-design.md, rules 1 to 8) is always `private`.

## Changes of mode

- Opening (`private` to `service` to `public`) adds a slot. It needs no new key. It applies to all existing data, and the command says so.
- Closing (`public` or `service` to something stricter) needs a **project key rotation**. The new epoch has no slot for the removed audience. Data from earlier epochs stays readable to whoever already had it, and the command says so in one line.

## Project key rotation (the same model as overlay epochs)

- The project vault has epochs. Each epoch has its own identity. Records, states and slots of an epoch are encrypted to its identity.
- `layr remote rotate <remote> [--remove <principal>...] [--reason <text>]` (owner) makes a new epoch with slots for the remaining readers, and seals each earlier epoch identity into the new epoch. A current reader can read all history. A removed reader can read nothing written after the rotation.
- The signed epoch record names the machine, time, reason and the reader list hash. The site shows epochs on Settings > Access.
- Removing a person or a machine from a project always rotates.
- Readers fetch the new epoch with their own slot. Nothing else changes for them.

## Checks

- A removed person can read data written before the rotation and cannot read data written after it.
- `public` to `private`: after the rotation, an anonymous reader gets nothing new. `public.json` is removed for the new epoch.
- The overlay mode rule: a more open overlay mode is refused, and a stricter project mode makes overlays stricter at once.
- A hidden overlay stays `private`, and the rule 8 byte-for-byte test still passes.
