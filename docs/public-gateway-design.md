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
