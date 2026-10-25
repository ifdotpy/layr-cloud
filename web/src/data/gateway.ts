/** Same-origin gateway access. This module runs only in the model worker. */
export interface PersonView {
  login: string;
  recipient: string | null;
  slots: { name: string; data: string }[];
  index: string | null;
  inbox: { id: string; from: string; time: number; data: string }[];
}
export interface Descriptor { format: number; project: string; recipient: string }
export interface ObjectEntry { key: string; modified?: number }
export const fromBase64 = (text: string): Uint8Array => Uint8Array.from(atob(text), c => c.charCodeAt(0));

class HttpError extends Error {
  constructor(readonly status: number, message: string) { super(message); }
}
export class Gateway {
  private tokens = new Map<string, { token: string; expires: number }>();
  private ciphertext = new Map<string, Uint8Array>();
  private bytes = 0;
  readonly base: string;
  constructor(base = '') {
    const url = new URL(base || '/', location.origin);
    if (url.origin !== location.origin) throw new Error('The gateway must use the site origin.');
    this.base = url.pathname.replace(/\/$/, '');
  }
  private async request(path: string, init: RequestInit = {}): Promise<Response> {
    const response = await fetch(this.base + path, { ...init, credentials: 'same-origin', cache: 'no-store' });
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new HttpError(response.status, body.error || `HTTP ${response.status}`);
    }
    return response;
  }
  async person(): Promise<PersonView> { return (await this.request('/api/person')).json(); }
  async projects(): Promise<string[]> { return (await (await this.request('/api/projects')).json()).projects; }
  private route(project: string, path: string): string { return `/v1/p/${encodeURIComponent(project)}/${path}`; }
  async descriptor(project: string): Promise<Descriptor> {
    return (await this.request(this.route(project, 'store.json'))).json();
  }
  async slot(project: string, name: string): Promise<Uint8Array> {
    return new Uint8Array(await (await this.request(this.route(project, `slots/${encodeURIComponent(name)}`))).arrayBuffer());
  }
  private async token(project: string, decrypt: (bytes: Uint8Array) => string): Promise<string> {
    const old = this.tokens.get(project);
    if (old && old.expires > Date.now() + 5000) return old.token;
    const { challenge } = await (await this.request(this.route(project, 'challenge'))).json();
    const response = await this.request(this.route(project, 'token'), {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ challenge: decrypt(fromBase64(challenge)) }),
    });
    const token = await response.json();
    this.tokens.set(project, token);
    return token.token;
  }
  private async authorized(project: string, path: string, decrypt: (bytes: Uint8Array) => string): Promise<Response> {
    for (let attempt = 0; ; attempt++) {
      const token = await this.token(project, decrypt);
      try { return await this.request(this.route(project, path), { headers: { authorization: `Bearer ${token}` } }); }
      catch (error) {
        if (!(error instanceof HttpError) || error.status !== 401 || attempt > 0) throw error;
        this.tokens.delete(project);
      }
    }
  }
  async list(project: string, prefix: string, decrypt: (bytes: Uint8Array) => string): Promise<ObjectEntry[]> {
    const result = await (await this.authorized(project, `objects?prefix=${encodeURIComponent(prefix)}`, decrypt)).json();
    return result.objects;
  }
  async signedObject(project: string, key: string, decrypt: (bytes: Uint8Array) => string): Promise<Uint8Array> {
    if (!/^overlays\/[^/]+\/keys\/[^/]+\.json$/.test(key)) throw new Error('Invalid overlay key object.');
    return new Uint8Array(await (await this.authorized(project, `objects/${key.split('/').map(encodeURIComponent).join('/')}`, decrypt)).arrayBuffer());
  }
  async object(project: string, key: string, decrypt: (bytes: Uint8Array) => string): Promise<Uint8Array> {
    if (!key.endsWith('.age') || key.split('/').some(part => !part || part === '.' || part === '..')) throw new Error('Invalid ciphertext object.');
    const id = `${project}/${key}`;
    const cached = this.ciphertext.get(id);
    if (cached) return cached;
    const result = new Uint8Array(await (await this.authorized(project, `objects/${key.split('/').map(encodeURIComponent).join('/')}`, decrypt)).arrayBuffer());
    // This cache contains only age ciphertext and never outlives its worker.
    if (this.bytes + result.byteLength > 64 * 1024 * 1024) { this.ciphertext.clear(); this.bytes = 0; }
    if (result.byteLength <= 64 * 1024 * 1024) { this.ciphertext.set(id, result); this.bytes += result.byteLength; }
    return result;
  }
  private publicRoute(project: string, path: string): string { return `/p/${encodeURIComponent(project)}/${path}`; }
  async publicDescriptor(project: string): Promise<Descriptor> { return (await this.request(this.publicRoute(project, 'store.json'))).json(); }
  async publicManifest(project: string): Promise<Uint8Array> { return new Uint8Array(await (await this.request(this.publicRoute(project, 'public.json'))).arrayBuffer()); }
  async publicList(project: string, prefix: string): Promise<ObjectEntry[]> {
    return (await (await this.request(this.publicRoute(project, `objects?prefix=${encodeURIComponent(prefix)}`))).json()).objects;
  }
  async publicObject(project: string, key: string): Promise<Uint8Array> {
    if (!key.endsWith('.age') || key.split('/').some(part => !part || part === '.' || part === '..')) throw new Error('Invalid ciphertext object.');
    return new Uint8Array(await (await this.request(this.publicRoute(project, key.split('/').map(encodeURIComponent).join('/')))).arrayBuffer());
  }
  lock(): void { this.tokens.clear(); this.ciphertext.clear(); this.bytes = 0; }
}
