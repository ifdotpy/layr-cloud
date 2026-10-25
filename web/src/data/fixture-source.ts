import { textDiff } from "../text-diff";
import type {
  RequestAction, RequestStatus, ActionAccess, Comment,
  Attestation,
  Behavior,
  Change,
  ChangeDiff,
  Copy,
  DataSource,
  Diff,
  FileContent,
  Id,
  Json,
  Line,
  LogOptions,
  Machine,
  MergeStatus,
  Operation,
  Person,
  Project,
  QueueEntry,
  Ref,
  RefChange,
  ScreenDiff,
  State,
  Store,
  Tree,
  You,
} from "./source";

/** Only JSON emitted by store_golden enters this adapter. It reshapes verified output.
 * Pass a map of filename (with or without .json) to parsed JSON; tests use the same files.
 * Gateway facts are intentionally empty until a real gateway capture is supplied.
 */
export class FixtureSource implements DataSource {
  async actionAccess(_project: Id): Promise<ActionAccess> { return {available: false, reason: 'Actions need an unlocked person signing key.'}; }
  async submitRequest(_project: Id, _action: RequestAction): Promise<RequestStatus> { throw new Error('Actions need a connected store.'); }
  async retryRequest(_project: Id, _id: Id): Promise<RequestStatus> { throw new Error('Requests need a connected store.'); }
  async requestStatus(_project: Id, _id: Id): Promise<RequestStatus> { throw new Error('Requests need a connected store.'); }
  async comments(project: Id, change: Id): Promise<Comment[]> { return (await this.changeShow(project, change)).comments ?? []; }
  private readonly values: Record<string, unknown>;
  private locked = false;
  constructor(files: Record<string, unknown>) {
    this.values = Object.fromEntries(
      Object.entries(files).map(([name, value]) => [
        name
          .split("/")
          .at(-1)!
          .replace(/\.json$/, ""),
        value,
      ]),
    );
  }
  private read<T>(name: string, fallback: T): T {
    if (this.locked) throw new Error("Your session is locked.");
    return (this.values[name] ?? fallback) as T;
  }
  private async project(id: Id) {
    const p = (await this.projects()).find((p) => p.id === id);
    if (!p) throw new Error("Project not found.");
    return p;
  }
  async projects(): Promise<Project[]> {
    const raw = this.read<Record<string, Json>>("config", {});
    const config = (
      raw.values && typeof raw.values === "object" && !Array.isArray(raw.values)
        ? raw.values
        : raw
    ) as Record<string, Json>;
    const projects = this.read<Project[]>("projects", []),
      overlays = this.read<Project["overlays"]>("overlays", undefined),
      policy = this.read<Project["policy"]>(
        "policy",
        raw.policy as unknown as Project["policy"],
      );
    return projects.map((p) => ({
      ...p,
      config: p.config ?? config,
      policy: p.policy ?? policy,
      overlays: p.overlays ?? overlays,
      checks: p.checks ?? this.checks(config),
      probes: p.probes ?? this.probes(config),
      bisect_results: p.bisect_results ?? this.read("bisect", []),
    }));
  }
  private checks(config: Record<string, Json>) {
    const definitions = Object.entries(config)
      .filter(([key]) => key.startsWith("check."))
      .map(([key, value]) => ({
        name: key.slice(6),
        argv:
          typeof value === "string"
            ? [value]
            : Array.isArray(value)
              ? value.map(String)
              : [],
      }));
    const records = this.read<Operation[]>(
      "operations-all",
      this.read("operations", []),
    );
    const results = records.flatMap((record) => {
      const data =
        record.data &&
        typeof record.data === "object" &&
        !Array.isArray(record.data)
          ? record.data
          : {};
      const c =
        data.check &&
        typeof data.check === "object" &&
        !Array.isArray(data.check)
          ? data.check
          : undefined;
      if (!c || c.probe || typeof c.exit !== "number") return [];
      const argv = Array.isArray(c.argv)
        ? c.argv.map(String)
        : typeof c.command === "string"
          ? [c.command]
          : [];
      const matching = definitions.find(
        (d) => d.argv.join(" ") === argv.join(" "),
      );
      const att =
        data.attestation &&
        typeof data.attestation === "object" &&
        !Array.isArray(data.attestation)
          ? data.attestation
          : {};
      const subject =
        att.subject &&
        typeof att.subject === "object" &&
        !Array.isArray(att.subject)
          ? att.subject
          : {};
      return [
        {
          name:
            typeof c.name === "string"
              ? c.name
              : (matching?.name ?? argv.join(" ")),
          argv,
          exit: c.exit,
          state:
            typeof subject.id === "string"
              ? subject.id
              : typeof data.subject === "string"
                ? data.subject
                : undefined,
          machine: record.machine,
          duration_ms:
            typeof c.duration_ms === "number" ? c.duration_ms : undefined,
          reused: typeof c.reused === "boolean" ? c.reused : undefined,
          log: typeof c.log === "string" ? c.log : undefined,
        },
      ];
    });
    return [...definitions, ...results];
  }
  private probes(config: Record<string, Json>) {
    return Object.entries(config)
      .filter(([key]) => key.startsWith("probe."))
      .flatMap(([key, value]) => {
        if (!value || typeof value !== "object" || Array.isArray(value))
          return [];
        return [
          {
            name: key.slice(6),
            kind: String(value.kind ?? "files"),
            run: Array.isArray(value.run) ? value.run.map(String) : [],
            outputs: Array.isArray(value.outputs)
              ? value.outputs.map(String)
              : [],
            fail_over:
              typeof value.fail_over === "number" ? value.fail_over : undefined,
          },
        ];
      });
  }
  async lines(project: Id) {
    await this.project(project);
    return this.read<Line[]>("lines", []);
  }
  async log(project: Id, selector: string, options: LogOptions = {}) {
    const p = await this.project(project),
      lines = await this.lines(project),
      line = lines.find((l) => l.name === selector || l.id === selector);
    let states: State[];
    if (line && line.name === (p.default_line ?? "main"))
      states = this.read<State[]>(options.auto ? "log-auto" : "log", []);
    else {
      const all = this.read<State[]>(
        "states",
        this.read<State[]>("log-auto", []),
      );
      const id = line?.head ?? selector,
        seen = new Set<string>(),
        todo = [id];
      while (todo.length) {
        const next = todo.shift()!;
        if (seen.has(next)) continue;
        seen.add(next);
        todo.push(...(all.find((s) => s.id === next)?.parents ?? []));
      }
      if (!all.some((s) => s.id === id))
        throw new Error("State or line not found.");
      states = all
        .filter((s) => seen.has(s.id) && (options.auto || s.kind !== "auto"))
        .sort((a, b) => b.time - a.time);
    }
    const offset = options.offset ?? 0;
    return states.slice(offset, offset + (options.limit ?? 100));
  }
  async changes(project: Id) {
    await this.project(project);
    return this.read<Change[]>(
      "changes",
      this.values.change ? [this.values.change as Change] : [],
    );
  }
  async changeShow(project: Id, id: Id) {
    const change = (await this.changes(project)).find(
      (c) => c.change_id === id,
    );
    if (!change) throw new Error("Change not found.");
    return change;
  }
  async refs(project: Id) {
    await this.project(project);
    return this.read<Ref[]>("refs", []);
  }
  async refLog(project: Id, name: string) {
    await this.project(project);
    return this.read<RefChange[]>("ref-log", []).filter((r) => r.name === name);
  }
  async queue(project: Id) {
    await this.project(project);
    return this.read<QueueEntry[]>("queue", []);
  }
  async attestations(project: Id, state: Id) {
    await this.project(project);
    return this.read<Attestation[]>(
      "attestations-all",
      this.read("attestations", []),
    ).filter((a) => a.subject.id === state);
  }
  async opLog(project: Id) {
    await this.project(project);
    return this.read<Operation[]>(
      "operations-all",
      this.read("operations", []),
    );
  }
  async probeDiff(project: Id, base: Id, head: Id): Promise<Behavior> {
    await this.project(project);
    const result = this.read<Behavior>("probe-diff", {});
    if (result.base === base && result.head === head) return result;
    const changes = await this.changes(project);
    const behavior = changes
      .map((c) => c.behavior)
      .find((b) => b.base === base && b.head === head);
    if (behavior) return behavior;
    for (const q of await this.queue(project)) {
      if (
        q.probes &&
        typeof q.probes === "object" &&
        !Array.isArray(q.probes)
      ) {
        const b = q.probes.behavior as unknown as Behavior;
        if (b?.base === base && b?.head === head) return b;
      }
    }
    throw new Error("No stored probe comparison for these exact states.");
  }
  async tree(project: Id, state: Id, path = ""): Promise<Tree> {
    await this.project(project);
    const tree = this.read<Tree[]>("trees", []).find(
      (t) => t.state === state && t.path === path.replace(/\/$/, ""),
    );
    if (!tree) throw new Error("Folder not found in the captured store.");
    return tree;
  }
  async file(project: Id, state: Id, path: string): Promise<FileContent> {
    await this.project(project);
    const file = this.read<FileContent[]>("files", []).find(
      (f) => f.state === state && f.path === path,
    );
    if (!file) throw new Error("File not found in the captured store.");
    return file;
  }
  async diff(project: Id, base: Id, head: Id, path?: string): Promise<Diff> {
    await this.project(project);
    const captured = this.read<Diff | undefined>("diff", undefined);
    if (captured?.base === base && captured.head === head)
      return {
        ...captured,
        files: captured.files
          .filter((f) => !path || f.path === path)
          .map((f) =>
            path
              ? f
              : (({ before, after, unified, added, removed, ...metadata }) =>
                  metadata)(f),
          ),
      };
    const all = this.read<FileContent[]>("files", []),
      left = all.filter((f) => f.state === base),
      right = all.filter((f) => f.state === head);
    if (!left.length || !right.length)
      throw new Error(
        "File differences are not available for these exact states.",
      );
    const files = [...new Set([...left, ...right].map((f) => f.path))]
      .filter((p) => !path || p === path)
      .flatMap((p) => {
        const a = left.find((f) => f.path === p),
          b = right.find((f) => f.path === p);
        if (
          a?.text === b?.text &&
          a?.base64 === b?.base64 &&
          a?.size === b?.size
        )
          return [];
        const rows = textDiff(a?.text ?? "", b?.text ?? "");
        return [
          {
            path: p,
            before: a?.text,
            after: b?.text,
            binary: !!(
              (a && a.text === undefined) ||
              (b && b.text === undefined)
            ),
            added: rows.filter((r) => r.kind === "add").length,
            removed: rows.filter((r) => r.kind === "remove").length,
          },
        ];
      });
    return {
      base,
      head,
      files: files.map((f) =>
        path
          ? f
          : (({ before, after, added, removed, ...metadata }) => metadata)(f),
      ),
    };
  }
  async copies(project: Id) {
    await this.project(project);
    return this.read<Copy[]>("copies", []);
  }
  async people() {
    return this.read<Person[]>("people", []);
  }
  async machines() {
    return this.read<Machine[]>("machines", []);
  }
  async stores() {
    return this.read<Store[]>("stores", []);
  }
  async you() {
    return this.read<You>("you", { invites: [], disk: [] });
  }
  async screens(project: Id, base: Id, head: Id) {
    await this.project(project);
    const pair = this.read<
      { base: Id; head: Id; screens: ScreenDiff[] } | undefined
    >("screens", undefined);
    return pair?.base === base && pair.head === head ? pair.screens : [];
  }
  async changeDiff(
    project: Id,
    change: Id,
    from: string,
    to: string,
    path?: string,
  ): Promise<ChangeDiff> {
    await this.project(project);
    const result = ["interdiff", "interdiff-replay"]
      .map((name) => this.read<ChangeDiff | undefined>(name, undefined))
      .find(
        (value) =>
          value?.change_id === change && value.from === from && value.to === to,
      );
    if (!result)
      throw new Error("This version compare is not available from this store.");
    return {
      compared: true,
      change_id: change,
      from,
      to,
      diff: path ? result.diff : undefined,
      files: result.files
        .filter((file) => !path || file.path === path)
        .map((file) =>
          path ? file : { path: file.path, status: file.status },
        ),
    };
  }
  async mergeStatus(
    project: Id,
    state: Id,
    target: string,
  ): Promise<MergeStatus> {
    await this.project(project);
    const value = this.read<MergeStatus | MergeStatus[] | undefined>(
      "merge_status_browser",
      this.read("merge-status", undefined),
    );
    const result = (Array.isArray(value) ? value : value ? [value] : []).find(
      (v) => v.state === state && v.target === target,
    );
    if (!result)
      throw new Error(
        "No merge evaluation stored for this exact state and target.",
      );
    return result;
  }
  async lock() {
    this.locked = true;
  }
}
