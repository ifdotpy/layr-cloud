// The writer's fixture with signatures (fixture/signed.json: Node signed the fixture states with
// a test key, and desk-core makes the same signatures), and forged trees: what anyone who can
// write to the bucket can put there. The reader tests and the site e2e use them.
import * as age from "age-encryption";
import { compress } from "@bokuweb/zstd-wasm";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { VaultReader, zstdReady } from "../src/reader.mjs";

const fixture = new URL("./fixture/", import.meta.url).pathname;
export const vectors = JSON.parse(
  readFileSync(join(fixture, "signed.json"), "utf8"),
);
export const identity = readFileSync(
  join(fixture, "identity.txt"),
  "utf8",
).trim();
const recipient = JSON.parse(
  readFileSync(join(fixture, "expected.json"), "utf8"),
).recipient;

/** vault.json of the fixture vault, with the test writer from `signedFrom` on, or unsigned. */
export const signedVault = {
  recipient,
  writer: vectors.writer,
  signedFrom: vectors.signedFrom,
};
export const unsignedVault = { recipient, writer: null, signedFrom: null };

const treeKey = (prefix, { seq, attempt }) =>
  `${prefix}/states/${String(seq).padStart(10, "0")}.${attempt}.tree.age`;

async function encrypt(data) {
  const encrypter = new age.Encrypter();
  encrypter.addRecipient(recipient);
  return encrypter.encrypt(data);
}

function treeObject(header, body) {
  const line = new TextEncoder().encode(JSON.stringify(header) + "\n");
  const out = new Uint8Array(line.length + body.length);
  out.set(line);
  out.set(body, line.length);
  return out;
}

/** A tree object of `objects`, decrypted, changed by `change(header)` and encrypted again. */
export async function changeTree(objects, key, change) {
  const decrypter = new age.Decrypter();
  decrypter.addIdentity(identity);
  const plain = await decrypter.decrypt(objects.get(key));
  const newline = plain.indexOf(10);
  const header = JSON.parse(
    new TextDecoder().decode(plain.subarray(0, newline)),
  );
  change(header);
  objects.set(
    key,
    Buffer.from(await encrypt(treeObject(header, plain.subarray(newline + 1)))),
  );
}

/** The trees of `prefix` in `objects` get the signatures of signed.json from `signedFrom` on. */
export async function sign(objects, prefix) {
  for (const state of vectors.states)
    if (state.seq >= vectors.signedFrom)
      await changeTree(objects, treeKey(prefix, state), (header) => {
        header.sig = state.sig;
      });
}

/**
 * A forged state after the last one: the files of the last state, but `src/app.py` holds the
 * content of `README.md`, with `sig` as given (none, or a signature of another state).
 */
export async function forgeState(objects, prefix, sig) {
  const last = vectors.states.at(-1);
  const reader = new VaultReader({
    prefix,
    keys: [...objects.keys()],
    get: async (k) => objects.get(k),
    identity,
    vault: unsignedVault,
  });
  const { canonical } = await reader.tree(last.seq, last.attempt);
  const tree = JSON.parse(new TextDecoder().decode(canonical));
  tree.files["src/app.py"] = tree.files["README.md"];
  await zstdReady();
  const header = {
    v: 1,
    seq: last.seq + 1,
    attempt: "ffffffff",
    state: "forged",
    time: 1791800000,
    tree: "full",
    base: null,
    parts: 0,
    ...(sig === undefined ? {} : { sig }),
  };
  const body = compress(new TextEncoder().encode(JSON.stringify(tree)), 3);
  const key = treeKey(prefix, header);
  objects.set(key, Buffer.from(await encrypt(treeObject(header, body))));
  return key;
}
