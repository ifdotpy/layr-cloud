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
// recipient, the Desk site and a new access key) as JSON on stdin and stores it where the writer runs. It runs
// when the writer has no key yet or --new-writer-key asks for one. The secret goes through the
// pipe, never to a file, and the key is deleted again if the command fails.
import {
  CreateAccessKeyCommand,
  DeleteAccessKeyCommand,
  IAMClient,
  ListAccessKeysCommand,
} from "@aws-sdk/client-iam";
import {
  HeadObjectCommand,
  PutObjectCommand,
  S3Client,
} from "@aws-sdk/client-s3";
import { spawnSync } from "node:child_process";
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
  },
});
if (!args.project || !args.profile) {
  console.error(
    "usage: vault-deploy --project <prefix> --profile <aws profile> [--region <region>] [--writer <command>] [--new-writer-key]",
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
const cfn = new CloudFormationClient(config);
const iam = new IAMClient(config);
const stack = `studio-vault-${args.project}`;

// 1. The vault stack.
const outputs = await deployStack(cfn, stack, "template.yaml", {
  Prefix: args.project,
  GatewayRoleArn: gatewaySettings.role,
});

// 2. The public vault record and the paper slot.
await s3.send(
  new PutObjectCommand({
    Bucket: outputs.BucketName,
    Key: `${args.project}/vault.json`,
    Body: JSON.stringify({
      format: 1,
      recipient,
      created: Math.floor(Date.now() / 1000),
    }),
    ContentType: "application/json",
  }),
);
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

// 4. A write-only key for the writer, passed through a pipe.
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
    const configure = spawnSync(args.writer, {
      shell: true,
      input: JSON.stringify({
        bucket: outputs.BucketName,
        region: args.region,
        prefix: args.project,
        recipient,
        site: `https://${gatewaySettings.host}/`,
        accessKeyId: key.AccessKeyId,
        secretAccessKey: key.SecretAccessKey,
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
  }
}
// Public facts for vault-restore and vault-view.
const site = `https://${gatewaySettings.host}/${args.project}/`;
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
