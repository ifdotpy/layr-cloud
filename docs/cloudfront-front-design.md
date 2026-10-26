# CloudFront in front of the gateway (lead decision, 2026-10-26)

Problem: the gateway host is IPv6 only. Many networks, including igor's Mac today, have no IPv6, so they cannot reach `cloud.layr.ws`.

Decision (igor, 2026-10-26): put CloudFront in front of the gateway. CloudFront accepts IPv4 and IPv6 from clients.

## Names

- `cloud.layr.ws` points to the CloudFront distribution (Route53 alias A and AAAA). The certificate is ACM in us-east-1, with DNS validation in Route53.
- The gateway host gets the name `origin.cloud.layr.ws` (AAAA only) and keeps its own ACME certificate for that name.
- The prototype `layr.ws` and its distribution do not change.

## Origin

- CloudFront connects to the origin over HTTPS only and checks its certificate.
- CloudFront adds a secret origin header. The secret lives in SSM; the gateway reads it at start. The gateway refuses any request without the right header with 403, so the origin is not a second public entry. Restrict the security group to the CloudFront origin-facing prefix list as well, if that list covers IPv6.
- No caching: the managed CachingDisabled policy. Pass all methods, the query string, and the headers the API needs (Authorization, Content-Type, Content-Length, Range).
- The gateway takes the client address for rate limits from CloudFront-Viewer-Address, and only when the origin header is valid.

## Limits to verify before deploy

- Request body size for release file uploads (up to 512 MiB plus age overhead) and streaming downloads through CloudFront. If CloudFront has a lower limit, record it and keep the CLI on a limit that works.
- Origin response timeout for long responses (set it to the largest allowed value, 60 s or more if the account allows).
- IPv6 origin support in CloudFront for this account and region.

## Trust

- CloudFront terminates TLS, so AWS edge servers see bearer tokens and request paths. Project data stays end-to-end encrypted; no plaintext of code or comments reaches CloudFront. This is the same trust as running the gateway on AWS.
- The site and the CLI need no change, because the name stays `cloud.layr.ws`.

## Checks

- From an IPv4-only network (igor's Mac): site sign-in, CLI clone and push, one comment applied by the executor.
- From IPv6: the same through CloudFront.
- Direct `https://origin.cloud.layr.ws` without the origin header: 403, and no project data.
- Rate limits still apply per client address, and a forged CloudFront-Viewer-Address header without the origin header has no effect.
- A 128 MiB release file upload and download through CloudFront, or the recorded limit.
