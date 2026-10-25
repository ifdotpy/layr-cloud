import type {
  Change,
  DataSource,
  MergeStatus,
  Project,
  QueueEntry,
} from "./source";

/** The list and navigation count use the same verified merge and queue evidence. */
export function changeStatus(
  change: Change,
  queue: QueueEntry[],
  evaluation?: MergeStatus,
) {
  const entries = queue.filter(
    (entry) =>
      entry.change_id === change.change_id ||
      change.versions.some((version) => version.state.id === entry.state),
  );
  if (
    evaluation?.already_in_target ||
    entries.some((entry) => entry.status === "done")
  )
    return "merged";
  return entries.some((entry) =>
    ["pending", "queued", "running"].includes(entry.status),
  )
    ? "in queue"
    : "open";
}

export async function loadChangeIndex(source: DataSource, project: Project) {
  const provisional = source.loadStatus ? (await source.loadStatus(project.id)).provisional : project.provisional;
  if (provisional && source.openChanges) {
    const summaries = await source.openChanges(project.id);
    return { changes:[] as Change[], queue:[] as QueueEntry[], statuses:{} as Record<string,MergeStatus|undefined>, open:summaries.length, summaries };
  }
  const [changes, queue] = await Promise.all([
    source.changes(project.id),
    source.queue(project.id),
  ]);
  const statuses = Object.fromEntries(
    await Promise.all(
      changes.map(async (change) => {
        try {
          return [
            change.change_id,
            await source.mergeStatus(
              project.id,
              change.versions.at(-1)!.state.id,
              project.default_line ?? "main",
            ),
          ] as const;
        } catch {
          return [change.change_id, undefined] as const;
        }
      }),
    ),
  );
  const open = source.openChanges ? (await source.openChanges(project.id)).length : undefined;
  return {
    changes,
    queue,
    statuses,
    summaries: undefined as import("./source").OpenChange[] | undefined,
    open: open ?? changes.filter(
      (change) =>
        changeStatus(change, queue, statuses[change.change_id]) === "open",
    ).length,
  };
}
