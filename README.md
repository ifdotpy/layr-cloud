# Layr Cloud

Layr Cloud runs [Layr](https://github.com/ifdotpy/layr) in your own AWS account.
Each project has an encrypted S3 store.
The gateway serves encrypted objects over HTTPS.
The design uses person keys to give each person access to their stores.
The executor runs Layr and project tasks.
CloudFormation templates create the AWS resources.
The web UI needs further work.
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
The shared `layr-store` crate comes from Layr revision `82314b72b7d3175310c9fdd1114948b606556790`.
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

## Person keys and access

The gateway asks the local tailscaled for the login of each peer address.
It ignores login headers from the client.
Tagged devices have no person login.
Use `--tailscale-socket <path>` to select another tailscaled socket.
The default is `/var/run/tailscale/tailscaled.sock`.

A store without person slots remains visible to tailnet devices.
A store with person slots shows only to logins that have a slot.
Its other routes return the same HTTP 404 response as an unknown store.
A valid store read token can still read objects until it expires.
The public slots route returns the caller's own person slot and excludes other person slots.

The optional `people` configuration names the gateway's own bucket, region, and prefix:

```json
{"people": {"bucket": "gateway-artifacts", "region": "eu-central-1", "prefix": "people/"}}
```

The host script adds this configuration.
For person records, the gateway role reads and writes that prefix in the artifacts bucket.
Person records contain a public recipient, encrypted slots, an encrypted index, and encrypted invites.
The index is at most 48 KiB.
An inbox holds at most 64 invites, each at most 4096 bytes.
The gateway never receives a person identity or paper key.

The person API preserves the Desk paths and formats:

- `GET /api/person`: the caller's login, recipient, slots, index, and inbox.
- `PUT /api/person`: register `{recipient, paper}` once. A different recipient returns HTTP 409.
- `GET /api/person/challenge`: an age ciphertext with a token for this person.
- `PUT /api/person/index`: replace `{data, previous}` with a person bearer token.
- `PUT|DELETE /api/person/slots/passkey-<id>`: change a passkey slot with a person token.
- `DELETE /api/person/inbox/<id>`: remove an invite with a person token.
- `POST /api/people/<login>/inbox`: add `{data}` as the caller's login.
- `GET /api/projects`: list the stores visible to the caller.

`previous` is the SHA-256 of the index ciphertext, or null for the first index.
A stale value returns HTTP 409.
The person token is the decrypted challenge, as in Desk.
It differs from the store protocol, which exchanges a challenge for a read token.

Person API writes and store token requests require `Content-Type: application/json`.
The v1 object PUT protocol uses `Content-Type: application/octet-stream`.
Every write refuses an `Origin` from another site or `Sec-Fetch-Site` other than `same-origin` or `none`.
The gateway grants no cross-origin preflight requests.

Use `layr person keys --site https://gateway.example.com` to register a person key.
Use `layr remote grant cloud --person carol@example.com --recipient age1...` to add a person slot and send an invite.
The first person grant must name the owner's own login.
The owner needs the store identity to grant another person.
A new person key gets a separate slot with its recipient fingerprint.
The gateway creates it conditionally and never replaces an object.
Use `layr person accept --site https://gateway.example.com --invite <id>` to accept an invite.
The client checks the descriptor and person slot before it updates the encrypted index.
The client keeps no person identity or paper key in project configuration or records.

V2 person slots use `slots/person-<login>-<fp>.age`.
`fp` is the first 16 lowercase hexadecimal characters of SHA-256 of the person's age recipient string.
The reader computes this name and reads the exact slot through `GET /v1/p/<project>/slots/<name>.age`.
That route returns only slots for the caller's own tailnet login.
Old person slots stay in the store until a separate collection design exists.
A repeated grant prints `already granted` and sends the sealed invite again.
