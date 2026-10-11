#!/usr/bin/env node
// Deploy or update the vault gateway (cloud/gateway.yaml, docs/vault.md "Gateway"): one server in
// the tailnet that serves the site and every vault. `layr-desk deploy` registers each vault with it.
//
//   node vault-gateway.mjs --host <desk.example.com> --profile <aws profile> [--region <region>]
//        [--domain <example.com>] [--new-image]
//   node vault-gateway.mjs --tailscale --profile <aws profile> [--region <region>] [--new-image]
//
// With --tailscale the gateway has no domain of its own: it serves its tailnet name
// (vault-gateway.<tailnet>.ts.net) with a certificate from the tailnet, so no Route 53 zone and
// no registrar take part. The tailnet needs MagicDNS and HTTPS certificates turned on.
//
// The registrar of the domain must use the name servers that this prints. The tailnet auth key
// goes to SSM before the first start (docs/vault.md, "Gateway").
import { PutObjectCommand, S3Client } from "@aws-sdk/client-s3";
import { GetParameterCommand, SSMClient } from "@aws-sdk/client-ssm";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";
import {
  CloudFormationClient,
  awsConfig,
  deployStack,
  here,
  log,
  settings,
  stackExists,
  stackOutputs,
} from "../src/cloud.mjs";

const { values: args } = parseArgs({
  options: {
    host: { type: "string" },
    tailscale: { type: "boolean", default: false },
    profile: { type: "string" },
    region: { type: "string", default: "eu-central-1" },
    domain: { type: "string" },
    "new-image": { type: "boolean", default: false },
  },
});
if (!args.profile || !args.host === !args.tailscale) {
  console.error(
    "usage: vault-gateway (--host <desk.example.com> [--domain <example.com>] | --tailscale) --profile <aws profile> [--region <region>] [--new-image]",
  );
  process.exit(2);
}
const host = args.tailscale ? "" : args.host.toLowerCase();
const domain = args.tailscale
  ? ""
  : (args.domain ?? host.split(".").slice(1).join(".")).toLowerCase();
const config = awsConfig(args.profile, args.region);
const cfn = new CloudFormationClient(config);
const s3 = new S3Client(config);
const ssm = new SSMClient(config);
const STACK = "studio-vault-gateway";
const AUTH_KEY = "/studio-vault/gateway/tailscale-auth-key";
const IMAGE = "/aws/service/debian/release/13/latest/arm64";

// 1. The domain zone (none with --tailscale), and the artifacts bucket of this account and
// region.
const dns = args.tailscale
  ? { ZoneId: "", NameServers: "" }
  : await deployStack(
      cfn,
      `studio-vault-dns-${domain.replace(/[^a-z0-9-]/g, "-")}`,
      "dns.yaml",
      { Domain: domain },
    );
const artifacts = (
  await deployStack(cfn, "studio-vault-bootstrap", "bootstrap.yaml")
).ArtifactsBucket;

// 2. The release, then the pointer to it, then the gateway's own facts.
const sha256 = execFileSync(
  process.execPath,
  [join(here, "gateway/build.mjs")],
  {
    stdio: ["ignore", "pipe", "inherit"],
  },
)
  .toString()
  .trim()
  .split("\n")
  .pop();
const key = `gateway/releases/${sha256}.tgz`;
const put = (Key, Body, ContentType) =>
  s3.send(new PutObjectCommand({ Bucket: artifacts, Key, Body, ContentType }));
await put(
  key,
  readFileSync(join(here, "dist/gateway.tgz")),
  "application/gzip",
);
await put(
  "gateway/release.json",
  JSON.stringify({ key, sha256 }),
  "application/json",
);
await put(
  "gateway/gateway.json",
  JSON.stringify(
    args.tailscale ? { tls: "tailscale" } : { host, zone: dns.ZoneId },
  ),
  "application/json",
);
log(`release ${sha256.slice(0, 12)}`);

// 3. The server. The image stays the same unless --new-image: a new image replaces the instance,
// and a new instance needs a new tailnet auth key.
let image;
if ((await stackExists(cfn, STACK)) && !args["new-image"])
  image = (await stackOutputs(cfn, STACK)).parameters.ImageId;
image ??= (await ssm.send(new GetParameterCommand({ Name: IMAGE }))).Parameter
  .Value;
let hasKey = true;
try {
  await ssm.send(new GetParameterCommand({ Name: AUTH_KEY }));
} catch {
  hasKey = false;
}
const gateway = await deployStack(cfn, STACK, "gateway.yaml", {
  ArtifactsBucket: artifacts,
  ImageId: image,
  Host: host,
  ZoneId: dns.ZoneId,
  AuthKeyParameter: AUTH_KEY,
});

// Public facts for `layr-desk deploy`.
mkdirSync(settings, { recursive: true });
writeFileSync(
  join(settings, "gateway.json"),
  JSON.stringify(
    {
      profile: args.profile,
      region: args.region,
      host,
      artifacts,
      role: gateway.RoleArn,
    },
    null,
    2,
  ) + "\n",
);
console.log(
  JSON.stringify(
    {
      site: args.tailscale
        ? "https://vault-gateway.<your tailnet>.ts.net/"
        : `https://${host}/`,
      role: gateway.RoleArn,
      instance: gateway.InstanceId,
      ...(args.tailscale ? {} : { nameServers: dns.NameServers.split(",") }),
      tailnetAuthKey: hasKey ? "stored" : `missing: put it in SSM ${AUTH_KEY}`,
    },
    null,
    2,
  ),
);
