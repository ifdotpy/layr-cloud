#!/usr/bin/env node
// Deploy or update the AWS side of a project vault (cloud/template.yaml) with the keys that
// vault-keys made, register the vault with the gateway (vault-gateway), and give the writer a
// write-only key.
//
//   node vault-deploy.mjs --project <prefix> --profile <aws profile> --region <region>
//        [--writer <command>] [--new-writer-key]
//
// The vault may live in another AWS account than the gateway: --profile names the vault's account.
// --writer names a shell command that reads the writer configuration (bucket, region, prefix,
// recipient, the Desk site, a new access key and a new signing key) as JSON on stdin and stores it
// where the writer runs. It runs when the writer has no key yet or --new-writer-key asks for one;
// --new-writer-key also starts signatures on a vault made before them. The secrets go through the
// pipe, never to a file or to the output, and the access key is deleted again if the command
// fails. Then vault.json names the writer's public key (docs/design.md, "Signatures").
import {
  CreateAccessKeyCommand,
  DeleteAccessKeyCommand,
  IAMClient,
  ListAccessKeysCommand,
} from "@aws-sdk/client-iam";
import {
  GetObjectCommand,
  HeadObjectCommand,
  ListObjectsV2Command,
  PutObjectCommand,
  S3Client,
} from "@aws-sdk/client-s3";
import { spawnSync } from "node:child_process";
import { generateKeyPairSync } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";
import {
  CloudFormationClient,
  awsConfig,
  deployStack,
  settings,
} from "../src/cloud.mjs";

const { values: args } = parseArgs({
  options: {
    project: { type: "string" },
    profile: { type: "string" },
    region: { type: "string", default: "eu-central-1" },
    writer: { type: "string" },
    "new-writer-key": { type: "boolean", default: false },
    // The gateway role of another person who serves this vault from their own gateway.
    "also-gateway": { type: "string", multiple: true, default: [] },
  },
});
if (!args.project || !args.profile) {
  console.error(
    "usage: vault-deploy --project <prefix> --profile <aws profile> [--region <region>] [--writer <command>] [--new-writer-key] [--also-gateway <role arn>]...",
  );
  process.exit(2);
}
// The gateway serves /api/ and /assets/ itself.
if (["api", "assets"].includes(args.project)) {
  console.error(
    `vault-deploy: ${args.project} is a gateway path; choose another name`,
  );
  process.exit(2);
}
const keys = join(settings, args.project);
const recipient = readFileSync(join(keys, "recipient.txt"), "utf8").trim();
const paperSlot = readFileSync(join(keys, "paper.age"));
let gatewaySettings;
try {
  gatewaySettings = JSON.parse(
    readFileSync(join(settings, "gateway.json"), "utf8"),
  );
} catch {
  console.error("vault-deploy: no gateway yet; run vault-gateway first");
  process.exit(2);
}
const config = awsConfig(args.profile, args.region);
const s3 = new S3Client(config);
// The site of the gateway. A gateway without a domain (vault-gateway --tailscale) serves its
// tailnet name, which this Mac sees once the gateway has joined the tailnet.
function gatewayHost() {
  if (gatewaySettings.host) return gatewaySettings.host;
  const status = JSON.parse(
    spawnSync("tailscale", ["status", "--json"], { encoding: "utf8" }).stdout ||
      "{}",
  );
  const peer = Object.values(status.Peer ?? {}).find(
    (p) => p.HostName === "vault-gateway" && p.DNSName,
  );
  if (!peer) {
    console.error(
      "vault-deploy: the gateway has not joined the tailnet yet; run this again later",
    );
    process.exit(1);
  }
  return peer.DNSName.replace(/\.$/, "");
}
const host = gatewayHost();
const cfn = new CloudFormationClient(config);
const iam = new IAMClient(config);
const stack = `studio-vault-${args.project}`;

// 1. The vault stack.
const outputs = await deployStack(cfn, stack, "template.yaml", {
  Prefix: args.project,
  GatewayRoleArns: [gatewaySettings.role, ...args["also-gateway"]].join(","),
});

// 2. The public vault record and the paper slot.
const recordKey = `${args.project}/vault.json`;
async function readRecord() {
  try {
    const object = await s3.send(
      new GetObjectCommand({ Bucket: outputs.BucketName, Key: recordKey }),
    );
    return JSON.parse(await object.Body.transformToString());
  } catch (error) {
    if (error.name === "NoSuchKey") return null;
    throw error;
  }
}
const writeRecord = (record) =>
  s3.send(
    new PutObjectCommand({
      Bucket: outputs.BucketName,
      Key: recordKey,
      Body: JSON.stringify(record),
      ContentType: "application/json",
    }),
  );
// A run without a new writer key keeps the writer key and signedFrom of an earlier run.
const earlier = await readRecord();
let record = {
  format: 1,
  recipient,
  created: earlier?.created ?? Math.floor(Date.now() / 1000),
  ...(earlier?.writer
    ? { writer: earlier.writer, signedFrom: earlier.signedFrom }
    : {}),
};
await writeRecord(record);
try {
  await s3.send(
    new HeadObjectCommand({
      Bucket: outputs.BucketName,
      Key: `${args.project}/slots/paper.age`,
    }),
  );
} catch {
  await s3.send(
    new PutObjectCommand({
      Bucket: outputs.BucketName,
      Key: `${args.project}/slots/paper.age`,
      Body: paperSlot,
      IfNoneMatch: "*",
    }),
  );
}

// 3. The gateway serves the vault through its reader role; it reads this within 5 minutes.
await new S3Client(
  awsConfig(gatewaySettings.profile, gatewaySettings.region),
).send(
  new PutObjectCommand({
    Bucket: gatewaySettings.artifacts,
    Key: `gateway/projects/${args.project}.json`,
    Body: JSON.stringify({
      bucket: outputs.BucketName,
      region: args.region,
      role: outputs.ReaderRoleArn,
      recipient,
    }),
    ContentType: "application/json",
  }),
);

/** The highest seq with a tree in the bucket, 0 for none. */
async function highestSeq() {
  let highest = 0;
  let token;
  do {
    const page = await s3.send(
      new ListObjectsV2Command({
        Bucket: outputs.BucketName,
        Prefix: `${args.project}/states/`,
        ContinuationToken: token,
      }),
    );
    for (const { Key } of page.Contents ?? []) {
      const match = /\/(\d{10})\.[0-9a-f]{8}\.tree\.age$/.exec(Key);
      if (match) highest = Math.max(highest, Number(match[1]));
    }
    token = page.NextContinuationToken;
  } while (token);
  return highest;
}

// 4. A write-only key and a signing key for the writer, passed through a pipe.
if (args.writer) {
  const listed = (
    await iam.send(
      new ListAccessKeysCommand({ UserName: outputs.WriterUserName }),
    )
  ).AccessKeyMetadata;
  if (!listed.length || args["new-writer-key"]) {
    const key = (
      await iam.send(
        new CreateAccessKeyCommand({ UserName: outputs.WriterUserName }),
      )
    ).AccessKey;
    // The writer signs each tree with this Ed25519 key; the private half exists only in this
    // process and in the pipe.
    const signing = generateKeyPairSync("ed25519");
    const base64 = (jwk) => Buffer.from(jwk, "base64url").toString("base64");
    const signingKey = base64(signing.privateKey.export({ format: "jwk" }).d);
    const writerKey = base64(signing.publicKey.export({ format: "jwk" }).x);
    const configure = spawnSync(args.writer, {
      shell: true,
      input: JSON.stringify({
        bucket: outputs.BucketName,
        region: args.region,
        prefix: args.project,
        recipient,
        site: `https://${host}/`,
        accessKeyId: key.AccessKeyId,
        secretAccessKey: key.SecretAccessKey,
        signingKey,
      }),
      stdio: ["pipe", "inherit", "inherit"],
    });
    if (configure.status !== 0) {
      await iam.send(
        new DeleteAccessKeyCommand({
          UserName: outputs.WriterUserName,
          AccessKeyId: key.AccessKeyId,
        }),
      );
      throw new Error(
        "the writer command did not take the key; the key is deleted again",
      );
    }
    for (const old of listed)
      await iam.send(
        new DeleteAccessKeyCommand({
          UserName: outputs.WriterUserName,
          AccessKeyId: old.AccessKeyId,
        }),
      );
    // Signatures start after the highest state in the bucket now. The order leaves no gap: the
    // writer command has returned, so the writer holds the signing key, and every state that this
    // listing does not show comes from a push with the key. A listing before the command could
    // miss a state that an unsigned push adds in between, and vault.json before the command
    // would ask for signatures on states that the writer still pushes without a key; readers
    // would refuse both. This needs a writer command that returns only after a push that runs
    // with the old configuration has ended.
    try {
      record = {
        ...record,
        writer: writerKey,
        signedFrom: (await highestSeq()) + 1,
      };
      await writeRecord(record);
    } catch (error) {
      throw new Error(
        `the writer signs with its new key, but vault.json does not name it (${error.message}); run again with --new-writer-key`,
      );
    }
  }
}
// Public facts for vault-restore and vault-view.
const site = `https://${host}/${args.project}/`;
// The public facts that another person's gateway needs to serve this vault (vault-register).
writeFileSync(
  join(keys, "facts.json"),
  JSON.stringify(
    {
      prefix: args.project,
      bucket: outputs.BucketName,
      region: args.region,
      role: outputs.ReaderRoleArn,
      recipient,
    },
    null,
    2,
  ) + "\n",
);
writeFileSync(
  join(keys, "cloud.json"),
  JSON.stringify(
    {
      profile: args.profile,
      region: args.region,
      bucket: outputs.BucketName,
      site,
      stack,
    },
    null,
    2,
  ) + "\n",
);
console.log(
  JSON.stringify(
    {
      stack,
      site,
      bucket: outputs.BucketName,
    },
    null,
    2,
  ),
);
