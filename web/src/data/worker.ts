/// <reference lib="webworker" />
import init, { Session } from '../../wasm/layr_web';
import { uuidV7 } from './requests';
import { epochRoute } from './epochs';
import { Gateway, HttpError, fromBase64 } from './gateway';

const scope = self as unknown as DedicatedWorkerGlobalScope;
const idleMs = 30 * 60 * 1000;
let session: Session | undefined;
let gateway: Gateway | undefined;
let timer: ReturnType<typeof setTimeout>;
const routes = new Map<string, {gateway: string; prefix: string}>();
const historyPrefixes = new Map<string,string[]>();
const objectSources = new Map<string,string>();
const aliases = new Map<string, string>();
const attempts = new Map<string, string>();
const loaded = new Set<string>();
const requestOutcomes = new Map<string,string>();
const requestEnvelopes = new Map<string, {project: string; envelope: string; uncertain: boolean}>();
let busy = Promise.resolve();
let generation = 0;
let registration: {login:string; body:string} | undefined;
const refreshing = new Map<string, Promise<void>>();
const publicProjects = new Set<string>();
function lock() {
  generation++; refreshing.clear(); registration = undefined;
  clearTimeout(timer);
  session?.lock(); session?.free(); session = undefined;
  gateway?.lock(); gateway = undefined;
  routes.clear(); historyPrefixes.clear(); objectSources.clear(); aliases.clear(); attempts.clear(); loaded.clear(); requestEnvelopes.clear(); requestOutcomes.clear(); publicProjects.clear();
}
function touch() { clearTimeout(timer); timer = setTimeout(() => { lock(); scope.postMessage({ locked: true }); scope.close(); }, idleMs); }
function current() { if (!session || !gateway) throw new Error('Your session is locked.'); return { session, gateway }; }
function route(name: string) { const route = routes.get(name); if (!route) throw new Error('The store route is absent.'); return route; }
function proof(name: string) { return (bytes: Uint8Array) => current().session.challenge(route(name).gateway, bytes); }
function sourceObject(name: string, key: string) {
  return objectSources.get(`${name}/${key}`) ?? route(name).prefix + key;
}
async function objectBytes(name: string, key: string, maximum?: number, direct?: boolean) {
  const target = route(name), {gateway} = current();
  const known = objectSources.get(`${name}/${key}`);
  const candidates=known ? [known] : [target.prefix + key, ...(historyPrefixes.get(name) ?? []).map(prefix=>prefix+key)];
  for (let i=0;i<candidates.length;i++) {
    try {
      const bytes = maximum !== undefined
        ? await gateway.releaseObject(target.gateway, candidates[i], maximum, publicProjects.has(target.gateway) ? undefined : proof(name),direct)
        : publicProjects.has(target.gateway) ? await gateway.publicObject(target.gateway,candidates[i]) : await gateway.object(target.gateway,candidates[i],proof(name),direct);
      objectSources.set(`${name}/${key}`,candidates[i]); return bytes;
    } catch(error) { if (!(error instanceof HttpError) || error.status!==404 || i===candidates.length-1) throw error; }
  }
  throw new Error('The object is unavailable.');
}
async function verifiedObjects<T>(name: string, keys: string[], verify: (bytes: Uint8Array[], retry: boolean) => T | Promise<T>, maximum?: number) {
  const {gateway,session}=current(), target=route(name);
  return gateway.verifiedObjects(target.gateway,keys,proof(name),async (bytes,retry)=>{
    if (current().session !== session) throw new Error('Your session changed.');
    return verify(bytes,retry);
  },maximum,(key,direct)=>objectBytes(name,key,maximum,direct));
}
async function loadEpochHistory(name:string, epochs:{prefix:string; history:string[]}, anonymous=false) {
  if (!epochs.prefix) return;
  const {session,gateway}=current(), target=route(name);
  historyPrefixes.set(name,epochs.history.slice().reverse().map(epoch=>epoch==='root' ? '' : `project/epochs/${epoch}/`));
  for (const epoch of epochs.history) {
    const key=`${epochs.prefix}history/${epoch}.age`;
    if (anonymous) session.add_epoch_history(name,epoch,await gateway.publicObject(target.gateway,key));
    else await gateway.verifiedObjects(target.gateway,[key],proof(name),([bytes])=>session.add_epoch_history(name,epoch,bytes),undefined,
      (key,direct)=>gateway.object(target.gateway,key,proof(name),direct));
  }
  const records=anonymous ? await gateway.publicList(target.gateway,'records/') : await gateway.list(target.gateway,'records/',proof(name));
  for (const entry of records) {
    const bytes=anonymous ? await gateway.publicObject(target.gateway,entry.key) : await gateway.object(target.gateway,entry.key,proof(name));
    session.add_root_segment(name,entry.key,bytes);
  }
}
async function listObjects(name: string, prefix: string) {
  const target = route(name);
  const {gateway} = current();
  const entries = publicProjects.has(target.gateway) ? await gateway.publicList(target.gateway, target.prefix + prefix) : await gateway.list(target.gateway, target.prefix + prefix, proof(name));
  return entries.map(entry => ({...entry, key: entry.key.slice(target.prefix.length)}));
}
async function loadStore(name: string) {
  const {session}=current();
  const keys=(await listObjects(name,'records/')).sort((a,b)=>b.key.localeCompare(a.key)).map(entry=>entry.key);
  await verifiedObjects(name,keys,bytes=>{
    session.clear_record_segments(name);
    keys.forEach((key,i)=>session.add_segment(name,sourceObject(name,key),bytes[i]));
    session.finish_records(name);
  });
}
async function loadStoreFast(name: string): Promise<boolean> {
  const {session} = current();
  if (historyPrefixes.has(name)) { await loadStore(name); return false; }
  let found;
  try { found = await listObjects(name, 'index/heads/'); }
  catch { await loadStore(name); return false; }
  if (!found.length) { await loadStore(name); return false; }
  let completeRecords = false;
  try {
    // Bootstrap complete prefixes from each writer, never an index-provided key.
    const records = await listObjects(name, 'records/');
    const initial=records.filter(entry => /\/0*1-[0-9]+\.age$/.test(entry.key)).map(entry=>entry.key);
    await verifiedObjects(name,initial,bytes=>{
      session.clear_record_segments(name);
      initial.forEach((key,i)=>session.add_segment(name,sourceObject(name,key),bytes[i]));
      session.finish_records(name);
    });
    const latest = new Map<string, string>();
    for (const entry of found) {
      const match = /^index\/heads\/([^/]+)\/(\d{20})\.age$/.exec(entry.key);
      if (match && entry.key > (latest.get(match[1]) || '')) latest.set(match[1], entry.key);
    }
    for (const object of latest.values()) {
      const bytes = await objectBytes(name, object);
      try { session.add_heads(name, sourceObject(name,object), bytes); }
      catch (error) {
        if (completeRecords) throw error;
        // A later signed machine.trust record can authorize a new index writer.
        // Authenticate complete history before deciding that its cache is invalid.
        await loadStore(name); completeRecords = true;
        session.add_heads(name, sourceObject(name,object), bytes);
      }
    }
    if (session.use_heads(name)) {
      if (!completeRecords) return true;
      // This path already fetched the full history; reconcile before returning it.
      // TODO: reuse authenticated source watermarks here. finish_records currently
      // drops exact signed segment bytes, which reconciliation still needs.
      await loadStore(name); return false;
    }
  } catch { session.discard_index(name); }
  if (!completeRecords) await loadStore(name);
  return false;
}
function reconcile(name: string, overlays: () => Promise<void>) {
  const expected = generation;
  const promise = (async () => {
    // Yield so the unlock response and first-page requests can render the snapshot.
    await new Promise(resolve => setTimeout(resolve, 0));
    if (generation !== expected) return;
    await loadStore(name);
    if (generation !== expected) return;
    await overlays();
    if (generation === expected) scope.postMessage({updated: true});
  })();
  refreshing.set(name, promise);
  void promise.catch(error => {
    if (generation !== expected) return;
    // A failed authoritative load must remove the provisional view, never bless it.
    current().session.discard_index(name);
    lock(); scope.postMessage({locked:true, error: error instanceof Error ? error.message : String(error)});
  }).finally(() => { if (refreshing.get(name) === promise) refreshing.delete(name); });
}
async function complete(name: string) { await refreshing.get(name); }
async function locateTree(name: string, state: string): Promise<string | undefined> {
  const id = `${name}/${state}`;
  if (!attempts.has(id)) {
    const target=route(name), {gateway}=current();
    for (const prefix of [target.prefix,...(historyPrefixes.get(name) ?? [])]) {
      const entries=publicProjects.has(target.gateway) ? await gateway.publicList(target.gateway,`${prefix}states/${state}/`) : await gateway.list(target.gateway,`${prefix}states/${state}/`,proof(name));
      const entry=entries.find(entry=>/\/[a-f0-9]{8}\.tree\.age$/.test(entry.key));
      if (entry) {
        const logical=entry.key.slice(prefix.length);
        objectSources.set(`${name}/${logical}`,entry.key); attempts.set(id,logical); break;
      }
    }
  }
  return attempts.get(id);
}
function alias(project: string) { const name = aliases.get(project); if (!name) throw new Error('The project is unavailable.'); return name; }
async function tree(name: string, state: string, seen = new Set<string>()): Promise<string> {
  const object = await locateTree(name, state);
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
  await verifiedObjects(name,[object],async ([bytes])=>{
    const base: string | null = JSON.parse(session.tree_base(name,sourceObject(name,object),bytes));
    if (base) await loadTree(name,base,new Set(seen));
    session.install_tree(name,sourceObject(name,object),bytes); loaded.add(id);
  });
}
async function parts(name: string, object: string, path: string): Promise<void> {
  const { session } = current();
  const source = JSON.parse(session.file_location(name, object, path));
  const keys: string[] = JSON.parse(session.file_parts(source.name, source.object, path));
  await verifiedObjects(source.name,keys,bytes=>{
    keys.forEach((key,i)=>session.add_part(source.name,sourceObject(source.name,key),bytes[i]));
    session.verify_file(name,object,path);
  });
}
async function unlock(method: string, args: unknown[], base: string) {
  lock();
  await init();
  gateway = new Gateway(base); session = new Session();
  try {
    const {session, gateway} = current();
    const login = args[method === 'unlockRecovery' ? 1 : 2] as string;
    const person = await gateway.unlock(login);
    if (!person.recipient) throw new Error('Your person key is absent from this gateway.');
    if (method === 'unlockRecovery') session.unlock_recovery(args[0] as string, JSON.stringify(person));
    else {
      const secret = args[0] as Uint8Array;
      try { session.unlock_prf(secret, args[1] as string, JSON.stringify(person)); }
      finally { secret.fill(0); }
    }
    if (person.signing && person.signing_key) {
      const sealed = fromBase64(person.signing);
      try { session.open_signing_key(sealed, person.signing_key); } finally { sealed.fill(0); }
    }
    await gateway.signIn(person.login, challenge => session.sign_session(challenge));
    session.refresh_person(JSON.stringify(await gateway.person()));
    await loadProjects();
    touch();
  } catch (error) { lock(); throw error; }
}
async function loadProjects(onlyProject?: string) {
    const {session, gateway} = current();
    const slotName = session.slot_name();
    const pins: string[] = JSON.parse(session.query('', 'pins', '[]', Date.now()));
    for (const name of await gateway.projects()) {
      const descriptor = await gateway.descriptor(name);
      if (onlyProject && descriptor.project !== onlyProject) continue;
      if (!pins.includes(descriptor.project)) continue;
      const discovery=await gateway.discovery(name), epochs=epochRoute(descriptor,discovery);
      gateway.setEpoch(name,epochs.epoch);
      const project=epochs.prefix
        ? session.open_epoch_store(name,JSON.stringify(descriptor),JSON.stringify(discovery),JSON.stringify(await gateway.descriptor(name,epochs.prefix)),await gateway.slot(name,slotName,epochs.prefix))
        : session.open_store(name,JSON.stringify(descriptor),await gateway.slot(name,slotName));
      aliases.set(project,name); routes.set(name,{gateway:name,prefix:epochs.prefix});
      await loadEpochHistory(name,epochs);
      const loadOverlays = async () => {
      const overlayObjects = await gateway.list(name, 'overlays/', proof(name));
      const overlayKeys = new Set(overlayObjects.map(entry => entry.key));
      for (const entry of overlayObjects) {
        if (/^overlays\/[^/]+\/keys\/[^/]+\.json$/.test(entry.key)) session.overlay_key(name, entry.key, await gateway.signedObject(name, entry.key, proof(name)));
        const hidden = /^overlays\/([0-9a-f-]{36})\/keys\/([0-9a-f-]{36})-[0-9a-f-]{36}\.age$/.exec(entry.key);
        if (!hidden) continue;
        const prefix = `overlays/${hidden[1]}/epochs/${hidden[2]}`;
        const slotObject = `${prefix}/slots/${slotName}`;
        if (!overlayKeys.has(slotObject)) continue;
        const slot = await gateway.object(name, slotObject, proof(name));
        // A foreign slot is silent, including when its manifest is malformed or absent.
        if (!session.overlay_slot_opens(slot)) continue;
        const alias = session.open_hidden_overlay(name, entry.key, slot, await gateway.object(name, entry.key, proof(name)));
        if (!alias) continue;
        routes.set(alias, {gateway: name, prefix: prefix + '/'});
        await loadStore(alias);
      }
      for (const layer of JSON.parse(session.overlay_slots(name))) {
        const slot = await gateway.object(name, `${layer.prefix}/slots/${layer.slot}`, proof(name));
        const alias = session.open_overlay(name, layer.overlay, slot);
        routes.set(alias, {gateway: name, prefix: layer.prefix + '/'});
        await loadStore(alias);
      }
      };
      if (await loadStoreFast(name)) reconcile(name, loadOverlays);
      else await loadOverlays();
    }
}
async function acceptInvite(id: string) {
  const {session,gateway} = current();
  const view = JSON.stringify(await gateway.person());
  const project = session.invite_project(view,id);
  let body: string | undefined;
  for (const name of await gateway.projects()) {
    const descriptor = await gateway.descriptor(name);
    if (descriptor.project !== project) continue;
    body = session.prepare_invite_accept(view,id,JSON.stringify(descriptor),await gateway.slot(name,session.slot_name()));
    break;
  }
  if (!body) throw new Error('This invite no longer has a project on this gateway. Ask for a new invite.');
  let writeError;
  try { await gateway.putPersonIndex(body); } catch(error) { writeError = error; }
  // An exact ciphertext readback resolves a lost success reply without replaying a write.
  const saved = await gateway.person();
  if (saved.index !== JSON.parse(body).data) throw writeError ?? new Error('Your project index changed. Try accepting the invite again.');
  session.refresh_person(JSON.stringify(saved));
  const existing = aliases.get(project);
  if (existing) await complete(existing);
  else try { await loadProjects(project); }
  catch(error) {
    const name = aliases.get(project);
    if (name) {
      session.close_store(name); aliases.delete(project);
      for (const [alias,target] of routes) if (target.gateway === name) routes.delete(alias);
    }
    throw error;
  }
  let deleteError;
  try { await gateway.removeInvite(id); } catch(error) { deleteError = error; }
  const final = await gateway.person();
  session.refresh_person(JSON.stringify(final));
  if (final.inbox.some(invite => invite.id === id)) throw deleteError ?? new Error('The invite could not be removed. Try accepting it again.');
  scope.postMessage({updated:true});
  return project;
}
async function prepareRegistration(login: string, base: string) {
  lock(); await init(); gateway = new Gateway(base); session = new Session();
  const entropy = crypto.getRandomValues(new Uint8Array(96));
  try {
    const {words, ...body} = JSON.parse(session.prepare_person(login, entropy));
    registration = {login, body:JSON.stringify(body)};
    touch(); return {words};
  } catch (error) { lock(); throw error; }
  finally { entropy.fill(0); }
}
async function registerPerson(invite: string) {
  const {session, gateway} = current();
  if (!registration) throw new Error('Create your person key first.');
  const {login, body} = registration;
  let deliveryError;
  try { await gateway.register(login, invite, body); } catch(error) { deliveryError = error; }
  // If registration succeeded but its reply was lost, proof of this exact key recovers it.
  try { await gateway.signIn(login, challenge => session.sign_session(challenge)); }
  catch(error) { throw deliveryError ?? error; }
  session.refresh_person(JSON.stringify(await gateway.person()));
  await loadProjects(); registration = undefined; touch();
}
async function openPublic(project: string, base: string) {
  if (!session || !gateway) { await init(); gateway = new Gateway(base); session = new Session(); }
  const descriptor = await gateway.publicDescriptor(project);
  const existing = aliases.get(descriptor.project);
  if (existing && publicProjects.has(route(existing).gateway)) return descriptor.project;
  if (existing) {
    await complete(existing);
    session.mark_public(existing, await gateway.publicManifest(project,route(existing).prefix));
    return descriptor.project;
  }
  const discovery=await gateway.discovery(project,true), epochs=epochRoute(descriptor,discovery);
  gateway.setEpoch(project,epochs.epoch);
  const manifest=await gateway.publicManifest(project,epochs.prefix), slot=await gateway.publicObject(project,epochs.prefix+'slots/public.age');
  const id=epochs.prefix
    ? session.open_public_epoch_store(project,JSON.stringify(descriptor),JSON.stringify(discovery),JSON.stringify(await gateway.publicDescriptor(project,epochs.prefix)),manifest,slot)
    : session.open_public_store(project,JSON.stringify(descriptor),manifest,slot);
  publicProjects.add(project); aliases.set(id,project); routes.set(project,{gateway:project,prefix:epochs.prefix});
  try { await loadEpochHistory(project,epochs,true); await loadStore(project); touch(); return id; }
  catch(error) { publicProjects.delete(project); aliases.delete(id); routes.delete(project); lock(); throw error; }
}
async function fileHistory(method: string, project: string, state: string, path: string) {
  const name = alias(project);
  await complete(name);
  const {session} = current();
  const states: string[] = JSON.parse(session.query(name, 'historyInputs', JSON.stringify([state]), Date.now()));
  let cacheFailed = false;
  for (const state of states) {
    await tree(name, state);
    if (!publicProjects.has(route(name).gateway) && !cacheFailed) {
      for (const kind of ['paths','graph']) {
        const key = `index/${kind}/${state}.age`;
        // Missing optional objects are a normal cache miss; malformed ones discard it.
        const objects = await listObjects(name, key);
        if (!objects.some(object => object.key === key)) continue;
        try { await verifiedObjects(name,[key],([bytes])=>session.add_history_index(name,sourceObject(name,key),bytes)); }
        catch { session.discard_index(name); cacheFailed = true; break; }
      }
    }
  }
  const versions = session.history_versions(name, state, path);
  if (method === 'fileHistory') return JSON.parse(versions);
  if (method === 'blame') for (const selected of JSON.parse(session.blame_inputs(name, state, path))) {
    const object = await tree(name, selected);
    await parts(name, object, path);
  }
  return JSON.parse(session.query(name, method, JSON.stringify([state,path]), Date.now()));
}
async function call(method: string, args: unknown[], base = ''): Promise<unknown> {
  if (method === 'openPublic') return openPublic(args[0] as string, base);
  if (method === 'person') return new Gateway(base).unlock(args[0] as string);
  if (method === 'prepareRegistration') return prepareRegistration(args[0] as string, base);
  if (method === 'registerPerson') return registerPerson(args[0] as string);
  if (method === 'unlockRecovery' || method === 'unlockPrf') return unlock(method, args, base);
  if (method === 'lock') { lock(); return; }
  touch();
  const { session } = current();
  if (method === 'acceptInvite') return acceptInvite(args[0] as string);
  if (method === 'blame' || method === 'fileHistory') return fileHistory(method, ...(args as [string,string,string]));
  if (args[0] && aliases.has(args[0] as string) && !['tree','file','lines','refs','openChanges','copies','loadStatus'].includes(method)) await complete(alias(args[0] as string));
  if (method === 'releaseDownload') {
    const [project, reference, filename] = args as string[];
    const name = alias(project);
    const files: {name: string; size: number; object: string}[] = JSON.parse(session.query(name, 'releaseFiles', JSON.stringify([reference]), Date.now()));
    const file = files.find(file => file.name === filename);
    if (!file) throw new Error('Release file is absent.');
    if (file.size > 256 * 1024 * 1024) throw new Error('File exceeds the 256 MiB browser limit; use layr release get.');
    return verifiedObjects(name,[file.object],([bytes])=>{
      const plain=session.release_file(name,reference,filename,sourceObject(name,file.object),bytes,Date.now());
      try { return new Blob([plain as Uint8Array<ArrayBuffer>],{type:'application/octet-stream'}); }
      finally { plain.fill(0); }
    },file.size+(Math.floor(file.size/65536)+1)*16+4096);
  }
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
  if (method === 'submitRequest' || method === 'retryRequest') {
    const project = args[0] as string;
    const name = alias(project);
    const id = method === 'submitRequest' ? uuidV7() : args[1] as string;
    if (method === 'submitRequest') requestEnvelopes.set(id, {project, envelope:session.sign_request(name,id,JSON.stringify(args[1]),Date.now()), uncertain:false});
    const saved = requestEnvelopes.get(id);
    if (!saved || saved.project !== project) throw new Error('This request is unavailable in your session.');
    const target = route(name);
    try { await current().gateway.putRequest(target.gateway,id,saved.envelope); saved.uncertain = false; }
    catch (error) {
      if (error instanceof HttpError && error.status === 401) throw error;
      try { saved.uncertain = await current().gateway.requestObject(target.gateway,id,proof(name)) !== saved.envelope; }
      catch { saved.uncertain = true; }
    }
    return {id,status:'pending',...(saved.uncertain ? {reason:'Delivery could not be confirmed. Retry delivery with the same request ID.'} : {})};
  }
  if (method === 'requestStatus') {
    const name = alias(args[0] as string), id = args[1] as string;
    await complete(name);
    await loadStore(name);
    const result = JSON.parse(session.query(name,method,JSON.stringify([id]),Date.now()));
    const saved = requestEnvelopes.get(id);
    if (result.status === 'pending' && saved?.uncertain) result.reason = 'Delivery could not be confirmed. Retry delivery with the same request ID.';
    return result;
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
  if (['probeDiff', 'screens', 'changes', 'changeShow'].includes(method)) {
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
      if (!await locateTree(name, state)) continue;
      const object = await tree(name, state);
      const entries = JSON.parse(session.tree(name, object, '')).entries;
      if (entries.some((entry: {path: string}) => entry.path === 'layr.toml')) await parts(name, object, 'layr.toml');
    }
  }
  if (method === 'projects') {
    // Readable overlay listings use verified tree metadata, without file data parts.
    for (const name of new Set(aliases.values())) {
      const inputs: {name: string; state: string}[] = JSON.parse(session.overlay_inputs(name));
      for (const input of inputs) if (await locateTree(input.name, input.state)) await tree(input.name, input.state);
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
    try {
      const value = await call(method, args, base);
      scope.postMessage({ id, value });
      if (method === 'requestStatus' && value && typeof value === 'object' && 'status' in value && value.status !== 'pending') {
        const key = `${args[0]}/${args[1]}`;
        if (requestOutcomes.get(key) !== value.status) { requestOutcomes.set(key,String(value.status)); scope.postMessage({updated:true}); }
      }
    }
    catch (error) { scope.postMessage({ id, error: error instanceof Error ? error.message : String(error) }); if (method !== 'registerPerson' && error instanceof HttpError && error.status === 401) { lock(); scope.postMessage({locked:true}); } }
  });
};
