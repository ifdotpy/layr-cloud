// The built site behind the real gateway with a fake S3 that holds the writer's fixture
// (js/vault/test/fixture) for two vaults, in headless Chromium with a virtual authenticator
// that has PRF: the paper key unlocks, a passkey is added and then unlocks alone, the same
// passkey opens the second vault, files download and show in Quick Look with the writer's
// content, and a passkey is removed.
//
//   pnpm --filter layr-desk-web run build && node tests/site.e2e.mjs
//
// With LAYR_DESK=<the layr-desk program>, the gateway is `layr-desk serve` (plain HTTP on
// 127.0.0.1) and the fake is AWS itself: STS and path-style S3 over the same objects.
//
//   cargo build -p layr-desk && LAYR_DESK=../target/debug/layr-desk node tests/site.e2e.mjs
import { chromium } from "playwright";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { Readable } from "node:stream";
import { makeGateway } from "../../js/vault/gateway/server.mjs";
import { makeVault } from "../../js/vault/src/service.mjs";
import {
  generateIdentity,
  recipientOf,
  identityFromPaper,
  sealSlot,
  wordsOfIdentity,
} from "../../js/vault/src/keys.mjs";

import * as age from "../../js/vault/node_modules/age-encryption/dist/index.js";
import { VaultReader } from "../../js/vault/src/reader.mjs";

const dist = new URL("../dist/", import.meta.url).pathname;
const fixture = new URL("../../js/vault/test/fixture/", import.meta.url)
  .pathname;
const vaultIdentity = readFileSync(
  join(fixture, "identity.txt"),
  "utf8",
).trim();
const expected = JSON.parse(
  readFileSync(join(fixture, "expected.json"), "utf8"),
);
const paper = await generateIdentity();

// The same writer objects serve two vaults, "proj" and "other".
const objects = new Map();
const walk = (dir) =>
  readdirSync(dir).forEach((n) => {
    if (statSync(join(dir, n)).isDirectory()) return walk(join(dir, n));
    const key = relative(join(fixture, "objects"), join(dir, n));
    objects.set(key, readFileSync(join(dir, n)));
    objects.set(key.replace(/^proj\//, "other/"), readFileSync(join(dir, n)));
  });
walk(join(fixture, "objects"));
const paperSlot = Buffer.from(
  await sealSlot(vaultIdentity, await recipientOf(paper)),
);
objects.set("proj/slots/paper.age", paperSlot);
objects.set("other/slots/paper.age", paperSlot);

const recipient = await recipientOf(vaultIdentity);
// The second vault has the optional directory index. Its tree must never be fetched.
const fixtureReader = new VaultReader({
  prefix: "other",
  keys: [...objects.keys()],
  identity: vaultIdentity,
  get: async (key) => objects.get(key),
});
const indexed = fixtureReader.latest();
const indexedFiles = await fixtureReader.files(indexed.seq);
const encryptor = new age.Encrypter();
encryptor.addRecipient(recipient);
const stem = `${String(indexed.seq).padStart(10, "0")}.${indexed.attempt}`;
const folders = new Map([["", new Map()]]);
for (const [path, entry] of Object.entries(indexedFiles)) {
  const parts = path.split("/");
  for (let i = 0; i < parts.length; i++) {
    const parent = parts.slice(0, i).join("/");
    if (!folders.has(parent)) folders.set(parent, new Map());
    folders
      .get(parent)
      .set(
        parts[i],
        i === parts.length - 1
          ? { kind: "file", name: parts[i], entry }
          : { kind: "directory", name: parts[i], count: 0 },
      );
  }
}
// Force a second root page without a large fixture or a large DOM.
const sampleEntry = indexedFiles["src/app.py"];
for (let i = 0; i < 260; i++)
  folders.get("").set(`page-${String(i).padStart(3, "0")}`, {
    kind: "file",
    name: `page-${String(i).padStart(3, "0")}`,
    entry: sampleEntry,
  });
for (const [path, children] of folders) {
  const entries = [...children.values()].sort((a, b) =>
    a.name < b.name ? -1 : 1,
  );
  for (const item of entries)
    if (item.kind === "directory")
      item.count = folders.get(path ? `${path}/${item.name}` : item.name).size;
  for (let page = 0; page === 0 || page * 256 < entries.length; page++) {
    const data = {
      version: 1,
      ...indexed,
      path,
      page,
      total: entries.length,
      entries: entries.slice(page * 256, (page + 1) * 256),
      next: (page + 1) * 256 < entries.length ? page + 1 : null,
    };
    const hash = createHash("sha256").update(path).digest("hex");
    objects.set(
      `other/states/${stem}.${hash}.${String(page).padStart(8, "0")}.dir.age`,
      Buffer.from(await encryptor.encrypt(JSON.stringify(data))),
    );
  }
}
objects.set(
  `other/states/${stem}.browse.age`,
  Buffer.from(
    await encryptor.encrypt(
      JSON.stringify({
        version: 1,
        ...indexed,
        state: "fixture",
        files: Object.keys(indexedFiles).length,
      }),
    ),
  ),
);
const fetchedObjects = [];

/** The Node gateway with a fake S3 client. */
async function startNode() {
  const s3 = {
    async send(command) {
      const input = command.input;
      switch (command.constructor.name) {
        case "ListObjectsV2Command":
          return {
            Contents: [...objects.keys()]
              .filter((k) => k.startsWith(input.Prefix))
              .map((Key) => ({ Key })),
          };
        case "GetObjectCommand": {
          fetchedObjects.push(input.Key);
          const data = objects.get(input.Key);
          if (!data)
            throw Object.assign(new Error("missing"), { name: "NoSuchKey" });
          return {
            ContentLength: data.length,
            Body: Object.assign(Readable.from([data]), {
              transformToByteArray: async () => data,
            }),
          };
        }
        case "PutObjectCommand":
          if (input.IfNoneMatch === "*" && objects.has(input.Key))
            throw Object.assign(new Error("exists"), {
              name: "PreconditionFailed",
            });
          objects.set(input.Key, Buffer.from(input.Body));
          return {};
        case "DeleteObjectCommand":
          objects.delete(input.Key);
          return {};
      }
      throw new Error(command.constructor.name);
    },
  };
  const secret = Buffer.alloc(32, 9);
  const vaults = Object.fromEntries(
    ["proj", "other"].map((prefix) => [
      prefix,
      makeVault({ s3, bucket: "b", prefix, recipient, secret }),
    ]),
  );
  const server = http.createServer(
    makeGateway({
      config: () => ({ projects: { proj: {}, other: {} } }),
      vaultFor: (name) => vaults[name] ?? null,
      site: dist,
    }),
  );
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { port: server.address().port, stop: () => server.close() };
}

/** Path-style S3 and STS in one fake: STS takes the gateway's key, S3 the role's. */
function fakeAws() {
  const role = { id: "ASIAROLE", secret: "role-secret", token: "role-token" };
  return http.createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    const url = new URL(req.url, "http://aws");
    const auth = String(req.headers.authorization ?? "");
    const reply = (status, text = "") => {
      res.writeHead(status, { "content-type": "application/xml" });
      res.end(text);
    };
    const error = (status, code) =>
      reply(status, `<Error><Code>${code}</Code></Error>`);
    if (req.method === "POST" && url.pathname === "/") {
      const form = new URLSearchParams(body.toString());
      if (
        !auth.startsWith("AWS4-HMAC-SHA256 Credential=AKIAGATEWAY/") ||
        form.get("Action") !== "AssumeRole" ||
        form.get("RoleSessionName") !== "vault-gateway"
      )
        return error(403, "AccessDenied");
      const expiration = new Date(Date.now() + 3600e3)
        .toISOString()
        .replace(/\.\d+Z$/, "Z");
      return reply(
        200,
        `<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>${role.id}</AccessKeyId>` +
          `<SecretAccessKey>${role.secret}</SecretAccessKey><SessionToken>${role.token}</SessionToken>` +
          `<Expiration>${expiration}</Expiration></Credentials></AssumeRoleResult></AssumeRoleResponse>`,
      );
    }
    if (
      !auth.startsWith(`AWS4-HMAC-SHA256 Credential=${role.id}/`) ||
      req.headers["x-amz-security-token"] !== role.token
    )
      return error(403, "AccessDenied");
    const [, bucket, ...rest] = url.pathname.split("/");
    const key = decodeURIComponent(rest.join("/"));
    if (bucket !== "b") return error(404, "NoSuchBucket");
    if (req.method === "GET" && !key) {
      const prefix = url.searchParams.get("prefix") ?? "";
      const keys = [...objects.keys()]
        .filter((k) => k.startsWith(prefix))
        .sort();
      return reply(
        200,
        `<ListBucketResult>${keys.map((k) => `<Contents><Key>${k}</Key></Contents>`).join("")}</ListBucketResult>`,
      );
    }
    if (req.method === "GET") {
      fetchedObjects.push(key);
      const data = objects.get(key);
      if (!data) return error(404, "NoSuchKey");
      const range = /^bytes=(\d+)-(\d+)$/.exec(req.headers.range ?? "");
      if (range) {
        const start = Number(range[1]),
          end = Math.min(Number(range[2]), data.length - 1);
        if (start >= data.length) return error(416, "InvalidRange");
        res.writeHead(206, {
          "content-range": `bytes ${start}-${end}/${data.length}`,
          "content-length": end - start + 1,
        });
        return res.end(data.subarray(start, end + 1));
      }
      res.writeHead(200, { "content-length": data.length });
      return res.end(data);
    }
    if (req.method === "PUT") {
      if (req.headers["if-none-match"] === "*" && objects.has(key))
        return error(412, "PreconditionFailed");
      objects.set(key, body);
      return reply(200);
    }
    if (req.method === "DELETE") {
      objects.delete(key);
      return reply(204);
    }
    return error(405, "MethodNotAllowed");
  });
}

/** `layr-desk serve` on 127.0.0.1 with the fake AWS. */
async function startRust(program) {
  const aws = fakeAws();
  await new Promise((resolve) => aws.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${aws.address().port}`;
  const dir = mkdtempSync(join(tmpdir(), "layr-desk-e2e-"));
  const project = (name) => ({
    bucket: "b",
    region: "eu-central-1",
    role: `arn:aws:iam::123456789012:role/reader-${name}`,
    recipient,
  });
  writeFileSync(
    join(dir, "config.json"),
    JSON.stringify({
      listen: "127.0.0.1",
      projects: { proj: project("proj"), other: project("other") },
    }),
  );
  const child = spawn(
    program,
    [
      "serve",
      "--config",
      join(dir, "config.json"),
      "--site",
      dist,
      "--tls",
      "none",
      "--port",
      "0",
    ],
    {
      env: {
        ...process.env,
        AWS_ACCESS_KEY_ID: "AKIAGATEWAY",
        AWS_SECRET_ACCESS_KEY: "gateway-secret",
        AWS_SESSION_TOKEN: "",
        AWS_ENDPOINT_URL_S3: endpoint,
        AWS_ENDPOINT_URL_STS: endpoint,
      },
      stdio: ["ignore", "inherit", "pipe"],
    },
  );
  const port = await new Promise((resolve, reject) => {
    let text = "";
    child.stderr.on("data", (chunk) => {
      process.stderr.write(chunk);
      text += chunk;
      const found = text.match(/listening on http:\/\/127\.0\.0\.1:(\d+)/);
      if (found) resolve(Number(found[1]));
    });
    child.on("exit", (code) =>
      reject(new Error(`layr-desk serve stopped (${code})`)),
    );
  });
  return {
    port,
    stop: () => {
      child.kill();
      aws.close();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

const { port, stop } = process.env.LAYR_DESK
  ? await startRust(process.env.LAYR_DESK)
  : await startNode();
const sha = (data) => createHash("sha256").update(data).digest("hex");

const browser = await chromium.launch();
try {
  const context = await browser.newContext({ acceptDownloads: true });
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      hasPrf: true,
      automaticPresenceSimulation: true,
    },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const site = `http://localhost:${port}`;

  // Two vaults: the root page asks for one.
  await page.goto(`${site}/`);
  await page.waitForSelector("#empty-text:has-text('Choose a vault')");
  await page.click("#vaults a:has-text('proj')");
  await page.waitForSelector("#unlock:not([hidden])");
  assert.equal(
    await page.isHidden("#with-passkey"),
    true,
    "no passkey slot yet",
  );

  // The paper key is 24 words. Two words in the wrong order fail the checksum, and the page
  // says so; numbered words, some cut to their first four letters, unlock the latest state.
  const words = wordsOfIdentity(paper).split(" ");
  const swapped = words
    .map((_, i) => {
      const copy = [...words];
      [copy[i], copy[i + 1]] = [copy[i + 1], copy[i]];
      return copy.join(" ");
    })
    .find((text) => {
      try {
        identityFromPaper(text);
        return false;
      } catch {
        return true;
      }
    });
  await page.fill("#paper", swapped);
  await page.click("#paper-form button");
  await page.waitForFunction(() =>
    document.querySelector("#status").textContent.includes("checksum"),
  );
  await page.fill(
    "#paper",
    words
      .map((w, i) => `${i + 1}. ${i % 2 ? w.slice(0, 4).toUpperCase() : w}`)
      .join("\n"),
  );
  await page.click("#paper-form button");
  await page.waitForSelector("#columns:not([hidden])");
  assert.equal(await page.inputValue("#state"), "4");

  // Column view: a folder opens the next column, a file shows its preview with Download.
  await page.click("#columns .item:has-text('src')");
  await page.click("#columns .item:has-text('app.py')");
  await page.waitForSelector(".column.preview h3:has-text('app.py')");
  const download = page.waitForEvent("download");
  await page.click(".column.preview button:has-text('Download')");
  assert.equal(
    sha(readFileSync(await (await download).path())),
    expected.states[3]["src/app.py"][0],
  );
  assert.equal(await page.textContent("#crumbs"), "projsrcapp.py");

  // Quick Look shows the text; Escape closes it.
  await page.keyboard.press(" ");
  await page.waitForSelector("#ql-body pre");
  assert.equal(
    sha(Buffer.from(await page.textContent("#ql-body pre"))),
    expected.states[3]["src/app.py"][0],
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector("#quicklook-panel", { state: "hidden" });

  // List view and an earlier state.
  await page.click("#view-list");
  await page.waitForSelector("#list:not([hidden])");
  await page.selectOption("#state", "2");
  await page.click("#crumbs button:first-child");
  await page.waitForSelector("#list-rows .item:has-text('empty.txt')");

  // Search finds a file in any folder.
  await page.fill("#filter", "tool");
  await page.waitForSelector("#list-rows .item:has-text('bin/tool.bin')");
  await page.fill("#filter", "");

  // A passkey on this device, then a fresh tab that opens with the passkey alone.
  await page.click("#passkeys");
  await page.fill("#label", "test device");
  await page.click("#add-form button");
  await page.waitForSelector("#passkey-list li button");
  assert.ok(
    [...objects.keys()].some((k) => k.startsWith("proj/slots/passkey-")),
  );
  await page.click("#close-passkeys");
  // A fresh load asks for the passkey by itself: no click.
  await page.reload();
  await page.waitForSelector("#list:not([hidden])");
  assert.equal(await page.inputValue("#state"), "4");
  // After Lock the page waits for the person.
  await page.click("#lock");
  await page.waitForSelector("#with-passkey:not([hidden])");
  await page.waitForTimeout(500);
  assert.equal(await page.isVisible("#unlock"), true);
  await page.click("#with-passkey");
  await page.waitForSelector("#list:not([hidden])");

  // The same passkey opens the second vault after one paper unlock there.
  await page.click("#vaults a:has-text('other')");
  await page.waitForSelector("#unlock:not([hidden])");
  assert.equal(await page.isHidden("#with-passkey"), true);
  await page.fill("#paper", words.join(" "));
  await page.click("#paper-form button");
  await page.waitForSelector("#list:not([hidden])");
  assert.equal(
    await page.getAttribute("#filter", "placeholder"),
    "Search this folder page",
  );
  assert.equal(await page.locator("#list-rows .item").count(), 256);
  await page.click("#list-rows button:has-text('Next page')");
  await page.waitForSelector("#list-rows .item:has-text('src')");
  await page.dblclick("#list-rows .item:has-text('src')");
  await page.waitForSelector("#list-rows .item:has-text('app.py')");
  await page.click("#list-rows .item:has-text('app.py')");
  await page.click("#quicklook");
  await page.waitForSelector("#ql-body pre");
  assert.equal(
    sha(Buffer.from(await page.textContent("#ql-body pre"))),
    expected.states[3]["src/app.py"][0],
  );
  await page.keyboard.press("Escape");
  await page.click("#back");
  await page.waitForSelector("#list-rows .item:has-text('src')");
  await page.click("#forward");
  await page.waitForSelector("#list-rows .item:has-text('app.py')");
  assert.equal(
    fetchedObjects.filter(
      (key) => key.startsWith("other/") && key.endsWith(".tree.age"),
    ).length,
    0,
    "indexed navigation must not fetch the full tree",
  );
  await page.click("#passkeys");
  await page.click("#use-existing");
  await page.waitForSelector("#passkey-list li button");
  const ids = (prefix) =>
    [...objects.keys()]
      .filter((k) => k.startsWith(`${prefix}/slots/passkey-`))
      .map((k) => k.slice(prefix.length + 7));
  assert.deepEqual(ids("other"), ids("proj"));
  // One credential, two vaults: the slots differ, because the PRF salt is per vault.
  assert.notDeepEqual(
    objects.get(`other/slots/${ids("other")[0]}`),
    objects.get(`proj/slots/${ids("proj")[0]}`),
  );
  await page.click("#close-passkeys");
  await page.reload();
  await page.waitForSelector("#list:not([hidden])");

  // Removing the passkey removes its slot; the paper slot stays.
  await page.click("#passkeys");
  await page.click("#passkey-list li button");
  await page.waitForFunction(
    () => !document.querySelector("#passkey-list li button"),
  );
  assert.equal(
    [...objects.keys()].filter((k) => k.startsWith("other/slots/")).join(),
    "other/slots/paper.age",
  );
  // A rejected encrypted page reports an error, with no full-tree fallback.
  await page.click("#close-passkeys");
  await page.click("#lock");
  const rootKey = `other/states/${stem}.${createHash("sha256").update("").digest("hex")}.00000000.dir.age`;
  const validRoot = objects.get(rootKey);
  objects.set(
    rootKey,
    Buffer.from(
      await encryptor.encrypt(
        JSON.stringify({
          version: 1,
          ...indexed,
          path: "wrong",
          page: 0,
          total: 0,
          entries: [],
          next: null,
        }),
      ),
    ),
  );
  await page.fill("#paper", words.join(" "));
  await page.click("#paper-form button");
  await page.waitForSelector("#status:has-text('Invalid folder page')");
  assert.equal(await page.isHidden("#list"), true);
  objects.set(rootKey, validRoot);
  await page.click("#lock");
  // Hold a worker's folder request, lock, then release it. The old view must stay closed.
  let release;
  const held = new Promise((resolve) => {
    release = resolve;
  });
  let arrived;
  const arrival = new Promise((resolve) => {
    arrived = resolve;
  });
  await page.route(`**/${rootKey.split("/").at(-1)}`, async (route) => {
    arrived();
    await held;
    await route.continue().catch(() => {});
  });
  await page.fill("#paper", words.join(" "));
  await page.click("#paper-form button");
  await arrival;
  await page.click("#lock");
  release();
  await page.waitForTimeout(100);
  assert.equal(await page.isVisible("#unlock"), true);
  assert.equal(await page.isHidden("#list"), true);
  assert.equal(await page.textContent("#status"), "");
  assert.deepEqual(errors, []);
  console.log(
    `site e2e: passed (${process.env.LAYR_DESK ? "layr-desk serve" : "Node gateway"})`,
  );
} finally {
  await browser.close();
  stop();
}
