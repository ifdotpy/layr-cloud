# Layr Cloud

Layr Cloud runs [Layr](https://github.com/ifdotpy/layr) in your own AWS account.
Each project has an encrypted S3 store.
The gateway serves encrypted objects over HTTPS.
The design uses person keys to give each person access to their stores.
The executor runs Layr and project tasks.
CloudFormation templates create the AWS resources.
Person keys and the web UI need further work.
The current gateway uses store identity proofs and separate write and garbage collection (GC) tokens.

## Build and test

Install Rust, then run:

```sh
cargo build --locked
cargo test --locked
python3 -m pip install boto3 PyYAML
python3 -m unittest discover -s infra/aws -p 'test_*.py'
```

The binary is `target/debug/layr-gateway`.
The shared `layr-store` crate comes from Layr revision `14639be`.
The `web/` folder contains groundwork for the future UI and is excluded from CI.
CI means continuous integration.
The [AWS runbook](infra/aws/README.md) describes host setup and release archives.

## Store gateway

`layr-gateway serve` serves encrypted store objects over HTTPS without a local project or snapshot filesystem.
The server refuses to run as root.
Use the [AWS runbook](infra/aws/README.md) for the host, certificates, and public-key enrollment.

```sh
layr-gateway serve --config /etc/layr-gateway/config.json
layr-gateway serve --config ./gateway.json --tls files --cert ./tls.crt --key ./tls.key
layr-gateway serve --config ./gateway.json --tls tailscale --tls-dir /etc/layr-gateway/tls
```

The listen address must belong to the tailnet or loopback.
`--tls none` permits loopback only, for local tests.
The default TLS mode reads the systemd credentials `tls.crt` and `tls.key`.
The `tailscale` mode reads `<host>.crt` and `<host>.key` from the TLS folder.
A renewed certificate replaces the previous certificate after the new pair loads successfully.
The host script runs `tailscale cert` or Certbot to renew those files.

The server reads config changes on each request.
A new listen address requires a service restart.
An invalid config fails closed.
The config file must not permit group or public writes.

Issue write and GC tokens on the gateway host:

```sh
layr-gateway token example --write --until 24h --config /etc/layr-gateway/config.json
layr-gateway token example --gc --until 1h --config /etc/layr-gateway/config.json
```

The token command can run as root.
It prints the new token once and atomically adds its SHA-256 hash to the config.
Token updates and the host script use the same file lock.
The default token duration is 24 hours.
Duration units are `s`, `m`, `h`, and `d`.
Read requests use a slot challenge and a bearer token that lasts at most ten minutes.
Write tokens can list names under `records/` and `states/`.
Write tokens cannot read object bodies or add slots.
A slot write requires proof of the store identity.
GC tokens can delete only `states/<state>/<part>.data.age` objects.
The gateway requires conditional creation in S3 and returns HTTP 412 when an object already exists.

The server accepts four connections at a time and closes each connection after one request.
Limits: 4 KiB request line, 64 headers, 16 KiB header bytes, 32 MiB object body, and 16 KiB slots.
It refuses chunked requests, duplicate headers, folded headers, unsafe paths, and multiple byte ranges.
Header, body, and response time limits are 15, 120, and 60 seconds.
A TLS handshake has a time limit of ten seconds.
A list returns at most 10,000 objects; the slots endpoint returns at most 1,024 slots.
The gateway never opens or decrypts state data.

Add the gateway URL to an existing project:

```sh
export LAYR_GATEWAY_TOKEN_FILE=/private/path/write-token
chmod 600 "$LAYR_GATEWAY_TOKEN_FILE"
layr remote add cloud layr+https://gateway.example.com/project
layr push cloud
layr fetch cloud
```

The token file must be a regular file with mode `0600`, owned by the caller.
The client refuses symbolic links and does not save the token in project data.
For a private certificate authority, set `LAYR_GATEWAY_CA_FILE` to its PEM certificate file.
The client adds these roots and keeps certificate and hostname checks enabled.
Read commands use the machine slot or `LAYR_PAPER_KEY_FILE` to answer a fresh challenge.
A gateway remote cannot initialize an empty store. Create it first with the owner's S3 credentials.
Enroll an executor with its public key as the AWS runbook describes.

The MinIO integration test uses a separate temporary bucket and a server that runs as `nobody`.
It verifies conditional writes, pagination, ranges, grants, malformed requests, config revocation, and real CLI push, clone, fetch, and GC.
It does not create AWS resources.
Use test credentials for a local MinIO server:

```sh
cargo build
sudo env LAYR_MINIO_ENDPOINT=http://127.0.0.1:19000 \
  LAYR_MINIO_ACCESS_KEY=layrtest LAYR_MINIO_SECRET_KEY=layrtestsecret \
  LAYR_BIN=/path/to/layr bash tests/t27-gateway.sh
```

Debug builds expose `--test-static-credentials` for this test only.
It requires a loopback listener and a loopback HTTP S3 endpoint.
The test uses verified TLS with a temporary certificate authority.
Release builds exclude this credential path.
Production uses IMDSv2 and STS role sessions.
