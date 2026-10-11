# A gateway of your own

A person who does not want to trust the owner of `desk.layr.ws` (or any shared gateway) runs a
gateway in their own AWS account and their own tailnet. Their browser then loads the site code
from their own server, and the vaults they read never pass through someone else's server. A
vault of a shared project trusts the gateway of each person who reads it.

The gateway needs no domain: it serves its tailnet name, `vault-gateway.<tailnet>.ts.net`, with
a certificate from the tailnet.

## The person with the new gateway

1. In the tailnet admin console, turn on MagicDNS and HTTPS certificates.
2. In the tailnet policy, let the tag `tag:vault` exist and let the members reach it:

   ```json
   "tagOwners": { "tag:vault": ["autogroup:admin"] },
   "grants": [{ "src": ["autogroup:member"], "dst": ["tag:vault"], "ip": ["tcp:443"] }],
   "ssh": [{ "action": "accept", "src": ["autogroup:member"], "dst": ["tag:vault"], "users": ["root"] }]
   ```

3. Make an auth key: not reusable, not ephemeral, pre-approved, tag `tag:vault`. Store it in
   your AWS account from your own terminal:

   ```sh
   read -s KEY && aws ssm put-parameter --profile <profile> --region eu-central-1 \
     --name /studio-vault/gateway/tailscale-auth-key --type SecureString --value "$KEY" && unset KEY
   ```

4. Read the code you are about to run, then deploy it:

   ```sh
   pnpm install
   node js/vault/bin/vault-gateway.mjs --tailscale --profile <profile>
   ```

   Building the release needs Rust with the `aarch64-unknown-linux-musl` target, zig and
   `cargo-zigbuild` (see `js/vault/gateway/build.mjs`). Within 5 minutes the server joins the
   tailnet and gets its certificate; the site is then `https://vault-gateway.<tailnet>.ts.net/`.

5. Send the gateway role ARN that the command prints to the owner of each vault you read.

## The owner of a vault

1. Add the other gateway to the vault's trust:

   ```sh
   layr-desk deploy --project <prefix> --profile <vault account> \
     --also-gateway <their gateway role ARN>
   ```

   Give every gateway that reads the vault, each time: the list replaces the earlier one.

2. Send `~/.config/studio-vault/<prefix>/facts.json` to the other person. It holds public facts
   only: prefix, bucket, region, reader role and recipient.

## Back on the new gateway

```sh
node js/vault/bin/vault-register.mjs --facts facts.json
```

The gateway serves the vault within 5 minutes. Opening it still needs a slot: the vault owner
adds one for the person (a slot sealed to that person's own key), so no key passes through
anyone else.
