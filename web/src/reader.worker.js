import init, { BlockReader, decrypt_object } from "./generated/desk_web.js";
import { VaultReader, unzstd, zstdReady } from "layr-desk-js/reader";
import { GatewayApi } from "./gateway-api.js";

import {
  validateMarker,
  validateDirectory,
  validatePath,
} from "./browse-index.js";

const BLOCK = 128 * 1024;
const CACHE = 128 * 1024 * 1024;
let reader,
  vault,
  report = () => {},
  downloaded = 0;

class RangeReader extends VaultReader {
  constructor(options) {
    super(options);
    this.parts = new Map();
    this.locks = new Map();
  }
  async body(response, limit = Infinity) {
    const total = Number(response.headers.get("content-length")) || undefined;
    const chunks = [];
    let size = 0;
    const stream = response.body.getReader();
    for (;;) {
      const { done, value } = await stream.read();
      if (done) break;
      chunks.push(value);
      size += value.length;
      downloaded += value.length;
      if (size > limit || (total !== undefined && size > total)) {
        await stream.cancel();
        throw new Error("The response exceeds its declared size.");
      }
      report({ phase: "download", bytes: downloaded });
    }
    if (total !== undefined && size !== total)
      throw new Error("The response is incomplete.");
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    return bytes;
  }
  async response(key, range) {
    return vault.authorized(`states/${key.split("/").at(-1)}`, {
      raw: true,
      range,
    });
  }
  decrypt(bytes) {
    report({ phase: "decrypt", bytes: downloaded });
    return decrypt_object(this.identity, bytes);
  }
  async object(key) {
    // Trees keep the existing promise cache. Failed fetches can be retried.
    if (!this.cache.has(key)) {
      const promise = this.response(key)
        .then((r) => this.body(r))
        .then((b) => this.decrypt(b));
      this.cache.set(key, promise);
      promise.catch(() => this.cache.delete(key));
    }
    return this.cache.get(key);
  }
  trim() {
    let size = [...this.parts.values()].reduce((n, p) => n + p.size, 0);
    for (const [key, part] of this.parts) {
      if (size <= CACHE) break;
      if (this.locks.has(key)) continue;
      part.decoder?.free();
      this.parts.delete(key);
      size -= part.size;
    }
  }
  async directory({ seq, path, page }) {
    validatePath(path);
    if (!Number.isSafeInteger(page) || page < 0 || page > 99999999)
      throw new Error("Invalid folder page.");
    const state = this.states().find((item) => item.seq === seq);
    if (!state) throw new Error("Unknown version.");
    const stem = `${String(seq).padStart(10, "0")}.${state.attempt}`;
    const json = async (name) =>
      JSON.parse(
        new TextDecoder("utf-8", { fatal: true }).decode(
          await this.object(`${this.prefix}/states/${name}.age`),
        ),
      );
    const marker = validateMarker(await json(`${stem}.browse`), state);
    const hash = [
      ...new Uint8Array(
        await crypto.subtle.digest("SHA-256", new TextEncoder().encode(path)),
      ),
    ]
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    const value = validateDirectory(
      await json(`${stem}.${hash}.${String(page).padStart(8, "0")}.dir`),
      state,
      path,
      page,
    );
    // Keep directory metadata bounded independently of the archive part cache.
    while (this.cache.size > 32)
      this.cache.delete(this.cache.keys().next().value);
    return { ...value, files: marker.files };
  }
  async frame(link) {
    const key = this.key(...link.slice(0, 3));
    const previous = this.locks.get(key) ?? Promise.resolve();
    const task = previous
      .catch(() => {})
      .then(() => this.frameUnlocked(key, link));
    this.locks.set(key, task);
    try {
      return await task;
    } catch (error) {
      const part = this.parts.get(key);
      part?.decoder?.free();
      this.parts.delete(key);
      throw error;
    } finally {
      if (this.locks.get(key) === task) this.locks.delete(key);
      this.trim();
    }
  }
  async frameUnlocked(key, [, , , offset, length]) {
    if (
      ![offset, length, offset + length].every(
        (n) => Number.isSafeInteger(n) && n >= 0,
      )
    )
      throw new Error("Invalid frame bounds.");
    let part = this.parts.get(key);
    if (!part) {
      part = { decoder: new BlockReader(this.identity), requests: 0, size: 0 };
      this.parts.set(key, part);
    }
    // Refresh insertion order for cache eviction.
    this.parts.delete(key);
    this.parts.set(key, part);
    const slice = () => {
      if (offset + length > part.full.length)
        throw new Error("A frame is outside its part.");
      return part.full.slice(offset, offset + length);
    };
    const keepFull = (bytes) => {
      part.full = this.decrypt(bytes);
      part.size = part.full.length;
      part.decoder.free();
      part.decoder = null;
      return slice();
    };
    if (part.full) return slice();
    if (length >= 512 * 1024)
      return keepFull(await this.body(await this.response(key)));
    for (;;) {
      report({ phase: "decrypt", bytes: downloaded });
      const frame = part.decoder.frame(BigInt(offset), length);
      if (frame !== undefined) return frame;
      if (part.requests >= 8)
        return keepFull(await this.body(await this.response(key)));
      const start = Number(part.decoder.needed());
      const end = start + BLOCK - 1;
      const response = await this.response(key, `bytes=${start}-${end}`);
      if (response.status === 200) return keepFull(await this.body(response));
      const match = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(
        response.headers.get("content-range") ?? "",
      );
      if (response.status !== 206 || !match)
        throw new Error("Invalid range response.");
      const [a, b, total] = match.slice(1).map(Number);
      if (
        ![a, b, total].every(Number.isSafeInteger) ||
        a !== start ||
        b !== Math.min(end, total - 1) ||
        a > b
      )
        throw new Error("Incorrect range bounds.");
      const bytes = await this.body(response, b - a + 1);
      if (bytes.length !== b - a + 1)
        throw new Error("Incomplete range response.");
      part.decoder.insert(BigInt(start), BigInt(total), bytes);
      part.requests++;
      part.size += bytes.length;
    }
  }
  async read(entry) {
    const chain = [...entry[3]].reverse();
    if (!chain.length || chain[0][5] !== "full")
      throw new Error("Invalid file chain.");
    // At most two frames in flight. Decoding the delta chain still follows its order.
    const frames = Array.from({ length: chain.length });
    let next = 0;
    const fetched = await Promise.allSettled(
      [0, 1].map(async () => {
        while (next < chain.length) {
          const i = next++;
          frames[i] = await this.frame(chain[i]);
        }
      }),
    );
    const failed = fetched.find((result) => result.status === "rejected");
    if (failed) throw failed.reason;
    report({ phase: "restore", bytes: downloaded });
    let data;
    for (let i = 0; i < chain.length; i++)
      data = await unzstd(
        frames[i],
        chain[i][5] === "delta" ? data : undefined,
      );
    const hash = [
      ...new Uint8Array(await crypto.subtle.digest("SHA-256", data)),
    ]
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    if (data.length !== entry[1] || hash !== entry[2])
      throw new Error("The content does not match its file hash.");
    return data;
  }
}

// Serialize user operations so their progress and cache ownership cannot cross.
let queue = Promise.resolve();
self.onmessage = ({ data: { id, method, value } }) => {
  queue = queue.then(async () => {
    downloaded = 0;
    report = (progress) => self.postMessage({ id, progress });
    try {
      let result;
      if (method === "init") {
        await Promise.all([init(), zstdReady("/zstd.wasm")]);
        vault = new GatewayApi(value.prefix);
        vault.identity = value.identity;
        vault.token = value.token;
        reader = new RangeReader(value);
      } else if (method === "directory") result = await reader.directory(value);
      else if (method === "files") result = await reader.files(value);
      else if (method === "read") result = await reader.read(value);
      else throw new Error("Unknown reader operation.");
      self.postMessage(
        { id, result },
        result instanceof Uint8Array ? [result.buffer] : [],
      );
    } catch (error) {
      self.postMessage({ id, error: error?.message ?? String(error) });
    }
  });
};
