# AWS deployment of Layr Cloud

Measured state: 2026-10-25, 22:53 UTC. Both hosts are deployed in Stockholm with SSM administration and no public IPv4 address. The gateway serves IPv6 HTTPS with a valid Let's Encrypt certificate. The exact-main executor build, fresh clone, push, index and reporter cycle passed. One hosted browser comment was delivered encrypted and applied exactly once; native signature verification passed. Cleanup is recorded below.

Deployment branch: `cloud/m8-deploy`. Real project keys and stores remain uninitialized; final verification uses only the isolated `browser-signin-final` smoke project. The earlier `browser-actions` project was retired and removed.

## Resources and network

AWS profile `ifdotpy`, account `134250228509`, region `eu-north-1`.

| Host | Instance | Size | Public IPv6 | Storage |
| --- | --- | --- | --- | --- |
| Gateway | `i-04da10c290655544f` | `t4g.nano` | `2a05:d016:df9:4000:8463:e4d6:66bb:c15f` | 8 GiB gp3 root |
| Executor | `i-0df29a43f3b810a82` | `t4g.small` | `2a05:d016:df9:4000:d35f:3f24:8eae:a94` | 12 GiB root; retained 10 GiB btrfs data |

VPC `vpc-0c5b8b1792383ecf5`; subnet `subnet-07f9b77a10c790951`, Availability Zone `eu-north-1a`; route table `rtb-07dfadc754796fa35`. The retained data volume is `vol-0c764a630871fc14c`, mounted at `/var/lib/layr`.

The AMI is `ami-0309727f887f847cb`, official Debian 13 ARM64 publisher `136693071363`. Both hosts reported SSM Online, agent `3.3.5538.0`, and no swap. The gateway accepts TCP 443 from `::/0`; the executor has no inbound security-group rules. Neither host accepts SSH. Tailscale is optional and is not required for this deployment.

| Stack | Bucket or role |
| --- | --- |
| `layr-bootstrap` | `layr-bootstrap-artifacts-ik3kxdaezcnd` |
| `layr-store-layr` | `layr-store-layr-bucket-mg1n1j6rlomt` |
| `layr-store-m8-final` | `layr-store-m8-final-bucket-noitqelxwhmd` |
| `layr-gateway` | IAM role `layr-gateway` |
| `layr-executor-layr` | IAM role `layr-executor-layr` |

Each store has separate `layr-store-reader-<prefix>`, `layr-store-writer-<prefix>` and `layr-store-gc-<prefix>` roles. Host roles assume these exact project roles rather than receiving direct project-object access. The writer creates immutable objects; the separate GC role deletes eligible state data. Retained buckets, object versions and the data volume continue to incur charges after stack deletion.

The executor currently uses the final smoke store roles and prefix `m8-final/`, project alias `browser-signin-final`. The `layr` store has no real project data or private project keys. The optional runner is not deployed.

## HTTPS and DNS

Origin: `https://cloud.layr.ws`, IPv6 only. Route 53 zone `Z00094863IAG61NP8YENE` has an AAAA record for the gateway address, TTL 300, and no A record for this host. ACME uses the Route 53 DNS challenge. The certificate has subject `CN=cloud.layr.ws`, issuer Let's Encrypt YE2, and expires 2027-01-23 at 16:26:42 UTC. The gateway supports HTTP/1.1 and HTTP/2 and refuses IPv4 listener connections.

The 19:19 UTC readback confirmed both hosts running without public IPv4. The final 22:53 UTC inventory confirmed the DNS baseline preserved and the real project store empty. Only `cloud.layr.ws` and `_acme-challenge.cloud.layr.ws` were changed. Protected records:

- Apex A/AAAA aliases: `d601z3odhuaeg.cloudfront.net`.
- `desk.layr.ws A 100.68.127.7`.
- Existing ACM validation CNAME `_4c72782be16027c8dc92cab529a0814a.layr.ws` to `_a9f28679b4330e8c850d67573fd71795.wzccmgtwzk.acm-validations.aws`.

Initial cloud-init reported a scripts-user failure because the release manifest had not yet been published. After publication, the normal sync service completed and activated HTTPS. It was not an IPv6 or SSM connectivity failure.

## Exact-main executor release

Final CLI source: merged Layr main `1793e69b8df39add2c9ebf1275071949b7591389`. Later main `34ad8c0` adds the browser policy helper without changing CLI code. Main subsequently advanced to `ddcce23` with epoch support; the lead explicitly retained executor `1793e69` for this root-only gateway. Epoch rollout is a separate task.

After merge, `git ls-remote` confirmed that main hash. All 364 files in the build folder were compared with its exact Git archive; none differed. The source archive SHA-256 was `3a2d7dc22c420e593a6985b72970a85656f79fb9888fa6a2a914dbfdcf024c3a`. A new `cargo build --release --locked --target aarch64-unknown-linux-musl --bin layr` completed successfully using the verified source and compiler cache.

| Artifact | SHA-256 |
| --- | --- |
| Static executor CLI | `c4f016e5c3cd17f0f763657912018ae383aa97531c67c5bc4503c2bda822d814` |
| Executor archive, three validated files | `30df05a0ce57096466dd79f1020f120352fc78768902785d44ca60f5bcfc0943` |
| Reporter `checks.py` | `895b5568af8c8e8f7fd904f237c860c9cd07bf006415eba1bac8aaa1043ed92a` |

The public executor identity remains `01a17428-6b5f-7593-b722-ae3fbaba2957`, public key `c12088572a3a43049e33d4b4936d31c2f6f126cbe8fcb86376679629855c1cc7`. The owner trusted that exact public identity. Fresh-clone checks preserve it and retire only the previous disposable project folder outside the active projects directory.

The executor fetches, applies signed encrypted person requests, scans pending checks as the local `layr-checks` reader, pushes records, and publishes signed indexes. Both machine-local runner flags are false and `queue.run` is empty. No Rust toolchain or check execution is required on this host. An optional runner may be configured later; it is not part of this live verification.

Person signing pins are machine-local. The final smoke executor has the verified public signing key for `web-signin-final-34ad@example.com` and a maintainer grant. Real project enrollment must pin Igor's verified public key locally; gateway registration does not replace that pin. Private person keys and recovery words stay off AWS hosts.

## Gateway and site releases

The active gateway protocol implementation is from `40c79a15936c01677081ec8b95411d7a06998fb5`, with shared dependency pin `45080bb53b4de5c84726ac85563c979374308024`.

- Gateway binary: `04cb9bf878de17803ef8a7782914b22336eb8a756b3091802a1cba555ee48c54`.
- Active gateway archive with the final site: `04857823dc3e7e2492491695123e53b20e0059c8086483517a5713d87e19c990`.
- Site source: `c8ca115a32094f20547a8bb9d77426a05011fc4b`, shared Layr main `34ad8c0e9e31b4898ec3b98cbbd33ad6f30c41bf`. Public site archive SHA-256: `386b4bd05e2388699e598ca1c22ba5be1e0c4e1e4dd6c717128662e03c761358`. Hosted asset readback: `index-bCtVUjWs.js`, `worker-Bqz6U2K9.js`, `layr_web_bg-Dfb8tmGe.wasm`. The gateway binary is unchanged.

The gateway uses the shared `layr_store::session` signing message. Challenges are random 32-byte values, valid for one minute and single use. Sessions are login-bound, expire after 12 hours, and are held only as token hashes in RAM. Session tokens encode 32 random bytes as lowercase hexadecimal. A gateway restart invalidates sessions.

First registration requires a hashed seven-day single-use invite. The administrator creates it locally through SSM; the command writes the raw value directly into a SecureString and prints only its name. Registration grants no session until the new signing key proves a fresh challenge.

Public unlock returns one fixed-size base64 field with 65536 decoded bytes, without an index or inbox. Known and unknown logins have the same status, outer shape and size. Deterministic unknown-login packets follow age 0.11.5 GREASE distributions from the source. Their HMAC key is fetched into gateway memory from `/layr-store/gateway/session-secret`. The 1,000-login distribution/restart regression and HTTP structure/restart regression passed.

Unlock and sign-in share a strict 12/minute client-address limit; all routes have a 600/minute limit. Person routes use sessions. Machines bootstrap only their exact project descriptor, own encrypted slot, challenge and proof exchange with a scoped grant; subsequent data reads require their read proof. Machines make no aggregate slot request. Wrong, expired, other-project or absent grants cannot discover private projects.

Request v2 contains an encrypted action and a verified outer person signature. The gateway refuses plaintext v1 and project/epoch/path/identity mismatches. Current deployment verification covers root requests; epoch rollout is separate.

## Credential handling

The approved runtime exception is a root-owned mode 0600 write-token file under tmpfs: `/run/layr-executor-token/write-token`, loaded from `/layr-store/executor/write-token`. Its directory is mode 0700. Stopping `layr.service` stops the token service and removes this runtime material. Transfer/report cycles refresh it from SSM. No service token is stored in a persistent file, artifact, command argument or log.

The gateway can read only its exact persistent decoy-secret parameter and write only the approved invite/service-token parameter names. The executor can read only its own token parameter. Owner/private person material is never used as a workaround for machine authentication.

## Existing-host program convergence

`infra/aws/executor.yaml` contains the normal fallback transfer program: fetch, apply person requests, push, then index. A failed apply or push prevents index publication. The runner path does not apply person requests or publish indexes.

Source commit: `da5f98eb8fcf1a1eb65349bde949fdb31a0fa712`. Script SHA-256: `1a0390654872d7201232326f3ddcd0b763013de003307961d9473cb0ecae2b2b`.

CloudFormation user data does not rewrite an existing host's program. As an approved one-time convergence step, the exact committed program was installed through SSM and its hash read back, command `0f474e32-8c5d-4200-acab-e605b26cdae2`. Rebuilding from the Git template installs the same program.

Reporter launcher fix: `a52943877a1d50f8e5d8ae0267202e2e9e10f552` uses `/usr/sbin/runuser` because the reader's restricted PATH excludes sbin. The real restricted-PATH launcher regression passed. The reviewed reporter travels in the normal content-addressed release archive.

## Live verification path

The Mac has no IPv6 connectivity; `layr-dev` has only an OrbStack ULA. The browser and disposable recovery material therefore stay on `layr-dev`. SSM forwards HTTPS through the AWS executor, which reaches the gateway over IPv6:

```sh
AWS_PROFILE=ifdotpy AWS_REGION=eu-north-1 aws ssm start-session \
  --target i-0df29a43f3b810a82 \
  --document-name AWS-StartPortForwardingSessionToRemoteHost \
  --parameters '{"host":["cloud.layr.ws"],"portNumber":["443"],"localPortNumber":["18443"]}'
```

The temporary CONNECT proxy binds only the OrbStack bridge `192.168.139.3:18445`, accepts only the VM address and `CONNECT cloud.layr.ws:443`, and forwards opaque TLS to localhost port 18443. The browser retains the real Host and TLS SNI `cloud.layr.ws` and validates the real certificate. Early DNS negative caching required the measured IPv6 literal as the SSM remote host; later sessions used the hostname successfully. No resolver files were changed.

This proves the gateway path through the executor. It does not prove public access from an IPv4-only network. Such networks need an IPv4 front later, for example CloudFront or one public IPv4 address. No private test material goes to AWS hosts.

## Verification evidence

After merging reviewed layr-cloud main `c8ca115`, exact merge source `ac02cf44123eab3bebdaec15ca67480281f8e5d4` passed `cargo test --locked --workspace`: 62 passed, zero failed, eight environment-dependent tests ignored. CloudFormation lint and all 35 infrastructure tests passed. These checks ran from the exact Git archive in `layr-dev`; no deployed binaries or epoch routes were changed.

- Gateway: 31 tests passed; one environment-dependent test ignored. Exact paper-slot proof/authority regression passed subsequently, commit `d4ecef0a743151467e62f9d3e4c177731f775a75`.
- Infrastructure: 35 Python tests passed and cfn-lint passed. These include real btrfs volume checks, release validation, IAM boundaries and transfer failure ordering.
- Real CLI, gateway and MinIO: private-project push and signed index publication passed. Proof precedes data listing; machines make no aggregate slot requests. Public-project push/index, invite registration, signature sign-in and encrypted v2 comment submission also passed. Test material was removed from `/run` after each run. Harness commit `44dfb110afb3b28fea25d9210113afbc32dcb80e`.
- Deployed read-only browser: recovery unlock, signature sign-in, three viewports, zero axe violations, lock and no persistent browser secrets passed.
- Multi-writer diagnosis: native person pins and protected-main config were intact. The browser needed the project-creator policy perspective and a retry after full trust-history verification. The published index itself validated against the full records. The staged corrected browser passed signing-pin enabled, protected-main visible and no discarded index, without submitting a comment.
- Exact-main AWS rerun passed: installation readback `d853121e-cdce-4815-9eb0-5cc505150d62`; fresh clone `45cb2b68-cb89-4182-a073-49a10014af1a` imported 56 records and three states. Command `d93ad15d-7829-4e1f-b324-67c25f05e561` pushed one record, published one heads snapshot, and completed the reporter cycle with count/pending zero and `work_done=false`.
- Final published-site browser run, 22:46–22:47 UTC: one signature exchange; one encrypted v2 comment PUT returned 201; the signed applied comment appeared in Conversation. Three viewports had zero axe violations. Lock and no persistent browser secrets passed. Request `01a1753e-6581-7f24-bc49-41a1c0e56071`; project `01a17539-ce9f-7439-acc7-d9c4b566bd03`; change `01a17539-cebe-721b-acef-3d5dcec19802`.
- Independent native proof, SSM `fe85415e-c489-47bc-9e08-8647423b84a7`: exactly one comment record `01a1753e-cdd5-726a-9f53-08177eae53f9`, executor sequence 15. Signed-envelope SHA-256 `64da76460ba77a42d6144a99b4c2986f4044c7b4117d99e5070edeaf03332c3f`. Request digest `ce6e2e0eb42f5948a17c3b99a14b9a3cb7cf30216a183a8f175222a3e9494b00` matched the stored 1417-byte encrypted S3 request. Its outer fields contain ciphertext and public bindings, with no plaintext action or comment. Native fsck verified both machine chains and signatures with zero problems; reporter succeeded and daemon remained active.
- Fresh final-project clone/push/index/reporter proof: SSM `dd2d0142-dc68-4672-b142-bd7959808e9b` imported 14 records and three states, pinned the new unique person, pushed 12 records, published one heads snapshot plus three paths and three graph objects, and completed the reporter cycle successfully.

## Fixture replacement and cleanup

The VM rebooted at about 22:32 UTC during the merge hold. This cleared the original tmpfs fixture `/run/layr-web-actions.f627Ed` and its private recovery material. Its earlier persistent `/root/work/layr-web-actions.wcGUsl` copy had already been unmounted and removed before AWS enrollment. No private backup or AWS copy existed. The disabled-button attempt against the earlier site delivered no comment; the old S3 request prefix was empty.

The lead approved a fresh disposable person `web-signin-final-34ad@example.com` and project `browser-signin-final`, generated only under VM tmpfs `/run/layr-web-actions.6ihzJN`. The new store uses the same reviewed template in `layr-store-m8-final`. The existing executor public identity was trusted; only encrypted project/person objects and public descriptors were uploaded. One invite was issued through SSM. The first registration returned 429; the exact person record was confirmed absent, the rate window elapsed, and the same invite retry returned 200. Rate limits were unchanged.

Gateway StoreRoleArns and executor reader/writer role parameters were updated through the previous CloudFormation templates. Each change set modified only the IAM Role, with Replacement false; neither instance nor user data changed. The final write grant is represented in remote project config only by its irreversible hash, so later config synchronization preserves it. The active raw grant stays in the approved SSM/tmpfs runtime path.

Cleanup completed after the final comment proof:

- Removed the old `gateway/projects/browser-actions.json` object and all its versions. Gateway sync completed successfully, command `e573f15c-0230-4ee0-914f-0843bb15bac4`.
- Removed the old `people/owner@example.com/` registration and encrypted slots/index, four versions/markers, plus two versions/markers of its registration invite metadata and used marker. The new unique person's registration remains separate.
- Purged all 382 versions/markers from `layr-store-m8-smoke-bucket-ainpgseboqyh`, deleted the `layr-store-m8-smoke` stack and its project roles, then deleted the retained empty bucket. The gateway no longer lists those role ARNs; the executor role parameters now reference only the final smoke store.
- Removed disposable SSM invite parameters `/layr-store/invites/m8-smoke/token` and `/layr-store/invites/m8-final/token`. The active executor grant and persistent gateway decoy secret remain in their required parameters.
- Removed all three retired old smoke clones from `/var/lib/layr/smoke-retired/`, including six btrfs subvolumes per clone, command `158ff726-b239-4c92-8f27-c10915e46716`. The current final project and global executor identity were preserved.
- Unmounted and removed `/run/layr-web-actions.6ihzJN`, its btrfs backing image and private person recovery material. The browser worker verified absence and no fixture mount at 22:51 UTC; an independent check also verified the final fixture, original tmpfs fixture and earlier persistent fixture absent, with no remaining fixture mount.
- Closed both forwarding processes at 22:51 UTC and explicitly terminated SSM session `ifdotpy-p37no9ccvbsyegry9c7czvho5e`. Neither local port 18443 nor 18445 has a listener.

Post-cleanup health readbacks passed: gateway command `552b01d4-59e1-4aa1-bfbd-eb3f85c1f1e1` confirmed only the final smoke project and its preserved scoped grant. Executor command `bca528f8-afa0-4db8-a49c-7fd558528f69` completed the reporter cycle successfully, confirmed the daemon active, and reached public IPv6 HTTPS with status 200, certificate verification 0 and HTTP/2.

The final encrypted smoke project remains available for deployment evidence. No disposable recovery material remains, and no real project has been imported. The `layr-store-layr` stack was not changed.

## Cost and optional runner

Prices were read from the AWS Price List API on 2026-10-25, USD before tax, using 730 hours/month:

| Item | Rate | Monthly estimate |
| --- | --- | ---: |
| Gateway `t4g.nano` | $0.0043/hour | $3.14 |
| Executor `t4g.small` | $0.0172/hour | $12.56 |
| 30 GiB gp3 | $0.0836/GiB-month | $2.51 |
| Base total | | About $18.21 |

Usage-based S3 bytes/versions/requests, DNS queries and transfer are additional. Initial total is estimated at about $20/month; this is not a billing cap. The existing zone charge is unchanged. No public IPv4, NAT gateway, load balancer or new hosted zone was added.

The optional `t4g.large` runner is not deployed. A later runner would cost $0.0688/running hour plus about $5.68/month for 68 GiB of retained gp3 storage, including while stopped. It would run checks and stop after 30 idle minutes. The executor can start only its configured runner instance and has no stop, terminate or SSH authority over it. Reviewed local runner policy and a verified toolchain are required before enabling that separate role.
