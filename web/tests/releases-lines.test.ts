import assert from "node:assert/strict";
import { test } from "node:test";
import type { DataSource, Line, Operation, Ref, State } from "../src/data/source";
import { checkNames, releaseNotes, releaseOperation, releases } from "../src/data/releases";
import { filterLines, groupLines, lineActivity, madeTogether, staleAfter } from "../src/data/line-views";

const state = (id: string, change_id = id, parents: string[] = []): State => ({ id, change_id, parents, kind: "commit", subvol: id, author: { name: "author", email: "" }, message: id, time: 1 });
const line = (name: string, id = name): Line => ({ id, name, head: "head", owner: 7, machine: "one" });
const op = (time: number, extra: Partial<Operation> = {}): Operation => ({ id: String(time), machine: "one", seq: time, time, op: "commit", project: "p", actor: { uid: 7, name: "alice" }, ...extra });
const moved = (ref: Ref, time: number) => op(time, { op: "ref.set", data: { ref_change: { name: ref.name, before: null, after: { ...ref } } } });

test("release notes exclude shared ancestry, count each change once and retain the included version", () => {
  const base = state("base"), old = state("old", "amended"), update = state("update", "amended"), newer = state("newer", "amended");
  assert.deepEqual(releaseNotes([newer, update, old, base, state("new")], [old, base]).map((s) => s.id), ["newer", "new"]);
});

test("release authors and dates come from matching ref operations, never state metadata", () => {
  const ref = { name: "release/1", target: "target" };
  assert.equal(releaseOperation(ref, [moved(ref, 1), moved({ ...ref, target: "other" }, 9), moved(ref, 4)])?.time, 4);
  assert.equal(releaseOperation(ref, []), undefined);
});

test("releases use ref dates, page all ancestry and scope checks and environments to the exact state", async () => {
  const old = { name: "release/9", target: "base" }, latest = { name: "release/10", target: "head" };
  const states = [state("head", "head", ["base"]), state("base")];
  const source = {
    refs: async () => [old, latest, { name: "env/prod", target: "head" }, { name: "env/test", target: "other" }],
    opLog: async () => [moved(old, 1), moved(latest, 2)],
    log: async (_: string, target: string, options: { offset: number; limit: number }) => (target === "head" ? states : [states[1]]).slice(options.offset, options.offset + options.limit),
  } as unknown as DataSource;
  const rows = await releases(source, "p", [{ name: "ok", state: "head", argv: [], exit: 0 }, { name: "wrong", state: "other", argv: [], exit: 0 }]);
  assert.deepEqual(rows.map((r) => r.ref.name), ["release/10", "release/9"]);
  assert.deepEqual(rows[0].notes?.map((s) => s.id), ["head"]);
  assert.deepEqual(rows[0].checks.map((c) => c.name), ["ok"]);
  assert.deepEqual(rows[0].environments.map((r) => r.name), ["env/prod"]);
  const many = Array.from({ length: 401 }, (_, i) => state(String(i), String(i), i < 400 ? [String(i + 1)] : []));
  source.refs = async () => [{ name: "release/long", target: "0" }];
  source.opLog = async () => [moved({ name: "release/long", target: "0" }, 2)];
  source.log = async (_, __, options) => many.slice(options!.offset, options!.offset! + options!.limit!);
  assert.equal((await releases(source, "p"))[0].notes?.length, 401);
  source.log = async () => [state("0", "0", ["missing"])];
  const incomplete = (await releases(source, "p"))[0];
  assert.equal(incomplete.notes, undefined);
  assert.match(incomplete.error!, /incomplete/);
  source.opLog = async () => [];
  const undated = (await releases(source, "p"))[0];
  assert.equal(undated.notes, undefined);
  assert.equal(undated.previous, undefined);
});

test("line filters retain all lines, select agent names and respect the 21-day save boundary", () => {
  const all = [line("main"), line("agent-1"), line("agent-2"), line("exp-new"), line("unknown")];
  const now = staleAfter + 100;
  const operations = [op(100, { states: [{ ...state("saved"), line: "main", time: 100 }] }), op(101, { states: [{ ...state("recent"), line: "agent-1", time: 101 }] }), op(50, { lines: { "exp-new": { before: null, after: { ...line("exp-new") } } } })];
  assert.equal(filterLines(all, operations, "All").length, 5);
  assert.deepEqual(filterLines(all, operations, "Agents").map((l) => l.name), ["agent-1", "agent-2"]);
  assert.deepEqual(filterLines(all, operations, "Stale", undefined, now).map((l) => l.name), ["main", "exp-new"]);
  assert.equal(lineActivity(all[4], operations).saved, undefined);
});

test("Yours matches a recorded session name on the same machine and UID only", () => {
  const lines = [line("main"), { ...line("other"), machine: "two" }, { ...line("other-user"), owner: 8 }];
  assert.deepEqual(filterLines(lines, [op(1)], "Yours", "alice").map((l) => l.name), ["main"]);
  assert.equal(filterLines(lines, [op(1)], "Yours").length, 0);
  assert.equal(filterLines(lines, [op(1)], "Yours", "alice@example.com").length, 0);
});

test("hyphen groups require two names, use the first prefix and leave complete names and singletons alone", () => {
  const groups = groupLines([line("main"), line("exp"), line("exp-"), line("exp-one"), line("exp-two-nested"), line("solo-one"), line("agent-1"), line("agent-2")]);
  assert.deepEqual(groups.map((g) => g.prefix || g.lines[0].name), ["main", "exp", "exp-", "solo-one", "agent-", "exp-"]);
  assert.deepEqual(groupLines([line("exp-one")]), [{ prefix: "", lines: [line("exp-one")] }]);
});

test("the default line leads, then single lines by activity, then groups", () => {
  const lines = [line("agent-1"), line("older"), line("agent-2"), line("newer"), line("trunk"), line("unknown")];
  const operations = [op(1, { states: [{ ...state("a"), line: "older", time: 1 }] }), op(2, { states: [{ ...state("b"), line: "newer", time: 2 }] })];
  assert.deepEqual(groupLines(lines, operations, "trunk").map((g) => g.prefix || g.lines[0].name), ["trunk", "newer", "older", "unknown", "agent-"]);
  assert.deepEqual(groupLines([line("work-main"), line("work-one")], [], "work-main").map((g) => g.lines[0].name), ["work-main", "work-one"]);
});

test("check labels require the configured command's exact argv", () => {
  const check = { name: "sh -c true", argv: ["sh", "-c", "true"] };
  assert.deepEqual(checkNames(check, { "check.smoke": "true", "check.other": "false" }), ["smoke"]);
  assert.deepEqual(checkNames(check, { "check.argv": ["sh", "-c", "true"] }), ["argv"]);
  assert.deepEqual(checkNames(check, { "check.smoke": " true " }), []);
  assert.deepEqual(checkNames(check), []);
  assert.deepEqual(checkNames({ ...check, argv: [] }, { "check.invalid": null }), []);
});

test("made together requires one verified creation record, not matching names or adjacent records", () => {
  const lines = [line("agent-1"), line("agent-2")];
  const changes = Object.fromEntries(lines.map((line) => [line.id, { before: null, after: { ...line } }]));
  assert.equal(madeTogether(lines, [op(1, { op: "line.create", lines: changes })]), true);
  assert.equal(madeTogether(lines, lines.map((line, i) => op(i, { op: "line.create", lines: { [line.id]: changes[line.id] } }))), false);
  assert.equal(madeTogether(lines, [op(1, { op: "merge", lines: changes })]), false);
});
