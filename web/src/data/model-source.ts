import type {
  RequestAction, RequestStatus, ActionAccess, Comment, DataSource, Project, Line, State, Change, Ref, RefChange, QueueEntry, Attestation,
  Operation, Behavior, Tree, FileContent, Diff, Copy, Person, Machine, Store, You,
  ScreenDiff, LogOptions, MergeStatus, ChangeDiff, OpenChange, Blame, LoadStatus,
} from './source';
import { passkeySecret } from './unlock';
import type { PersonView } from './gateway';

export interface ModelSourceOptions { gateway?: string; worker?: () => Worker; onLock?: () => void }
/** The page has no keys. Lock terminates the worker and releases its WASM memory. */
export class ModelSource implements DataSource {
  private worker?: Worker;
  private listeners = new Set<()=>void>();
  private publicProject?: string;
  private nextId = 0;
  private active = false;
  private lastActivity = -Infinity;
  private readonly activityEvents = ['pointerdown', 'keydown', 'scroll'] as const;
  private readonly onActivity = () => {
    if (!this.active || !this.worker || performance.now() - this.lastActivity < 15000) return;
    this.lastActivity = performance.now();
    void this.call('touch').catch(() => this.stop());
  };
  private pending = new Map<number, { resolve(value: unknown): void; reject(error: Error): void }>();
  private idle?: ReturnType<typeof setTimeout>;
  private readonly onClose = () => { void this.lock(); };
  constructor(private readonly options: ModelSourceOptions = {}) {
    addEventListener('pagehide', this.onClose);
    for (const event of this.activityEvents) addEventListener(event, this.onActivity, {passive: true});
  }
  private start(): Worker {
    if (this.worker) return this.worker;
    const worker = this.options.worker?.() ?? new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = event => {
      if (event.data.updated) { for (const listener of this.listeners) listener(); return; }
      if (event.data.locked) { void this.lock(); return; }
      const call = this.pending.get(event.data.id);
      if (!call) return;
      this.pending.delete(event.data.id);
      if (event.data.error) call.reject(new Error(event.data.error)); else call.resolve(event.data.value);
    };
    worker.onerror = event => { this.stop(new Error(event.message || 'The data worker stopped.')); };
    this.worker = worker; return worker;
  }
  private stop(error = new Error('Your session is locked.')) {
    clearTimeout(this.idle);
    const wasOpen = Boolean(this.worker);
    this.active = false;
    this.worker?.terminate(); this.worker = undefined;
    for (const call of this.pending.values()) call.reject(error);
    this.pending.clear();
    if (wasOpen) this.options.onLock?.();
  }
  private call<T>(method: string, args: unknown[] = [], transfer: Transferable[] = []): Promise<T> {
    const worker = this.start(); const id = ++this.nextId;
    clearTimeout(this.idle);
    // The page also terminates the worker if a background timer fails there.
    this.idle = setTimeout(() => this.stop(), 30 * 60 * 1000);
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: value => resolve(value as T), reject });
      worker.postMessage({ id, method, args, base: this.options.gateway || '' }, transfer);
    });
  }
  subscribe(listener:()=>void):()=>void { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  async openPublic(project:string):Promise<string> { const id = await this.call<string>('openPublic',[project]); this.publicProject = project; this.active = true; return id; }
  openChanges(project:string):Promise<OpenChange[]> { return this.call('openChanges',[project]); }
  blame(project:string,state:string,path:string):Promise<Blame> { return this.call('blame',[project,state,path]); }
  fileHistory(project:string,state:string,path:string):Promise<State[]> { return this.call('fileHistory',[project,state,path]); }
  loadStatus(project:string):Promise<LoadStatus> { return this.call('loadStatus',[project]); }
  private async restorePublic() { if (this.publicProject) await this.openPublic(this.publicProject); }
  person(): Promise<PersonView> { return this.call('person'); }
  async unlockRecovery(words: string): Promise<void> { await this.call('unlockRecovery', [words]); this.active = true; await this.restorePublic(); }
  async unlockPasskey(): Promise<void> {
    const person = await this.person();
    const { slot, secret } = await passkeySecret(person);
    try { await this.call('unlockPrf', [secret, slot], [secret.buffer as ArrayBuffer]); this.active = true; await this.restorePublic(); }
    finally { if (secret.byteLength) secret.fill(0); }
  }
  actionAccess(project: string): Promise<ActionAccess> { return this.call('actionAccess', [project]); }
  submitRequest(project: string, action: RequestAction): Promise<RequestStatus> { return this.call('submitRequest', [project, action]); }
  retryRequest(project: string, id: string): Promise<RequestStatus> { return this.call('retryRequest', [project, id]); }
  requestStatus(project: string, id: string): Promise<RequestStatus> { return this.call('requestStatus', [project, id]); }
  comments(project: string, change: string): Promise<Comment[]> { return this.call('comments', [project, change]); }
  projects(): Promise<Project[]> { return this.call('projects'); }
  lines(project: string): Promise<Line[]> { return this.call('lines', [project]); }
  log(project: string, line: string, options: LogOptions = {}): Promise<State[]> { return this.call('log', [project, line, options]); }
  changes(project: string): Promise<Change[]> { return this.call('changes', [project]); }
  changeShow(project: string, id: string): Promise<Change> { return this.call('changeShow', [project, id]); }
  changeDiff(project: string, id: string, from: string, to: string, path = ''): Promise<ChangeDiff> { return this.call('changeDiff', [project, id, from, to, path]); }
  mergeStatus(project: string, state: string, target: string): Promise<MergeStatus> { return this.call('mergeStatus', [project, state, target]); }
  refs(project: string): Promise<Ref[]> { return this.call('refs', [project]); }
  refLog(project: string, name: string): Promise<RefChange[]> { return this.call('refLog', [project, name]); }
  queue(project: string): Promise<QueueEntry[]> { return this.call('queue', [project]); }
  attestations(project: string, state: string): Promise<Attestation[]> { return this.call('attestations', [project, state]); }
  opLog(project: string): Promise<Operation[]> { return this.call('opLog', [project]); }
  probeDiff(project: string, base: string, head: string): Promise<Behavior> { return this.call('probeDiff', [project, base, head]); }
  tree(project: string, state: string, path = ''): Promise<Tree> { return this.call('tree', [project, state, path]); }
  file(project: string, state: string, path: string): Promise<FileContent> { return this.call('file', [project, state, path]); }
  diff(project: string, base: string, head: string, path = ''): Promise<Diff> { return this.call('diff', [project, base, head, path]); }
  copies(project: string): Promise<Copy[]> { return this.call('copies', [project]); }
  people(): Promise<Person[]> { return this.call('people'); }
  machines(): Promise<Machine[]> { return this.call('machines'); }
  stores(): Promise<Store[]> { return this.call('stores'); }
  you(): Promise<You> { return this.call('you'); }
  screens(project: string, base: string, head: string): Promise<ScreenDiff[]> { return this.call('screens', [project, base, head]); }
  async lock(): Promise<void> { this.stop(); }
  dispose(): void { this.stop(); removeEventListener('pagehide', this.onClose); for (const event of this.activityEvents) removeEventListener(event, this.onActivity); }
}
