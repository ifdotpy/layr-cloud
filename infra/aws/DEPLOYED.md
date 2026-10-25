# AWS deployment of Layr Cloud

Measured state: 2026-10-25, 11:23 UTC. Deployment is pending approval and tailnet enrollment.
This file records the M8 deployment from `layr-cloud` base `e4fdf656a0cae5ada5e2e3c7ebaf2fbe372c5f8d`.
The deployment branch is `cloud/m8-deploy`.

## Target and scope

- AWS profile: `ifdotpy`; account: `134250228509`; region: `eu-north-1` (Stockholm).
- Gateway URL: `https://cloud.layr.ws`, reachable through igor's tailnet.
- Existing Route 53 zone: `Z00094863IAG61NP8YENE` (`layr.ws`). No new hosted zone is planned.
- Allowed DNS changes: `cloud.layr.ws` and `_acme-challenge.cloud.layr.ws` only.
- The `layr.ws` apex, `desk.layr.ws`, and the existing ACM validation record must stay unchanged.
- Real projects remain unimported. Verification will use a disposable test project.
- Planned executor source: Layr `1a91cd32555a878e87d63d6311520ecca3be7173`.

## Resources

No AWS resources were created as of the measured time.
The following stacks await the lead's explicit approval:

| Stack | Planned resources |
| --- | --- |
| `layr-bootstrap` | Retained, encrypted, versioned artifacts S3 bucket |
| `layr-store-m8-smoke` | Separate disposable verification store bucket and exact reader, writer and GC roles |
| `layr-store-layr` | Retained, encrypted, versioned project S3 bucket; reader, writer and GC roles; bucket policy |
| `layr-gateway` | IPv6-only VPC and subnet, internet gateway, route table and IPv6 route, WireGuard security group, IAM role and instance profile, `t4g.nano` EC2 instance with 8 GiB gp3 root |
| `layr-executor-layr` | WireGuard security group, IAM role and instance profile, `t4g.small` EC2 instance with 12 GiB gp3 root, retained encrypted 10 GiB gp3 btrfs volume and attachment |

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

1. Lead approval of stacks, sizes, and cost before the first CloudFormation deployment.
2. Separate reusable, pre-approved Tailscale auth keys in the two SSM SecureStrings:
   `/layr-store/gateway/tailscale-auth-key` and `/layr-store/executor/tailscale-auth-key`.
   Each key needs its respective `tag:layr-gateway` or `tag:layr-executor` tag.
3. Tailnet grants: members to gateway TCP 443; executor to gateway TCP 443;
   members to executor TCP 5000; administrators to root through Tailscale SSH.
4. Resolve executor write-token input. Pinned Layr requires a regular private token file and rejects symlinks.
   This conflicts with the task's rule against credential files. No token file has been created.
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
