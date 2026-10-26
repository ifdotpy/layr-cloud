/** Same-origin gateway access. This module runs only in the model worker. */
export interface PersonView {
  login: string;
  recipient: string | null;
  signing_key?: string | null;
  signing?: string | null;
  slots: { name: string; data: string }[];
  index: string | null;
  inbox: { id: string; from: string; time: number; data: string }[];
}
export interface Descriptor { format: number; project: string; recipient: string }
export interface ObjectEntry { key: string; modified?: number }
export const fromBase64 = (text: string): Uint8Array => Uint8Array.from(atob(text), c => c.charCodeAt(0));

export class HttpError extends Error {
  constructor(readonly status: number, message: string) { super(message); }
}
export class Gateway {
  private epochs = new Map<string,string>();
  setEpoch(project: string, epoch: string) {
    if (epoch !== "root" && !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(epoch)) throw new Error("Invalid epoch.");
    if (this.epochs.get(project) !== epoch) this.tokens.delete(project);
    this.epochs.set(project,epoch);
  }
  private epochQuery(project: string) { const epoch=this.epochs.get(project); return epoch && epoch !== "root" ? `?epoch=${epoch}` : ""; }
  private currentPrefix(project: string) { const epoch=this.epochs.get(project); return epoch && epoch !== "root" ? `project/epochs/${epoch}/` : ""; }
  private tokens = new Map<string, { token: string; expires: number }>();
  private personSession?: {login: string; token: string; expires: number};
  private signer?: {login: string; sign: (challenge: string) => string};
  private signingIn?: Promise<void>;
  private generation = 0;
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
  async unlock(login: string): Promise<PersonView> {
    const result = await (await this.request(`/api/person/unlock?login=${encodeURIComponent(login)}`)).json();
    const bytes = fromBase64(result.unlock);
    if (bytes.length !== 65536) throw new Error('Invalid person unlock response.');
    const view = JSON.parse(new TextDecoder('utf-8', {fatal:true}).decode(bytes));
    if (view.login !== login || !Array.isArray(view.slots) || 'index' in view || 'inbox' in view) throw new Error('Invalid person unlock response.');
    return {...view, index:null, inbox:[]};
  }
  async signIn(login: string, sign: (challenge: string) => string): Promise<void> {
    this.signer = {login,sign};
    await this.renew();
  }
  private async renew(): Promise<void> {
    if (this.signingIn) return this.signingIn;
    const signer = this.signer, generation = this.generation;
    if (!signer) throw new Error('Sign in to the gateway.');
    const pending = (async () => {
      this.personSession = undefined;
      const {challenge} = await (await this.request(`/api/session/challenge?login=${encodeURIComponent(signer.login)}`)).json();
      if (generation !== this.generation) throw new Error('Your session is locked.');
      const response = await this.request('/api/session', {method:'POST', headers:{'content-type':'application/json'}, body:JSON.stringify({login:signer.login,challenge,signature:signer.sign(challenge)})});
      const token = await response.json();
      if (generation !== this.generation) throw new Error('Your session is locked.');
      if (token.login !== signer.login || typeof token.token !== 'string' || !token.token || !Number.isFinite(token.expires) || token.expires <= Date.now()) throw new Error('Invalid gateway session.');
      this.personSession = token;
    })();
    this.signingIn = pending;
    try { await pending; } finally { if (this.signingIn === pending) this.signingIn = undefined; }
  }
  private async personRequest(path: string, init: RequestInit = {}): Promise<Response> {
    for (let attempt = 0; ; attempt++) {
      if (!this.personSession || this.personSession.expires <= Date.now() + 5000) await this.renew();
      const headers = new Headers(init.headers);
      headers.set('authorization', `Bearer ${this.personSession!.token}`);
      try { return await this.request(path, {...init, headers}); }
      catch (error) {
        if (!(error instanceof HttpError) || error.status !== 401) throw error;
        this.personSession = undefined;
        if (attempt) throw error;
      }
    }
  }
  async person(): Promise<PersonView> { return (await this.personRequest('/api/person')).json(); }
  async putPersonIndex(body: string): Promise<void> {
    await this.personRequest('/api/person/index', {method:'PUT', headers:{'content-type':'application/json'}, body});
  }
  async removeInvite(id: string): Promise<void> {
    await this.personRequest(`/api/person/inbox/${encodeURIComponent(id)}`, {method:'DELETE'});
  }
  async register(login: string, invite: string, body: string): Promise<void> {
    await this.request(`/api/person?login=${encodeURIComponent(login)}`, {method:'PUT',headers:{'content-type':'application/json',authorization:`Bearer ${invite}`},body});
  }
  async putRequest(project: string, id: string, envelope: string): Promise<void> {
    await this.personRequest(this.route(project, `objects/${this.currentPrefix(project)}requests/${encodeURIComponent(id)}.json`), {
      method: 'PUT', headers: {'content-type': 'application/json'}, body: envelope,
    });
  }
  async requestObject(project: string, id: string, decrypt: (bytes: Uint8Array) => string): Promise<string> {
    return (await this.authorized(project, `objects/${this.currentPrefix(project)}requests/${encodeURIComponent(id)}.json`, decrypt)).text();
  }
  async projects(): Promise<string[]> { return (await (await this.personRequest('/api/projects')).json()).projects; }
  private route(project: string, path: string): string { return `/v1/p/${encodeURIComponent(project)}/${path}`; }
  async descriptor(project: string, prefix = ""): Promise<Descriptor> {
    return (await this.personRequest(this.route(project, prefix ? `objects/${prefix}store.json` : "store.json"))).json();
  }
  async slot(project: string, name: string, prefix = ""): Promise<Uint8Array> {
    return new Uint8Array(await (await this.personRequest(this.route(project, `${prefix}slots/${encodeURIComponent(name)}`))).arrayBuffer());
  }
  private async token(project: string, decrypt: (bytes: Uint8Array) => string): Promise<string> {
    const old = this.tokens.get(project);
    if (old && old.expires > Date.now() + 5000) return old.token;
    const { challenge } = await (await this.personRequest(this.route(project, 'challenge' + this.epochQuery(project)))).json();
    const response = await this.personRequest(this.route(project, 'token' + this.epochQuery(project)), {
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
  async discovery(project: string, anonymous = false): Promise<Record<string,string>> {
    const entries = anonymous ? await this.publicList(project, 'keys/') : (await (await this.personRequest(this.route(project,'objects?prefix=keys%2F'))).json()).objects as ObjectEntry[];
    if (entries.length > 128) throw new Error('Too many project epochs.');
    const records: Record<string,string> = {};
    for (const {key} of entries) {
      if (!/^keys\/[0-9a-f-]{36}-[0-9a-f-]{36}\.json$/.test(key)) throw new Error('Invalid epoch discovery object.');
      const response = anonymous ? await this.request(this.publicRoute(project,key)) : await this.personRequest(this.route(project,`objects/${key}`));
      records[key] = await response.text();
    }
    return records;
  }
  async signedObject(project: string, key: string, decrypt: (bytes: Uint8Array) => string): Promise<Uint8Array> {
    if (!/^overlays\/[^/]+\/keys\/[^/]+\.json$/.test(key)) throw new Error('Invalid overlay key object.');
    return new Uint8Array(await (await this.authorized(project, `objects/${key.split('/').map(encodeURIComponent).join('/')}`, decrypt)).arrayBuffer());
  }
  async releaseObject(project: string, key: string, maximum: number, decrypt?: (bytes: Uint8Array) => string): Promise<Uint8Array> {
    if (!/(?:^|\/)files\/[0-9a-f-]{36}\.age$/.test(key) || key.split('/').some(part => !part || part === '.' || part === '..')) throw new Error('Invalid release object.');
    const path = key.split('/').map(encodeURIComponent).join('/');
    const response = decrypt ? await this.authorized(project, `objects/${path}`, decrypt) : await this.request(this.publicRoute(project, path));
    const length = response.headers.get('content-length');
    if (length !== null && (!/^\d+$/.test(length) || Number(length) > maximum)) throw new Error('Release ciphertext exceeds its size limit.');
    const reader = response.body?.getReader();
    if (!reader) throw new Error('Release response has no body.');
    const chunks: Uint8Array[] = []; let total = 0;
    try {
      for (;;) {
        const {done,value} = await reader.read(); if (done) break;
        total += value.byteLength;
        if (total > maximum) throw new Error('Release ciphertext exceeds its size limit.');
        chunks.push(value);
      }
    } catch (error) { await reader.cancel().catch(() => {}); throw error; }
    finally { reader.releaseLock(); }
    const bytes = new Uint8Array(total); let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk,offset); offset += chunk.length; }
    return bytes;
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
  async publicDescriptor(project: string, prefix = ''): Promise<Descriptor> { return (await this.request(this.publicRoute(project, prefix + 'store.json'))).json(); }
  async publicManifest(project: string, prefix = ''): Promise<Uint8Array> { return new Uint8Array(await (await this.request(this.publicRoute(project, prefix + 'public.json'))).arrayBuffer()); }
  async publicList(project: string, prefix: string): Promise<ObjectEntry[]> {
    return (await (await this.request(this.publicRoute(project, `objects?prefix=${encodeURIComponent(prefix)}`))).json()).objects;
  }
  async publicObject(project: string, key: string): Promise<Uint8Array> {
    if (!key.endsWith('.age') || key.split('/').some(part => !part || part === '.' || part === '..')) throw new Error('Invalid ciphertext object.');
    return new Uint8Array(await (await this.request(this.publicRoute(project, key.split('/').map(encodeURIComponent).join('/')))).arrayBuffer());
  }
  lock(): void { this.generation++; this.personSession = undefined; this.signer = undefined; this.signingIn = undefined; this.tokens.clear(); this.epochs.clear(); this.ciphertext.clear(); this.bytes = 0; }
}
