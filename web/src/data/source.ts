/** The pages' only data boundary. All methods are asynchronous.
 * CLI twins preserve layr-model's JSON names, full IDs, and millisecond timestamps.
 * Sources must reject unavailable objects; they must never invent successful results.
 * Keys, tokens, plaintext, and recovery words stay out of browser persistent storage.
 */
export type Json =
  | null
  | boolean
  | number
  | string
  | Json[]
  | { [key: string]: Json };
export type Id = string;
export interface Author {
  name: string;
  email: string;
  uid?: number;
  session?: string;
}
/** `layr line list --json`, excluding the machine-local path absent from stores. */
export interface Line {
  id: Id;
  name: string;
  head: Id;
  owner: number;
  machine: Id;
  path?: string;
}
/** `layr log --json`. Internal kind names are format values, not UI vocabulary. */
export interface State {
  id: Id;
  kind: "commit" | "auto" | "stash" | "import" | "cache" | "derived";
  parents: Id[];
  subvol: string;
  author: Author;
  message: string;
  time: number;
  change_id?: Id;
  derived_from?: Id;
  context?: string;
  line?: Id;
  running?: boolean;
  conflicts?: {
    path: string;
    kind: string;
    base?: string;
    ours?: string;
    theirs?: string;
  }[];
  layers?: Record<string, string>;
  layer_by?: number;
  layer_shared?: boolean;
  layer_platform?: string;
}
export interface Subject {
  kind: string;
  id: Id;
}
/** `layr attestations --json`. Payloads load separately; a signature is not a pass result. */
export interface Attestation {
  format: number;
  attestation_id: Id;
  subject: Subject;
  change_id?: Id;
  kind: string;
  payload_digest: string;
  payload_ref?: string;
  signer: string;
  time: number;
}
export interface Ref {
  name: string;
  target: Id;
  expires_at?: number;
}
export interface RefChange {
  name: string;
  before: Ref | null;
  after: Ref | null;
}
export interface QueueEntry {
  id: Id;
  into: string;
  target_line: Id;
  state: Id;
  change_id?: Id;
  added_by: number;
  time: number;
  status: string;
  attempts: number;
  result?: Id;
  reason?: string;
  probes?: Json;
}
export interface LinePointer {
  name: string;
  head: Id;
  owner: number;
  machine: Id;
}
/** `layr op log --json`, already verified by the source. */
export interface Operation {
  id: Id;
  machine: Id;
  seq: number;
  time: number;
  op: string;
  project: Id;
  actor: { uid: number; name: string; session?: string };
  states?: State[];
  lines?: Record<
    Id,
    {
      before: LinePointer | null;
      after: LinePointer | null;
      working_before?: Id;
      working_after?: Id;
    }
  >;
  data?: Json;
}
/** `layr change show --json`. Version order is authoritative. */
export interface Change {
  comments?: Comment[];
  change_id: Id;
  versions: { version: string; state: State; attestations: Attestation[]; operation?: VersionOperation }[];
  behavior: Behavior;
}
/** `layr probe diff --json`. Probe-specific fields remain lossless JSON. */
export interface Behavior {
  base?: Id;
  head?: Id;
  base_source?: string;
  head_source?: string;
  base_views?: string[];
  head_views?: string[];
  status?: string;
  reason?: string;
  probes?: ProbeResult[];
}
export interface ProbeResult {
  probe: string;
  kind?: string;
  status: string;
  changes?: Json[];
  base?: Json;
  head?: Json;
  [key: string]: Json | undefined;
}
/** Organization/project metadata from verified project records and gateway listings.
 * Missing optional fields mean unknown, not zero. No fake display metadata is added.
 */
export interface Project {
  id: Id;
  name: string;
  organization?: string;
  description?: string;
  default_line?: string;
  config?: Record<string, Json>;
  policy?: Policy;
  counts?: {
    lines: number;
    states: number;
    changes: number;
    refs: number;
    queue: number;
    operations: number;
    attestations: number;
  };
  overlays?: Overlay[];
  checks?: Check[];
  probes?: ProbeDefinition[];
  images?: Image[];
  disk?: DiskUse[];
  bisect_results?: BisectResult[];
}
export interface Overlay {
  /** Exact state used for current readable overlay tree metadata. */
  state?: Id;
  id: Id;
  name: string;
  patterns?: string[];
  readers: string[];
  managers: string[];
  readable: boolean;
  hidden?: boolean;
  mode?: string;
  owner?: number;
  policy?: Policy;
  epoch?: string;
  paths?: string[];
  files?: FileContent[];
  reader_details?: { principal: string; key?: string; since?: number }[];
  epochs?: { id: string; number?: number; reason?: string; time?: number; machine?: string }[];
  history?: Operation[];
  peers?: string[];
  slots?: { store: string; epoch: string; count: number }[];
}
/** A tree lists only one directory. Content and diffs load on demand, per path. */
export interface TreeEntry {
  path: string;
  name: string;
  kind: "file" | "directory" | "symlink";
  mode?: string;
  text?: string;
  size?: number;
  overlay?: string;
  derived?: string;
  state?: Id;
}
export interface Tree {
  state: Id;
  line?: string;
  path: string;
  entries: TreeEntry[];
  hidden_overlays?: string[];
}
/** Text is decoded by the source. Binary bytes use base64; untrusted HTML is never executed. */
export interface FileContent {
  state: Id;
  path: string;
  text?: string;
  base64?: string;
  mime?: string;
  size: number;
}
export interface FileDiff {
  path: string;
  before?: string;
  after?: string;
  binary?: boolean;
  added?: number;
  removed?: number;
  derived?: string;
  before_mode?: string;
  after_mode?: string;
  status?: string;
  unified?: string;
}
export interface Diff {
  base: Id;
  head: Id;
  files: FileDiff[];
}
/** Signed sync/push records. Unknown completeness stays absent. */
export interface Transfer {
  id: Id;
  remote: string;
  url: string;
  result: "success";
  direction: string;
  machine: Id;
  time: number;
  actor: { uid: number; name: string; session?: string };
  records_sent: number;
  records_received: number;
  states_sent: number;
  states_received: number;
  bytes_sent: number;
  bytes_received: number;
  state_ids_sent: Id[];
  state_ids_received: Id[];
  overlays: string[];
}
export interface VersionOperation {
  id: Id;
  state: Id;
  kind: "amend" | "replay" | "pick";
  source: Id;
  target_line: Id | null;
  target_head: Id;
  actor: { uid: number; name: string; session?: string };
  machine: Id;
  time: number;
}
export interface Copy {
  id: string;
  name: string;
  kind: "store" | "peer" | "git" | "registry";
  address?: string;
  present?: number;
  expected?: number;
  bytes?: number;
  time?: number;
  machines?: string[];
  machine?: Id;
  last?: Transfer;
}
export interface Person {
  login: string;
  name?: string;
  role?: string;
  recipient?: string;
  projects?: string[];
  time?: number;
  invited?: boolean;
}
export interface Machine {
  id: Id;
  name?: string;
  os?: string;
  backend?: string;
  role?: string;
  fingerprint?: string;
  trusted?: boolean;
  online?: boolean;
  time?: number;
}
export interface Store {
  id: string;
  project: Id;
  address?: string;
  region?: string;
  bytes?: number;
  objects?: number;
  time?: number;
}
/** Public session metadata only. Unlock material is not part of this interface. */
export interface You {
  login?: string;
  recipient?: string;
  invites: { project: string; role?: string }[];
  /** Invalid pending invites do not grant access or block accepted project pins. */
  invite_errors?: { id: string; reason: string }[];
  disk: { store: string; address?: string; locked: boolean; path?: string }[];
  aws?: { profile: string; region?: string; status: string };
}
export interface Check {
  name: string;
  state?: Id;
  argv: string[];
  exit?: number;
  duration_ms?: number;
  machine?: string;
  reused?: boolean;
  log?: string;
}
export interface ProbeDefinition {
  name: string;
  kind: string;
  run: string[];
  outputs: string[];
  noise?: number;
  fail_over?: number;
}
export interface Image {
  name: string;
  tag: string;
  state?: Id;
  ref?: string;
  digest?: string;
  bytes?: number;
}
export interface DiskUse {
  line: string;
  shared_bytes: number;
  separate_bytes: number;
  cache_bytes: number;
}
export interface BisectResult {
  kind?: State["kind"];
  line?: Id;
  message?: string;
  origin?: string[];
  state?: Id;
  id: Id;
  good: Id;
  bad: Id;
  argv: string[];
  first_bad?: Id;
  time: number;
  machine: Id;
  actor: { uid: number; name: string; session?: string };
  steps: { state: Id; exit: number; reused: boolean }[];
}
/** Canonical screenshot row, with PNG URLs made only from verified stored bytes.
 * Structure is an escaped unified diff; it is never executed or rendered as HTML.
 */
export interface ScreenDiff {
  kind: "screenshots";
  size_changed: boolean;
  changed_percent: number | null;
  name: string;
  probe: string;
  screen: string;
  path: string;
  status: string;
  before: { width: number; height: number } | null;
  after: { width: number; height: number } | null;
  base_png: string | null;
  head_png: string | null;
  base_url?: string;
  head_url?: string;
  changed: boolean;
  changed_share: number | null;
  changed_pixels: number | null;
  total_pixels: number | null;
  regions: { x: number; y: number; width: number; height: number }[];
  base_structure: string | null;
  head_structure: string | null;
  structure_available: boolean;
  structure_diff: string;
  elements_added: number;
  elements_removed: number;
  text_changes: number;
  structure_regions: { x: number; y: number; width: number; height: number; change: string }[];
}
export interface LogOptions {
  auto?: boolean;
  limit?: number;
  offset?: number;
}
export type RequestAction =
  | { kind: 'approve' | 'withdraw' | 'queue_add'; state: Id; into: string }
  | { kind: 'queue_remove'; entry: Id }
  | { kind: 'ref_move'; name: string; target: Id; expected: Id | null }
  | { kind: 'comment'; change_id: Id; version: Id; path?: string; line?: number; text: string; reply_to?: Id };
export interface ActionAccess { available: boolean; reason?: string | null }
export interface RequestStatus { id: Id; status: 'pending' | 'applied' | 'refused'; record?: Id; reason?: string | null }
export interface Comment {
  id: Id; change_id: Id; version: Id; path?: string; line?: number;
  text: string; reply_to?: Id; actor: { uid: number; name: string }; time: number;
}
export interface DataSource {
  actionAccess(project: Id): Promise<ActionAccess>;
  submitRequest(project: Id, action: RequestAction): Promise<RequestStatus>;
  requestStatus(project: Id, id: Id): Promise<RequestStatus>;
  retryRequest(project: Id, id: Id): Promise<RequestStatus>;
  comments(project: Id, change: Id): Promise<Comment[]>;
  projects(): Promise<Project[]>;
  lines(project: Id): Promise<Line[]>;
  /** Selector is a line name, line ID, or exact state ID, as with the CLI. */
  log(project: Id, line: string, options?: LogOptions): Promise<State[]>;
  changeShow(project: Id, id: Id): Promise<Change>;
  /** All change IDs with their complete CLI-compatible show output. */
  changes(project: Id): Promise<Change[]>;
  refs(project: Id): Promise<Ref[]>;
  refLog(project: Id, name: string): Promise<RefChange[]>;
  queue(project: Id): Promise<QueueEntry[]>;
  attestations(project: Id, state: Id): Promise<Attestation[]>;
  /** All verified project operations, without a hidden count limit. */
  opLog(project: Id): Promise<Operation[]>;
  probeDiff(project: Id, base: Id, head: Id): Promise<Behavior>;
  tree(project: Id, state: Id, path?: string): Promise<Tree>;
  file(project: Id, state: Id, path: string): Promise<FileContent>;
  /** No path returns changed-file metadata. An exact path loads only that file. */
  diff(project: Id, base: Id, head: Id, path?: string): Promise<Diff>;
  copies(project: Id): Promise<Copy[]>;
  people(): Promise<Person[]>;
  machines(): Promise<Machine[]>;
  stores(): Promise<Store[]>;
  you(): Promise<You>;
  /** Screen bytes and structure come from stored probe outputs, never from running a probe. */
  screens(project: Id, base: Id, head: Id): Promise<ScreenDiff[]>;
  /** Canonical version patch comparison. Content loads only for an exact path. */
  changeDiff(
    project: Id,
    change: Id,
    from: string,
    to: string,
    path?: string,
  ): Promise<ChangeDiff>;
  /** Canonical policy evaluation for the exact state and verified target machine. */
  mergeStatus(project: Id, state: Id, target: string): Promise<MergeStatus>;
  /** Lock discards worker memory. FixtureSource has no secret material. */
  lock(): Promise<void>;
}

/** Effective access and stored protection rules from layr-model::policy::Policy.
 * Role names are format values. The browser does not decide authorization. */
export type Role = "none" | "reader" | "writer" | "maintainer" | "admin";
export interface ProtectionRule {
  update?: string;
  expect?: boolean;
  check?: string[];
  direct?: boolean;
  rewrite?: string;
  paths?: Record<string, string>;
  approvals?: number;
  approvers?: string[];
}
export interface Policy {
  access: Record<string, Role>;
  grants: Record<string, [Role, number]>;
  deny: Record<string, string[]>;
  protect: Record<string, ProtectionRule>;
}

export interface MergeStatus {
  state: Id;
  target: string;
  approvals: {
    approver: number;
    state: Id;
    time: number;
    met: boolean | null;
  }[];
  approvals_needed: number;
  approvers: string[];
  checks: { name: string; argv: string[]; result: string; exit?: number }[];
  path_requirements: { path: string; role: Role; met: boolean | null }[];
  allowed: boolean | null;
  already_in_target: boolean;
  target_machine: string | null;
  status: string;
  undecided_reasons: string[];
  reason?: string;
}

/** The native CLI interdiff. Fixture bytes come from explicitly approved native snapshots. */
export interface ChangeDiff {
  /** False means paths are candidates from tree metadata, not a completed patch compare. */
  compared?: boolean;
  change_id: Id;
  from: string;
  to: string;
  diff?: string;
  files: {
    path: string;
    status: string;
    added?: number;
    removed?: number;
    interdiff?: string;
  }[];
}
