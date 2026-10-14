// Exercise the built browser worker against real age ciphertext and bounded HTTP ranges.
import { chromium } from "playwright";
import assert from "node:assert/strict";
import { randomBytes, createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import http from "node:http";
import { join } from "node:path";
import * as age from "../../js/vault/node_modules/age-encryption/dist/index.js";
import {
  init,
  compress,
  createCCtx,
  freeCCtx,
  compressUsingDict,
} from "@bokuweb/zstd-wasm";

await init();
const identity = await age.generateIdentity();
const encryptor = new age.Encrypter();
encryptor.addRecipient(await age.identityToRecipient(identity));
const challenge = Buffer.from(await encryptor.encrypt("test")).toString(
  "base64",
);
const hash = (data) => createHash("sha256").update(data).digest("hex");
const name = (part) =>
  `0000000001.aaaaaaaa.${String(part).padStart(4, "0")}.data.age`;
const objects = new Map();
const entries = [],
  contents = [];
const chunks = [randomBytes(1024 * 1024)];
let offset = chunks[0].length;
for (let i = 0; i < 12; i++) {
  const data = randomBytes(16_000 + i),
    frame = compress(data);
  entries.push([
    "100644",
    data.length,
    hash(data),
    [[1, "aaaaaaaa", 0, offset, frame.length, "full"]],
  ]);
  contents.push(data);
  chunks.push(frame, randomBytes(256 * 1024));
  offset += frame.length + 256 * 1024;
}
objects.set(
  name(0),
  Buffer.from(await encryptor.encrypt(Buffer.concat(chunks))),
);
const revised = Buffer.concat([contents[0], Buffer.from("new text")]);
const ctx = createCCtx();
const delta = compressUsingDict(ctx, revised, contents[0], 3);
freeCCtx(ctx);
objects.set(
  name(1),
  Buffer.from(
    await encryptor.encrypt(Buffer.concat([randomBytes(1024 * 1024), delta])),
  ),
);
const deltaEntry = [
  "100644",
  revised.length,
  hash(revised),
  [[1, "aaaaaaaa", 1, 1024 * 1024, delta.length, "delta"], ...entries[0][3]],
];
const dist = new URL("../dist/", import.meta.url).pathname;
const asset = readdirSync(join(dist, "assets")).find((n) =>
  /^reader.worker-.*\.js$/.test(n),
);
let mode = "range",
  calls = [],
  active = 0,
  peak = 0;
const server = http.createServer((req, res) => {
  const path = new URL(req.url, "http://localhost").pathname;
  if (path.endsWith("/challenge")) {
    res
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify({ challenge }));
    return;
  }
  if (path.startsWith("/api/")) {
    const all = objects.get(path.split("/").at(-1));
    if (!all) {
      res.writeHead(404).end();
      return;
    }
    if (req.headers.authorization !== "Bearer test") {
      res.writeHead(401).end();
      return;
    }
    const match = /^bytes=(\d+)-(\d+)$/.exec(req.headers.range ?? "");
    let bytes = all,
      status = 200,
      headers = {};
    if (match && mode !== "full") {
      const start = Number(match[1]),
        end = Math.min(Number(match[2]), all.length - 1);
      bytes = Buffer.from(all.subarray(start, end + 1));
      status = 206;
      headers["content-range"] =
        `bytes ${start}-${end}/${all.length + (mode === "changed" && calls.length ? 1 : 0)}`;
      if (mode === "corrupt") bytes[Math.min(100, bytes.length - 1)] ^= 1;
      // Corrupt the age chunk that contains the requested frame, not an unused neighbor.
      const damaged = 1024 * 1024 + 1000;
      if (mode === "payload" && start <= damaged && damaged <= end)
        bytes[damaged - start] ^= 1;
      if (mode === "short") bytes = bytes.subarray(0, bytes.length - 1);
    }
    calls.push({ range: Boolean(match), bytes: bytes.length });
    active++;
    peak = Math.max(peak, active);
    setTimeout(() => {
      res.writeHead(status, { ...headers, "content-length": bytes.length });
      res.end(bytes);
      active--;
    }, 10);
    return;
  }
  if (path === "/") {
    res
      .writeHead(200, { "content-type": "text/html" })
      .end("<!doctype html><title>Reader test</title>");
    return;
  }
  try {
    const bytes = readFileSync(join(dist, path));
    res
      .writeHead(200, {
        "content-type": path.endsWith(".wasm")
          ? "application/wasm"
          : "text/javascript",
      })
      .end(bytes);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  const reset = async (nextMode) => {
    mode = nextMode;
    calls = [];
    peak = 0;
    await page.evaluate(
      async ({ asset, identity, token }) => {
        window.reader?.terminate();
        window.reader = new Worker(`/assets/${asset}`, { type: "module" });
        window.seq = 0;
        window.pending = new Map();
        window.progress = [];
        window.reader.onmessage = ({ data }) => {
          if (data.progress) {
            window.progress.push(data.progress);
            return;
          }
          const p = window.pending.get(data.id);
          window.pending.delete(data.id);
          if (data.error) p.reject(Error(data.error));
          else p.resolve(data.result);
        };
        window.call = (method, value) =>
          new Promise((resolve, reject) => {
            const id = ++window.seq;
            window.pending.set(id, { resolve, reject });
            window.reader.postMessage({ id, method, value });
          });
        await window.call("init", {
          identity,
          prefix: "proj",
          keys: [],
          token,
        });
      },
      { asset, identity, token: nextMode === "expired" ? "expired" : "test" },
    );
  };
  const read = (entry) =>
    page.evaluate(async (entry) => {
      const b = await window.call("read", entry);
      return [...new Uint8Array(await crypto.subtle.digest("SHA-256", b))]
        .map((n) => n.toString(16).padStart(2, "0"))
        .join("");
    }, entry);
  await reset("range");
  assert.equal(await read(entries[0]), entries[0][2]);
  assert.ok(calls.every((c) => c.range));
  assert.ok(calls.reduce((n, c) => n + c.bytes, 0) <= 4 * 128 * 1024);
  const count = calls.length;
  assert.equal(await read(entries[0]), entries[0][2]);
  assert.equal(calls.length, count);
  for (const entry of entries) assert.equal(await read(entry), entry[2]);
  assert.equal(calls.filter((c) => c.range).length, 8);
  assert.equal(calls.filter((c) => !c.range).length, 1);
  await reset("range");
  assert.equal(await read(deltaEntry), deltaEntry[2]);
  assert.equal(peak, 2);
  assert.ok(
    await page.evaluate(
      () =>
        window.progress.some((p) => p.phase === "download") &&
        window.progress.some((p) => p.phase === "restore"),
    ),
  );
  const bad = structuredClone(deltaEntry);
  bad[2] = "00".repeat(32);
  await assert.rejects(read(bad), /file hash/);
  for (const badMode of ["corrupt", "payload", "short", "changed"]) {
    await reset(badMode);
    await assert.rejects(read(entries[0]), undefined, badMode);
  }
  await reset("expired");
  assert.equal(await read(entries[0]), entries[0][2]);
  await reset("full");
  assert.equal(await read(entries[0]), entries[0][2]);
  assert.equal(calls.length, 1);
  console.log(
    "browser reader: range, cache, budget, parallel delta, integrity, malformed responses and old gateway passed",
  );
} finally {
  await browser.close();
  server.close();
}
