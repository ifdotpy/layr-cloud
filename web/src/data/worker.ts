/// <reference lib="webworker" />
import init, { Session } from '../../wasm/layr_web';
import { Gateway } from './gateway';

const scope = self as unknown as DedicatedWorkerGlobalScope;
const idleMs = 30 * 60 * 1000;
let session: Session | undefined;
let gateway: Gateway | undefined;
let timer: ReturnType<typeof setTimeout>;
const routes = new Map<string, {gateway: string; prefix: string}>();
const aliases = new Map<string, string>();
const attempts = new Map<string, string>();
const loaded = new Set<string>();
let busy = Promise.resolve();
function lock() {
  clearTimeout(timer);
  session?.lock(); session?.free(); session = undefined;
  gateway?.lock(); gateway = undefined;
  routes.clear(); aliases.clear(); attempts.clear(); loaded.clear();
}
function touch() { clearTimeout(timer); timer = setTimeout(() => { lock(); scope.postMessage({ locked: true }); scope.close(); }, idleMs); }
function current() { if (!session || !gateway) throw new Error('Your session is locked.'); return { session, gateway }; }
function route(name: string) { const route = routes.get(name); if (!route) throw new Error('The store route is absent.'); return route; }
function proof(name: string) { return (bytes: Uint8Array) => current().session.challenge(route(name).gateway, bytes); }
async function objectBytes(name: string, key: string) { const target = route(name); return current().gateway.object(target.gateway, target.prefix + key, proof(name)); }
async function listObjects(name: string, prefix: string) {
  const target = route(name);
  return (await current().gateway.list(target.gateway, target.prefix + prefix, proof(name))).map(entry => ({...entry, key: entry.key.slice(target.prefix.length)}));
}
async function loadStore(name: string) {
  const { session } = current();
  for (const entry of await listObjects(name, 'records/')) session.add_segment(name, entry.key, await objectBytes(name, entry.key));
  session.finish_records(name);
  for (const entry of await listObjects(name, 'states/')) {
    const match = /^states\/([^/]+)\/([a-f0-9]{8})\.tree\.age$/.exec(entry.key);
    if (match && !attempts.has(`${name}/${match[1]}`)) attempts.set(`${name}/${match[1]}`, entry.key);
  }
}
function alias(project: string) { const name = aliases.get(project); if (!name) throw new Error('The project is unavailable.'); return name; }
async function tree(name: string, state: string, seen = new Set<string>()): Promise<string> {
  const object = attempts.get(`${name}/${state}`);
  if (!object) throw new Error('The state has no stored tree.');
  await loadTree(name, object, seen);
  const layers = JSON.parse(current().session.query(name, 'layers', JSON.stringify([state]), Date.now()));
  for (const layer of layers) await tree(layer.name, layer.state);
  return object;
}
async function loadTree(name: string, object: string, seen: Set<string>): Promise<void> {
  const id = `${name}/${object}`;
  if (loaded.has(id)) return;
  if (seen.has(id) || seen.size >= 32) throw new Error('The tree chain has a loop or exceeds 32 states.');
  seen.add(id);
  const { session } = current();
  const bytes = await objectBytes(name, object);
  const base: string | null = JSON.parse(session.tree_base(name, object, bytes));
  if (base) await loadTree(name, base, seen);
  session.install_tree(name, object, bytes); loaded.add(id);
}
async function parts(name: string, object: string, path: string): Promise<void> {
  const { session } = current();
  const source = JSON.parse(session.file_location(name, object, path));
  const keys: string[] = JSON.parse(session.file_parts(source.name, source.object, path));
  for (const key of keys) session.add_part(source.name, key, await objectBytes(source.name, key));
}
async function unlock(method: string, args: unknown[], base: string) {
  lock();
  await init();
  gateway = new Gateway(base); session = new Session();
  try {
    const person = await gateway.person();
    if (!person.recipient) throw new Error('Your person key is absent from this gateway.');
    if (method === 'unlockRecovery') session.unlock_recovery(args[0] as string, JSON.stringify(person));
    else {
      const secret = args[0] as Uint8Array;
      try { session.unlock_prf(secret, args[1] as string, JSON.stringify(person)); }
      finally { secret.fill(0); }
    }
    const slotName = session.slot_name();
    const pins: string[] = JSON.parse(session.query('', 'pins', '[]', Date.now()));
    for (const name of await gateway.projects()) {
      const descriptor = await gateway.descriptor(name);
      if (!pins.includes(descriptor.project)) continue;
      const project = session.open_store(name, JSON.stringify(descriptor), await gateway.slot(name, slotName));
      aliases.set(project, name);
      routes.set(name, {gateway: name, prefix: ''});
      await loadStore(name);
      for (const entry of await gateway.list(name, 'overlays/', proof(name))) {
        if (/^overlays\/[^/]+\/keys\/[^/]+\.json$/.test(entry.key)) session.overlay_key(name, entry.key, await gateway.signedObject(name, entry.key, proof(name)));
      }
      for (const layer of JSON.parse(session.overlay_slots(name))) {
        const slot = await gateway.object(name, `${layer.prefix}/slots/${layer.slot}`, proof(name));
        const alias = session.open_overlay(name, layer.overlay, slot);
        routes.set(alias, {gateway: name, prefix: layer.prefix + '/'});
        await loadStore(alias);
      }
    }
    touch();
  } catch (error) { lock(); throw error; }
}
async function call(method: string, args: unknown[], base = ''): Promise<unknown> {
  if (method === 'person') return new Gateway(base).person();
  if (method === 'unlockRecovery' || method === 'unlockPrf') return unlock(method, args, base);
  if (method === 'lock') { lock(); return; }
  touch();
  const { session } = current();
  if (['tree', 'file', 'diff'].includes(method)) {
    const [project, state, path = ''] = args as string[];
    const name = alias(project); const object = await tree(name, state);
    if (method === 'tree') return JSON.parse(session.tree(name, object, path));
    if (method === 'file') { await parts(name, object, path); return JSON.parse(session.file(name, object, path)); }
    const other = await tree(name, path); const filter = args[3] as string || '';
    // A summary compares verified tree metadata. Only one selected file loads bytes.
    if (filter) {
      for (const selected of [object, other]) {
        const directory = filter.split('/').slice(0, -1).join('/');
        let entries: {path: string; kind: string}[] = [];
        try { entries = JSON.parse(session.tree(name, selected, directory)).entries; } catch { /* The file can be absent on one side. */ }
        if (entries.some(entry => entry.path === filter && entry.kind !== 'directory')) await parts(name, selected, filter);
      }
    }
    return JSON.parse(session.diff(name, object, other, filter));
  }
  if (method === 'changeDiff') {
    const name = alias(args[0] as string);
    const states: string[] = JSON.parse(session.query(name, 'changeInputs', JSON.stringify(args.slice(1)), Date.now()));
    const path = args[4] as string || '';
    for (const state of states) {
      const object = await tree(name, state);
      if (path) {
        const directory = path.split('/').slice(0, -1).join('/');
        let entries: {path: string;kind: string;size?: number}[] = [];
        try { entries = JSON.parse(session.tree(name, object, directory)).entries; } catch { /* A path can be absent in a version. */ }
        if (entries.some(entry => entry.path === path && entry.kind !== 'directory' && (entry.size || 0) <= 64 * 1024 * 1024)) await parts(name, object, path);
      }
    }
  }
  if (method === 'mergeStatus') {
    const name = alias(args[0] as string);
    const input = JSON.parse(session.query(name, 'mergeInputs', JSON.stringify(args.slice(1)), Date.now()));
    await tree(name, input.state);
    if (input.base) await tree(name, input.base);
  }
  if (['probeDiff', 'changes', 'changeShow'].includes(method)) {
    const name = alias(args[0] as string);
    const inputs: {sources: string[]; outputs: string[]} = JSON.parse(session.query(name, 'probeInputs', JSON.stringify([method, ...args.slice(1)]), Date.now()));
    for (const state of inputs.outputs) {
      const object = await tree(name, state);
      async function loadOutputs(directory: string): Promise<void> {
        for (const entry of JSON.parse(session.tree(name, object, directory)).entries) {
          if (entry.kind === 'directory') await loadOutputs(entry.path);
          else await parts(name, object, entry.path);
        }
      }
      await loadOutputs('');
    }
    for (const state of inputs.sources) {
      if (!attempts.has(`${name}/${state}`)) continue;
      const object = await tree(name, state);
      const entries = JSON.parse(session.tree(name, object, '')).entries;
      if (entries.some((entry: {path: string}) => entry.path === 'layr.toml')) await parts(name, object, 'layr.toml');
    }
  }
  const global = ['projects', 'people', 'machines', 'stores', 'you'].includes(method);
  const name = global ? '' : alias(args[0] as string);
  return JSON.parse(session.query(name, method, JSON.stringify(global ? args : args.slice(1)), Date.now()));
}
scope.onmessage = event => {
  const { id, method, args = [], base = '' } = event.data;
  if (method === 'touch') {
    if (session && gateway) { touch(); scope.postMessage({id}); }
    else scope.postMessage({id, error: 'Your session is locked.'});
    return;
  }
  // Serialize calls so a lock or unlock cannot race an in-flight fetch.
  busy = busy.then(async () => {
    try { const value = await call(method, args, base); scope.postMessage({ id, value }); }
    catch (error) { scope.postMessage({ id, error: error instanceof Error ? error.message : String(error) }); }
  });
};
