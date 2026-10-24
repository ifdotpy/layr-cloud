/** The pages' only data boundary. All methods are read-only and asynchronous.
 * CLI twins preserve layr-model's JSON names, full IDs, and millisecond timestamps.
 * Sources must reject unavailable objects; they must never invent successful results.
 * Keys, tokens, plaintext, and recovery words stay out of browser persistent storage.
 */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Id = string;
export interface Author { name: string; email: string; uid?: number; session?: string }
/** `layr line list --json`, excluding the machine-local path absent from stores. */
export interface Line { id: Id; name: string; head: Id; owner: number; machine: Id; path?: string }
/** `layr log --json`. Internal kind names are format values, not UI vocabulary. */
export interface State { id: Id; kind: 'commit' | 'auto' | 'stash' | 'import' | 'cache' | 'derived'; parents: Id[]; subvol: string; author: Author; message: string; time: number; change_id?: Id; derived_from?: Id; context?: string; line?: Id; running?: boolean; conflicts?: { path: string; kind: string; base?: string; ours?: string; theirs?: string }[]; layers?: Record<string, string>; layer_by?: number; layer_shared?: boolean; layer_platform?: string }
export interface Subject { kind: string; id: Id }
/** `layr attestations --json`. Payloads load separately; a signature is not a pass result. */
export interface Attestation { format: number; attestation_id: Id; subject: Subject; change_id?: Id; kind: string; payload_digest: string; payload_ref?: string; signer: string; time: number }
export interface Ref { name: string; target: Id; expires_at?: number }
export interface RefChange { name: string; before: Ref | null; after: Ref | null }
export interface QueueEntry { id: Id; into: string; target_line: Id; state: Id; change_id?: Id; added_by: number; time: number; status: string; attempts: number; result?: Id; reason?: string; probes?: Json }
export interface LinePointer { name: string; head: Id; owner: number; machine: Id }
/** `layr op log --json`, already verified by the source. */
export interface Operation { id: Id; machine: Id; seq: number; time: number; op: string; project: Id; actor: { uid: number; name: string; session?: string }; states?: State[]; lines?: Record<Id, { before: LinePointer | null; after: LinePointer | null; working_before?: Id; working_after?: Id }>; data?: Json }
/** `layr change show --json`. Version order is authoritative. */
export interface Change { change_id: Id; versions: { version: string; state: State; attestations: Attestation[] }[]; behavior: Behavior }
/** `layr probe diff --json`. Probe-specific fields remain lossless JSON. */
export interface Behavior { base?: Id; head?: Id; base_source?: string; head_source?: string; base_views?: string[]; head_views?: string[]; status?: string; reason?: string; probes?: ProbeResult[] }
export interface ProbeResult { probe: string; kind?: string; status: string; changes?: Json[]; base?: Json; head?: Json; [key: string]: Json | undefined }
/** Organization/project metadata from verified project records and gateway listings.
 * Missing optional fields mean unknown, not zero. No fake display metadata is added.
 */
export interface Project { id: Id; name: string; organization?: string; description?: string; default_line?: string; config?: Record<string, Json>; overlays?: Overlay[]; checks?: Check[]; probes?: ProbeDefinition[]; images?: Image[]; disk?: DiskUse[]; bisect?: BisectResult; }
export interface Overlay { id: Id; name: string; patterns: string[]; readers: string[]; managers: string[]; readable: boolean; epoch?: string }
/** A tree lists only one directory. Content and diffs load on demand, per path. */
export interface TreeEntry { path: string; name: string; kind: 'file' | 'directory'; size?: number; overlay?: string; derived?: string; state?: Id }
export interface Tree { state: Id; path: string; entries: TreeEntry[]; hidden_overlays?: string[] }
/** Text is decoded by the source. Binary bytes use base64; untrusted HTML is never executed. */
export interface FileContent { state: Id; path: string; text?: string; base64?: string; mime?: string; size: number }
export interface FileDiff { path: string; before?: string; after?: string; binary?: boolean; added: number; removed: number; derived?: string }
export interface Diff { base: Id; head: Id; files: FileDiff[] }
/** Signed sync/push records. Unknown completeness stays absent. */
export interface Copy { id: string; name: string; kind: 'store' | 'peer' | 'git'; address?: string; present?: number; expected?: number; bytes?: number; time?: number; machines?: string[] }
export interface Person { login: string; name?: string; role?: string; recipient?: string; projects?: string[]; time?: number; invited?: boolean }
export interface Machine { id: Id; name?: string; os?: string; backend?: string; role?: string; fingerprint?: string; trusted?: boolean; online?: boolean; time?: number }
export interface Store { id: string; project: Id; address?: string; region?: string; bytes?: number; objects?: number; time?: number }
/** Public session metadata only. Unlock material is not part of this interface. */
export interface You { login?: string; recipient?: string; invites: { project: string; role: string }[]; disk: { store: string; address?: string; locked: boolean; path?: string }[]; aws?: { profile: string; region?: string; status: string } }
export interface Check { name: string; state?: Id; argv: string[]; exit?: number; duration_ms?: number; machine?: string; reused?: boolean; log?: string }
export interface ProbeDefinition { name: string; kind: string; run: string[]; outputs: string[]; fail_over?: number }
export interface Image { name: string; tag: string; state?: Id; ref?: string; digest?: string; bytes?: number }
export interface DiskUse { line: string; shared_bytes: number; separate_bytes: number; cache_bytes: number }
export interface BisectResult { good: Id; bad: Id; command: string[]; first_bad?: Id; steps: { state: Id; result: string; reused: boolean }[] }
/** Stored screenshot probe output. URLs must refer to local verified bytes.
 * Structure is escaped text, never an iframe or executable HTML.
 */
export interface ScreenDiff { name: string; route: string; width: number; height: number; base_url: string; head_url: string; changed_share: number; regions: { x: number; y: number; width: number; height: number }[]; structure?: { summary: string; before: string; after: string } }
export interface LogOptions { auto?: boolean; limit?: number; offset?: number }
export interface DataSource {
  projects(): Promise<Project[]>;
  lines(project: Id): Promise<Line[]>;
  log(project: Id, line: string, options?: LogOptions): Promise<State[]>;
  changeShow(project: Id, id: Id): Promise<Change>;
  /** All change IDs with their complete CLI-compatible show output. */
  changes(project: Id): Promise<Change[]>;
  refs(project: Id): Promise<Ref[]>;
  refLog(project: Id, name: string): Promise<RefChange[]>;
  queue(project: Id): Promise<QueueEntry[]>;
  attestations(project: Id, state: Id): Promise<Attestation[]>;
  opLog(project: Id): Promise<Operation[]>;
  probeDiff(project: Id, base: Id, head: Id): Promise<Behavior>;
  tree(project: Id, state: Id, path?: string): Promise<Tree>;
  file(project: Id, state: Id, path: string): Promise<FileContent>;
  diff(project: Id, base: Id, head: Id, path?: string): Promise<Diff>;
  copies(project: Id): Promise<Copy[]>;
  people(): Promise<Person[]>;
  machines(): Promise<Machine[]>;
  stores(): Promise<Store[]>;
  you(): Promise<You>;
  /** Screen bytes and structure come from stored probe outputs, never from running a probe. */
  screens(project: Id, base: Id, head: Id): Promise<ScreenDiff[]>;
  /** Lock discards worker memory. FixtureSource has no secret material. */
  lock(): Promise<void>;
}
