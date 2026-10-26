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

## Flow

1. The client asks the gateway for an object, as today, with its read token. It adds a header that says it can follow a CDN link.
2. The gateway checks the read token for that project and epoch, checks that the key is in a cacheable class and that the object exists. Then it answers with a short-lived CloudFront signed URL for `/o/<store-alias>/<key>` (canned policy, path-exact, expiry 5 minutes). A client without the header gets the bytes as today.
3. The client fetches the signed URL. CloudFront checks the signature, then serves from its cache or from S3.
4. The client checks the bytes as it does now (age decryption, record signatures, SHA where it has one). A wrong or missing object falls back to the direct gateway route once, then fails.

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

## Clients

- CLI (crates in layr): follow the CDN link on the cacheable routes, with bounded parallel fetches. Keep the gateway route as the fallback.
- Site worker: the same, through fetch on the same origin.
- Both send the capability header only for cacheable keys.

## Checks

- The gateway signs only for cacheable keys, only after a valid read token, only for existing objects. Everything else gets the old answer. Tests for each class and for each refusal.
- A signed URL for one key does not open another key, and an expired one gets 403.
- `/o/*` without a signature gets 403. A forged or reused-after-expiry signature gets 403.
- A hidden overlay object: no signed URL without access, and the same 404 as before.
- A fresh clone of the layr-studio fixture: time, gateway request count, gateway host CPU and egress, and the CloudFront hit ratio on a second clone, before and after.
- The site loads a large project through the CDN path, with the same browser checks as before.
- The fallback works when the CDN path is blocked.
- Cost: the measured CloudFront and S3 request and transfer cost for these clones, written into DEPLOYED.md.
