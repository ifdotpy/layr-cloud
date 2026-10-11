// The JavaScript reader against objects that the real writer made with age and zstandard
// (test/make_fixture.py), the same objects signed with the test key of fixture/signed.json and
// forged, and the slot keys.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash, randomBytes } from "node:crypto";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { VaultReader, treeMessage } from "../src/reader.mjs";
import {
  changeTree,
  forgeState,
  sign,
  signedVault,
  unsignedVault,
  vectors,
} from "./signed.mjs";
import {
  bech32Decode,
  checkVaultIdentity,
  generateIdentity,
  identityFromSecret,
  openSlot,
  recipientOf,
  sealSlot,
} from "../src/keys.mjs";

const fixture = new URL("./fixture/", import.meta.url).pathname;
const expected = JSON.parse(
  readFileSync(join(fixture, "expected.json"), "utf8"),
);
const identity = readFileSync(join(fixture, "identity.txt"), "utf8").trim();

function objects() {
  const root = join(fixture, "objects");
  const found = new Map();
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else found.set(relative(root, path), new Uint8Array(readFileSync(path)));
    }
  };
  walk(root);
  return found;
}

const readerOf = (store, vault, key = identity) =>
  new VaultReader({
    prefix: "proj",
    keys: [...store.keys()],
    get: async (k) => store.get(k),
    identity: key,
    vault,
  });

test("every pushed state reads back with the writer's content", async () => {
  const store = objects();
  const reader = readerOf(store, unsignedVault);
  assert.deepEqual(
    reader.states().map((s) => s.seq),
    [1, 2, 3, 4],
  );
  for (const [index, state] of expected.states.entries()) {
    const files = await reader.files(index + 1);
    assert.deepEqual(Object.keys(files).sort(), Object.keys(state).sort());
    for (const [path, [sha, mode]] of Object.entries(state)) {
      const data = await reader.read(files[path]);
      assert.equal(createHash("sha256").update(data).digest("hex"), sha, path);
      assert.equal(files[path][0], mode, path);
    }
  }
  const latest = await reader.files();
  assert.equal(latest["src/app.py"][3][0][5], "delta");
});

test("a passkey secret becomes an age identity that opens its slot", async () => {
  const vault = await generateIdentity();
  const secret = randomBytes(32);
  const opener = identityFromSecret(secret);
  assert.deepEqual(bech32Decode(opener).bytes, new Uint8Array(secret));
  assert.equal(bech32Decode(await generateIdentity()).hrp, "age-secret-key-");
  const slot = await sealSlot(vault, await recipientOf(opener));
  assert.equal(await openSlot(slot, opener), vault);
  await assert.rejects(openSlot(slot, await generateIdentity()));
});

test("the fixture recipient belongs to the fixture identity", async () => {
  assert.equal(await recipientOf(identity), expected.recipient);
});

test("the paper key is 24 BIP39 words that give back the same identity", async () => {
  const { identityFromPaper, wordsOfIdentity } =
    await import("../src/keys.mjs");
  const paper = await generateIdentity();
  const words = wordsOfIdentity(paper).split(" ");
  assert.equal(words.length, 24);
  assert.equal(identityFromPaper(words.join(" ")), paper);
  assert.equal(
    identityFromPaper(
      words
        .map((w, i) => `${i + 1}) ${w.slice(0, 4).toUpperCase()}`)
        .join("\n"),
    ),
    paper,
  );
  assert.equal(identityFromPaper(paper.toLowerCase()), paper);
  assert.throws(
    () => identityFromPaper(words.slice(0, 23).join(" ")),
    /24 words/,
  );
  assert.throws(
    () => identityFromPaper(["zzzz", ...words.slice(1)].join(" ")),
    /not a word/,
  );
});

test("the signed message and the signatures match the vectors of desk-core", async () => {
  const reader = readerOf(objects(), unsignedVault);
  const key = await crypto.subtle.importKey(
    "raw",
    Buffer.from(vectors.writer, "base64"),
    { name: "Ed25519" },
    false,
    ["verify"],
  );
  for (const state of vectors.states) {
    const { header, canonical } = await reader.tree(state.seq, state.attempt);
    const sha = createHash("sha256").update(canonical).digest("hex");
    const message = treeMessage("proj", header, sha);
    assert.equal(message, state.message);
    assert.ok(
      await crypto.subtle.verify(
        { name: "Ed25519" },
        key,
        Buffer.from(state.sig, "base64"),
        new TextEncoder().encode(message),
      ),
    );
  }
});

test("a signed vault reads, and states before signedFrom show as unsigned", async () => {
  const store = objects();
  await sign(store, "proj");
  const reader = readerOf(store, signedVault);
  assert.deepEqual(
    reader.states().map((s) => reader.signed(s.seq)),
    [false, true, true, true],
  );
  for (const [index, state] of expected.states.entries()) {
    const files = await reader.files(index + 1);
    for (const [path, [sha]] of Object.entries(state)) {
      const data = await reader.read(files[path]);
      assert.equal(createHash("sha256").update(data).digest("hex"), sha, path);
    }
  }
  // The unsigned fixture reads when signatures start after its last state.
  const later = readerOf(objects(), { ...signedVault, signedFrom: 5 });
  for (const { seq } of later.states()) {
    assert.equal(later.signed(seq), false);
    await later.files(seq);
  }
});

test("a forged, unsigned or wrongly signed state is refused", async () => {
  const signed = objects();
  await sign(signed, "proj");
  const [, , third, last] = vectors.states;
  const lastKey = [...signed.keys()].find((k) =>
    k.endsWith(`${last.attempt}.tree.age`),
  );
  const refused = async (change, pattern, seq = last.seq) => {
    const store = new Map(signed);
    await change(store);
    await assert.rejects(readerOf(store, signedVault).files(seq), pattern);
  };
  await refused(
    (s) => changeTree(s, lastKey, (h) => delete h.sig),
    /State #4 has no writer signature/,
  );
  await refused(
    (s) => changeTree(s, lastKey, (h) => (h.sig = third.sig)),
    /State #4 has a writer signature that does not match/,
  );
  await refused(
    (s) => changeTree(s, lastKey, (h) => (h.sig = "AAAA")),
    /does not match/,
  );
  await refused(
    (s) => changeTree(s, lastKey, (h) => (h.time += 1)),
    /does not match/,
  );
  // A new state with a changed file, unsigned or with another state's signature.
  await refused((s) => forgeState(s, "proj"), /State #5 has no writer/, 5);
  await refused(
    (s) => forgeState(s, "proj", last.sig),
    /State #5 has a writer signature that does not match/,
    5,
  );
  // A tree that names another state.
  await refused(
    (s) => changeTree(s, lastKey, (h) => (h.seq = 3)),
    /names itself/,
  );
  // Without vault.json signatures, the forged state reads: this is what they prevent.
  const open = new Map(signed);
  await forgeState(open, "proj");
  const files = await readerOf(open, unsignedVault).files(5);
  assert.deepEqual(files["src/app.py"], files["README.md"]);
});

test("content that does not match its tree entry is refused", async () => {
  const reader = readerOf(objects(), unsignedVault);
  const files = await reader.files();
  const entry = files["src/app.py"];
  await reader.read(entry);
  for (const wrong of [
    [entry[0], entry[1], "0".repeat(64), entry[3]],
    [entry[0], entry[1] + 1, entry[2], entry[3]],
  ])
    await assert.rejects(reader.read(wrong), /does not match its tree entry/);
});

test("the key of a forged slot is refused", async () => {
  const stranger = await generateIdentity();
  await assert.rejects(
    checkVaultIdentity(stranger, expected.recipient),
    /may be forged/,
  );
  await checkVaultIdentity(identity, expected.recipient);
  const reader = readerOf(objects(), unsignedVault, stranger);
  await assert.rejects(reader.files(), /may be forged/);
  assert.throws(() => readerOf(objects(), undefined), /recipient/);
});
