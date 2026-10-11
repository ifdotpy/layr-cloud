// The person index, invites and pins (docs/design.md, "Person key"), and the vectors that
// desk-core sealed (crates/desk-core/tests/person-vectors.json): the same format in both.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as age from "age-encryption";
import { generateIdentity, recipientOf } from "../src/keys.mjs";
import {
  INDEX_HEADER,
  checkPin,
  openIndex,
  openInvite,
  pinOf,
  sealIndex,
  sealInvite,
  validPin,
} from "../src/person.mjs";

const vectors = JSON.parse(
  readFileSync(
    new URL(
      "../../../crates/desk-core/tests/person-vectors.json",
      import.meta.url,
    ),
    "utf8",
  ),
);
const unb64 = (text) => Uint8Array.from(Buffer.from(text, "base64"));

async function encrypt(recipient, text) {
  const encrypter = new age.Encrypter();
  encrypter.addRecipient(recipient);
  return encrypter.encrypt(text);
}

async function pin(project) {
  return {
    project,
    recipient: await recipientOf(await generateIdentity()),
    writer: Buffer.alloc(32, 5).toString("base64"),
    signedFrom: 4,
  };
}

test("the index and the invite that desk-core sealed open here", async () => {
  const { identity, pins } = vectors;
  assert.deepEqual(await openIndex(identity, unb64(vectors.rustIndex)), {
    vaults: pins,
  });
  assert.deepEqual(
    await openInvite(identity, unb64(vectors.rustInvite)),
    pins[0],
  );
  await assert.rejects(
    openIndex(await generateIdentity(), unb64(vectors.rustIndex)),
    /not for this person key/,
  );
});

test("an index opens for its person only, and a forged one fails its MAC", async () => {
  const person = await generateIdentity();
  const recipient = await recipientOf(person);
  const pins = [await pin("q"), await pin("p")];
  const sealed = await sealIndex(person, pins);
  const { vaults } = await openIndex(person, sealed);
  assert.deepEqual(
    vaults.map((v) => v.project),
    ["p", "q"],
  );
  // Anyone can encrypt to the recipient, but not make the MAC.
  const forged = `${INDEX_HEADER}\n${"0".repeat(64)}\n${JSON.stringify({ vaults: pins })}`;
  await assert.rejects(
    openIndex(person, await encrypt(recipient, forged)),
    /does not match/,
  );
  const stranger = await generateIdentity();
  const decrypter = new age.Decrypter();
  decrypter.addIdentity(stranger);
  const theirs = await decrypter.decrypt(await sealIndex(stranger, pins));
  await assert.rejects(
    openIndex(person, await encrypt(recipient, theirs)),
    /does not match/,
  );
  await assert.rejects(
    openIndex(
      person,
      await encrypt(recipient, "layr-desk person index v2\n00\n{}"),
    ),
    /unknown format/,
  );
});

test("an invite carries one valid pin", async () => {
  const person = await generateIdentity();
  const recipient = await recipientOf(person);
  const given = await pin("p");
  assert.deepEqual(
    await openInvite(person, await sealInvite(recipient, given)),
    given,
  );
  const plain = { ...given, writer: null, signedFrom: null };
  assert.deepEqual(
    await openInvite(person, await sealInvite(recipient, plain)),
    plain,
  );
  for (const bad of [
    { ...given, project: "../x" },
    { ...given, recipient: "age1nope" },
    { ...given, writer: "AAAA" },
    { ...given, signedFrom: -1 },
  ]) {
    assert.throws(() => validPin(bad));
    const data = await encrypt(
      recipient,
      JSON.stringify({ invite: 1, ...bad }),
    );
    await assert.rejects(openInvite(person, data));
  }
  await assert.rejects(
    openInvite(
      person,
      await encrypt(recipient, JSON.stringify({ invite: 2, ...given })),
    ),
    /unknown format/,
  );
});

test("a vault.json must match its pin", async () => {
  const pinned = await pin("p");
  const vault = { ...pinned, recipient: `${pinned.recipient}\n` };
  checkPin(vault, pinned);
  assert.deepEqual(pinOf("p", vault), pinned);
  const other = await recipientOf(await generateIdentity());
  for (const [what, changed] of [
    ["recipient", { recipient: other }],
    ["writer key", { writer: Buffer.alloc(32, 6).toString("base64") }],
    ["writer key", { writer: null }],
    ["first signed state", { signedFrom: 99 }],
  ])
    assert.throws(
      () => checkPin({ ...vault, ...changed }, pinned),
      new RegExp(`\\(${what}\\)`),
    );
});
