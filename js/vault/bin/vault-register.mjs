#!/usr/bin/env node
// Serve a vault that lives in another person's AWS account from this gateway.
//
//   node vault-register.mjs --facts <facts.json> [--replace]
//
// The facts are public: prefix, bucket, region, reader role and recipient. The owner of the
// vault finds them in ~/.config/studio-vault/<prefix>/facts.json after `layr-desk deploy`, and adds
// this gateway's role to the vault with `layr-desk deploy ... --also-gateway <this role>`. This
// gateway's role is in ~/.config/studio-vault/gateway.json. Nothing here gives a key: a
// person still needs a slot that opens the vault.
import {
  HeadObjectCommand,
  PutObjectCommand,
  S3Client,
} from "@aws-sdk/client-s3";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { awsConfig, log, settings } from "../src/cloud.mjs";

const { values: args } = parseArgs({
  options: {
    facts: { type: "string" },
    replace: { type: "boolean", default: false },
  },
});
if (!args.facts) {
  console.error("usage: vault-register --facts <facts.json> [--replace]");
  process.exit(2);
}
const facts = JSON.parse(readFileSync(args.facts, "utf8"));
const checks = {
  prefix: /^[A-Za-z0-9][A-Za-z0-9_-]{0,43}$/,
  bucket: /^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$/,
  region: /^[a-z]{2}(-[a-z]+)+-\d$/,
  role: /^arn:aws:iam::\d{12}:role\/studio-vault-reader-[A-Za-z0-9_-]+$/,
  recipient: /^age1[02-9ac-hj-np-z]{58}$/,
};
for (const [name, pattern] of Object.entries(checks)) {
  if (typeof facts[name] !== "string" || !pattern.test(facts[name])) {
    console.error(`vault-register: ${name} is missing or not valid`);
    process.exit(2);
  }
}
if (["api", "assets"].includes(facts.prefix)) {
  console.error(`vault-register: ${facts.prefix} is a gateway path`);
  process.exit(2);
}
const gateway = JSON.parse(
  readFileSync(join(settings, "gateway.json"), "utf8"),
);
const s3 = new S3Client(awsConfig(gateway.profile, gateway.region));
const Key = `gateway/projects/${facts.prefix}.json`;
let exists = true;
try {
  await s3.send(new HeadObjectCommand({ Bucket: gateway.artifacts, Key }));
} catch {
  exists = false;
}
if (exists && !args.replace) {
  console.error(
    `vault-register: this gateway already serves ${facts.prefix}; use --replace`,
  );
  process.exit(1);
}
const { prefix, ...served } = facts;
await s3.send(
  new PutObjectCommand({
    Bucket: gateway.artifacts,
    Key,
    Body: JSON.stringify(served),
    ContentType: "application/json",
  }),
);
log(`${prefix}: this gateway serves it within 5 minutes`);
