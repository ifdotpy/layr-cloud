# Public gateway without Tailscale (lead decision, 2026-10-25)

Tailscale is no longer required. It stays optional for people who want a private network.

## Network

- The gateway serves public HTTPS on `cloud.layr.ws`, **IPv6 only** for now (AAAA record, no public IPv4 address).
- TLS: ACME with the Route 53 DNS challenge (the runbook already supports it). HTTP/1.1 and HTTP/2.
- Security group: TCP 443 from `::/0`. No SSH port.
- Admin access to hosts: AWS SSM Session Manager. No Tailscale SSH.
- Rate limits per client address on every route; the existing anonymous public limits stay.
- IPv4 can be added later in front (for example CloudFront) without a host change.

## Person sign-in (replaces Tailscale whois)

- `GET /api/session/challenge?login=<login>` returns a random challenge (32 bytes, 60 seconds, single use).
- The client signs `layr/session/v1\0 || login || \0 || challenge` with the person signing key (the same key as request v2).
- `POST /api/session` with login, challenge and signature. The gateway verifies against the stored person signing key and returns a session token: random, 12 hours, stored only as a hash, bound to the login.
- Every person route (requests v2, person record, inbox) takes `Authorization: Bearer <session>` instead of whois.
- First registration of a person (no signing key yet): needs an invite token made by an owner (`layr-cloud invite <login>`), single use, 7 days. This replaces "anyone on the tailnet".
- The site signs in after unlock (passkey or recovery words already open the signing key in the worker). The CLI signs in with `LAYR_PERSON_KEY_FILE`.

## Machines

Unchanged: read proof by machine key challenge, writes by write token.

## Checks

- Sign-in with a wrong key, a reused or expired challenge, and an expired session is refused.
- Request v2 with a session for login A cannot write as login B.
- No route reads Tailscale identity when the gateway runs without the Tailscale socket.
- Live: site unlock, sign-in and an encrypted comment over public IPv6 HTTPS from the VM.


## Wire contract

The signature bytes are defined once in `layr_store::session::message(login, challenge)`.
The challenge is canonical padded STANDARD base64 for 32 random bytes. Decode it before signing:
`b"layr/session/v1\0" || login UTF-8 || b"\0" || decoded challenge bytes`.
`session::sign` returns a lowercase hexadecimal Ed25519 signature (64 bytes, 128 characters).

- `GET /api/session/challenge?login=<percent-encoded login>` returns 200 with `{challenge, expires}`.
  Expiry is Unix milliseconds. Unknown logins get the same response type. A challenge lasts 60 seconds
  and is consumed on the first exchange attempt, including an invalid signature.
- `POST /api/session` takes JSON `{login, challenge, signature}` and returns 200 with
  `{login, token, expires}`. Invalid, expired or used proofs return 401. Tokens last 12 hours;
  only their hashes and login bindings are kept in gateway memory. Tokens contain 32 random bytes
  encoded as 64 lowercase hexadecimal characters. A restart ends existing sessions.
- `GET /api/person/unlock?login=<login>` is the unlock bootstrap exception to session authentication.
  It returns 200 with `{unlock: <STANDARD base64>}`. Decode the field and parse the JSON, including
  trailing ASCII space padding. The decoded value is exactly 65536 bytes, containing
  `{login, recipient, slots, signing_key, signing}` and no index or inbox.
  An unknown login returns a stable HMAC-derived paper-slot decoy with the same outer shape and size.
  Its HMAC key is an exact SSM SecureString loaded into memory, so decoys stay stable across restarts.
  The gateway permits at most eight person slots, each at most 4096 bytes, plus a sealed signing key
  of at most 4096 bytes. These bounds fit the fixed bootstrap response.
- First registration is `PUT /api/person?login=<login>`, with an invite bearer and JSON
  `{recipient, paper, signing_key, signing}`. Success is 200 `{login, new:true}`. The invite is
  single use and bound to that login. An existing person returns 409 and is never overwritten.
  Registration grants no session: the client must then prove the newly registered signing key.
- Every other person API and person request write uses `Authorization: Bearer <session>`.
  A person session does not replace the project read proof, machine identity or machine write grant.
  There are no authentication cookies and no Tailscale identity lookup.

The global socket-address rate limit is 600 requests per minute. Unlock, challenge and session exchange
share a stricter limit of 12 requests per minute per socket address. Forwarding headers are ignored.
A 429 response has `Retry-After: 60`. Sessions and outstanding challenges are each bounded to 4096 entries.

An administrator creates an invite on the gateway through SSM:
`layr-cloud invite <login> --ssm-parameter /layr-store/invites/<name>/token`.
The command stores only the token hash, login and seven-day expiry in the gateway bucket. It sends the
raw token directly to a new SSM SecureString in memory and prints only the parameter name.
The token must never be included in Run Command text, command arguments, output or logs.

Unknown-login unlock fields follow the age 0.11.5 X25519 packet structure. The header contains one
X25519 stanza and one GREASE stanza. As in age-core 0.11.0 `format::grease_the_joint`, GREASE
uses a tag prefix of 1–8 printable ASCII characters (33–126), 0–4 arguments of 1–8 such characters,
and 0–99 body bytes. All ranges are uniform. HMAC with rejection sampling selects the shape and
fills the opaque bytes, separately for each login and field. Base64 body lines wrap at 64 characters
and end with a short line, including an empty final line when required. The payload lengths match
a 75-byte paper-slot identity and a 32-byte signing seed. Thus decoy lengths follow the real GREASE
distribution instead of using one fixed median length. The persistent SSM secret keeps each response
stable across requests and restarts. The outer padded unlock buffer remains 64 KiB.
