// The person key (docs/design.md, "Person key"): the person index, invites and pins. The index
// is sealed to the person recipient with a MAC whose key comes from the person identity, so that
// only the person can write it. Each entry pins the public part of one vault.json.
import * as age from "age-encryption";
import { bech32Decode } from "./keys.mjs";

export const INDEX_HEADER = "layr-desk person index v1";
const PROJECT = /^[A-Za-z0-9][A-Za-z0-9_-]{0,43}$/;
const encoder = new TextEncoder();

/** The passkey PRF input of a person key: a passkey gives it a secret of its own. */
export const personSalt = (login) => encoder.encode(`layr-person/v1/${login}`);

const hex = (bytes) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

async function hmac(key, data) {
  const imported = await crypto.subtle.importKey(
    "raw",
    key,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return new Uint8Array(await crypto.subtle.sign("HMAC", imported, data));
}

/** HMAC-SHA256 of `body` with the key HMAC-SHA256(secret, INDEX_HEADER). */
async function indexMac(identity, body) {
  const secret = bech32Decode(identity).bytes;
  return hex(
    await hmac(await hmac(secret, encoder.encode(INDEX_HEADER)), body),
  );
}

function validRecipient(text) {
  try {
    const { hrp, bytes } = bech32Decode(String(text));
    return hrp === "age" && bytes.length === 32 && text === text.toLowerCase();
  } catch {
    return false;
  }
}

/** A pin with its four fields, or an error that names what is wrong. */
export function validPin(pin) {
  const { project, recipient } = pin ?? {};
  const writer = pin?.writer ?? null;
  const signedFrom = pin?.signedFrom ?? null;
  if (typeof project !== "string" || !PROJECT.test(project))
    throw new Error(
      `The pin names no valid vault: ${JSON.stringify(project)}.`,
    );
  if (!validRecipient(recipient))
    throw new Error(`The pin of ${project} has no valid recipient.`);
  let raw = null;
  try {
    raw = writer === null ? null : unb64(writer);
  } catch {}
  if (writer !== null && raw?.length !== 32)
    throw new Error(
      `The writer key in the pin of ${project} is not base64 of 32 bytes.`,
    );
  if (
    signedFrom !== null &&
    !(Number.isSafeInteger(signedFrom) && signedFrom >= 0)
  )
    throw new Error(`The pin of ${project} has no valid first signed state.`);
  return { project, recipient, writer, signedFrom };
}

/** The pin of a vault, from the public part of its vault.json. */
export const pinOf = (project, vault) =>
  validPin({
    project,
    recipient: String(vault.recipient).trim(),
    writer: vault.writer ?? null,
    signedFrom: vault.signedFrom ?? null,
  });

/**
 * Refuse a vault.json that differs from its pin: another recipient, writer key or first signed
 * state. A larger signedFrom would let unsigned states through.
 */
export function checkPin(vault, pin) {
  const differ = [];
  if (String(vault.recipient).trim() !== pin.recipient)
    differ.push("recipient");
  if ((vault.writer ?? null) !== pin.writer) differ.push("writer key");
  if ((vault.signedFrom ?? null) !== pin.signedFrom)
    differ.push("first signed state");
  if (differ.length)
    throw new Error(
      `The vault.json of ${pin.project} differs from your pin (${differ.join(", ")}). Someone may have changed it; ask the owner of the vault for a new invite.`,
    );
}

async function encrypt(recipient, data) {
  const encrypter = new age.Encrypter();
  encrypter.addRecipient(recipient);
  return encrypter.encrypt(data);
}

async function decrypt(identity, data, what) {
  const decrypter = new age.Decrypter();
  decrypter.addIdentity(identity);
  try {
    return await decrypter.decrypt(data);
  } catch {
    throw new Error(`The ${what} is not for this person key.`);
  }
}

/** The index {vaults: [pin]} with its MAC, encrypted to the person recipient. */
export async function sealIndex(identity, vaults) {
  const sorted = vaults
    .map(validPin)
    .sort((a, b) =>
      a.project < b.project ? -1 : a.project > b.project ? 1 : 0,
    );
  const body = encoder.encode(JSON.stringify({ vaults: sorted }));
  const head = encoder.encode(
    `${INDEX_HEADER}\n${await indexMac(identity, body)}\n`,
  );
  const plain = new Uint8Array(head.length + body.length);
  plain.set(head);
  plain.set(body, head.length);
  return encrypt(await age.identityToRecipient(identity), plain);
}

/** The index in `data`; an index without a valid MAC is refused. */
export async function openIndex(identity, data) {
  const plain = await decrypt(identity, data, "person index");
  const first = plain.indexOf(10);
  const second = first < 0 ? -1 : plain.indexOf(10, first + 1);
  if (second < 0) throw new Error("The person index is not complete.");
  const decoder = new TextDecoder();
  if (decoder.decode(plain.subarray(0, first)) !== INDEX_HEADER)
    throw new Error("The person index has an unknown format.");
  const body = plain.subarray(second + 1);
  const given = decoder.decode(plain.subarray(first + 1, second));
  if (given !== (await indexMac(identity, body)))
    throw new Error(
      "The MAC of the person index does not match; someone else may have written it.",
    );
  const { vaults } = JSON.parse(decoder.decode(body));
  return { vaults: vaults.map(validPin) };
}

/** An invite to one vault, encrypted to the person recipient. */
export async function sealInvite(recipient, pin) {
  return encrypt(
    recipient,
    encoder.encode(JSON.stringify({ invite: 1, ...validPin(pin) })),
  );
}

/** The pin in an invite. */
export async function openInvite(identity, data) {
  const plain = await decrypt(identity, data, "invite");
  let invite;
  try {
    invite = JSON.parse(new TextDecoder().decode(plain));
  } catch {
    throw new Error("The invite is not valid.");
  }
  if (invite?.invite !== 1)
    throw new Error("The invite has an unknown format.");
  return validPin(invite);
}
