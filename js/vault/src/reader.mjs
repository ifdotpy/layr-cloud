// The vault reader (docs/design.md, "Reader"): the browser, vault-restore and vault-view use it.
// It checks the vault identity against the recipient in vault.json, the writer signature of each
// tree that vault.json asks for (docs/design.md, "Signatures"), and each file against the size
// and SHA-256 of its tree entry. Signatures use WebCrypto Ed25519.
import * as age from "age-encryption";
import {
  createDCtx,
  decompress,
  decompressUsingDict,
  init,
} from "@bokuweb/zstd-wasm";
import { checkVaultIdentity } from "./keys.mjs";

let zstd;

/** Load the zstd decoder once; `wasmUrl` is needed in a browser bundle. */
export function zstdReady(wasmUrl) {
  zstd ??= wasmUrl ? init(wasmUrl) : init();
  return zstd;
}

/** One zstd frame; a delta frame takes the base content as a raw dictionary. */
export async function unzstd(frame, base) {
  await zstdReady();
  return base && base.length
    ? decompressUsingDict(createDCtx(), frame, base)
    : decompress(frame);
}

const TREE = /^(\d{10})\.([0-9a-f]{8})\.tree\.age$/;

const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
const hex = (bytes) =>
  Array.from(new Uint8Array(bytes), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
const sha256 = async (data) => hex(await crypto.subtle.digest("SHA-256", data));

/**
 * The message that the writer signs for one tree: the header fields and the SHA-256 (lowercase
 * hex) of the canonical tree bytes.
 */
export function treeMessage(prefix, header, sha) {
  const base = header.base ? `${header.base[0]}.${header.base[1]}` : "-";
  return [
    "layr-desk tree v1",
    prefix,
    header.seq,
    header.attempt,
    header.state,
    header.time,
    header.tree,
    base,
    sha,
  ].join("\n");
}

async function importWriter(text) {
  let raw;
  try {
    raw = unb64(String(text));
  } catch {
    raw = null;
  }
  if (raw?.length !== 32)
    throw new Error("The writer key in vault.json is not base64 of 32 bytes.");
  try {
    return await crypto.subtle.importKey(
      "raw",
      raw,
      { name: "Ed25519" },
      false,
      ["verify"],
    );
  } catch (error) {
    if (error.name === "NotSupportedError")
      throw new Error(
        "This browser cannot check Ed25519 signatures (WebCrypto). Use a current browser.",
      );
    throw new Error(
      `The writer key in vault.json is not valid: ${error.message}`,
    );
  }
}

export class VaultReader {
  /**
   * `vault` is the public part of vault.json: {recipient, writer, signedFrom}.
   * @param {{prefix: string, keys: string[], get: (key: string) => Promise<Uint8Array>, identity: string, vault: {recipient: string, writer?: string | null, signedFrom?: number | null}}} options
   */
  constructor({ prefix, keys, get, identity, vault }) {
    if (!vault?.recipient)
      throw new Error("The reader needs the recipient of the vault.");
    this.prefix = prefix;
    this.get = get;
    this.identity = identity;
    this.vault = vault;
    this.signedFrom = vault.signedFrom ?? 0;
    // Every decryption waits for this check, so no reader works with a forged slot's key.
    this.checked = checkVaultIdentity(identity, vault.recipient);
    this.checked.catch(() => {});
    this.cache = new Map();
    this.decoded = new Map();
    this.trees = new Map();
    for (const key of keys) {
      const match =
        key.startsWith(`${prefix}/states/`) &&
        TREE.exec(key.slice(prefix.length + 8));
      if (match) {
        const seq = Number(match[1]);
        this.trees.set(seq, [...(this.trees.get(seq) ?? []), match[2]].sort());
      }
    }
  }

  /** Every pushed state, oldest first: [{seq, attempt}]. */
  states() {
    return [...this.trees.keys()]
      .sort((a, b) => a - b)
      .map((seq) => ({ seq, attempt: this.trees.get(seq)[0] }));
  }

  latest() {
    const all = this.states();
    if (!all.length) throw new Error("the vault has no state yet");
    return all[all.length - 1];
  }

  /**
   * Whether state `seq` carries a checked writer signature: reading it fails without a valid
   * one. The other states read as before and show as unsigned.
   */
  signed(seq) {
    return Boolean(this.vault.writer) && seq >= this.signedFrom;
  }

  key(seq, attempt, part) {
    const name =
      `${String(seq).padStart(10, "0")}.${attempt}.` +
      (part === undefined ? "tree" : `${String(part).padStart(4, "0")}.data`);
    return `${this.prefix}/states/${name}.age`;
  }

  async object(key) {
    if (!this.cache.has(key)) {
      const decrypted = (async () => {
        await this.checked;
        const decrypter = new age.Decrypter();
        decrypter.addIdentity(this.identity);
        return decrypter.decrypt(await this.get(key));
      })();
      this.cache.set(key, decrypted);
      decrypted.catch(() => this.cache.delete(key));
    }
    return this.cache.get(key);
  }

  /**
   * The header and the canonical tree bytes of one state. Each tree on the way is checked
   * against the signature policy of the vault.
   */
  tree(seq, attempt) {
    const key = `${seq}.${attempt}`;
    if (!this.decoded.has(key)) {
      const decoded = this.decode(seq, attempt);
      this.decoded.set(key, decoded);
      decoded.catch(() => this.decoded.delete(key));
    }
    return this.decoded.get(key);
  }

  async decode(seq, attempt) {
    const raw = await this.object(this.key(seq, attempt));
    const newline = raw.indexOf(10);
    if (newline < 0)
      throw new Error(`The tree of state #${seq} has no header.`);
    const header = JSON.parse(
      new TextDecoder().decode(raw.subarray(0, newline)),
    );
    if (header.v !== 1)
      throw new Error(`Tree format ${header.v} is not known.`);
    if (header.seq !== seq || header.attempt !== attempt)
      throw new Error(
        `The tree ${seq}.${attempt} names itself ${header.seq}.${header.attempt}.`,
      );
    const body = raw.subarray(newline + 1);
    let canonical;
    if (header.tree === "full") canonical = await unzstd(body);
    else if (header.tree === "delta" && header.base?.[0] < seq)
      canonical = await unzstd(
        body,
        (await this.tree(...header.base)).canonical,
      );
    else throw new Error(`The tree of state #${seq} names no earlier base.`);
    await this.verify(header, canonical);
    return { header, canonical };
  }

  async verify(header, canonical) {
    if (!this.signed(header.seq)) return;
    const state = `State #${header.seq}`;
    if (typeof header.sig !== "string")
      throw new Error(
        `${state} has no writer signature, which this vault requires from #${this.signedFrom} on. It may be forged.`,
      );
    let sig;
    try {
      sig = unb64(header.sig);
    } catch {
      sig = new Uint8Array();
    }
    this.writer ??= importWriter(this.vault.writer);
    const message = new TextEncoder().encode(
      treeMessage(this.prefix, header, await sha256(canonical)),
    );
    const valid =
      sig.length === 64 &&
      (await crypto.subtle.verify(
        { name: "Ed25519" },
        await this.writer,
        sig,
        message,
      ));
    if (!valid)
      throw new Error(
        `${state} has a writer signature that does not match. It may be forged.`,
      );
  }

  /** {path: [mode, size, sha256, chain]} of one state (the latest by default). */
  async files(seq, attempt) {
    const state =
      seq === undefined
        ? this.latest()
        : { seq, attempt: attempt ?? this.trees.get(seq)[0] };
    const { canonical } = await this.tree(state.seq, state.attempt);
    return JSON.parse(new TextDecoder().decode(canonical)).files;
  }

  /**
   * The content of one tree entry: its chain decoded from the full frame up, checked against
   * the size and SHA-256 of the entry.
   */
  async read(entry) {
    const chain = entry[3];
    if (!chain.length || chain.at(-1)[5] !== "full")
      throw new Error("A file chain must end with a full frame.");
    let data;
    for (const [seq, attempt, part, offset, length, kind] of [
      ...chain,
    ].reverse()) {
      const blob = await this.object(this.key(seq, attempt, part));
      const frame = blob.subarray(offset, offset + length);
      data = kind === "delta" ? await unzstd(frame, data) : await unzstd(frame);
    }
    if (data.length !== entry[1] || (await sha256(data)) !== entry[2])
      throw new Error("The content does not match its tree entry.");
    return data;
  }
}
