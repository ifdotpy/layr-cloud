# Cached delivery of immutable objects through CloudFront (lead decision, 2026-10-26)

Goal: a fresh clone, a runner sync and a site load fetch immutable ciphertext from the CloudFront edge or straight from S3, not through the gateway host. Access control stays the same: the gateway still decides who may read what.

Measured before (docs/cloudfront-front-design.md, infra/aws/DEPLOYED.md): a fresh clone of layr-studio is about 15,000 objects and 367 MB, and every byte goes S3, then the t4g.nano gateway, then CloudFront. A one-commit push is about 20 requests and does not need this.

## What may be cached

Only objects that never change after creation (create-once):

- states, state trees and their files: `states/...` and `project/epochs/<e>/states/...`
- record segments: `records/<machine>/<from>-<to>.age` and the epoch form
- `index/paths/...` and `index/graph/...`
- release files: `files/<uuid>.age` and the epoch form
- sealed epoch history: `project/epochs/<e>/history/...`

Never cached: `store.json`, `public.json`, slots, `keys/` discovery, `index/heads/...`, requests, claims, person records, listings, challenges and tokens. Their routes stay exactly as today.

## Flow (lead decision 2026-10-26)

1. The client asks the gateway for an object, as today, with its read token. It adds a header that says it can follow a CDN link.
2. The gateway checks the read token for that project and epoch, checks that the key is in a cacheable class and that the object exists. Then it answers with a short-lived CloudFront signed URL for `/o/<store-alias>/<key>` (canned policy, path-exact, expiry 5 minutes). A client without the header gets the bytes as today.
3. A batch signing request carries at most 256 keys for one store and one epoch, under one read proof. The gateway applies the same authorization, cacheable-class, existence and epoch checks to every key. It returns a signed URL or the same refusal as the single-key request for each key. The single-key form remains available.
4. The client fetches the signed URL. CloudFront checks the signature, then serves from its cache or from S3.
5. The client checks the bytes as it does now (age decryption, record signatures, SHA where it has one). A wrong or missing object falls back to the direct gateway route once, then fails.

## CloudFront and S3

- The same distribution as cloud.layr.ws, so the site needs no CORS. A new behavior `/o/*` goes to the store buckets through Origin Access Control. The bucket policy allows reads only from this distribution and only under the store prefixes.
- The behavior requires a trusted key group. The signing private key lives in SSM as a SecureString. The gateway reads it at start, like the origin secret. Rotation: two keys in the group, a documented swap.
- Cache key: path only. The signature query parameters are not part of it, so all readers share one cached copy. TTL: one year, because the objects never change.
- No other behavior changes. `/o/*` without a valid signature gets 403 from CloudFront.

## Security notes

- A cached object is reachable only with a signed URL, and only the gateway signs, after its normal check. So rule 8 holds: the existence of a hidden overlay object is not revealed.
- After a reader removal or a rotation, the gateway stops signing for that reader at once. A URL that was already issued stays valid for at most 5 minutes. That is the same exposure as a read that started before the removal, and the content is ciphertext.
- After GC deletes an object, a cached copy may stay at the edge until its TTL, but nobody gets a signed URL for it, because the gateway checks that the object exists.
- The signature scheme is the standard CloudFront signed URL. It is AWS's own scheme, and we use an existing RSA library for it. No own cryptography.

## Clients (lead decision 2026-10-26)

- CLI (crates in layr): follow the CDN link on the cacheable routes, with bounded parallel fetches. Keep the gateway route as the fallback.
- Site worker: the same, through fetch on the same origin.
- Both send the capability header only for cacheable keys, use batch signing for object sets, and limit concurrent signing requests to four.
- A 503 busy response waits for Retry-After with jitter before a bounded retry. It does not trigger the same burst through the direct fallback.
- The outer connection cap is 128. CloudFront uses HTTP/1.1 to the custom origin, so 32 connections cannot fill the 64-waiter queue. Keep-alive is disabled; each connection serves one request. Header reads time out after 15 seconds; TLS handshakes time out after ten seconds.
- All gateway routes share four active workers and at most 64 queued requests. A queued request waits at most ten seconds. A full queue or timeout returns 503 with Retry-After: 1, increments the refusal counter and logs the reason without the request path or credentials.

## Checks

- The gateway signs only for cacheable keys, only after a valid read token, only for existing objects. Everything else gets the old answer. Tests for each class and for each refusal.
- A signed URL for one key does not open another key, and an expired one gets 403.
- `/o/*` without a signature gets 403. A forged or reused-after-expiry signature gets 403.
- A hidden overlay object: no signed URL without access, and the same 404 as before.
- A fresh clone of the layr-studio fixture: time, gateway request count, gateway host CPU and egress, and the CloudFront hit ratio on a second clone, before and after.
- The site loads a large project through the CDN path, with the same browser checks as before.
- The fallback works when the CDN path is blocked.
- Cost: the measured CloudFront and S3 request and transfer cost for these clones, written into DEPLOYED.md.

### Batch request and response (lead decision 2026-10-26)

`POST /v1/p/<store>/cdn` accepts JSON `{"epoch":"root","keys":["records/<machine>/<from>-<to>.age"]}` and one project read proof. An epoch UUID replaces `root` for a scoped request. The maximum is 256 keys, all relative to that epoch.

The response preserves input order: `{"objects":[{"status":200,"url":"https://...","expires":1793000000,"bytes":4096}]}`. Each failed entry has the same status and error body as the single GET: `{"status":404,"error":{"error":"no such object"}}`. A readable object outside the cacheable classes returns `{"status":200,"direct":true}`. The gateway checks access and existence without reading an object body. It does not expose a hidden object's existence through the batch route.

Clients include only cacheable classes in a batch. A `direct:true` reply indicates a client defect: log it at debug level and use the direct GET. It does not make that key eligible for caching.

The Linux test build used 21,159,936 bytes of RSS (20.18 MiB) with 128 open TLS HTTP/1.1 connections and all 64 queue slots occupied. The idle test process used 9,711,616 bytes. A separate process held the client sockets. This measures transport and queue memory; the test held the four worker permits without loading S3 payload bodies. At the same time, the production nano reported 328,552,448 bytes of available memory. See `benchmarks/cdn/admission-rss.json` for the measured fields and limits.

### Batch execution and deadline retry (lead review, 2026-10-26)

The gateway checks at most 16 keys in parallel inside a batch, and returns entries in input order. A regional measurement of 256 S3 HEAD requests from the gateway took 4.895 seconds in series and 0.712 seconds with 16 parallel calls; the ordered object lengths matched. The median serial HEAD took 18.10 ms. This measurement covers S3 metadata calls, not gateway authorization or RSA signing.

If the whole batch returns HTTP 408, both clients split it into two smaller batches and process those halves. Each further 408 halves the affected batch again. They do not repeat the full batch after that deadline. A deadline for a single key ends the operation; it does not start a direct-read burst.

For a CLI clone, one restore planner groups both tree and data keys from the existing state listing, in parent-first restore order. Each ordinary window contains up to 256 keys. The client downloads each object only when verification needs it. It renews expired links and refreshes a window after four minutes if local restore work takes that long. A single state with more than 256 objects continues through the bounded per-file path after the first window.

The reader retains at most 64 MiB of verified compressed tree frames, separately from its small cache of expanded trees. It verifies the reconstructed canonical tree and signature again when it reuses a compressed frame. This avoids another object fetch when restore revisits a parent after the expanded-tree cache was cleared. The frame cache is local to the reader operation and is discarded when that reader closes. Eviction can require another fetch for a larger history. Tree-depth tracking, the 32-state chain bound and seed read counters remain in force.
