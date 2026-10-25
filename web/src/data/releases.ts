import type { Check, DataSource, Operation, Ref, State } from "./source";
import { object } from "../labels";

export interface Release {
  ref: Ref;
  operation?: Operation;
  previous?: Ref;
  notes?: State[];
  error?: string;
  checks: Check[];
  environments: Ref[];
}

/** Use the signed ref operation, never the target state's author or timestamp. */
export function releaseOperation(ref: Ref, operations: Operation[]) {
  return operations.filter((op) => {
    const change = object(object(op.data).ref_change);
    const after = object(change.after);
    return op.op === "ref.set" && change.name === ref.name && after.target === ref.target;
  }).sort((a, b) => b.time - a.time || b.seq - a.seq || b.machine.localeCompare(a.machine))[0];
}

/** A release's notes use the version actually in its history, once per change. */
export function releaseNotes(states: State[], previous: State[]) {
  const before = new Set(previous.map((s) => s.id));
  const changes = new Set<string>();
  return states.filter((state) => {
    if (before.has(state.id) || !state.change_id || changes.has(state.change_id)) return false;
    changes.add(state.change_id);
    return true;
  });
}

async function history(source: DataSource, project: string, state: string) {
  const all: State[] = [];
  const seen = new Set<string>();
  for (let offset = 0; ; offset += 200) {
    const page = await source.log(project, state, { offset, limit: 200 });
    for (const item of page) {
      if (seen.has(item.id)) throw new Error("Release history could not be read completely.");
      seen.add(item.id);
      all.push(item);
    }
    if (page.length < 200) break;
  }
  if (!all.some((s) => s.id === state) || all.some((s) => s.parents.some((id) => !seen.has(id))))
    throw new Error("Release history is incomplete in this store.");
  return all;
}

export async function releases(source: DataSource, project: string, checks: Check[] = []) {
  const [refs, operations] = await Promise.all([source.refs(project), source.opLog(project)]);
  const rows: Release[] = refs.filter((ref) => ref.name.startsWith("release/")).map((ref) => ({
    ref,
    operation: releaseOperation(ref, operations),
    checks: checks.filter((check) => check.state === ref.target),
    environments: refs.filter((env) => env.name.startsWith("env/") && env.target === ref.target),
  })).sort((a, b) => (b.operation?.time ?? -Infinity) - (a.operation?.time ?? -Infinity) || a.ref.name.localeCompare(b.ref.name));
  const histories = new Map<string, Promise<State[]>>();
  const load = (state: string) => {
    if (!histories.has(state)) histories.set(state, history(source, project, state));
    return histories.get(state)!;
  };
  await Promise.all(rows.map(async (row, index) => {
    // Without ref dates the previous release cannot be determined honestly.
    if (rows.some((r) => !r.operation)) {
      row.error = "Release order is not recorded.";
      return;
    }
    row.previous = rows[index + 1]?.ref;
    try {
      const [current, previous] = await Promise.all([load(row.ref.target), row.previous ? load(row.previous.target) : []]);
      row.notes = releaseNotes(current, previous);
    } catch (error) {
      row.error = error instanceof Error ? error.message : String(error);
    }
  }));
  return rows;
}
