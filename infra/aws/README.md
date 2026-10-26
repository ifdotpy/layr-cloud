# Layr Cloud on AWS

Clients reach `https://cloud.layr.ws` over IPv4 or IPv6 through CloudFront. CloudFront connects
with verified HTTPS to the IPv6-only gateway at `origin.cloud.layr.ws`. Person identity comes from
a signature session. The gateway requires the secret origin header before serving any route or
trusting `CloudFront-Viewer-Address` for rate limits. It reads that secret from SSM at startup.

Tailscale is optional. The gateway permits TCP 443 from `::/0`; the AWS origin-facing prefix list
currently covers IPv4 only, so it cannot restrict this IPv6 origin. Executor and runner hosts have
no inbound rules. Administration uses SSM Session Manager. No host has public IPv4 or SSH.

The [public gateway design](../../docs/public-gateway-design.md) defines the authentication protocol.
[DEPLOYED.md](DEPLOYED.md) records the exact deployed revisions, resources, costs and measured checks.
Do not infer a successful deployment from a template lint or local test.

| File | Purpose |
| --- | --- |
| bootstrap.yaml | Retained, encrypted, versioned artifacts bucket |
| template.yaml | Project bucket and reader, writer and GC roles |
| gateway.yaml | IPv6 network, gateway origin and SSM instance role |
| cloudfront-certificate.yaml | DNS-validated viewer certificate in us-east-1 |
| cloudfront.yaml | IPv4/IPv6 distribution and public Route 53 aliases |
| cloudfront.py | SDK deployment plans with secret handling and replacement checks |
| package_gateway.py | Checks build-machine size/hash, preserves the site and validates the archive |
| executor.yaml | Executor or optional idle-stopping runner and retained btrfs disk |
| sync.py | Atomic release installation, configuration, DNS and ACME certificate setup |
| checks.py | Fetch/apply/report/push/index loop and optional runner wake/idle control |
| dns.yaml | Optional dedicated hosted zone; do not use for an existing shared zone |

The gateway assumes only the exact project roles named in its template parameters. The executor
and runner assume reader and writer roles, with no GC role. The writer creates immutable records,
states, slots, requests and indexes, and the allowed overlay objects. It cannot overwrite or delete
objects or initialize a store. The reader opens ciphertext. GC can delete only state data parts,
after application checks of references and the grace period. Versioning retains deleted state bytes
for 30 days before the lifecycle rule removes noncurrent versions. Current objects never expire.
The owner alone creates store.json. Keep real owner and person recovery keys on the owner's machine.

## Requirements and account

Use AWS CLI v2, Python 3, tar and a Debian 13 ARM64 AMI with cloud-init and `/dev/xvda` as its root disk.
Build static AArch64 musl executables. Building the browser site needs Node.js and npm on the build
machine; hosts need no Node.js or Rust except the optional check runner.

```sh
export AWS_PROFILE=ifdotpy AWS_REGION=eu-north-1 AWS_DEFAULT_REGION=eu-north-1
export ARTIFACTS_STACK=layr-bootstrap GATEWAY_STACK=layr-gateway
export PROJECT=m8-smoke EXECUTOR_STACK=layr-executor-layr
export GATEWAY_ROLE_NAME=layr-gateway EXECUTOR_ROLE_NAME=layr-executor-layr
export GATEWAY_HOST=cloud.layr.ws ORIGIN_HOST=origin.cloud.layr.ws ZONE_ID=Z00094863IAG61NP8YENE
export IMAGE_ID=ami-0309727f887f847cb
aws sts get-caller-identity --query '{Account:Account,Arn:Arn}'
```

For this deployment the account must be `134250228509`. Only `cloud.layr.ws`, `origin.cloud.layr.ws`,
their ACME challenges and the new viewer certificate validation CNAME may change in zone `layr.ws`.
Keep the apex, desk.layr.ws and the existing ACM validation record. Distribution `ETP5AYPASWZMY`
serves the prototype and must not change.
Credentials come from the selected AWS profile or instance role, never a file made by these procedures.
Keep shell tracing off for all secret handling. Do not include credentials in SSM command text or output.

## Build and publish a release

Pin the shared Layr crates and the executor executable to the approved merged Layr main revision.
Run the relevant tests before publishing. Cross compilation needs the musl linker and target.

```sh
npm ci --prefix web
npm run build --prefix web
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
  CC_aarch64_unknown_linux_musl=musl-gcc \
  cargo build --release --locked --target aarch64-unknown-linux-musl -p layr-gateway
```

Build the native `layr` executable from the same pinned Layr source. Strip both executables.
Prepare a gateway directory with `layr-gateway`, `sync.py` and `site/` copied from `web/dist/`.
Prepare an executor directory with `layr`, `sync.py` and `checks.py`. Copy executables as files;
command stdout can truncate binary streams. Measure the stripped executable's SHA-256 and byte count
on the build host, then compare both after copying. The gateway packager requires those independent
measurements. Test the candidate's `--help` on the actual host before changing the active manifest.

The release validator refuses links, path escapes, dynamic executables, wrong architectures,
truncated ELF segments, files over 32 MiB and total contents over 128 MiB. It checks the archive digest
and the manifest's build-machine binarySha256/binaryBytes before installing anything. Historical
manifests without binary metadata remain usable for rollback.

```sh
python3 infra/aws/package_gateway.py --binary gateway/layr-gateway \
  --binary-sha256 "$GATEWAY_BINARY_SHA256" --binary-bytes "$GATEWAY_BINARY_BYTES" \
  --site-archive "$PREVIOUS_GATEWAY_ARCHIVE" --site-sha256 "$PREVIOUS_GATEWAY_SHA256" \
  --output gateway.tgz > gateway-release.json
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
  -czf executor.tgz -C executor ./layr ./sync.py ./checks.py
```

Deploy the artifacts stack once, then upload each content-addressed archive and publish its manifest.
The following Python uses the administration profile and prints only a nonsecret digest:

```sh
aws cloudformation deploy --stack-name "$ARTIFACTS_STACK" \
  --template-file infra/aws/bootstrap.yaml --no-fail-on-empty-changeset
export ARTIFACTS_BUCKET=$(aws cloudformation describe-stacks --stack-name "$ARTIFACTS_STACK" \
  --query 'Stacks[0].Outputs[?OutputKey==`ArtifactsBucket`].OutputValue' --output text)
python3 - <<'PYRELEASE'
import hashlib,json,os,pathlib,boto3
s3=boto3.Session(profile_name=os.environ['AWS_PROFILE']).client('s3',region_name=os.environ['AWS_REGION'])
for kind in ('gateway','executor'):
    data=pathlib.Path(kind+'.tgz').read_bytes(); digest=hashlib.sha256(data).hexdigest()
    key=f'{kind}/releases/{digest}.tgz'
    s3.put_object(Bucket=os.environ['ARTIFACTS_BUCKET'],Key=key,Body=data,ContentType='application/gzip')
    manifest={'key':key,'sha256':digest}
    if kind=='gateway':
        build=json.loads(pathlib.Path('gateway-release.json').read_text())
        manifest.update({field:build[field] for field in ('binarySha256','binaryBytes')})
    s3.put_object(Bucket=os.environ['ARTIFACTS_BUCKET'],Key=f'{kind}/release.json',
                  Body=json.dumps(manifest).encode(),ContentType='application/json')
    print(kind,digest)
PYRELEASE
```

Hosts check every five minutes, install atomically, and retain the current and previous releases.
For rollback, publish the previous manifest. Do not change only the host symlink: sync restores
what the manifest names. Infrastructure user data runs only at first boot, not on template updates.

## Project and gateway stacks

Deploy a store stack for each project prefix. Do not initialize real stores in a deployment test.

```sh
aws cloudformation deploy --stack-name "layr-store-$PROJECT" \
  --template-file infra/aws/template.yaml --capabilities CAPABILITY_NAMED_IAM \
  --parameter-overrides "Prefix=$PROJECT" \
  "GatewayRoleArns=arn:aws:iam::134250228509:role/$GATEWAY_ROLE_NAME,arn:aws:iam::134250228509:role/$EXECUTOR_ROLE_NAME" \
  --no-fail-on-empty-changeset
```

Use the exact reader, writer and GC role ARNs from each store's outputs in `StoreRoleArns`.
Publish `gateway/gateway.json` to the artifacts bucket as JSON
`{"host":"origin.cloud.layr.ws","publicHost":"cloud.layr.ws","zone":"Z00094863IAG61NP8YENE","cloudfront":true}`.
The host is the origin DNS and ACME name; publicHost is the allowed browser Origin for writes.
The configuration contains the secret's region only; it never contains the origin secret. Publish one
`gateway/projects/<name>.json` per served project, with the gateway's `Project` configuration fields:
`bucket`, `region`, `prefix`, `readerRoleArn`, `writerRoleArn`, `gcRoleArn`, `recipient`,
`writeTokenHashes` and `gcTokenHashes`. The recipient must match the owner's immutable store descriptor.
These files contain only public configuration and token hashes. Do not put raw tokens in them.

```sh
aws cloudformation deploy --stack-name "$GATEWAY_STACK" \
  --template-file infra/aws/gateway.yaml --capabilities CAPABILITY_NAMED_IAM \
  --parameter-overrides "ArtifactsBucket=$ARTIFACTS_BUCKET" "ImageId=$IMAGE_ID" \
  "RoleName=$GATEWAY_ROLE_NAME" "Host=$ORIGIN_HOST" "ZoneId=$ZONE_ID" \
  "StoreRoleArns=$STORE_ROLE_ARNS" --no-fail-on-empty-changeset
```

The host selects its global IPv6 address, publishes AAAA, and uses Certbot's Route 53 DNS challenge
for TLS. IAM limits DNS changes to the host and ACME names. SSM Agent uses regional dualstack endpoints;
its channels are outbound. [AWS documents the Debian agent installation](https://docs.aws.amazon.com/systems-manager/latest/userguide/agent-install-deb.html).
The gateway supports HTTP/1.1 and HTTP/2 with a listener that sets `IPV6_V6ONLY`.
It runs as a systemd dynamic user and reads the ACME pair through systemd credentials.

Read gateway outputs and open an administration session:

```sh
aws cloudformation describe-stacks --stack-name "$GATEWAY_STACK" --query 'Stacks[0].Outputs'
aws ssm start-session --target "$GATEWAY_INSTANCE_ID"
```

Inside the session use `sudo -i`, then inspect `cloud-init status`, SSM and sync unit status.
A missing archive or configuration fails closed; the sync timer retries after publication.
Do not print environment variables or secret parameter values into the session log.

## CloudFront front

Coordinate with any active gateway/executor deployment before changing AWS. Update an existing
gateway stack through a reviewed change set; refuse instance replacement. In particular, user data
runs only at first boot. Deploy the gateway IAM change so its SSM read exception includes the exact
`/layr-store/gateway/origin-secret` name, and its DNS grant names the origin host. Publish the tested
gateway release and public configuration through the normal artifacts manifest. For the first
upgrade from a sync script that does not know CloudFront, stop the gateway and sync service/timer
before publication. Use the old script's existing `Sync.release()` method to install the new release
and stable sync entrypoint without starting the gateway. Then start the sync service; the new script
writes originSecretRegion/publicHost, obtains the origin certificate and starts the guarded gateway.
Read back the actual running executable digest, guarded configuration and direct-origin 403 before
starting the sync timer and executing the distribution change set. This avoids an unguarded origin
startup during the entrypoint upgrade.

Create the origin secret once as a random 32-byte lowercase hex SecureString. Fetch it only into
SDK memory. It must not enter shell arguments, a file, public configuration or command output.
The distribution needs the value as its custom header; pass it through the template's NoEcho
parameter. Do not print full distribution configuration, which contains that header value.

Use Python 3 with boto3 for the tracked deployment helper. It checks the account, keeps the origin
secret in SDK memory, preserves existing stack parameters on update and recovers a named change set
before retrying. It refuses removal of CloudFormation resources and instance replacement.

Create the secret, then plan the viewer certificate in us-east-1. Read the printed change-set name
into CERT_CHANGE. Inspection prints only resource names/actions, never parameter values:

```sh
python3 infra/aws/cloudfront.py secret
python3 infra/aws/cloudfront.py certificate-plan
python3 infra/aws/cloudfront.py certificate-inspect --change "$CERT_CHANGE"
python3 infra/aws/cloudfront.py certificate-execute --change "$CERT_CHANGE"
aws cloudformation wait stack-create-complete --region us-east-1 \
  --stack-name layr-cloudfront-certificate
export CERT_ARN=$(aws cloudformation describe-stacks --region us-east-1 \
  --stack-name layr-cloudfront-certificate \
  --query 'Stacks[0].Outputs[?OutputKey==`CertificateArn`].OutputValue' --output text)
python3 infra/aws/cloudfront.py gateway-plan
```

Inspect and execute the printed gateway change set using gateway-inspect and gateway-execute.
Keep the previous public gateway active while the distribution deploys. Plan the front with
PublishDns=false initially:

```sh
python3 infra/aws/cloudfront.py front-plan --certificate "$CERT_ARN"
python3 infra/aws/cloudfront.py front-inspect --change "$FRONT_CHANGE"
python3 infra/aws/cloudfront.py front-execute --change "$FRONT_CHANGE"
```

Read the change-set resources before executing it. The helper checks both managed policy IDs
against the AWS API by name before planning. Wait for the stack and distribution to finish.
Then activate the preflighted guarded gateway, test through the distribution with the real public
SNI/Host, and publish the DNS aliases immediately on HTTP 200. On any error, restore the previous
release and original host configuration through Sync.release() immediately, before further diagnosis.
The distribution uses the managed CachingDisabled policy, passes every HTTP method and query string,
and has zero error-cache TTLs. Its origin policy forwards Authorization, Content-Type, Content-Length
and Range while replacing Host with the origin name. Verify that the generated viewer-address header
arrives before publishing DNS. The gateway rejects missing/malformed trusted viewer addresses.

Before the one-time DNS cutover, stop the old host-name sync path and verify the gateway now updates
only `origin.cloud.layr.ws AAAA`. Remove only the former `cloud.layr.ws AAAA` address record, then execute
the reviewed PublishDns=true change set to create CloudFormation-owned A and AAAA aliases. Do not
remove a pre-existing alias on repeat deployment. Compare protected records and the prototype's
configuration hash with the pre-deployment snapshot.

CloudFront supports IPv6-only custom origins in commercial regions. Its documented request-body
quota is 64 GB, above the 512 MiB release-file limit; no CLI limit change is required. Configure the
origin read timeout to the largest accepted value, up to 120 seconds. See the
[AWS quotas](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/cloudfront-limits.html)
and [IPv6 origin documentation](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/cloudfront-enable-ipv6.html).
The current object endpoint buffers at most 32 MiB; use a 30 MiB object for that path's transfer check.
Run the 128 MiB release-file check after the streamed release endpoint is merged and deployed.

From an IPv4-only Mac and an IPv6-capable AWS host, verify sign-in, clone/push and an encrypted comment
applied once by the executor. Verify a direct origin request without the secret returns 403 with no
project data, including a forged viewer-address header. Verify one viewer's 12/minute sign-in limit
cannot be reset by changing that header and does not exhaust the other viewer's limit. Record the
actual policy, transfer results and timeout in DEPLOYED.md.

## Executor stack

Use the gateway's VPC and subnet outputs and that subnet's availability zone. The small executor
has no Rust toolchain and runs no checks. It syncs, applies person requests and publishes the index.

```sh
aws cloudformation deploy --stack-name "$EXECUTOR_STACK" \
  --template-file infra/aws/executor.yaml --capabilities CAPABILITY_NAMED_IAM \
  --parameter-overrides "ArtifactsBucket=$ARTIFACTS_BUCKET" "ImageId=$IMAGE_ID" \
  "VpcId=$VPC_ID" "SubnetId=$SUBNET_ID" "AvailabilityZone=$AVAILABILITY_ZONE" \
  "RoleName=$EXECUTOR_ROLE_NAME" "ReaderRoleArn=$READER_ROLE_ARN" "WriterRoleArn=$WRITER_ROLE_ARN" \
  HostKind=executor InstanceType=t4g.small RootVolumeSize=12 DataVolumeSize=10 \
  --no-fail-on-empty-changeset
```

The data volume is retained and is formatted only if empty. Repeat setup verifies its serial,
filesystem and UUID. Never replace or reformat an enrolled machine disk as a repair shortcut.

## Configure the executor profile

Read `/etc/layr-executor/machine.json` through SSM. It contains only the machine ID and Ed25519
public key. On the trusted owner's machine, grant the encrypted machine slot using that public key:

```sh
layr --project "$PROJECT" remote trust origin "$EXECUTOR_MACHINE_ID" --public-key "$EXECUTOR_PUBLIC_KEY"
```

The owner opens their own key locally. Only the encrypted slot reaches the store. The executor
never receives the owner's recovery words or another machine's private identity.

On the gateway, issue the project write token directly into SSM, without printing it:

```sh
layr-cloud token "$PROJECT" --write --until 30d \
  --ssm-parameter /layr-store/executor/write-token
```

The gateway host's `layr-cloud` entry points to its local gateway administration binary. Invites and
token creation run locally through SSM. The repository's deployment CLI also provides the invite wrapper.
The executor token loader reads its exact SecureString in memory and creates a root-owned 0600 file
under `/run/layr-executor-token` only after verifying tmpfs. Stopping `layr.service` removes the file
and runtime directory. It refreshes before transfers. No token goes in a command argument or log.

In an executor root session:

```sh
export LAYR_ROOT=/var/lib/layr
export LAYR_GATEWAY_TOKEN_FILE=/run/layr-executor-token/write-token
systemctl start layr-executor-token.service
ln -sfn /opt/layr-executor/current/layr /usr/local/bin/layr
layr clone "layr+https://$GATEWAY_HOST/$PROJECT" "$PROJECT"
layr --project "$PROJECT" config queue.run ''
layr --project "$PROJECT" config registry.listen 127.0.0.1:5000
layr --project "$PROJECT" config registry.user layr-registry
layr --project "$PROJECT" access person-key "$PERSON_LOGIN" "$PERSON_SIGNING_KEY"
printf 'PROJECT=%s\n' "$PROJECT" > /etc/layr-executor/profile.env
chmod 0600 /etc/layr-executor/profile.env
touch /etc/layr-executor/ready
systemctl enable --now layr.service layr-transfer.timer
```

Check the person's public key with them through a trusted channel. Their key pin and access policy
are machine-local; a fetch cannot import them. Grant the needed `person:<login>` actions locally.
The normal transfer timer fetches, applies the current signed requests once, pushes, and publishes the index.
An apply or push failure stops that cycle before index publication. This path runs no checks.
Keep `queue.run` empty and both check runner flags false on the small executor.
After reviewed check policy setup, create `checks-ready` and enable the checks timer; in reporter mode
this loop applies requests, reports pending checks, pushes results and writes the signed index.
No remote GC runs automatically. The optional runner executes checks and returns its own signed results.

## Person enrollment and sign-in

On the gateway through SSM:

```sh
layr-cloud invite "$PERSON_LOGIN" --ssm-parameter /layr-store/invites/person/token
```

This prints the parameter name only. The seven-day invite is single use and bound to the login.
Deliver it through an approved private channel or read the SecureString directly into client memory.
Never put it in SSM command text, a file, shell tracing, terminal output or a message.
The first registration submits the person recipient, encrypted paper slot and encrypted signing key
with the invite. It must then prove the newly registered signing key to receive a session.

The browser asks for the login and opens the fixed-size unlock bootstrap. Recovery words or a passkey
open the signing key inside the worker. It signs the one-minute challenge and keeps the 12-hour bearer
in memory. The full person view, index, inbox and encrypted requests require that session.
A restart or expiry requires sign-in again. Machine read proofs and write tokens keep their existing role.
Plaintext v1 requests are refused. The gateway never decrypts the v2 action; the executor verifies its
inner signature, matching metadata, expiry, local person key and access policy before applying it.

## Verify and recover

From an IPv6-capable AWS executor through SSM, verify certificate validation and both protocols:

```sh
curl -6 --http1.1 --fail --silent --show-error -o /dev/null -w '%{http_version} %{http_code}\n' "https://$GATEWAY_HOST/"
curl -6 --http2 --fail --silent --show-error -o /dev/null -w '%{http_version} %{http_code}\n' "https://$GATEWAY_HOST/"
```

Then verify the actual client flow: machine clone/push, browser recovery unlock and signature sign-in,
and an encrypted comment followed by a signed executor outcome. Check that object writes use the
writer bearer and that a writer makes only its permitted anonymous public-manifest side request.
Record the exact source revisions, commands, nonsecret output and test-data deletion in DEPLOYED.md.

Run template and host-script checks in the Linux VM:

```sh
cfn-lint infra/aws/*.yaml
python3 -m unittest discover -s infra/aws -p 'test_*.py' -v
cargo test --locked --workspace --all-targets
```

Use a debug gateway for synthetic MinIO credentials; release builds deliberately refuse that option.
Test recovery material stays under `/run` tmpfs on the trusted local VM and is removed after the test.
Do not create real project owner keys as part of smoke verification.

Rollback through release manifests, not ad hoc symlink changes. Stop the optional runner and executor
before deleting the gateway subnet. Retained buckets and data disks remain billable until separately
removed after recovery verification. In a shared zone, remove only this deployment's allowed host and
ACME records; keep the zone, delegation, apex, desk and ACM validation records.

## Optional check runner

Reuse `executor.yaml` for the runner. Set `HostKind=runner`, `InstanceType=t4g.large`,
`DataVolumeSize=60`, `RootVolumeSize=8`, a distinct role and hostname, and separate runner SSM parameter names.
The extra root disk is separate from the 60 GiB btrfs data disk.
The template installs the runner tools and enables its idle timer.
The runner has no EC2 start grant. Its project roles include reader and writer, with no GC role.

Deploy this host only after the local M3 watcher is merged into the selected Layr main revision.
Enroll its own machine slot through the trusted owner, as for the executor.
The token is project-scoped and follows the same SSM-to-tmpfs rule.
Use `/layr-store/runner/write-token` on this host. No tailnet auth key is required.

After both machines are enrolled, provision each host's local reviewed check policy.
Imported policy is namespaced by its source machine and does not become effective automatically.
Run the following as root on each host, using that host's project:

```sh
export LAYR_ROOT=/var/lib/layr
layr --project "$PROJECT" access grant layr-checks reader
# Run the reviewed tools/checks/configure.sh from the checked-out project on this host.
# On the runner only:
layr --project "$PROJECT" config --local checks.runner true
layr --project "$PROJECT" config --local checks.privileged-runner true
test -S /run/layr.sock
runuser -u layr-checks -- env LAYR_ROOT=/var/lib/layr LAYR_SOCKET=/run/layr.sock \
  layr --project "$PROJECT" change list --json
touch /etc/layr-executor/checks-ready
systemctl enable --now layr-checks.timer
```

Before enabling the runner timer, seed its offline dependencies from the verified checked-out source:

```sh
source_dir=$(layr --project "$PROJECT" --line main rev-parse --show-toplevel)
(
  cd "$source_dir"
  CARGO_HOME=/usr/local/share/layr-checks/cargo \
  RUSTUP_HOME=/usr/local/share/layr-checks/rustup \
    /usr/local/share/layr-checks/cargo/bin/cargo fetch --locked
)
```

Use this seed on the runner only. It must contain no credential files.
The normal sandbox has no network and copies the cargo seed into its private home.
Repeat the fetch after a reviewed lockfile update, before enabling its new check commands.

The executor runs the watcher only in reporter mode, with both local runner flags false. It needs no Rust.
Keep its `queue.run` empty; applying a person queue request must not launch checks on this small host.
After check setup, enable `queue.run=main` on the runner only.
Set `RunnerInstanceId` on the executor stack to the exact runner instance ID.
Its IAM policy can describe instance state and start only that instance with `layr:kind=runner`.
A positive read-only pending count wakes the runner. The executor runs no checks. No SSH connection is used between the hosts.
The runner independently fetches, scans all configured checks, and pushes the signed results.
A host lock prevents shutdown during a scan or tool setup.
After 30 minutes with no check work, its idle timer powers off the host. EC2 stops it and retains both disks.

See [DEPLOYED.md](DEPLOYED.md) for the policy snippet, cost formula, measured resources, and current deployment limits.
