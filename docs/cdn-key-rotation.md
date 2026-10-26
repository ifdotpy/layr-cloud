# Rotate the ciphertext signing key

The gateway reads one RSA 2048 private key from SSM at startup. CloudFront trusts
two public keys. Private keys stay in the exact SecureString parameters
`/layr-store/gateway/cdn-signing-a` and `/layr-store/gateway/cdn-signing-b`.
Do not export a private key, put it in a release archive, or print a signed URL.
The gateway role can read these parameters. It cannot replace them.

Use the repository's `infra/aws/cdn.py` helper with an operations identity that
can update the named SSM parameters and the existing CloudFormation stack.
Its Python environment needs `boto3`, `PyYAML` and `cryptography`.
The commands below use `CDN_PY` for that environment's Python executable and
`AWS_PROFILE=ifdotpy`. Run them from the repository root.

This example switches from slot `a` to slot `b`. Reverse the letters next time.
Check the public `cdn.keyParameter` field in
`s3://layr-bootstrap-artifacts-ik3kxdaezcnd/gateway/gateway.json` first.
Use unique receipt paths for each rotation and keep them until it is complete.
Receipts contain public keys, configuration metadata and resource IDs.

## Prepare the inactive slot

1. Confirm that the gateway serves the configured active key. Wait at least ten
   minutes after the previous confirmed signer switch. Signed URLs last five
   minutes. This wait also allows configuration synchronization to complete.
   Do not rotate while a deployment is unhealthy or its signer is uncertain.
2. Prepare the inactive key and its change set:

   ```sh
   AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py plan-rotation \
     --slot b --rotation-receipt /tmp/layr-cdn-rotation-b-01.json
   ```

   The helper refuses the active slot. It generates the new private key in
   memory, stores it directly in SSM and records only its public key. A retry
   with the same receipt checks SSM before it writes again.
3. Inspect the returned change set:

   ```sh
   AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py inspect --change "$CHANGE"
   ```

   Require one new CloudFront public-key resource and a key-group update.
   The old public-key resource stays present. CloudFront cannot replace the PEM
   of an existing public key; changing only its comment does not rotate it.
   The distribution, gateway origin, cache behavior and DNS definitions must
   stay the same. The group must contain exactly two trusted keys.
4. Execute that change set and wait for the stack and distribution to finish:

   ```sh
   AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py execute --change "$CHANGE"
   AWS_PROFILE=ifdotpy aws cloudformation wait stack-update-complete \
     --region eu-north-1 --stack-name layr-cloudfront
   AWS_PROFILE=ifdotpy aws cloudfront wait distribution-deployed \
     --id E3SU1MYTKJEE06
   ```

5. Test the new key before the gateway uses it:

   ```sh
   AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py probe --slot b \
     --receipt /tmp/layr-cdn-probe-b-01.json
   ```

   This creates one owned test object in the disposable store. It checks signed
   access, a cache hit, path binding and expiry, then removes that exact object
   version. It does not print a bearer URL. The completed receipt records the
   result; repeating its path does not run another probe.

## Switch the gateway

1. Publish the new public configuration with a conditional S3 write:

   ```sh
   AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py configure-source --slot b \
     --source-receipt /tmp/layr-cdn-switch-b-01.json
   ```

   The helper verifies that SSM's public key matches the trusted CloudFront key
   ID. It preserves unrelated source fields. A lost reply is recovered by
   reading the current source and comparing its exact expected content.
2. Through SSM, start `layr-gateway-sync.service`. Confirm that
   `layr-gateway.service` is active, the running process started after the new
   configuration was installed, and `/etc/layr-gateway/config.json` names slot
   `b` and its expected public key ID. Read only those public fields.
3. With an authorized project reader, fetch one immutable object through the
   gateway's capability route. Keep the URL in memory; check only that its
   `Key-Pair-Id` matches the new configured ID. Verify the returned ciphertext
   through the normal CLI or site reader. Confirm direct-origin requests and
   forged viewer-header requests still return 403.
4. Record the successful switch time. Keep both current public keys trusted.
   If the gateway check fails, use `configure-source --slot a` with a new source
   receipt, synchronize, and verify the previous signer before proceeding.

To disable CDN delivery during an incident, use `configure-source --disable-cdn`
with a new `--source-receipt`, then synchronize and verify a direct read. This
keeps the signed-URL infrastructure in place and stops the gateway issuing links.
Do not delete a stack or replace an instance for a signer problem.

## Retire the replaced public-key resource

After the switch checks pass and the ten-minute interval has elapsed, take the
old logical resource ID from `previousLogicalKey` in the rotation receipt.
This is the resource that left the trusted group during preparation, not the
other currently trusted slot.

```sh
AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py plan-retirement \
  --retire-key "$OLD_LOGICAL_KEY"
AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py inspect --change "$RETIRE_CHANGE" \
  --retire-key "$OLD_LOGICAL_KEY"
AWS_PROFILE=ifdotpy "$CDN_PY" infra/aws/cdn.py execute --change "$RETIRE_CHANGE" \
  --retire-key "$OLD_LOGICAL_KEY"
```

Require removal of only that unreferenced public-key resource. The helper refuses
keys still referenced by the key group, outputs or any other template field.
Retiring unused resources prevents accumulation against CloudFront's key quota.
Keep the two SSM parameter names and the two trusted slots for the next rotation.
