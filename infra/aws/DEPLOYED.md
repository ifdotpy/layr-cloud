# AWS deployment of Layr Cloud

## M1 preparation, 2026-10-26

Preparation branch: `cloud/move-projects`, from accepted main `4f3eb35`. No real project was imported or enrolled. The active gateway and executor releases remain the CloudFront releases recorded below. The migration procedure is [move-projects.md](../../docs/move-projects.md).

### Store and host IAM

The original `layr-store-layr` stack received epoch and release-file permissions through change set `m1-epoch-release-4f3eb35`. Only Writer and Gc changed, both Modify with Replacement=False. Its prefix and trust parameters were retained.

Two stores were created from the same unchanged `template.yaml` (source `4f3eb35`):

| Stack | Prefix | Bucket |
| --- | --- | --- |
| `layr-store-layr` | `layr` | `layr-store-layr-bucket-mg1n1j6rlomt` |
| `layr-store-layr-cloud` | `layr-cloud` | `layr-store-layr-cloud-bucket-rqsdhssfnnca` |
| `layr-store-layr-studio` | `layr-studio` | `layr-store-layr-studio-bucket-8xbg6olreskn` |

Both new stacks reached CREATE_COMPLETE using change set `m1-create-4f3eb35`. Each has exact reader/writer/GC roles `arn:aws:iam::134250228509:role/layr-store-{reader,writer,gc}-{prefix}`. Their trust names only the gateway and executor host roles. No instance, volume, hosted zone or KMS key was added. Encryption is SSE-S3 AES256. The two empty buckets and their IAM roles add **$0 fixed monthly cost**, reported to the lead before creation; storage and requests are usage charges. [AWS S3 billing](https://docs.aws.amazon.com/AmazonS3/latest/userguide/GetStartedWithS3.html).

The gateway and executor now assume the exact project roles for the three stores. Change sets `m1-iam-387c250355224473` (gateway) and `m1-iam-312ca3d1f8841c05` (executor) reached UPDATE_COMPLETE after inspection showed only Role Modify, Replacement=False. The executor's GetParameter allow list and explicit-deny exceptions include the exact three names `/layr-store/executor/projects/{layr,layr-cloud,layr-studio}/write-token`, plus the existing single-project parameter. These real-project parameters have not been created.

`project_iam.py` fetches the live original template, preserves all non-Role resources and current parameters, and refuses execution unless every resource change is Role Modify with Replacement=False. Its later source revision also prepares nine exact gateway PutParameter targets (owner, executor and runner for each project). That final gateway permission extension and its matching gateway CLI change are **prepared, not applied**. The live gateway still uses the legacy token parameter names.

### Executor template drift and planned maintenance

The rejected full-template executor plan `cloudfront-f04484382fc364fa` proposed Instance and DataAttachment modifications with conditional replacement. It was not executed. The approved IAM updates used the fetched live template and left both resources unchanged.

The exact pre-existing UserData difference is in the fallback `/usr/local/sbin/layr-executor-transfer` program. The repository adds these three lines between fetch and push; the live CloudFormation template omits them:

```sh
if [ '${HostKind}' = executor ]; then
  /opt/layr-executor/current/layr --project "$PROJECT" requests apply --remote origin --json >/dev/null
fi
```

The epoch rollout installed request application and service configuration through SSM, so the running software and the stack's historical UserData are separate deployment records. The new multi-project runtime remains uninstalled. It has one public profile and one root-owned 0600 tmpfs grant per project, a single timer, and failure isolation for malformed profiles, token reads and project commands. The legacy single-project path remains available.

- [ ] In a planned maintenance window, reconcile `executor.yaml` UserData with the live stack and installed service programs. Review the new change set's instance and volume-attachment effects with the lead before execution. Do not accept conditional replacement as an IAM update.
- [ ] After igor's owner steps, publish the validated executor release, enroll only completed projects, install the rendered daemon drop-in/profiles/single timer, and record its live three-project behavior. The drop-in removes the legacy global-token dependency; the new tick refreshes and removes each project token independently.

### Disposable network rehearsal

Synthetic project `01a177aa-a7a5-716c-8774-7f165724c16b`, alias `m1-move`, used only `layr-store-m8-final` and a new APFS RAM disk `/Volumes/LayrM1Smoke`. Source Git history has five commits including a merge. Core `c1d7229` imported it into an encrypted local seed; agents read only this disposable paper key on RAM. No real project paper key or recovery words were created or read.

The public executor identity was trusted through an encrypted machine slot. SSM enrollment command `82c7f0cb-2a62-40c0-81db-b3a7edccfe2f` cloned 21 records and two states through public CloudFront, applied reporter-only policies, pushed, wrote its index and passed fsck. The executor received no paper key.

A fresh, independently trusted Mac clone used `layr+https://cloud.layr.ws/m1-move`, then pushed eight records, one state and one file from that same clone; fsck reported zero problems. The published Chromium site over forced IPv4 passed recovery sign-in, one signature session, an encrypted v2 comment, three viewports, zero axe violations, lock and no storage writes. Request `01a177bf-15ea-7d21-962e-bf9c7e369fd6` was applied exactly once as executor record `01a177bf-2378-76b1-bd23-2f3133dda71f`, digest `555f6a2e52b8da46be6c1844999adc40f08b8bb349d5fde6b98bcdff20bc91d4`. Reapplication returned ignored; SSM proof `5ee917eb-1c42-46de-b6b2-3a2f1d06e112`.

A second synthetic person `m1-ui-7d619c@example.test` exercised the actual Join with an invite form, browser key creation, saved-word confirmation, registration, signature sign-in and lock over Mac IPv4. Its recovery material and invite were written only to the owned RAM disk; browser storage stayed empty. `web/tests/register-deployed.mjs` refuses real login domains and records a mutation attempt before submitting, so a lost response cannot cause a blind retry.

The tracked ciphertext publisher and public hash persistence helpers ran against this disposable alias. The per-command SSM grant loader verified the Mac RAM disk, fetched through CloudFront and removed its private token file. Linux tests covered grant cleanup on caller failure. Runtime tests covered all three profiles, later projects after a failed push, malformed-profile isolation and systemd unit parsing. Latest checks: gateway 39 passed, one environment-dependent MinIO test ignored; infra lint and 68 tests passed.

**Limits and gates:** fresh Cloud clone export to an empty local Git mirror succeeded as a command but retained only one of the source's five commits. Both main trees were `70958e525f01db2eee70a881b32c735c89590e98`. Full history and original commit identity remain blocked on git-history task `work-1793009127566503168-7`; this rehearsal does not prove that gate. Real store creation waits on reviewed paper-tty (`--paper-key-tty`, private-terminal final-word confirmation). Real person pin acceptance waits on site-accept task `work-1793010263212975104-9`; no real person-key file is an allowed substitute.

Runner receipts and final cleanup are still pending. The synthetic fixture remains for the Mac and Linux runner checks; do not treat this preparation section as the completed project move.

## CloudFront deployment, 09:54 UTC

Measured state: 2026-10-26, 09:54 UTC. `https://cloud.layr.ws` serves IPv4 and IPv6 through CloudFront. The guarded gateway includes epoch and streaming release-file routes. Live sign-in, clone/push, encrypted comments, rate limits, 30 MiB object transfer and 128 MiB release transfer passed. The disposable project, person, invite, service grant and private RAM disk were removed. No real project has been imported.

Deployment branch: `cloud/cloudfront`. Gateway production source: `6701b3aa06ce3d41f95f1c6a49f9dac8e4d5c002`, merging epoch main `0c3807f` and release-files main `d349a4f`. Shared gateway crates pin `b41642ab1f4ba4591072886f7ea8153ffced2c57`. The executor retains exact core `01038b931575e1880fbc7b5829fec649720886f1`; the successful fresh Mac clone/push and release transfer used core `c1d7229`.

## Current front and release

| Item | Measured value |
| --- | --- |
| Distribution | `E3SU1MYTKJEE06`, Deployed, enabled, PriceClass_100 |
| Distribution name | `d960l4aujuosg.cloudfront.net` |
| Public DNS | `cloud.layr.ws` A and AAAA aliases to this distribution |
| Origin | `origin.cloud.layr.ws` AAAA `2a05:d016:df9:4000:8463:e4d6:66bb:c15f` |
| Origin connection | IPv6 only, HTTPS only, TLS 1.2, certificate verified; response timeout 120 seconds |
| Viewer certificate | ACM us-east-1 `c354e7b8-747f-4ab9-9524-260e89c34802`, ISSUED, `cloud.layr.ws` |
| Origin certificate | Let's Encrypt YE1, `origin.cloud.layr.ws`, expires 2027-01-24 08:06:27 UTC |
| Origin guard | `X-Layr-Origin`; persistent SSM SecureString `/layr-store/gateway/origin-secret` |
| Cache policy | Managed-CachingDisabled `4135ea2d-6df8-44a3-9df3-4b5a84be39ad`; error TTLs zero |
| Origin request policy | Managed-AllViewerExceptHostHeader `b689b0a8-53d0-40ab-baf2-68738e2966ac` |
| Gateway binary | SHA-256 `9ed589f6b03401e9396f5fbb3d0677f407e02fc1eb215039bcad9c45486f28b9`, 7,615,376 bytes |
| Gateway archive | SHA-256 `e9d8f75e28b5d7e48246f37512b1d65b51cb29479e5ae4c5f094fa220cf7afd5` |
| Gateway sync program | SHA-256 `3bc33961d321125f5e762f58634942fc5f6971153edce54614914eee925e37d9` |

All nine published site files were retained byte for byte from the epoch rollout archive. The gateway package contains the binary, sync program and site. Build source archive SHA-256: `a4c4f9713e93bf9a364e53b9fd7ed47404f8c148b69f47e380d2998fab08d3ff`.

Managed AllViewerExceptHostHeader was tested live first, as required. A Mac IPv4 request through the deployed distribution returned 200 with valid public-host TLS before DNS publication, while the gateway required a valid generated CloudFront-Viewer-Address. The managed policy supplies that address in this deployment and is retained. A dual-SAN certificate is unnecessary. All seven HTTP methods are enabled in the verified distribution configuration. The actual callers below exercised Authorization, query strings, GET, POST, PUT and Range. Only a valid secret permits the gateway to use the viewer address; direct origin and forged-header requests without the secret return 403 before body consumption.

The managed origin-facing prefix list `pl-fab65393` is IPv4 only. It cannot restrict this IPv6 origin, so the existing IPv6 443 rule remains and the secret guards the origin. No instance, disk, subnet or address was replaced.

## Cutover and deployment receipts

The previous gateway remained active while the corrected distribution deployed. The guarded activation began at 09:42:05 UTC. The Mac IPv4 distribution probe returned 200 and TLS verification 0 at 09:42:20. The former plain public AAAA record was removed at 09:42:41; DNS change `/change/C0464727ZVBOS6N9UQAO`. The reviewed alias change set executed at 09:42:43 and reached UPDATE_COMPLETE at 09:43:24.519 UTC. Normal public Mac IPv4 HTTPS then returned 200. Previously cached plain AAAA answers could remain for their former 300-second TTL.

| Operation | Receipt |
| --- | --- |
| Viewer certificate | `layr-cloudfront-certificate`, change `cloudfront-283d0def7d1a5b01`, CREATE_COMPLETE |
| Gateway origin-secret IAM | `cloudfront-2c92e17a10068979`, only Role Modify, Replacement=False, UPDATE_COMPLETE |
| Corrected distribution | `cloudfront-ac0c85933c3acfe5`, only Distribution Add, CREATE_COMPLETE |
| DNS aliases | `cloudfront-9a2535f322d85d85`, only A and AAAA Add, UPDATE_COMPLETE |
| Guarded initial activation | SSM `951eae29-ad90-4242-8b47-d4756a2b1599` |
| Post-switch IPv6 front and origin updater | SSM `291636b8-7d01-47ca-b34a-e3719dc257bf` |
| Post-switch executor release sync | SSM `092b5a6d-f746-4ef3-acb2-1eeec782ca1a`, Result=success, ExecMainStatus=0 |
| Streaming candidate host preflight | SSM `7d858add-d228-49f2-a374-558b6678d87a`, size/SHA and `--help` passed before publication |
| Streaming gateway activation | SSM `f6eb4365-3466-4eee-93ab-891d8b42ea06` |
| Disposable release-file IAM | `cloudfront-release-files-6701b3a`, only Writer/Gc Modify, Replacement=False, UPDATE_COMPLETE |

The release-file IAM update applies only to `layr-store-m8-final`. At the 09:54 boundary, real `layr-store-layr` still needed epoch and release-file IAM changes. The later M1 preparation section above records their application.

## Live caller checks

| Check | Evidence |
| --- | --- |
| Mac IPv4 published browser | Fresh Playwright Chromium: recovery unlock, one signature session, encrypted v2 comment applied, lock, three viewports, zero axe violations, no persistent secrets |
| IPv6 published browser | Mac Chromium through a loopback CONNECT forwarder and SSM on the IPv6-only executor; real TLS hostname preserved; same checks passed |
| Mac native CLI | Fresh clone with core `c1d7229`; same clone pushed 8 records, one state and one file; signatures valid, fsck zero problems |
| AWS IPv6 native CLI | Executor cloned through public CloudFront, then pushed 3 records, one state and one file; signatures valid, fsck zero problems |
| Direct origin | Root and project-object route both 403 with only `origin access denied`, including forged viewer address |
| Per-viewer sign-in limit | Both IPv4 and IPv6: 12 challenges returned 200, next returned 429; three forged address headers each still returned 429 |
| Independent clients | At 09:50:50 UTC, IPv6 returned 200 while the Mac IPv4 bucket was exhausted; SSM `6c3d8958-4cd2-4eed-bba6-5c0f495fbcdb` |
| 30 MiB object | Encrypted size 31,465,290 bytes; PUT 201, GET 200, Range 206; SHA-256 `703da220f009782c2cd02847e651ba8105e80cdefa0446035702ff98d5615ecd` |
| 128 MiB release file | Actual CLI `release attach` and `release get` through Mac IPv4 CloudFront; plaintext 134,217,728 bytes and SHA-256 `99bd4f949e5fe514500425382eae9bd54d2fdbed8df2272c545c4fbb6f20edbb` matched |

The release was `release/cloudfront128`; its signed attachment named `release-128m.bin` and opaque object `files/01a1779d-9f7a-73d9-955b-2b2c53f3d329.age`. The 30 MiB probe obtained the normal encrypted project challenge and exchanged its proof for a project read token. A person session alone correctly refused encrypted object reads with 401.

Both encrypted browser requests were applied exactly once, with independent signed executor record checks in SSM `acde7d89-7fca-4995-877f-9e3dd78d79e3`. Two subsequent request-apply calls reported them ignored:

| Network | Request | Executor record | Request digest |
| --- | --- | --- | --- |
| IPv4 | `01a17799-7d20-702c-801a-3bdc07a35e39` | `01a17799-d0f3-74d3-acb5-0afae82b63f2` | `bc850ea8fa5d63b6b23bd5dbc8c89cf84d86a6f794d510235987de9fd86c7a27` |
| IPv6 | `01a1779b-9697-7e23-835b-92da6f29b439` | `01a1779c-5a50-756b-bd6b-d20785394605` | `084b2edc05f1ff02148bedd93f35141510bd0da7f333f898abc65f4acecbb81e` |

The browser used `web/tests/signin-deployed.mjs` against the published site, with a fresh Chromium context. No real Chrome profile was used. The private person and project fixture lived only on the owned macOS APFS RAM disk `/dev/disk18`, mounted as `/Volumes/LayrCFSmoke`. The IPv6 relay forwarded opaque TLS and did not receive private person keys. Mobile and desktop screenshots were inspected; the submitted comments were visible.

Gateway tests after merging release-files: 38 passed, one environment-dependent MinIO test ignored. CloudFormation lint and all 52 infrastructure tests passed. The shared Mac CLI was rebuilt after touching extracted sources so Cargo did not reuse a stale archive timestamp. The production musl binary was copied as a file and matched independent build-host size and SHA before packaging and host `--help` preflight.

CloudFront's request-body quota is 64 GB, above the 512 MiB release limit plus encryption overhead. No CLI limit change is required. The live 128 MiB check used the merged streaming route. See [AWS CloudFront quotas](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/cloudfront-limits.html).

## Current cleanup and protected resources

The disposable login was `cloudfront-live-7d619c@example.test`, project alias `cloudfront-live`. Executor retirement SSM `73f44084-046e-46f2-846a-855bb63a11bf` removed nine subvolumes. Daemon, token service, checks timer and transfer timer are inactive; ready flags are absent, the profile is empty and the runtime token is absent. The public executor machine identity is unchanged.

Cleanup removed 53 disposable store versions/markers, two project-config versions, four person versions and the registration invite plus used marker. The scoped SSM write-token and invite parameters are absent. Both store buckets have zero versions, zero delete markers and zero multipart uploads. The real store was not modified. The persistent origin and session SecureStrings remain.

SSM session `ifdotpy-x5dklc4rb3o8232o9l8r53lj7i` was terminated; local ports 18443/18445 are closed. The APFS RAM disk was detached at 09:52:46 UTC. The temporary candidate executables on the gateway were removed. Final gateway SSM `8df67db6-d3d4-4066-ab37-da5ba2bf6afe` confirmed active streaming release, empty projects, direct-origin 403 and normal public IPv6 HTTP 200 with certificate verification 0. Normal Mac IPv4 also returned 200 and verification 0.

Final AWS readback at 09:53:45 UTC confirmed all six protected DNS records unchanged and the prototype distribution `ETP5AYPASWZMY` configuration hash unchanged (`f84eb15892569bc58bd71161191685d2419442cb6a856d168d9654fd61ccc291`). Apex aliases still point to `d601z3odhuaeg.cloudfront.net`; desk and the previous ACM validation record are preserved.

## Monthly cost change

New fixed monthly cost: **$0**. No compute, storage volume or hosted zone was added. This uses pay-as-you-go CloudFront, a non-exportable ACM certificate and a standard SSM parameter. Traffic remains usage-based; the account's free allocation is shared, so a zero bill is not assumed.

For European edge traffic, estimate incremental CloudFront usage as `$0.020 × GB sent to the origin + $0.085 × billable GB sent to viewers + $0.0120 × billable HTTPS requests / 10,000`. The first 1 TB of viewer transfer and 10 million HTTP(S) requests per month are free across account usage; later transfer tiers differ. The 128 MiB upload itself is approximately $0.0025 of edge-to-origin transfer. Existing S3, KMS and normal origin-DNS query usage can also change. See [CloudFront pricing](https://aws.amazon.com/cloudfront/pricing/pay-as-you-go/).

The non-exportable viewer certificate has [no ACM charge](https://aws.amazon.com/certificate-manager/pricing/). Standard-throughput standard parameters have [no Parameter Store charge](https://aws.amazon.com/systems-manager/pricing/). A/AAAA alias queries to CloudFront have [no Route53 query charge](https://aws.amazon.com/route53/pricing/); normal origin AAAA queries remain metered.

## Earlier epoch rollout record

The following section records the 08:28 UTC boundary before the CloudFront deployment. Its gateway hashes and direct-public-host DNS are historical; the current state is above.

Measured state: 2026-10-26, 08:28 UTC. The epoch gateway and site are active. The exact Layr 0.3.0 executor is installed and unenrolled after the disposable live check. Rotation, a fresh machine-only executor clone, one hosted browser comment and removed-reader refusal passed. All disposable enrollments, store objects, person registrations, invites and private fixtures were removed.

Deployment branch: `cloud/epoch-rollout`. Gateway production source: `4b359da6aa424a92abb26ce8a7305ff72beb55ba`, including layr-cloud main `89ecc0b`. Shared crates and executor source: Layr main `01038b931575e1880fbc7b5829fec649720886f1`. No real project has been imported.

### Resources and network

AWS profile `ifdotpy`, account `134250228509`, region `eu-north-1`.

| Host | Instance | Size | Public IPv6 | Storage |
| --- | --- | --- | --- | --- |
| Gateway | `i-04da10c290655544f` | `t4g.nano` | `2a05:d016:df9:4000:8463:e4d6:66bb:c15f` | 8 GiB gp3 root |
| Executor | `i-0df29a43f3b810a82` | `t4g.small` | `2a05:d016:df9:4000:d35f:3f24:8eae:a94` | 12 GiB root; retained 10 GiB btrfs data |

VPC `vpc-0c5b8b1792383ecf5`; subnet `subnet-07f9b77a10c790951`, Availability Zone `eu-north-1a`; route table `rtb-07dfadc754796fa35`. Retained volume `vol-0c764a630871fc14c` is mounted at `/var/lib/layr`. The AMI is official Debian 13 ARM64 `ami-0309727f887f847cb`, publisher `136693071363`.

Both hosts use SSM administration, with no public IPv4, SSH or swap. The gateway accepts IPv6 TCP 443; the executor has no inbound security-group rules. No instance, volume, network resource or DNS value was replaced during epoch rollout. The optional runner is not deployed.

| Stack | Bucket or role |
| --- | --- |
| `layr-bootstrap` | `layr-bootstrap-artifacts-ik3kxdaezcnd` |
| `layr-store-layr` | `layr-store-layr-bucket-mg1n1j6rlomt` |
| `layr-store-m8-final` | `layr-store-m8-final-bucket-noitqelxwhmd` |
| `layr-gateway` | IAM role `layr-gateway` |
| `layr-executor-layr` | IAM role `layr-executor-layr` |

Each store has separate reader, writer and GC roles. Host roles assume exact project roles. The disposable `layr-store-m8-final` stack remains, empty, with prefix `m8-final/`. The real `layr-store-layr` bucket has zero object versions and zero delete markers. Its stack and old IAM policy were not changed.

### Installed releases

| Artifact | SHA-256 |
| --- | --- |
| Gateway binary | `4606b9dac995b7062b771df5218bfcd72f3afe2d321ca6bd01b65104fd5bc56e` |
| Gateway archive, 11 validated files including the site | `66ff73f145bbf064aea1b1f92edacbc651d4ac3eb2fea62c9caae33998132b8b` |
| Executor binary | `2c7d5195ac15c6606291be52fc1c9f37538ef5a1420ff4a7fa01cdff34bc2de4` |
| Executor archive, three validated files | `6d479b77efa6aaab5c3b74ce01c2246d30c02b3eb0a6c78dd3c1756f30786819` |

The gateway source archive contained 202 verified files, SHA-256 `bddba6a72bd04b44d132ba8c3770882e95de8fe658b76d4688100986b03ae59a`. The exact executor source archive contained 377 verified files, SHA-256 `876a38b789ced245a5c5573a890c34e05b0ceaa6a47f0bc999e6177ca1de463d`. Builds used `cargo build --release --locked --target aarch64-unknown-linux-musl`; the installed static executables match the hashes above.

The gateway was installed first. This order matters: CLI `01038b9` discovers signed `keys/` even for a root store, which the previous gateway refused. Gateway activation and HTTPS 200 with certificate verification 0 passed in SSM command `f6b42a5a-6b45-4b3c-bc90-88983740853a`. Executor installation passed in `a879777f-89b5-439c-976f-94b3b913adbd`.

Hosted assets: `index-9KUKB4B8.js`, `worker-DSo2a-Or.js`, `layr_web_bg-CMP4BpkJ.wasm`. The site reads the current epoch, authenticates sealed history, routes encrypted requests to that epoch and shows access mode and earlier epochs in Settings > Access.

The gateway serves signed discovery, epoch descriptors, exact machine slots, records, states, sealed history, requests and claims. Write and GC grants can read their own project's signed `keys/` discovery and perform narrow machine bootstrap. They cannot list encrypted data or aggregate slots. Encrypted reads require a proof for the requested epoch or a later epoch. Wrong-project grants and hidden projects retain matching 404 responses. `files/` listing requires a read proof; file GET, PUT and streaming belong to the release-files task.

### Current service and credential state

The gateway is active with `projects: {}`. The executor binary remains installed, but `layr.service`, `layr-executor-token.service`, `layr-checks.timer` and `layr-transfer.timer` are inactive. `ready` and `checks-ready` are absent; `profile.env` is empty. No smoke project directory remains.

Public executor identity is preserved: `01a17428-6b5f-7593-b722-ae3fbaba2957`, key `c12088572a3a43049e33d4b4936d31c2f6f126cbe8fcb86376679629855c1cc7`.

During enrollment, the approved service-token exception is a root-owned mode 0600 file under tmpfs, `/run/layr-executor-token/write-token`, loaded from `/layr-store/executor/write-token`. Stop both the daemon and token service explicitly when retiring an enrollment; stopping the daemon alone does not remove the token file. Both the runtime file and its SSM parameter are now absent. The persistent gateway decoy-secret parameter `/layr-store/gateway/session-secret` is preserved. Private person signing keys and recovery words never went to AWS hosts.

The executor's transfer program fetches, applies encrypted person requests, pushes and publishes indexes. The reporter runs through `/usr/sbin/runuser` with restricted reader credentials. Both runner flags were false and `queue.run` was empty throughout the live check. It executed no checks.

### HTTPS and protected DNS

At the measured time, `https://cloud.layr.ws` is IPv6 only, with the gateway AAAA record above and no A record. Route 53 zone: `Z00094863IAG61NP8YENE`. The Let's Encrypt certificate for `cloud.layr.ws` expires 2027-01-23 at 16:26:42 UTC. Post-cleanup HTTPS returned 200, certificate verification 0 and HTTP/2, SSM command `e9f8b6b3-26ab-4932-88ee-bf96a71ad17b`.

The final readback confirmed these protected values unchanged:

- Apex A and AAAA aliases: `d601z3odhuaeg.cloudfront.net.`
- `desk.layr.ws A 100.68.127.7`.
- ACM validation CNAME `_4c72782be16027c8dc92cab529a0814a.layr.ws` to `_a9f28679b4330e8c850d67573fd71795.wzccmgtwzk.acm-validations.aws.`

AWS was released to the separate CloudFront worker after cleanup at 08:28 UTC. That work can change the frontend later; the state above records the epoch rollout boundary.

### Disposable store IAM update

The first live CLI connection returned 502 because the old writer role refused `keys/` listing. Change set `epoch-01038b9-20261026` updated only the disposable stack's `Writer` and `Gc` IAM roles. Both were `Modify`, `Replacement=False`; execution reached `UPDATE_COMPLETE`. Its ARN is `arn:aws:cloudformation:eu-north-1:134250228509:changeSet/epoch-01038b9-20261026/655f2457-2f16-40d5-a634-e1fb792915b3`.

The template adds epoch-scoped immutable writes, signed discovery listing and epoch-scoped GC data paths. Both existing parameters were retained. The gateway and executor stacks were not updated.

Before the real project move (M1), apply the same IAM-only update to `layr-store-layr`. This is required and has **not** been done. From the reviewed template, prepare this exact change set:

```sh
aws --profile ifdotpy --region eu-north-1 cloudformation create-change-set \
  --stack-name layr-store-layr --change-set-name m1-epoch-iam \
  --change-set-type UPDATE --template-body file://infra/aws/template.yaml \
  --capabilities CAPABILITY_NAMED_IAM \
  --parameters ParameterKey=GatewayRoleArns,UsePreviousValue=true \
               ParameterKey=Prefix,UsePreviousValue=true
aws --profile ifdotpy --region eu-north-1 cloudformation wait change-set-create-complete \
  --stack-name layr-store-layr --change-set-name m1-epoch-iam
aws --profile ifdotpy --region eu-north-1 cloudformation describe-change-set \
  --stack-name layr-store-layr --change-set-name m1-epoch-iam
```

Execute only after review confirms IAM-role modifications with replacement disabled:

```sh
aws --profile ifdotpy --region eu-north-1 cloudformation execute-change-set \
  --stack-name layr-store-layr --change-set-name m1-epoch-iam
aws --profile ifdotpy --region eu-north-1 cloudformation wait stack-update-complete \
  --stack-name layr-store-layr
```

### Live epoch and browser evidence

The new fixture used project `epoch-live-01038b9`, login `epoch-live-01038b9@example.test`, project UUID `01a17735-b3e5-70de-a693-f3a90bb962cb` and change `01a17735-b3ff-71da-b872-1833726b95de`. Its private material existed only under VM tmpfs `/run/layr-web-epoch.oDULIk`. Only encrypted objects and public metadata were uploaded.

A write grant was stored through SSM; only its hash was retained in project configuration. One single-use registration invite was issued. Encrypted registration returned 200. A root-store push and removed-reader machine clone passed through the real gateway.

The exact static CLI ran `layr remote rotate gateway --remove machine:01a17735-b43b-74a4-8b9b-1f79fb675d85 --reason 'AWS epoch smoke removed-reader proof'`. It published one private epoch, `01a1773f-0f66-748c-83be-bc79a87689f8`. A new save and push wrote state `01a1773f-321f-720f-ac78-0dcbffec7965` in that epoch.

The removed machine could no longer fetch. A proof made with the old root identity still read root history with 200, but current-epoch GET and LIST returned 404. The old identity could not decrypt the new epoch's challenge. The tested new encrypted data part was `project/epochs/01a1773f-0f66-748c-83be-bc79a87689f8/states/01a1773f-321f-720f-ac78-0dcbffec7965/a73c8f45.0000.data.age`. The old proof was refreshed after a provider delay so expiry could not explain refusal.

The executor first cloned **after** rotation, with its machine identity and scoped grant only: 24 records and four states. It pinned the unique person's public signing key locally, pushed 12 records, published one heads snapshot and four paths and graphs, and completed the reporter cycle. SSM command: `280c1a5e-64ac-43cf-9d81-9852eccbd1a2`.

The published-site browser checked current mode, both epochs and main/feature file bytes before submitting exactly one encrypted comment. Request `01a1774e-6ad4-7681-9abb-3c898569acb6` was written under the new epoch and applied once. One shared-signature session exchange, three viewports, zero axe violations, lock and no persistent browser secrets passed.

Independent native proof, SSM `67e67bcf-5c6f-4c2f-ad76-b635a09107d1`:

- Exactly one comment record `01a1774e-a336-7704-a1c6-80328a9fcbe6`, executor sequence 15.
- Signed-envelope SHA-256: `471f60f530a13c8e7a00390a8dba52a0e2016ae82663f1132cb2056b43d6f261`.
- Request digest: `2cf855010d9da90acb5f71e0e5bcc7730558b283c12fb11328ddc0f6071163fc`, matching the stored 1427-byte encrypted request.
- Outer fields: `ct`, `epoch`, `id`, `person`, `project`, `sig`, `v`; version 2, with no plaintext action or comment.
- Both machine chains and signatures passed native fsck, zero problems; reporter result was success.

A native `layr-web` reader also passed against an encrypted snapshot of the live rotated store. It authenticated discovery, sealed root history, current records and file parts from both epochs, and matched the CLI's explicit state-ID file oracles.

The live run found a CLI defect: with `LAYR_PAPER_KEY_FILE` still set, the next gateway push tried reading the root paper slot while learning the pin's paper recipient. The run scoped that variable to rotation and then pushed with the machine identity only. The lead authorized a separate narrow core correction on `cloud/paper-env-push`; it is not in the installed `01038b9` binary.

### Temporary test transport

This was an isolated verification path, not a user connection procedure. The Mac has no IPv6 route, and `layr-dev` has only an OrbStack ULA. An SSM session forwarded opaque gateway TLS through the IPv6 executor:

```sh
AWS_PROFILE=ifdotpy AWS_REGION=eu-north-1 aws ssm start-session \
  --target i-0df29a43f3b810a82 \
  --document-name AWS-StartPortForwardingSessionToRemoteHost \
  --parameters '{"host":["cloud.layr.ws"],"portNumber":["443"],"localPortNumber":["18443"]}'
```

The Mac CONNECT forwarder bound only `192.168.139.3:18445`, accepted only VM `192.168.139.239` and `CONNECT cloud.layr.ws:443`, and forwarded bytes to SSM localhost port 18443. The browser used that explicit proxy.

The static CLI ignores proxy environment variables. With lead approval, its VM-only byte forwarder temporarily used the existing gateway IPv6 address on loopback:

```sh
orb -m layr-dev -u root python3 /mnt/mac/tmp/layr-epoch-build/loopback-tls-relay.py
# The program adds the address with:
# ip -6 addr add 2a05:d016:df9:4000:8463:e4d6:66bb:c15f/128 dev lo
# Its finally block, including SIGTERM/SIGINT, removes it with:
# ip -6 addr del 2a05:d016:df9:4000:8463:e4d6:66bb:c15f/128 dev lo
```

The Python program performed the add itself, bound only that VM address on port 443, sent the permitted CONNECT request and forwarded opaque TLS bytes. Neither forwarder terminated TLS. The CLI kept hostname and SNI `cloud.layr.ws` and verified its real certificate. No resolver or DNS override was used.

The listener and address were removed in the forwarder's `finally` block. SSM session `ifdotpy-98h8srqrb9rg4tnaz6bfqfksjy` was explicitly terminated; both Mac forwarding ports are closed. This path proves the IPv6 origin through SSM; public IPv4 access belongs to CloudFront verification.

### Cleanup and checks

The earlier `browser-signin-final` fixture was retired before creating the new UUID and login. Cleanup removed six old executor subvolumes, 473 store versions/markers, two project-config versions, four person versions and two registration-invite versions. The old login was `web-signin-final-34ad@example.com`. SSM retirement: `3c10fd75-5243-4bfc-84c5-498b18fe5208`.

After the new proof, cleanup removed seven executor subvolumes, 47 store versions/markers, two project-config versions, four person versions and two registration-invite versions. `/layr-store/executor/write-token` and `/layr-store/invites/epoch-live/token` were deleted. SSM retirement: `ec15da9b-13de-4c2e-a6d4-0e40bb0b07ca`; empty gateway project configuration: `78edd677-8099-4a62-a337-07c883d80462`.

The VM fixture, its private recovery/signing material, backing image, mount and loop device were removed. Local encrypted staging copies were removed too. The first local native fixture had been cleared by a clean VM poweroff at 07:35:50 UTC; the live fixture above was generated after that restart. No private fixture was restored from persistent storage.

Verification:

- Exact cloud `4b359da` workspace: 65 passed, zero failed, nine environment-dependent tests ignored. The native rotated-store test was then run explicitly against live bytes and passed.
- Full actual CLI/gateway/MinIO t27 passed, including session-authenticated mount, paper-only S3 mount, unauthenticated gateway 401 with no served file, person registration/invites/clone, GC machine bootstrap and overlay reader/writer isolation.
- Gateway: 33 unit tests passed. GC discovery and bootstrap are project-bound; wrong-project GC grants match hidden 404s; a write grant cannot delete; `files/` listing rejects a write grant without read proof.
- Core t44/t45/t46 passed 61/28/17 checks sequentially. The real gateway harness reproduced the old staging failure, then passed rotation ordering, interruption after discovery, fail-closed reader message, one-epoch resume and machine-only cloning. The lead's merged-core full suite passed 51/51.
- Infrastructure: cfn-lint and 35 Python tests passed. Browser build, TypeScript and 85 unit tests passed; 44 page/browser tests and the additional epoch Access test passed.
- Final readbacks: both smoke stores and registrations absent, real store empty, protected DNS unchanged, gateway active, executor unenrolled, runtime token absent, and all test forwarding resources removed.


## CloudFront artifact verification incident (2026-10-26)

The first CloudFront gateway activation failed before configuration loading: systemd reported
`Main process exited, code=killed, status=11/SEGV`; running the executable with `--help` also
returned 139. The source tests had passed. The build-machine executable was 7,418,768 bytes with
SHA-256 `721c970bdd3ef8afc866873cbe7d01d1418d5a523223d4de6a9f9fba9a29062a`. A copy made through
captured `orb ... cat` stdout was truncated to 6,422,528 bytes, SHA-256
`898452cebc9352ddc3063d8e63ffb0f82f4afd505960c3ccae8320dba545d265`. Every copied byte matched
the original prefix; 996,240 trailing bytes were missing. The resulting archive was
`b7f25ac06e9f930ecde77afe90bf8e5c14345b35c333666acf10221dd461f5dd`.

The previous archive `66ff73f145bbf064aea1b1f92edacbc651d4ac3eb2fea62c9caae33998132b8b`
and original host configuration were restored through `Sync.release()`, SSM
`a1a7aeba-f5a2-4e57-acb6-d5b20e8eb63d`. AWS IPv6 checks returned HTTP 200 with certificate
verification 0 over HTTP/1.1 and HTTP/2. The distribution was still unexecuted and public DNS was
unchanged throughout the incident.

The corrected path uses file copying and the tracked `package_gateway.py`, with mandatory byte
count and SHA-256 measured independently on the build host. Release manifests include
`binaryBytes` and `binarySha256`; sync checks both before installation and on cached releases.
The ELF validator rejects a segment extending past the executable's end, even when the archive's
own digest matches. The actual truncated archive is rejected by this check. Historical manifests
remain usable for rollback. All 49 Linux infrastructure tests and CloudFormation lint passed.

The complete candidate passed `--help` on the gateway host before activation, SSM
`284999aa-955c-4d06-bf12-0ab374f7c402`; its size and build-machine SHA-256 matched. An independent
SSM SecureString read also passed without exposing its value, while the old service still returned
IPv6 HTTPS 200. The corrected package preserves all nine deployed site files byte for byte.

The first distribution create failed at 09:25 UTC because its template contained an incorrect
managed cache policy ID. CloudFormation reported `specified cache policy does not exist`, then
`ROLLBACK_COMPLETE`. The gateway was restored to the previous unguarded release at 09:27 UTC,
SSM `28a4d760-72b1-451d-b708-a1cef6321432`; HTTP/1.1 and HTTP/2 both returned 200 with TLS
verification 0. Public DNS did not change. The guarded activation window was approximately
09:21–09:27 UTC. After the lead approved deletion, resource readback showed only a deleted
Distribution with no physical ID. The empty failed stack was deleted.

The managed-policy API returned CachingDisabled `4135ea2d-6df8-44a3-9df3-4b5a84be39ad` and
AllViewerExceptHostHeader `b689b0a8-53d0-40ab-baf2-68738e2966ac`. The template now uses these
IDs. Before planning any front change set, the deployment helper lists AWS managed policies and
checks the IDs against their names. A missing or mismatched policy fails before reading the
origin secret or creating a change set. CloudFormation lint and all 51 infrastructure tests passed.

CloudFront's documented maximum request-body size is 64 GB. This exceeds the 512 MiB release-file
limit plus age encryption overhead, so no CLI limit change is needed. See the
[AWS CloudFront quotas](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/cloudfront-limits.html).
The configured origin response timeout is 120 seconds; the completed live transfer evidence is recorded above.

The corrected distribution `E3SU1MYTKJEE06` (`d960l4aujuosg.cloudfront.net`) reached Deployed.
At 09:42:05 UTC the guarded candidate was activated. At 09:42:20 UTC a Mac `curl -4` request
with the real `cloud.layr.ws` SNI and Host, connected to the distribution before the DNS switch,
returned HTTP 200, certificate verification 0, peer `18.244.102.119` and CloudFront response headers.
The gateway requires a valid CloudFront-Viewer-Address before serving any route. This live result
confirms that managed AllViewerExceptHostHeader supplies the generated address in this deployment;
that policy is retained and a dual-SAN origin certificate is unnecessary. The origin certificate
covers `origin.cloud.layr.ws`. Direct origin traffic and a forged viewer header without the secret
both returned 403, while an authenticated origin request returned verified HTTPS 200.
