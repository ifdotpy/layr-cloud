// The gateway API, the passkeys and the reader of the site (docs/design.md, "Gateway" and
// "Person key").
import { VaultReader, zstdReady } from "layr-desk-js/reader";
import {
  checkVaultIdentity,
  decryptWith,
  identityFromPaper,
  identityFromSecret,
  openSlot,
  prfSalt,
  recipientOf,
  sealSlot,
} from "layr-desk-js/keys";
import {
  checkPin,
  openIndex,
  openInvite,
  personSalt,
  sealIndex,
} from "layr-desk-js/person";

const b64 = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
export const b64url = (bytes) =>
  b64(bytes).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const unb64url = (text) =>
  unb64(
    text.replace(/-/g, "+").replace(/_/g, "/") +
      "===".slice((text.length + 3) % 4),
  );
const sha256 = async (data) =>
  Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", data)), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");

async function request(url, { method = "GET", body, token, raw } = {}) {
  const headers = {};
  // Every write is JSON: the gateway refuses other writes, which a page of another site could
  // send without a CORS preflight.
  if (body || method !== "GET") headers["content-type"] = "application/json";
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(url, {
    method,
    headers,
    cache: "no-store",
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  if (!response.ok) {
    const data = await response.json().catch(() => ({}));
    throw Object.assign(new Error(data.error ?? `HTTP ${response.status}`), {
      status: response.status,
    });
  }
  return raw ? response : response.json();
}

/** The vaults that this gateway shows to the caller. */
export async function projects() {
  return (await request("/api/projects")).projects;
}

const passkeysOf = (slots) =>
  slots.filter((s) => s.name.startsWith("passkey-"));

/**
 * What a vault and a person key share: an identity that slots open, passkey slots whose PRF
 * output opens them, and a gateway token that only the holder of the identity gets.
 */
class Keyholder {
  constructor(base, salt, user) {
    this.base = base;
    this.salt = salt;
    this.user = user;
    this.identity = null;
    this.token = null;
  }

  /** Call with a fresh token after a 401 (tokens live 10 minutes). */
  async authorized(path, options = {}) {
    for (let attempt = 0; ; attempt++) {
      if (!this.token) {
        const { challenge } = await request(`${this.base}challenge`);
        this.token = new TextDecoder().decode(
          await decryptWith(this.identity, unb64(challenge)),
        );
      }
      try {
        return await request(this.base + path, {
          ...options,
          token: this.token,
        });
      } catch (error) {
        if (error.status !== 401 || attempt) throw error;
        this.token = null;
      }
    }
  }

  /** The identity in the passkey slot of the passkey that the person picks. */
  async passkeyIdentity(slots) {
    const passkeys = passkeysOf(slots);
    if (!passkeys.length) throw new Error("No passkey opens this key yet.");
    const credential = await navigator.credentials.get({
      publicKey: {
        challenge: crypto.getRandomValues(new Uint8Array(32)),
        rpId: location.hostname,
        userVerification: "required",
        allowCredentials: passkeys.map((s) => ({
          type: "public-key",
          id: unb64url(s.name.slice(8)),
        })),
        extensions: { prf: { eval: { first: this.salt } } },
      },
    });
    const slot = passkeys.find(
      (s) => s.name === `passkey-${b64url(credential.rawId)}`,
    );
    return openSlot(slot.data, this.opener(credential));
  }

  opener(credential) {
    const secret = credential.getClientExtensionResults().prf?.results?.first;
    if (!secret)
      throw new Error(
        "This passkey provider gave no PRF result, so it cannot open the vault.",
      );
    return identityFromSecret(new Uint8Array(secret));
  }

  /** Make a new passkey on this device and give it a slot. */
  async addPasskey(label) {
    const created = await navigator.credentials.create({
      publicKey: {
        challenge: crypto.getRandomValues(new Uint8Array(32)),
        rp: { id: location.hostname, name: location.hostname },
        user: {
          id: crypto.getRandomValues(new Uint8Array(16)),
          name: this.user,
          displayName: label || "Vault",
        },
        pubKeyCredParams: [
          { type: "public-key", alg: -7 },
          { type: "public-key", alg: -257 },
        ],
        authenticatorSelection: {
          residentKey: "required",
          userVerification: "required",
        },
        extensions: { prf: { eval: { first: this.salt } } },
      },
    });
    // Some providers give PRF results only when a passkey signs in.
    const credential = created.getClientExtensionResults().prf?.results?.first
      ? created
      : await this.pick([created.rawId]);
    return this.seal(credential);
  }

  /** Give a slot to a passkey that already exists, such as one that opens another vault. */
  async useExistingPasskey() {
    return this.seal(await this.pick([]));
  }

  pick(ids) {
    return navigator.credentials.get({
      publicKey: {
        challenge: crypto.getRandomValues(new Uint8Array(32)),
        rpId: location.hostname,
        userVerification: "required",
        allowCredentials: ids.map((id) => ({ type: "public-key", id })),
        extensions: { prf: { eval: { first: this.salt } } },
      },
    });
  }

  async seal(credential) {
    const slot = await sealSlot(
      this.identity,
      await recipientOf(this.opener(credential)),
    );
    const name = `passkey-${b64url(credential.rawId)}`;
    await this.authorized(`slots/${name}`, {
      method: "PUT",
      body: { data: b64(slot) },
    });
    return name;
  }

  async removePasskey(name) {
    await this.authorized(`slots/${name}`, { method: "DELETE" });
  }
}

export class Vault extends Keyholder {
  /** A vault and the public part of its vault.json, which the reader checks against. */
  static async open(prefix) {
    await zstdReady("/zstd.wasm");
    const vault = new Vault(prefix);
    vault.info = await request(`${vault.base}vault`);
    return vault;
  }

  constructor(prefix) {
    super(
      `/api/p/${encodeURIComponent(prefix)}/`,
      prfSalt(prefix),
      `vault@${location.hostname}`,
    );
    this.prefix = prefix;
  }

  async slots() {
    return (await request(`${this.base}slots`)).slots.map((s) => ({
      name: s.name,
      data: unb64(s.data),
    }));
  }

  async unlockWithPaper(paperKey, slots) {
    const paper = slots.find((s) => s.name === "paper");
    if (!paper) throw new Error("This vault has no paper slot.");
    await this.unlocked(
      await openSlot(paper.data, identityFromPaper(paperKey)),
    );
  }

  async unlockWithPasskey(slots) {
    if (!passkeysOf(slots).length)
      throw new Error("No passkey opens this vault yet.");
    await this.unlocked(await this.passkeyIdentity(slots));
  }

  /** Keep the vault identity of an opened slot, unless the slot holds another identity. */
  async unlocked(identity) {
    await checkVaultIdentity(identity, this.info.recipient);
    this.identity = identity;
    this.token = null;
    const { states } = await this.authorized("states");
    this.reader = new VaultReader({
      prefix: this.prefix,
      keys: states.map((name) => `${this.prefix}/states/${name}`),
      identity,
      vault: this.info,
      get: (key) => this.fetchObject(key),
    });
  }

  async fetchObject(key) {
    const name = key.slice(this.prefix.length + 8);
    const response = await this.authorized(`states/${name}`, { raw: true });
    return new Uint8Array(await response.arrayBuffer());
  }

  lock() {
    this.identity = null;
    this.token = null;
    this.reader = null;
  }
}

/**
 * The person key of the caller (docs/design.md, "Person key"): it opens the person slot of each
 * vault in the person index, and the index pins each vault's recipient, writer key and
 * signedFrom.
 */
export class Person extends Keyholder {
  /** The caller's records, or null: no login (a tagged device) or a gateway without them. */
  static async load() {
    try {
      return new Person(await request("/api/person"));
    } catch {
      return null;
    }
  }

  constructor(record) {
    super("/api/person/", personSalt(record.login), record.login);
    this.apply(record);
    this.pins = null;
  }

  apply(record) {
    this.login = record.login;
    this.recipient = record.recipient;
    this.slots = record.slots.map((s) => ({
      name: s.name,
      data: unb64(s.data),
    }));
    this.indexData = record.index ? unb64(record.index) : null;
    this.inbox = record.inbox;
  }

  /** Whether the person has a person key at this gateway. */
  get registered() {
    return Boolean(this.recipient);
  }

  passkeys() {
    return passkeysOf(this.slots);
  }

  async refresh() {
    this.apply(await request("/api/person"));
    if (this.identity) this.pins = await this.readIndex();
  }

  async unlockWithPaper(text) {
    const paper = this.slots.find((s) => s.name === "paper");
    if (!paper) throw new Error("Your person key has no paper slot.");
    await this.unlocked(await openSlot(paper.data, identityFromPaper(text)));
  }

  async unlockWithPasskey() {
    await this.unlocked(await this.passkeyIdentity(this.slots));
  }

  /** Keep the person identity of an opened slot, unless the slot holds another identity. */
  async unlocked(identity) {
    if ((await recipientOf(identity)) !== this.recipient)
      throw new Error(
        "This key is not your person key: its recipient is not the one that the gateway keeps, so the slot may be forged.",
      );
    this.identity = identity;
    this.token = null;
    try {
      this.pins = await this.readIndex();
    } catch (error) {
      // An index that fails its check keeps nothing, so the page can lock and try again.
      this.lock();
      throw error;
    }
  }

  async readIndex() {
    if (!this.indexData) return new Map();
    const { vaults } = await openIndex(this.identity, this.indexData);
    return new Map(vaults.map((pin) => [pin.project, pin]));
  }

  /** The inbox, opened: each invite with its pin, or with the reason why it does not open. */
  async invites() {
    return Promise.all(
      this.inbox.map(async (item) => {
        try {
          return {
            ...item,
            pin: await openInvite(this.identity, unb64(item.data)),
          };
        } catch (error) {
          return { ...item, error: error.message };
        }
      }),
    );
  }

  /** The vault identity in this person's slot of a vault. */
  async vaultIdentity(slots) {
    const slot = slots.find((s) => s.name === `person-${this.login}`);
    if (!slot) throw new Error("You have no person slot in this vault.");
    return openSlot(slot.data, this.identity);
  }

  /**
   * Check an invite against the vault as the gateway serves it: its vault.json must match the
   * pin, and the person slot must open to the pinned recipient.
   */
  async checkInvite(pin) {
    let vault;
    try {
      vault = await Vault.open(pin.project);
    } catch (error) {
      if (error.status === 404)
        throw new Error(
          `The gateway does not show you the vault ${pin.project}.`,
        );
      throw error;
    }
    checkPin(vault.info, pin);
    await checkVaultIdentity(
      await this.vaultIdentity(await vault.slots()),
      pin.recipient,
    );
  }

  /** Add the pin of a checked invite to the index, then remove the invite. */
  async accept(invite) {
    await this.checkInvite(invite.pin);
    const pins = new Map(this.pins);
    pins.set(invite.pin.project, invite.pin);
    await this.saveIndex(pins);
    await this.authorized(`inbox/${invite.id}`, { method: "DELETE" });
    await this.refresh();
  }

  async decline(invite) {
    await this.authorized(`inbox/${invite.id}`, { method: "DELETE" });
    await this.refresh();
  }

  /** Write the index over the one that this tab read; the gateway refuses it after a change. */
  async saveIndex(pins) {
    const data = await sealIndex(this.identity, [...pins.values()]);
    const previous = this.indexData ? await sha256(this.indexData) : null;
    await this.authorized("index", {
      method: "PUT",
      body: { data: b64(data), previous },
    });
    this.indexData = data;
    this.pins = pins;
  }

  /** Open a pinned vault through the person slot, after its vault.json matches the pin. */
  async openVault(vault, slots) {
    const pin = this.pins.get(vault.prefix);
    checkPin(vault.info, pin);
    const identity = await this.vaultIdentity(slots);
    await checkVaultIdentity(identity, pin.recipient);
    await vault.unlocked(identity);
  }

  lock() {
    this.identity = null;
    this.token = null;
    this.pins = null;
  }
}
