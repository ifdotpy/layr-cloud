import type { Line, Operation } from "./source";

export const staleAfter = 21 * 24 * 60 * 60 * 1000;
export const lineFilters = ["All", "Yours", "Agents", "Stale"] as const;
export type LineFilter = typeof lineFilters[number];

export function lineActivity(line: Line, operations: Operation[]) {
  const saves = operations.flatMap((op) => (op.states ?? []).filter((state) =>
    ["commit", "auto", "import"].includes(state.kind) &&
    (state.line === line.id || op.lines?.[line.id]?.after?.head === state.id),
  ).map((state) => state.time));
  const created = operations.filter((op) => op.lines?.[line.id]?.before === null && op.lines[line.id]?.after)
    .map((op) => op.time);
  return { saved: saves.length ? Math.max(...saves) : undefined, created: created.length ? Math.min(...created) : undefined };
}

export function filterLines(lines: Line[], operations: Operation[], filter: LineFilter, login?: string, now = Date.now()) {
  return lines.filter((line) => {
    if (filter === "Agents") return line.name.startsWith("agent-");
    // The model has no person-key-to-UID binding. Match only the recorded
    // session name on this machine; never infer an owner from a head author.
    if (filter === "Yours") return Boolean(login && operations.some((op) =>
      op.machine === line.machine && op.actor.uid === line.owner && op.actor.name === login,
    ));
    if (filter === "Stale") {
      const activity = lineActivity(line, operations), last = activity.saved ?? activity.created;
      return last !== undefined && now - last >= staleAfter;
    }
    return true;
  });
}

export function groupLines(lines: Line[], operations: Operation[] = [], defaultLine = "main") {
  const primary = lines.filter((line) => line.name === defaultLine);
  const remaining = lines.filter((line) => line.name !== defaultLine);
  const groups = new Map<string, Line[]>();
  const prefixOf = (line: Line) => {
    const dash = line.name.indexOf("-");
    return dash > 0 && dash < line.name.length - 1 ? line.name.slice(0, dash + 1) : "";
  };
  for (const line of remaining) {
    const prefix = prefixOf(line);
    if (!prefix) continue;
    if (!groups.has(prefix)) groups.set(prefix, []);
    groups.get(prefix)!.push(line);
  }
  const activity = (line: Line) => {
    const last = lineActivity(line, operations);
    return last.saved ?? last.created ?? -Infinity;
  };
  const singles = remaining.filter((line) => (groups.get(prefixOf(line))?.length ?? 0) < 2)
    .sort((a, b) => activity(b) - activity(a) || a.name.localeCompare(b.name));
  return [
    ...[...primary, ...singles].map((line) => ({ prefix: "", lines: [line] })),
    ...[...groups].filter(([, items]) => items.length > 1).sort(([a], [b]) => a.localeCompare(b))
      .map(([prefix, items]) => ({ prefix, lines: items })),
  ];
}

/** A shared name prefix alone does not prove one creation command. */
export function madeTogether(lines: Line[], operations: Operation[]) {
  return lines.length > 1 && operations.some((op) =>
    ["line.create", "line.new"].includes(op.op) && lines.every((line) =>
      op.lines?.[line.id]?.before === null && Boolean(op.lines[line.id]?.after),
    ),
  );
}
