import type { Json, Machine, Operation, Person } from "./data/source";

export const shortText = (text: string) =>
  text.replace(
    /[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}/gi,
    (id) => id.replaceAll("-", "").slice(0, 12),
  );
export const plural = (count: number, noun: string, many = noun + "s") =>
  `${count} ${count === 1 ? noun : many}`;
export const object = (value: Json | undefined): Record<string, Json> =>
  value && typeof value === "object" && !Array.isArray(value) ? value : {};
export const machineLabel = (id: string, machines: Machine[]) =>
  machines.find((machine) => machine.id === id)?.name ||
  `machine ${id.replaceAll("-", "").slice(0, 7)}`;
export function personLabel(
  principal: string,
  operations: Operation[] = [],
  people: Person[] = [],
) {
  const login = principal.startsWith("person:")
    ? principal.slice(7).split("=", 1)[0]
    : undefined;
  if (login) return login;
  const uid = principal.match(/^(?:user:|uid:)?(\d+)$/)?.[1];
  if (uid) {
    const names = [
      ...new Set(
        operations
          .filter((operation) => String(operation.actor.uid) === uid)
          .map((operation) => operation.actor.name)
          .filter(Boolean),
      ),
    ];
    return names.length === 1 ? names[0] : `uid ${uid}`;
  }
  return shortText(principal);
}
/** One display vocabulary. Keep the original operation name in the caller's tooltip. */
export function operationLabel(operation: Operation | string): string {
  const name = typeof operation === "string" ? operation : operation.op;
  const data = typeof operation === "string" ? {} : object(operation.data);
  const attestation = object(data.attestation);
  const queueStatus =
    object(object(data.queue_change).after).status || data.status;
  if (name === "attest")
    return attestation.kind === "approval"
      ? "approved"
      : attestation.kind === "check"
        ? "added a check result"
        : attestation.kind === "revoke"
          ? "revoked an attestation"
          : `added ${attestation.kind || "an"} attestation`;
  const labels: Record<string, string> = {
    config: "changed settings",
    commit: "saved",
    save: "saved",
    snapshot: "saved automatically",
    "snapshot.auto": "saved automatically",
    "merge.candidate": "prepared a merge",
    merge: "merged",
    "queue.add": "added to queue",
    "queue.result": `queue result: ${queueStatus === "done" || queueStatus === "passed" ? "passed" : queueStatus === "failed" ? "failed" : "recorded"}`,
    "stash.drop": "dropped parked work",
    "stash.pop": "restored parked work",
    "stash.push": "parked work",
    stash: "parked work",
    "ref.set":
      `moved ref ${object(data.ref_change).name || data.name || ""}`.trim(),
    "overlay.use": "turned on overlay",
    "overlay.create": "created overlay",
    "overlay.key": "changed the key",
    "overlay.reader.add": "added reader",
    "overlay.reader.remove": "removed reader",
    "rebase.step": "replayed a step",
    rebase: "replayed",
    "cherry-pick": "picked",
    revert: "inverted",
    "line.create": "created line",
    "line.new": "created line",
    project: "created project",
    machine: "added machine",
    push: "pushed",
    sync: "synced",
    "bisect.run": "recorded bisect results",
    "overlay.show-to-project": "made overlay visible to project",
  };
  return shortText(labels[name] || name.replaceAll(".", " "));
}
