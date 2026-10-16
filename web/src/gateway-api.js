// Shared HTTP authentication for the page and its file worker.
import { decryptWith } from "layr-desk-js/keys";
const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

export async function request(
  url,
  { method = "GET", body, token, raw, range } = {},
) {
  const headers = {};
  if (body) headers["content-type"] = "application/json";
  if (range) headers.range = range;
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(url, {
    method,
    headers,
    cache: "no-store",
    signal: raw ? undefined : AbortSignal.timeout(60_000),
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

export class GatewayApi {
  constructor(prefix) {
    this.prefix = prefix;
    this.base = `/api/p/${encodeURIComponent(prefix)}/`;
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
}
