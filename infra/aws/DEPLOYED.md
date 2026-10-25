# AWS deployment of Layr Cloud

Measured state: 2026-10-25, 17:21 UTC. Three storage stacks and both public IPv6 hosts are created. Both hosts report SSM Online and have no public IPv4 address. Gateway application installation and live user checks are still pending. Tailscale enrollment is no longer required. Earlier tailnet plans and release gates below are historical.
This file records the M8 deployment from `layr-cloud` base `e4fdf656a0cae5ada5e2e3c7ebaf2fbe372c5f8d`.
The deployment branch is `cloud/m8-deploy`.

## Target and scope

- AWS profile: `ifdotpy`; account: `134250228509`; region: `eu-north-1` (Stockholm).
- Gateway URL: `https://cloud.layr.ws`, public IPv6 HTTPS.
- Existing Route 53 zone: `Z00094863IAG61NP8YENE` (`layr.ws`). No new hosted zone is planned.
- Allowed DNS changes: `cloud.layr.ws` and `_acme-challenge.cloud.layr.ws` only.
- The `layr.ws` apex, `desk.layr.ws`, and the existing ACM validation record must stay unchanged.
- Real projects remain unimported. Verification will use a disposable test project.
- Deployment executor source: the newest Layr main that contains all merged work when hosts deploy.
  Earlier local preparation used `1a91cd32555a878e87d63d6311520ecca3be7173`; that build is not the final deployment pin.

## Resources

Current host inventory:

| Host | Instance | IPv6 | Retained data |
| --- | --- | --- | --- |
| Gateway | `i-04da10c290655544f` | `2a05:d016:df9:4000:8463:e4d6:66bb:c15f` | None |
| Executor | `i-0df29a43f3b810a82` | `2a05:d016:df9:4000:d35f:3f24:8eae:a94` | `vol-0c764a630871fc14c`, 10 GiB |

The VPC is `vpc-0c5b8b1792383ecf5`; subnet `subnet-07f9b77a10c790951` is in `eu-north-1a`.
SSM agent version is `3.3.5538.0`. The gateway permits TCP 443 from `::/0`; the executor has no inbound rules.
Neither host accepts SSH. On first boot, the gateway sync timer reported a missing release manifest;
this is an application installation dependency, not an SSM or IPv6 connectivity failure.

## Live verification path

The administration Mac has no IPv6 connectivity, and `layr-dev` has only an OrbStack ULA.
The approved live check keeps the browser and all disposable private recovery material on `layr-dev`.
It forwards HTTPS through the executor with SSM:

```sh
AWS_PROFILE=ifdotpy AWS_REGION=eu-north-1 aws ssm start-session \
  --target i-0df29a43f3b810a82 \
  --document-name AWS-StartPortForwardingSessionToRemoteHost \
  --parameters '{"host":["cloud.layr.ws"],"portNumber":["443"],"localPortNumber":["18443"]}'
```

The browser must preserve `Host` and TLS SNI `cloud.layr.ws` and verify the real certificate.
The executor connects to the gateway over IPv6. This check proves that gateway path;
it does not prove public access from an IPv4-only network. Such networks need an IPv4 front later,
for example CloudFront or one public IPv4 address. No private test material is sent to AWS hosts.

The storage stacks below reached `CREATE_COMPLETE`. Gateway and executor deployment no longer needs auth-key parameters.
The five-stack plan was approved by the lead on 2026-10-25 at 12:04 UTC:

| Stack | Planned resources |
| --- | --- |
| `layr-bootstrap` | Retained, encrypted, versioned artifacts S3 bucket |
| `layr-store-m8-smoke` | Separate disposable verification store bucket and exact reader, writer and GC roles |
| `layr-store-layr` | Retained, encrypted, versioned project S3 bucket; reader, writer and GC roles; bucket policy |
| `layr-gateway` | IPv6-only VPC and subnet, internet gateway, route table and IPv6 route, HTTPS-only security group, IAM role and instance profile, `t4g.nano` EC2 instance with 8 GiB gp3 root |
| `layr-executor-layr` | No inbound rules; SSM administration; security group, IAM role and instance profile, `t4g.small` EC2 instance with 12 GiB gp3 root, retained encrypted 10 GiB gp3 btrfs volume and attachment |

The hosts use standard CPU credits. No public IPv4 address, NAT gateway, load balancer, or new hosted zone is planned.

The selected AMI is `ami-0309727f887f847cb`, `debian-13-arm64-20261021-2638`.
AWS reports publisher `136693071363`, architecture `arm64`, and root device `/dev/xvda`.
Debian identifies that publisher as its official AWS account:
[Debian 13 EC2 images](https://wiki.debian.org/Cloud/AmazonEC2Image/Trixie).

## Monthly cost estimate

Prices were read from the AWS Price List API on 2026-10-25, in USD before tax.
The estimate uses 730 hours per month. It excludes credits and free-tier discounts.

| Item | Rate | Monthly estimate |
| --- | --- | ---: |
| Gateway `t4g.nano` | $0.0043/hour | $3.14 |
| Executor `t4g.small` | $0.0172/hour | $12.56 |
| 30 GiB gp3 total | $0.0836/GiB-month | $2.51 |
| Base total | | Approximately $18.21 |

S3 bytes, object versions, requests, Route 53 queries, and network transfer depend on usage.
The expected initial total is about $20/month with a small test store. This is an estimate, not a billing cap.
The existing zone's monthly charge is unchanged. Standard SSM parameters and IAM roles add no fixed monthly charge.
Retained buckets and the executor data volume continue to incur charges after stack deletion.

## Preflight evidence

Commands run on the administration Mac:

```sh
AWS_PROFILE=ifdotpy aws sts get-caller-identity --query '{Account:Account,Arn:Arn}'
```

Output: account `134250228509`, assumed role `AWSReservedSSO_AdministratorAccess_55cde48788b30dec`.
No credentials were printed or exported to files.

```sh
AWS_PROFILE=ifdotpy aws cloudformation list-stacks --region eu-north-1 \
  --query 'StackSummaries[?StackStatus!=`DELETE_COMPLETE`].[StackName,StackStatus]'
```

Output: only unrelated stack `arval-telegram-watch`, `UPDATE_COMPLETE`.

```sh
AWS_PROFILE=ifdotpy aws ssm describe-parameters --region eu-north-1 \
  --parameter-filters 'Key=Name,Option=BeginsWith,Values=/layr-store/' \
  --query 'Parameters[].{Name:Name,Type:Type}'
```

Output: `[]`. This command reads metadata only.

`tailscale status --json` reports `BackendState=Running`, MagicDNS enabled,
and tailnet suffix `tailf00fa0.ts.net`.
The administration Mac reports its DNS name as `macbook-pro-lumina.tailf00fa0.ts.net`.

The zone baseline contains apex A/AAAA aliases to `d601z3odhuaeg.cloudfront.net`, its NS/SOA records,
`desk.layr.ws A 100.68.127.7`, and the existing ACM validation CNAME.
It contains no `cloud.layr.ws` record.

## Pending requirements and live verification

1. Lead stack/size/cost approval received before the first CloudFormation deployment. Storage deployment completed.
2. Separate reusable, pre-approved Tailscale auth keys in the two SSM SecureStrings:
   `/layr-store/gateway/tailscale-auth-key` and `/layr-store/executor/tailscale-auth-key`.
   Each key needs its respective `tag:layr-gateway` or `tag:layr-executor` tag.
3. Tailnet grants: members to gateway TCP 443; executor to gateway TCP 443;
   members to executor TCP 5000; administrators to root through Tailscale SSH.
4. Executor write-token input is resolved by the approved tmpfs exception documented below.
5. Deploy and inventory stack resources, then verify host setup, certificate and protected DNS records.
6. Clone and push a disposable project from the administration Mac, open the deployed site,
   unlock with recovery words, and verify its lines. These live checks have not run.

CloudFormation completion alone will not satisfy M8. M8 remains incomplete until the deployed user flow passes.

## Local verification

The build runs use isolated source folders in the existing ARM64 `layr-dev` VM.
No AWS build instance was created.

```sh
npm ci --prefix web
npm run build --prefix web
```

Result: exit 0. Vite built 62 modules, the browser worker, and a 4,158,225-byte WASM module.
The build reports an existing large-chunk warning and two npm audit findings.
Dependencies were left at the repository lockfile versions.

```sh
/opt/layr-cfn-lint/bin/cfn-lint infra/aws/*.yaml
/opt/layr-cfn-lint/bin/python -m unittest discover -s infra/aws -p 'test_*.py' -v
cargo test --locked -p layr-gateway
```

Results in `layr-dev`: cfn-lint exit 0; 23 Python tests passed, including real btrfs loop-volume checks.
Gateway tests: 20 passed, 0 failed, 1 ignored. The ignored case requires a separate MinIO integration setup.
The new tests check static files, MIME types, path escapes, host validation, API separation,
site archive paths, and the memory-only Tailscale auth-key pipe.

```sh
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
CC_aarch64_unknown_linux_musl=musl-gcc \
cargo build --release --locked --target aarch64-unknown-linux-musl --bin layr
```

Result: exit 0 from the exact Layr source revision above. The stripped binary is 15,880,448 bytes.
`readelf` reports AArch64, ELF64 EXEC, and no INTERP header.
SHA-256: `eca08ae13ea998481760117d42d4669a5d250511ada5da60c836dce3f920af52`.
Its `--version` command returns `layr version 0.1.0`.

The same target build for `layr-gateway` passed. Live host evidence remains pending.

The local gateway HTTP smoke check passed on the ARM64 release binary as an unprivileged user.
It served the built production site over a loopback HTTP listener for this check only.
This check does not prove an AWS host or tailnet connection.

```text
GET / => 200; 635 bytes; site CSP present
HEAD / => 200; Content-Length=635; no body
GET /assets/index-DCdY6MOU.js => 200; text/javascript; charset=utf-8; 2092661 bytes
GET /assets/index-BWGCPk8t.css => 200; text/css; charset=utf-8; 32154 bytes
GET WASM => 200; 4158225 bytes; wasm signature verified
GET /api/missing => 404; no SPA fallback
GET /v1/missing => 404; no SPA fallback
GET /missing.js => 404; no SPA fallback
GET /%2e%2e/etc/passwd => 400; no SPA fallback
GET /assets%2f..%2findex.html => 400; no SPA fallback
Wrong Host => 421; direct API/site host boundary preserved
```

The stripped gateway is 5,904,080 bytes, AArch64 ELF64 EXEC, with no INTERP header.
SHA-256: `b458d17c8eb2540086e1c374e62b8caa4b278a103dd9bc80d05e5346072461bc`.

## Prepared release artifacts

These preparation archives remain in `/root/work/m8-release` in `layr-dev`.
They predate the M2/M3/M7 integration and must be rebuilt from the final merged source before deployment.
They contain no AWS credentials or Tailscale auth keys.
They were created with sorted entries, zero timestamps, and numeric root ownership.
The host's `release_files` validator accepted both real archives:

```text
gateway a6c76a64f998662875b0a6d7d38656521521ac33c7a1937e5aada598e92cdd79 11 validated files
executor 3edbdcda0ee3b8b49a0beef1c7a893f68649c44a2f8b9289c1bef90219fdd244 2 validated files
```

The gateway archive contains the static binary, sync script, and nine built site files.
The executor archive contains the pinned static Layr binary and sync script.
The application changes and initial deployment record are in commit `d9ece2632c509981d69a8135af07ac051fbe4d76`.
Those preparation commits stayed local at that stage; later branch publication is recorded below.

## Executor and runner contract

The executor stays `t4g.small`, with no Rust toolchain. It fetches, applies person requests,
scans pending checks with both machine-local runner flags false, pushes, and optionally starts the runner.
All checks run on the optional `t4g.large` runner. Normal unit checks remain unprivileged in bubblewrap
with no network; privileged suites need reviewed local policy and root-only opt-in.
The final static Layr binary must include merged M2, M3 and M7 work.

Person-key pins are machine-local. Before enabling person requests, an administrator must run
`layr --project "$PROJECT" access person-key "$PERSON_LOGIN" "$PERSON_SIGNING_KEY"` on the executor,
using Igor's verified public signing key. The gateway's registered person key does not replace this local pin.
No private person key or recovery material belongs on the executor or runner.

## Tailnet policy to add

Merge these entries into the existing policy's `tagOwners`, `grants`, and `ssh` sections.
Keep unrelated policy entries. The runner entries prepare the later M3 runner.
Both network grants and SSH rules are required for Tailscale SSH.

```json
{
  "tagOwners": {
    "tag:layr-gateway": ["autogroup:admin", "autogroup:owner"],
    "tag:layr-executor": ["autogroup:admin", "autogroup:owner"],
    "tag:layr-runner": ["autogroup:admin", "autogroup:owner"]
  },
  "grants": [
    {"src": ["autogroup:member"], "dst": ["tag:layr-gateway"], "ip": ["tcp:443"]},
    {"src": ["tag:layr-executor", "tag:layr-runner"], "dst": ["tag:layr-gateway"], "ip": ["tcp:443"]},
    {"src": ["autogroup:member"], "dst": ["tag:layr-executor"], "ip": ["tcp:5000"]},
    {"src": ["autogroup:admin", "autogroup:owner"], "dst": ["tag:layr-gateway", "tag:layr-executor", "tag:layr-runner"], "ip": ["tcp:22"]}
  ],
  "ssh": [
    {"action": "accept", "src": ["autogroup:admin", "autogroup:owner"], "dst": ["tag:layr-gateway", "tag:layr-executor", "tag:layr-runner"], "users": ["root"]}
  ]
}
```

Syntax references: [Tailscale grants](https://tailscale.com/docs/reference/syntax/grants) and
[Tailscale SSH](https://tailscale.com/docs/features/tailscale-ssh).
The policy grants no runner or executor access to other tailnet devices.

Igor creates separate tagged, reusable, pre-approved auth keys and puts them directly into SSM SecureStrings.
The initial hosts use `/layr-store/gateway/tailscale-auth-key` and `/layr-store/executor/tailscale-auth-key`.
The later runner also needs `/layr-store/runner/tailscale-auth-key`, tagged `tag:layr-runner`.
Do not send any key in a message.

## Approved runtime credential exception

The lead approved a service-owned mode `0600` regular file under `/run` on tmpfs for gateway write tokens.
The executor token is stored in SSM at `/layr-store/executor/write-token` and loaded into
`/run/layr-executor-token/write-token` by a systemd oneshot service.
Its service user is root, matching the existing privileged Layr daemon.
The runtime directory is mode `0700`. Stopping `layr.service` stops the token service and removes the token and runtime directory.
The transfer service refreshes the token from SSM before each fetch/push.
No token is written to a persistent disk, artifact, command argument, or log.
This exception does not apply to Tailscale auth keys, which still pass through an inherited pipe.

The runner will also receive a project-scoped write token in tmpfs.
It needs the store writer role to publish its own signed attestations and derived outputs through existing signed sync.
Its grant therefore permits new records; it is not read-only.
The bucket policy still forbids overwrites. The writer role has no delete grant.
The lead explicitly selected this existing sync path instead of a new result-return protocol.

The lead removed all executor-to-runner SSH grants. The executor only calls EC2 StartInstances.
The runner performs its own sync/check loop and idle stop. No root SSH is allowed between these machines.

## Created storage resource inventory

All rows below were read back with `aws cloudformation list-stack-resources` after stack completion.

| Stack | Logical ID | AWS type | Physical ID | State |
| --- | --- | --- | --- | --- |
| `layr-bootstrap` | `Artifacts` | `AWS::S3::Bucket` | `layr-bootstrap-artifacts-ik3kxdaezcnd` | `CREATE_COMPLETE` |
| `layr-store-layr` | `Bucket` | `AWS::S3::Bucket` | `layr-store-layr-bucket-mg1n1j6rlomt` | `CREATE_COMPLETE` |
| `layr-store-layr` | `BucketPolicy` | `AWS::S3::BucketPolicy` | `layr-store-layr-bucket-mg1n1j6rlomt` | `CREATE_COMPLETE` |
| `layr-store-layr` | `Gc` | `AWS::IAM::Role` | `layr-store-gc-layr` | `CREATE_COMPLETE` |
| `layr-store-layr` | `Reader` | `AWS::IAM::Role` | `layr-store-reader-layr` | `CREATE_COMPLETE` |
| `layr-store-layr` | `Writer` | `AWS::IAM::Role` | `layr-store-writer-layr` | `CREATE_COMPLETE` |
| `layr-store-m8-smoke` | `Bucket` | `AWS::S3::Bucket` | `layr-store-m8-smoke-bucket-ainpgseboqyh` | `CREATE_COMPLETE` |
| `layr-store-m8-smoke` | `BucketPolicy` | `AWS::S3::BucketPolicy` | `layr-store-m8-smoke-bucket-ainpgseboqyh` | `CREATE_COMPLETE` |
| `layr-store-m8-smoke` | `Gc` | `AWS::IAM::Role` | `layr-store-gc-m8-smoke` | `CREATE_COMPLETE` |
| `layr-store-m8-smoke` | `Reader` | `AWS::IAM::Role` | `layr-store-reader-m8-smoke` | `CREATE_COMPLETE` |
| `layr-store-m8-smoke` | `Writer` | `AWS::IAM::Role` | `layr-store-writer-m8-smoke` | `CREATE_COMPLETE` |

No EC2 instance, public IP, or new hosted zone exists yet. The three empty buckets incur usage-based S3 charges only.

## Later privileged runner plan and duty-cycle cost

The sixth stack is `layr-runner-layr`. It reuses `executor.yaml` with `HostKind=runner`,
`InstanceType=t4g.large`, `DataVolumeSize=60`, `RootVolumeSize=8`, its own role and tailnet hostname,
and runner-specific SSM auth-key and write-token parameter names.
It will deploy only after M3 is merged into Layr main. The executor remains `t4g.small`.
The 60 GiB btrfs data volume holds the project, build caches, Rust toolchain, and loop-filesystem test data.
An additional 8 GiB gp3 root volume holds Debian and host packages.

AWS Price List reports `t4g.large` at $0.0688/hour in Stockholm.
Both disks remain billable while the runner is stopped: 68 × $0.0836 = $5.68/month.
Compute costs $0.0688 × running hours. Running hours include boot/setup, check execution,
and each isolated session's trailing 30-minute idle window.

| Example monthly use | Running hours including idle tails | Runner estimate | Total with $18.21 base |
| --- | ---: | ---: | ---: |
| Provisioned but stopped | 0 | $5.68 | $23.89 |
| 20 check-hours in 20 isolated sessions | 30 | $7.75 | $25.96 |
| 60 check-hours in 30 isolated sessions | 75 | $10.84 | $29.05 |
| Running all month | 730 | $55.91 | $74.12 |

These examples exclude usage-dependent S3 and traffic charges. Standard CPU credits avoid surplus-credit charges.
A host lock prevents idle shutdown during a scan or tool setup. Work, including a failed check,
updates the monotonic last-work time; empty scans do not. A timer requests poweroff after 1,800 idle seconds.
EC2 uses `InstanceInitiatedShutdownBehavior=stop`, retaining both disks and the machine identity.
The executor's grant can start only the configured instance ARN carrying `layr:kind=runner`.
It receives no stop, terminate, or SSH authority over the runner.

The executor runs no checks. Its read-only pending scan wakes the optional runner for normal or privileged work.
The runner syncs, invokes `checks watch --once --json` as the local `layr-checks` reader,
and pushes its signed results. Exit 0 means a completed scan without new failures, 1 means newly failed checks,
and 128 means an operational error. Only a successful JSON scan contributes work/idle evidence.

Policies must be provisioned locally on both hosts: imported config is namespaced by the source machine.
After cloning, grant `layr-checks` reader access and run the project's reviewed `tools/checks/configure.sh` locally.
Set `checks.runner=true` and `checks.privileged-runner=true` locally as root on the runner only. The executor runs the watcher in reporter mode and needs no Rust toolchain.

The final split keeps every check on the runner, including sandboxed unit checks.
The executor syncs, applies person requests, scans pending work without executing it, and optionally starts the runner.
The runner stack can be omitted; a laptop VM may supply the runner role later.
The offline cargo/rustup seed is bind-mounted at `/usr/local/share/layr-checks`, not symlinked out of `/usr`.
A symlink to a hidden `/var` path would not resolve inside the normal sandbox.
The runner bind-mounts its data-volume scratch directory at `/var/tmp`, so check builds and loop files use the 60 GiB disk.
Prewarm locked dependencies from the verified project source with `cargo fetch --locked` before enabling checks.

The reporter uses `checks watch --once --json` with both machine-local runner flags false.
The host loop explicitly enforces both false on the executor before each report scan.
It wakes on the total `pending` count, which includes normal checks.

## Additional verification while host enrollment waits

Storage IAM updates completed successfully on 2026-10-25 at 12:40–12:46 UTC.
They add the optional runner role to the exact trust list and allow create-only `requests/` writes.
The real `layr` project store remains uninitialized.

Latest infrastructure checks: `cfn-lint infra/aws/*.yaml` exited 0; Python tests: 29 passed, 0 failed.
Gateway tests against committed M2 shared source `39711d7` plus M7's shared public module from `75b4111`:
26 passed, 0 failed, 1 ignored. This temporary source combination is test evidence, not the deployment pin.
The final tests will use the merged Git dependency revision.

Anonymous reads use `/p/<project>/<object>` and `/p/<project>/objects?prefix=records/|states/`.
They require a signed public manifest bound to the configured store descriptor.
Object names use the shared anchored record and state parsers; only `store.json`, `public.json`,
`slots/public.age`, record segments and state tree/data objects are exposed.
Listings filter out noncanonical entries and never list overlays, people, requests or private slots.
The limit is 600 requests per real socket client IP per minute, with at most 4,096 tracked clients;
excess requests receive HTTP 429 with `Retry-After: 60`. Forwarding headers do not select the address.
Refusal tests cover missing/invalid publication, private namespaces, traversal and write methods.
Anonymous public access grants no write, GC or token-issuance authority.

## Disposable recovery material

At 12:41 UTC the lead approved a narrow exception for the disposable smoke store and test person:
recovery material may live only under `/run` on tmpfs in the trusted local `layr-dev` VM.
It must never appear on AWS hosts, persistent disks, logs, commits or messages.
It must be deleted after the live UI check. That live check and deletion are still pending;
this file must record the measured deletion result before the task is submitted as complete.
Igor will create real project keys for `layr`, `layr-cloud` and `layr-studio` later on his own machine.
This task creates none of those real project keys.

The extracted token loader and systemd units also passed a lifecycle check in `layr-dev`,
using an SSM stand-in with no live credentials:

```text
SSM stand-in -> extracted runtime loader: tmpfs, root-owned, mode0600; value not emitted
Stop parent service -> PartOf stopped token unit; token and runtime directory removed
```

The parent stop can return while the dependent token stop job is still finishing.
The check waited for that job and verified that both the file and runtime directory were absent.
This proves the local service lifecycle; live instance-role access to SSM remains unverified.

## Live public-writer regression: failed before slot opening

On 2026-10-25 at 13:13 UTC, `tests/public-writer-gateway.py` ran the exact Layr CLI
from M7 commit `75b4111` against the running debug gateway and a real local MinIO server.
An HTTPS trace proxy logged methods, paths, status codes and credential presence only.
It retained no headers or bodies. The gateway itself ran as `nobody` on loopback.

The test initialized a disposable project, pushed it with owner credentials, and published it with
`remote public owner on`. It verified that the signed manifest named this trusted machine and
that its existing machine slot was present. It issued a gateway write token into a mode `0600`
file on `/run` tmpfs, saved a new local state, removed AWS credentials from the client environment,
and invoked `layr push gateway -q`. The setup completed; the push failed with exit 128.

Exact push request sequence, after clearing the setup trace:

```text
GET /v1/p/public-test/store.json -> 200; Authorization absent
GET /v1/p/public-test/store.json -> 200; Authorization absent
GET /v1/p/public-test/store.json -> 200; Authorization absent
CLI error: gateway reads require a machine slot or paper key
```

There was no `/p/` request and no slot, challenge, token or PUT request.
The source calls `backend.get("public.json")` before opening the writer's slot;
with a write token, the M7 gateway client selects `read_request`, whose proof needs an identity
that has not opened yet. The lead received this reproduction and exact trace to decide where the fix belongs.
No client fix was made in this deployment task.

The test removed its disposable store, recovery material, write token, test server data and public certificate.
The TLS private key existed only in a memory-backed file descriptor, which was closed.
Cleanup output:

```text
CLEANUP all disposable recovery/token/test material removed from /run
```

This local live test exposes an M7 client integration failure. It does not replace the pending
AWS/tailnet clone, push and browser recovery verification for M8.

The lead clarified the replacement client's transport contract at 13:15 UTC:
one anonymous `GET /p/<project>/public.json` side request is allowed for the public manifest.
The rest of the push must use authenticated `/v1/p/` routes; every object write must carry the writer bearer.
The regression now checks this exact sequence and compares write bearers only in memory.
No authenticated public-manifest route was added. Rerun awaits the replacement M7 commit.

## Live public-writer rerun: passed with M7 fix

At 2026-10-25 13:25 UTC, the same committed regression (`c410290`) passed with the CLI
built from exact local M7 commit `88e3e227d30af7125b489e34ad343a7c775489ed`.
The commit was fetched from the local M7 worktree, not GitHub. The CLI build exited 0.
The live regression exited 0, verified new state objects in MinIO, and confirmed all three object writes
carried the exact issued writer bearer. The one public side request carried no bearer.
There was no public-only record, state, slot, list, write or token request.

Tested debug binary SHA-256 values:

```text
layr d94de1d040c9790d7e84fb60bba161a529884dd0ab69151bdb0dd0151d767f0a
layr-gateway fc734b376a0b3f963ac3df9471b0251eaf8bff874f36e09ef7fa3715033a734f
```

Exact push request sequence (`auth` shows only a comparison result, never credential bytes):

```text
1. GET /v1/p/public-test/store.json -> 200; auth=none
2. GET /v1/p/public-test/store.json -> 200; auth=none
3. GET /v1/p/public-test/store.json -> 200; auth=none
4. GET /v1/p/public-test/store.json -> 200; auth=none
5. GET /p/public-test/public.json -> 200; auth=none
6. GET /v1/p/public-test/slots -> 200; auth=none
7. GET /v1/p/public-test/challenge -> 200; auth=none
8. POST /v1/p/public-test/token -> 200; auth=none
9. GET /v1/p/public-test/objects?prefix=records%2F -> 200; auth=read-proof
10. GET /v1/p/public-test/objects/records/01a1733c-fc13-708c-815a-1b33c56d30e9/00000000000000000001-00000000000000000006.age -> 200; auth=read-proof
11. GET /v1/p/public-test/store.json -> 200; auth=none
12. GET /v1/p/public-test/objects?prefix=records%2F01a1733c-fc13-708c-815a-1b33c56d30e9%2F -> 200; auth=read-proof
13. PUT /v1/p/public-test/objects/records/01a1733c-fc13-708c-815a-1b33c56d30e9/00000000000000000007-00000000000000000010.age -> 201; auth=writer
14. GET /v1/p/public-test/objects?prefix=states%2F -> 200; auth=read-proof
15. GET /v1/p/public-test/objects?prefix=states%2F -> 200; auth=read-proof
16. GET /v1/p/public-test/slots -> 200; auth=none
17. GET /v1/p/public-test/objects?prefix=records%2F -> 200; auth=read-proof
18. GET /v1/p/public-test/objects/records/01a1733c-fc13-708c-815a-1b33c56d30e9/00000000000000000001-00000000000000000006.age -> 200; auth=read-proof
19. GET /v1/p/public-test/objects/records/01a1733c-fc13-708c-815a-1b33c56d30e9/00000000000000000007-00000000000000000010.age -> 200; auth=read-proof
20. GET /v1/p/public-test/objects/states/01a1733c-fc82-75ff-b8dc-a460de6efb49/f2e34c48.tree.age -> 200; auth=read-proof
21. PUT /v1/p/public-test/objects/states/01a1733c-ff90-7119-899e-d1b338b35c4f/e8ec84b2.0000.data.age -> 201; auth=writer
22. PUT /v1/p/public-test/objects/states/01a1733c-ff90-7119-899e-d1b338b35c4f/e8ec84b2.tree.age -> 201; auth=writer
23. GET /v1/p/public-test/store.json -> 200; auth=none
PASS trusted writer pushed a new public-project state through authenticated gateway transport
CLEANUP all disposable recovery/token/test material removed from /run
```

`POST /v1/p/public-test/token` exchanges the encrypted challenge proof in its request body;
no bearer is expected on that bootstrap exchange. Subsequent reads use the resulting read proof.
This resolves the earlier local M7 writer integration failure. AWS/tailnet deployment and browser
verification remain pending; this test created no AWS resources and no real project keys.

## Final shared dependency pin and executor index hook

The GitHub main ref resolved to `b337229691083d2738ebc6db4bdd48c0b9ede3c5` at 13:33 UTC.
All `layr-store` and `layr-model` dependencies now use that exact revision; the lockfile has one shared store package.
This main includes M2, M3, the M7 writer fix and signed store indexes.

Both executor transfer paths run `layr --project "$PROJECT" index write origin` after a successful push.
The fetch/apply/check-report/push loop publishes the index only after the successful cycle.
Failures propagate; a failed push does not write an index. The optional runner does not publish indexes.
The gateway supports canonical immutable `index/heads/<machine>/<20-digit-seq>.age`,
`index/paths/<state>.age` and `index/graph/<state>.age` object names on authenticated object routes.
Reading indexes requires the normal read proof; creation requires the writer bearer; overwrite and GC delete are refused.
Anonymous index routes remain excluded. The project IAM template adds create-only `index/*` writes;
that policy update is prepared and has not yet been deployed to the storage stacks.

Pinned-main verification on 2026-10-25 at 13:38 UTC:
workspace all-target tests passed (50 passed, 5 environment-dependent tests ignored);
the final gateway test run including the index grant boundary passed (27 passed, 1 ignored).
`cfn-lint` exited 0 and all 30 Python infrastructure tests passed.
The exact pinned-main CLI built successfully. The live public-writer and index regression exited 0:
one anonymous public-manifest GET, all object writes authenticated, and new state bytes confirmed in MinIO.
The index command then published five immutable objects and read them through normal read proofs:

```text
PUT /v1/p/public-test/objects/index/graph/01a17348-f050-7145-829d-b4ae6ac88937.age -> 201; exact writer bearer present
PUT /v1/p/public-test/objects/index/graph/01a17348-f2a9-766c-8334-c7a0b9611ed5.age -> 201; exact writer bearer present
PUT /v1/p/public-test/objects/index/paths/01a17348-f050-7145-829d-b4ae6ac88937.age -> 201; exact writer bearer present
PUT /v1/p/public-test/objects/index/paths/01a17348-f2a9-766c-8334-c7a0b9611ed5.age -> 201; exact writer bearer present
PUT /v1/p/public-test/objects/index/heads/01a17348-f026-7402-bb19-2b38907c07fe/00000000000000000001.age -> 201; exact writer bearer present
PASS executor index write published immutable signed index objects through gateway
CLEANUP all disposable recovery/token/test material removed from /run
```

The browser production build with the final dependency pin also passed:
`npm run build --prefix web` exited 0, including the WASM build, TypeScript check and Vite build.
Vite built 62 modules and the 4,316,443-byte WASM module (measured from the built file).
The existing large-chunk warning remains.

## Branch publication

The lead confirmed that the initial no-push restriction was his and that the user authorized
publication of verified work. At 13:40 UTC he pushed `cloud/m8-deploy` only.
A readback with `git ls-remote origin refs/heads/cloud/m8-deploy` returned
`b45d7c630ae0dbedcc13a7ff011cd23f50b42f1e`, matching the local HEAD at that time.
The verified gateway, dependency pin and executor index changes are in `d52da3f`.
Both web workers received the published branch and route contracts.
Further verified pushes to this branch are authorized; main pushes and merges remain excluded.
Static ARM64 release builds for the final dependency pin have started. Host deployment still waits
for the two Tailscale SSM parameter names, and the runner additionally needs its own tagged key.

## Static ARM64 preparation artifacts at b337229

Both release builds completed successfully from the pinned source and current gateway changes.
The stripped Layr binary reports `layr version 0.2.0`. The real host archive validator accepted both archives;
its static ELF checks verify AArch64 with no dynamic interpreter. `readelf` also confirmed those properties.
Artifacts are in `/root/work/m8-release-b337229` in the trusted local `layr-dev` VM:

| Kind | Binary SHA-256 | Archive SHA-256 | Validated files |
| --- | --- | --- | ---: |
| Gateway | `1bdbe10a98637451184bd06be7806c3d05f70cb3aa92303f9425aadfd9f1f8da` | `47dcf23c91fbd5109a1eaf29bf054067ab576dba902d5a043000e950fb07fca0` | 11 |
| Executor | `80355507d7727490327fa27099eb60ed0d14b6733a6daac50537eb199ca563f7` | `c2e79779db81efaead96c7df170189193e1a95a2594f7d5772e9d0089dfe0136` | 3 |

The gateway archive includes the site built with the b337229 shared dependencies.
The executor archive includes the static CLI, sync script and host check/index loop.
An actual HTTP smoke check of the stripped release gateway as `nobody` served the new production site:

```text
GET / => 200; 635 bytes; site CSP present
HEAD / => 200; Content-Length=635; no body
GET /assets/index-CO9mJLnI.js => 200; text/javascript; charset=utf-8; 2092661 bytes
GET /assets/index-BWGCPk8t.css => 200; text/css; charset=utf-8; 32154 bytes
GET WASM => 200; 4316443 bytes; wasm signature verified
GET /api/missing => 404; no SPA fallback
GET /v1/missing => 404; no SPA fallback
GET /missing.js => 404; no SPA fallback
GET /%2e%2e/etc/passwd => 400; no SPA fallback
GET /assets%2f..%2findex.html => 400; no SPA fallback
Wrong Host => 421; direct API/site host boundary preserved
```

## Deployment hold for encrypted person requests

At 13:47 UTC the lead required a new v2 request object encrypted to the current project recipient.
The outer person signature covers `layr/request/v2\0 || ciphertext`. The gateway will validate that
signature and the person, project and path bindings without reading the request plaintext.
The native shared v2 implementation is assigned separately. Gateway integration will follow its exact commit/API.
The gateway must refuse v1 person request bodies once v2 is integrated. Claims keep the current contract.

The b337229 archives above are preparation artifacts and contain the old v1 request route.
**Do not deploy those old gateway bytes.** The lead refined this hold at 13:49 UTC: hosting may deploy before v2
with every person request and claim write disabled. The gateway must be rebuilt with the refusal guard below.
No v1 person request route has been deployed to AWS; both project stores remain uninitialized.
The later epoch gateway extension also waits for its assigned merged main revision and client contract.

## Hosting release with request writes disabled

The lead's 13:49 UTC refinement permits deployment at b337229 before v2 when the keys arrive first.
The gateway now returns HTTP 503 for every non-read request below authenticated `objects/requests/`,
including machine claims, malformed names and encoded namespace separators. The response is:

```text
Person requests and claims are disabled until encrypted request support is ready.
```

This guard has no runtime configuration override. It runs before request body parsing or the legacy v1 validator.
A test checks PUT, POST, PATCH and DELETE with no bearer, a writer bearer, a GC bearer and a read proof,
then confirms no request object was created and ordinary store writes still work.
The old positive v1 request/claim test is explicitly ignored pending replacement by encrypted v2 tests.
Person registration, hosting, public reads and authenticated index publication remain available.

The gateway binary and archive must be rebuilt after this guard. The earlier gateway digest in the preparation table
is superseded; the new digest and test output will be recorded when the rebuild completes.
The executor binary and archive are unchanged. When shared v2 arrives, replace the legacy validator and refusal guard
with v2 verification, reject v1, update tests and repin before enabling request writes.

The disabled-write fallback release completed its checks at 13:55 UTC:
`cargo test --locked -p layr-gateway` passed 27 tests with 2 explicit ignores
(MinIO environment setup and the obsolete positive plaintext-v1 flow).
Static ARM64 build exited 0. The replacement binary SHA-256 is
`7ab8b9d77ae4e81b05d22502b241480615abfa4cc751f5a4d9e0907128de588a`.
The replacement gateway archive SHA-256 is
`4c772ce4837d7240e5c88f17da14a8db5c3ca98f6db4d4ac0e33c7c3be892f12`;
the host validator accepted its 11 real files.

An actual HTTP test of the stripped release binary as `nobody` confirmed:
PUT, POST, PATCH and DELETE each return 503 on both person-request and claim paths.
The same process served HTML, JavaScript, CSS and the 4,316,443-byte WASM with 200,
HEAD with an empty body, traversal with 400 and wrong Host with 421.
This guarded gateway archive can be used for hosting at b337229 if the keys arrive before v2.
The earlier unguarded gateway archives must not be deployed.


## Encrypted request v2 integration at 466f120

At 15:12 UTC, `git ls-remote https://github.com/ifdotpy/layr main` returned
`466f120f0846da86f63028bbec9eabb2fa69581f`, the lead-approved main with request v2.
All shared `layr-store` and `layr-model` dependencies and the release CLI are pinned to that hash.
The v2 build removes the temporary request-write HTTP 503 guard. Earlier guarded and unguarded
b337229 artifacts are superseded by the verified v2 artifacts recorded below.

The gateway accepts only the shared v2 sealed envelope for person request writes. It verifies
`layr/request/v2\0 || decoded ciphertext` against the stored person signing key and binds the
outer person, project and request UUID to the authenticated tailnet login, descriptor and path.
It stores the opaque original body with a conditional create. It does not decrypt or parse the action.
Signed plaintext v1 bodies, altered ciphertext, invalid signatures, wrong caller/project/path,
unknown fields and oversized envelopes are refused. Root requests are supported; non-root epoch
requests remain refused until the separately assigned gateway epoch integration.
Claims retain the existing write-bearer, structural and project/path checks. The executor verifies
machine trust and signatures. Its machine-local person-key pin and access policy remain mandatory.

The regression test can run all three native caller phases with:

```sh
LAYR_TEST_REQUEST=1 LAYR_TEST_INDEX=1 \
  LAYR_BIN=/root/work/m8-release-466f120/executor/layr \
  LAYR_GATEWAY_BINARY=/root/work/m8-release-466f120/gateway/layr-gateway \
  /opt/layr-cfn-lint/bin/python tests/public-writer-gateway.py
```

The disposable project, person recovery material, tokens and TLS test material stay in `/run`
tmpfs on the trusted local VM. The test deletes them on completion. No real project keys are made.
The local test uses MinIO and a Tailscale LocalAPI identity stand-in; it does not prove live AWS
or real tailnet enrollment. Those M8 checks still require the Tailscale keys.


## Public IPv6 gateway transition

The lead's design `docs/public-gateway-design.md` at `f8e5e00` replaces the tailnet requirement.
The worktree was rebased onto that origin/main before implementation. The approved base cost
and instance sizes remain unchanged: gateway t4g.nano, executor t4g.small, approximately $18.21/month
plus usage. There is no public IPv4 address or NAT gateway.

The gateway security group permits only TCP 443 from `::/0`. The executor has no inbound rules.
Both hosts install Amazon SSM Agent, attach `AmazonSSMManagedInstanceCore`, and enable its
`Agent.UseDualStackEndpoint` option. Administration uses SSM Session Manager or Run Command;
no SSH port or Tailscale key is required. The gateway publishes only the AAAA record for
`cloud.layr.ws` and the ACME DNS challenge allowed by its scoped Route 53 policy.

AWS source for the SSM setup: [Debian agent installation](https://docs.aws.amazon.com/systems-manager/latest/userguide/agent-install-deb.html)
and [AWS's IPv6 agent configuration](https://github.com/aws/amazon-ecs-agent/blob/master/scripts/ecs-anywhere-install.sh).
The package download uses the regional S3 dualstack endpoint. No inbound SSM port is needed.

The Mac has no IPv6 and layr-dev has only an OrbStack ULA with no internet IPv6 route.
Live IPv6 checks will therefore run from the executor through SSM. From networks without IPv6,
the site needs an IPv4 front later, such as CloudFront or one public IPv4 address.

Local implementation verification at 16:50 UTC: gateway tests 28 passed, 0 failed, 1 environment-dependent
MinIO test ignored; infrastructure tests 32 passed; cfn-lint exited 0. The actual new gateway binary
served both HTTPS HTTP/1.1 and HTTP/2 over `::1`, with a verified disposable TLS certificate.
The same listener refused an IPv4 connection, confirming `IPV6_V6ONLY`. Its disposable TLS key lived
only in a memfd; the descriptor was closed and tmpfs fixture removed. This is local protocol evidence,
not a claim of AWS deployment or real ACME issuance.


## Gateway host created

`layr-gateway` reached CREATE_COMPLETE at 16:56 UTC from infrastructure commit `afa8573`.
The deployed template SHA-256 at that point was
`ea809ed76b974de9a27c3e3f4ae335a47390c401fd4a095482f95935043f86b4`.

```text
InstanceId i-04da10c290655544f
InstanceType t4g.nano
State running
IPv6 2a05:d016:df9:4000:8463:e4d6:66bb:c15f
PublicIPv4 null
VpcId vpc-0c5b8b1792383ecf5
SubnetId subnet-07f9b77a10c790951
AvailabilityZone eu-north-1a
RouteTableId rtb-07dfadc754796fa35
RoleArn arn:aws:iam::134250228509:role/layr-gateway
SSM PingStatus Online
SSM AgentVersion 3.3.5538.0
```

SSM command `215507be-ae40-45fb-a5eb-512ea41f582e` succeeded: SSM agent and the release-sync timer
are active; an HTTPS request using `curl -6` to the regional S3 dualstack endpoint returned 307.
Cloud-init reported a scripts-user error and early recoverable IPv6 metadata route warnings.
The final release and gateway configuration have not yet been published, so service readiness and
ACME are not claimed. The failing bootstrap step will be inspected and the sync rerun after publication.


## Public signature gateway implementation

The shared signature helper is now on merged Layr main `2007480315e6bb48a06a686e8badaa4dc5266cbd`.
The cloud crates are repinned to it. The gateway imports its canonical session message format;
there is no independent signature-byte implementation in the gateway or browser.

The gateway uses a random 32-byte, one-minute, single-use challenge and a login-bound 12-hour session.
Only token hashes are retained in RAM. Session identity replaces the network login on every person
route. Registration requires an unused seven-day invite for that login and does not grant a session
until the new key proves a fresh challenge. Existing registrations cannot be overwritten.
Unlock and sign-in routes share a 12/minute socket-address limit; all routes have a 600/minute limit.
The bootstrap has one fixed-size base64 field (65536 decoded bytes), with no person index or inbox.
Known and unknown logins return the same status, outer keys and response size. Unknown decoys use a
persistent HMAC key in `/layr-store/gateway/session-secret`, fetched only into gateway memory.
No secret or raw token goes in source, a configuration file, a log or a command argument.

The administrator's invite command sends the raw invite directly to a new SecureString and prints
only its parameter name. Service write-token issuance supports the same memory-to-SSM path.
The gateway role can read only the decoy secret and can write only invite and the exact service-token
parameters. The executor can read only its own service write token; explicit IAM denies narrow the
broader parameter-read grant in the SSM managed agent policy.

The last workspace test with the development source override passed 52 tests, 0 failed, 5 ignored;
the gateway portion passed 28 tests, 0 failed, 1 ignored. Template lint and all 32 then-current Python
tests passed. The exact-main locked gateway check then passed 28 tests, 0 failed, 1 ignored; cfn-lint exited 0
and all 33 infrastructure tests passed, including persisted-secret handling.
SSM bootstrap inspection confirmed the gateway sync failure is `NoSuchKey` for the not-yet-published
release manifest, not an IPv6 or SSM connectivity failure. Timer retry remains active.
