# Shared work and review: proposal

Status: proposed design. This document does not describe a deployed service.
The current vault remains an archive of states from layr.

## Boundary

The gateway and S3 keep ciphertext. They do not receive a vault identity.
Clients decrypt files. A user can also give a trusted executor access to one vault.
That executor can run merge, search, and checks. This access is explicit, not a gateway capability.

The first release targets shared projects, change review, and checks.
It does not require public hosting, issue tracking, or a replacement for every GitHub feature.

## State and branch

A new shared state has an immutable identifier, parent identifiers, and a reference to its encrypted tree.
Its signed record identifies its author and format version.
The server can see identifiers and authorization metadata. Paths, file content, and review text stay encrypted.
This metadata disclosure needs acceptance before the shared protocol is implemented.

A branch is a name that refers to one state.
A branch update names its expected previous state and a unique request identifier.
The service changes the branch only when the expected state still matches.
A repeated request returns its original result. It must not create another update after a lost reply.

Two writers can publish independent states without a common sequence counter.
Only one conflicting branch update succeeds. The other writer fetches the new head and prepares another result.
The current archive sequence and random attempt suffix do not provide this concurrency contract.

## Publish order and recovery

1. Upload immutable encrypted data and tree objects.
2. Publish the signed state record after all referenced objects exist.
3. Update the branch with its expected previous state.

The service verifies the actor, signature, object references, and branch policy before the update.
The client verifies the decrypted file content against the tree hashes.
After a failed branch update, the state remains available by identifier.
Objects without a state record remain candidates for later garbage collection.
A retention period must protect pending uploads and retries.

Existing archive objects remain readable. A future shared-state record can refer to their immutable objects.
The archive sequence remains an import identifier, not a branch or shared commit identifier.
No data migration is part of the current range-read change.

## Review and checks

A review request names an exact source state and destination branch.
An approval names that state, the destination head, and the applicable policy revision.
A changed source or destination requires a new merge result and any checks that the policy requires.

A trusted executor creates the merge result and runs checks on that exact result.
Each signed check result names the state, command configuration, executor identity, and outcome.
The service accepts results only from executors allowed by the project policy.
A contributor cannot authorize their own check result by uploading an unsigned success value.

The final branch update checks the expected destination head again.
A concurrent merge causes a conflict response, not an overwrite.
Comments and review events have immutable identifiers and can refer to a path and line in an exact state.

## Access

The service authorizes read, publish, review, and branch-update actions separately.
The current vault key can decrypt the whole vault. It cannot enforce permissions for individual directories.
Projects that need separate confidentiality boundaries need separate vault keys or a new key scheme.

Removal of a key slot does not revoke a key that a device already copied.
Revocation of future access requires a policy change and a key-rotation design.
Old ciphertext cannot become secret again to a device that already has its key.

## Delivery order

1. Keep the current object format and add range reads for native clients.
2. Add the same access pattern to the browser through the planned Rust/WebAssembly reader.
3. Add immutable shared-state records and conditional branch updates.
4. Add review requests, exact-state approvals, and trusted check results.
5. Add sparse writable workspaces and indexes for history and search when measurements justify them.

## Acceptance cases for shared writes

- Two writers update the same expected head. Exactly one succeeds.
- A successful update loses its response. A retry returns the same result.
- A referenced object is missing. The state cannot become a branch head.
- The destination changes during checks. The merge cannot replace that newer head.
- A revoked writer or untrusted executor submits a validly signed record. The policy rejects it.
- A changed source reuses an old approval. The merge is refused.
- A client opens an old archive after the upgrade. Files and hashes remain identical.
