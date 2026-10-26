import {boundedBody, cacheable, signedLink} from './cdn';
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
    if (this.epochs.get(project) !== epoch) { this.tokens.delete(project); this.readExchanges.delete(project); }
    this.epochs.set(project,epoch);
  }
  private epochQuery(project: string) { const epoch=this.epochs.get(project); return epoch && epoch !== "root" ? `?epoch=${epoch}` : ""; }
  private currentPrefix(project: string) { const epoch=this.epochs.get(project); return epoch && epoch !== "root" ? `project/epochs/${epoch}/` : ""; }
  private tokens = new Map<string, { token: string; expires: number }>();
  private readExchanges = new Map<string, Promise<string>>();
  private personSession?: {login: string; token: string; expires: number};
  private signer?: {login: string; sign: (challenge: string) => string};
  private signingIn?: Promise<void>;
  private generation = 0;
  private signingActive = 0;
  private signingWaiters: (() => void)[] = [];
  private batchLinks = new Map<string, unknown>();
  private async signingRequest<T>(request: () => Promise<T>): Promise<T> {
    if (this.signingActive >= 4) await new Promise<void>(resolve => this.signingWaiters.push(resolve));
    else this.signingActive++;
    try { return await request(); }
    finally { const next = this.signingWaiters.shift(); if (next) next(); else this.signingActive--; }
  }
  private ciphertext = new Map<string, Uint8Array>();
  private bytes = 0;
  private cdnSources = new WeakSet<Uint8Array>();
  readonly base: string;
  constructor(base = '') {
    const url = new URL(base || '/', location.origin);
    if (url.origin !== location.origin) throw new Error('The gateway must use the site origin.');
    this.base = url.pathname.replace(/\/$/, '');
  }
  private async request(path: string, init: RequestInit = {}): Promise<Response> {
    const generation = this.generation;
    for (let attempt = 0; ; attempt++) {
      const response = await fetch(this.base + path, { ...init, credentials: 'same-origin', cache: 'no-store' });
      if (response.ok) return response;
      const body = await response.json().catch(() => ({}));
      if (response.status === 503 && body.error === 'gateway is busy; retry later' && attempt < 3) {
        const delay = Number(response.headers.get('retry-after'));
        if (Number.isFinite(delay) && delay >= 0 && delay <= 60) {
          await new Promise(resolve => setTimeout(resolve, delay * 1000 + Math.random() * 250));
          if (generation !== this.generation) throw new Error('Your session is locked.');
          continue;
        }
      }
      throw new HttpError(response.status, body.error || `HTTP ${response.status}`);
    }
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
    await this.personRequest(`/api/person/inbox/${encodeURIComponent(id)}`, {method:'DELETE', headers:{'content-type':'application/json'}});
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
    const existing = this.readExchanges.get(project); if (existing) return existing;
    const generation=this.generation, epoch=this.epochs.get(project);
    const pending=(async()=>{
      const {challenge}=await (await this.personRequest(this.route(project,'challenge'+this.epochQuery(project)))).json();
      if (generation!==this.generation || epoch!==this.epochs.get(project)) throw new Error('Your session or project epoch changed.');
      const response=await this.personRequest(this.route(project,'token'+this.epochQuery(project)),{
        method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({challenge:decrypt(fromBase64(challenge))}),
      });
      const token=await response.json();
      if (generation!==this.generation || epoch!==this.epochs.get(project)) throw new Error('Your session or project epoch changed.');
      if (typeof token.token!=='string' || !token.token || !Number.isFinite(token.expires) || token.expires<=Date.now()) throw new Error('Invalid project read proof.');
      this.tokens.set(project,token); return token.token as string;
    })();
    this.readExchanges.set(project,pending);
    try { return await pending; } finally { if (this.readExchanges.get(project)===pending) this.readExchanges.delete(project); }
  }
  private async authorized(project: string, path: string, decrypt: (bytes: Uint8Array) => string, cdn = false, init: RequestInit = {}): Promise<Response> {
    for (let attempt = 0; ; attempt++) {
      const token = await this.token(project, decrypt);
      try {
        const headers = new Headers(init.headers); headers.set('authorization', `Bearer ${token}`);
        if (cdn) headers.set('x-layr-cdn', '1');
        const get = () => this.request(this.route(project, path), {...init, headers});
        return await (cdn ? this.signingRequest(get) : get());
      }
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
  private async ciphertextObject(project: string, key: string, maximum: number, decrypt?: (bytes: Uint8Array) => string, direct = false): Promise<Uint8Array> {
    const generation = this.generation, path = key.split('/').map(encodeURIComponent).join('/');
    const get = (cdn: boolean) => decrypt ? this.authorized(project, `objects/${path}`, decrypt, cdn) : this.request(this.publicRoute(project, path));
    const prepared = direct ? undefined : this.batchLinks.get(`${project}/${key}`);
    this.batchLinks.delete(`${project}/${key}`);
    let response = prepared ? new Response(JSON.stringify(prepared), {headers:{'content-type':'application/vnd.layr.cdn+json'}}) : await get(!direct && prepared !== false && cacheable(key));
    let fromCdn = false, expected: number | undefined;
    if (response.headers.get('content-type')?.split(';')[0] === 'application/vnd.layr.cdn+json') {
      if (direct || !decrypt || !cacheable(key)) throw new Error('Unexpected CDN reply.');
      try {
        const json = await boundedBody(response, 4096);
        const link = signedLink(JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(json)), location.origin, project, key, maximum);
        // The signature is the only authority on this request. Do not forward a session or proof.
        response = await fetch(link.url, {credentials:'omit',redirect:'error',referrerPolicy:'no-referrer'});
        if (!response.ok) throw new Error('CDN ciphertext is unavailable.');
        expected = link.bytes; fromCdn = true;
      } catch { response = await get(false); }
    }
    let bytes: Uint8Array;
    try { bytes = await boundedBody(response, maximum, expected); }
    catch (error) {
      if (!fromCdn) throw error;
      bytes = await boundedBody(await get(false), maximum); fromCdn = false;
    }
    if (generation !== this.generation) throw new Error('Your session is locked.');
    if (fromCdn) this.cdnSources.add(bytes);
    return bytes;
  }
  async releaseObject(project: string, key: string, maximum: number, decrypt?: (bytes: Uint8Array) => string, direct?: boolean): Promise<Uint8Array> {
    if (!cacheable(key) || !/(?:^|\/)files\/[0-9a-f-]{36}\.age$/.test(key)) throw new Error('Invalid release object.');
    return this.ciphertextObject(project,key,maximum,decrypt,direct !== false);
  }
  async object(project: string, key: string, decrypt: (bytes: Uint8Array) => string, direct?: boolean): Promise<Uint8Array> {
    if (!key.endsWith('.age') || key.split('/').some(part => !part || part === '.' || part === '..')) throw new Error('Invalid ciphertext object.');
    const id = `${project}/${key}`;
    const cached = direct !== true && this.ciphertext.get(id);
    if (cached) return cached;
    const result = await this.ciphertextObject(project,key,32 * 1024 * 1024,decrypt,direct !== false);
    // This cache contains only age ciphertext and never outlives its worker.
    if (this.bytes + result.byteLength > 64 * 1024 * 1024) { this.ciphertext.clear(); this.bytes = 0; }
    if (result.byteLength <= 64 * 1024 * 1024) {
      const previous = this.ciphertext.get(id); if (previous) this.bytes -= previous.byteLength;
      this.ciphertext.set(id, result); this.bytes += result.byteLength;
    }
    return result;
  }
  private async prepareBatch(project: string, keys: string[], decrypt: (bytes: Uint8Array) => string): Promise<void> {
    const generation = this.generation;
    const groups = new Map<string, {key:string; relative:string}[]>();
    for (const key of keys) {
      if (!cacheable(key) || this.ciphertext.has(`${project}/${key}`)) continue;
      const scoped = /^project\/epochs\/([0-9a-f-]{36})\/(.+)$/.exec(key);
      const epoch = scoped?.[1] ?? 'root', relative = scoped?.[2] ?? key;
      const group = groups.get(epoch) ?? []; group.push({key, relative}); groups.set(epoch, group);
    }
    for (const [epoch, group] of groups) {
      const prepare = async(entries: {key:string; relative:string}[]): Promise<void> => {
      try {
      const response = await this.signingRequest(() => this.authorized(project, 'cdn', decrypt, false, {
        method:'POST', headers:{'content-type':'application/json'}, body:JSON.stringify({epoch, keys:entries.map(entry=>entry.relative)}),
      }));
      const raw = await boundedBody(response, 2 * 1024 * 1024);
      const value = JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(raw));
      if (!Array.isArray(value.objects) || value.objects.length !== entries.length) throw new Error('Invalid CDN batch reply.');
      if (generation !== this.generation) throw new Error('Your session is locked.');
      for (let i=0;i<entries.length;i++) {
        const item = value.objects[i], entry = entries[i];
        if (item.status !== 200) throw new HttpError(item.status, item.error?.error || 'CDN signing refused.');
        if (item.direct === true) { console.debug('A cacheable-key batch returned a direct object.'); this.batchLinks.set(`${project}/${entry.key}`, false); continue; }
        const {status: _status, ...link} = item;
        try { signedLink(link, location.origin, project, entry.key, 256 * 1024 * 1024); }
        catch { this.batchLinks.set(`${project}/${entry.key}`, false); continue; }
        this.batchLinks.set(`${project}/${entry.key}`, link);
      }
      } catch (error) {
        if (!(error instanceof HttpError) || error.status !== 408 || entries.length <= 1) throw error;
        const half = Math.ceil(entries.length / 2);
        await prepare(entries.slice(0,half)); await prepare(entries.slice(half));
      }
      };
      await prepare(group);
    }
  }

  /** The callback must verify all supplied objects before it publishes a result. */
  async verifiedObjects<T>(project: string, keys: string[], decrypt: (bytes: Uint8Array) => string,
    verify: (bytes: Uint8Array[], retry: boolean) => T | Promise<T>, maximum?: number, readObject?: (key: string, direct: boolean) => Promise<Uint8Array>): Promise<T> {
    const generation = this.generation;
    const read = (key: string, direct = false) => readObject ? readObject(key,direct) : maximum === undefined ? this.object(project,key,decrypt,direct) : this.releaseObject(project,key,maximum,decrypt,direct);
    const bodies: Uint8Array[] = [];
    // Fetch in bounded batches. WASM verification stays in the caller's serial order.
    for (let start=0;start<keys.length;start+=256) {
      const batch = keys.slice(start,start+256);
      if (!readObject) await this.prepareBatch(project,batch,decrypt);
      for (let i=0;i<batch.length;i+=8) bodies.push(...await Promise.all(batch.slice(i,i+8).map(key => read(key))));
    }
    try { return await verify(bodies,false); }
    catch (error) {
      const retry = bodies.map(body=>this.cdnSources.has(body));
      if (!retry.some(Boolean) || generation !== this.generation) throw error;
      for (let i=0;i<keys.length;i+=8) await Promise.all(keys.slice(i,i+8).map(async(key,index)=>{
        const at=i+index;
        if (retry[at]) { bodies[at]=await read(key,true); }
      }));
      if (generation !== this.generation) throw new Error('Your session is locked.');
      return verify(bodies,true);
    }
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
  lock(): void { this.generation++; this.personSession = undefined; this.signer = undefined; this.signingIn = undefined; this.tokens.clear(); this.readExchanges.clear(); this.epochs.clear(); this.ciphertext.clear(); this.batchLinks.clear(); this.cdnSources = new WeakSet(); this.bytes = 0; }
}
