// The site's client in Node: what the person key keeps when its unlock fails.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  generateIdentity,
  recipientOf,
  sealSlot,
} from "../../js/vault/src/keys.mjs";
import { INDEX_HEADER } from "../../js/vault/src/person.mjs";
import { Person } from "../src/client.js";

test("a person key whose index fails its check stays locked", async () => {
  const identity = await generateIdentity();
  const recipient = await recipientOf(identity);
  // An index that anyone could write to the recipient: its MAC does not match.
  const forged = await sealSlot(
    `${INDEX_HEADER}\n${"0".repeat(64)}\n{"vaults":[]}`,
    recipient,
  );
  const person = new Person({
    login: "ann@example.com",
    recipient,
    slots: [],
    index: Buffer.from(forged).toString("base64"),
    inbox: [],
  });
  await assert.rejects(person.unlocked(identity), /does not match/);
  assert.equal(person.identity, null);
  assert.equal(person.pins, null);
  // Another key is refused before anything is kept.
  await assert.rejects(
    person.unlocked(await generateIdentity()),
    /not your person key/,
  );
  assert.equal(person.identity, null);
});
