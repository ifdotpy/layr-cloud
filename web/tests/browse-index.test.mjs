import test from "node:test";
import assert from "node:assert/strict";
import { validateMarker, validateDirectory } from "../src/browse-index.js";
const state = { seq: 4, attempt: "aaaaaaaa" };
const marker = { version: 1, ...state, state: "snapshot", files: 4760000 };
const page = {
  version: 1,
  ...state,
  path: "",
  page: 0,
  total: 1,
  entries: [{ kind: "directory", name: "nested", count: 2 }],
  next: null,
};
test("indexes match the exact state and requested folder", () => {
  assert.equal(validateMarker(marker, state), marker);
  assert.equal(validateDirectory(page, state, "", 0), page);
  for (const change of [{ seq: 5 }, { attempt: "bbbbbbbb" }, { version: 2 }]) {
    assert.throws(() => validateMarker({ ...marker, ...change }, state));
    assert.throws(() =>
      validateDirectory({ ...page, ...change }, state, "", 0),
    );
  }
  for (const change of [
    { path: "nested" },
    { page: 1 },
    { total: 2 },
    { next: 1 },
    { next: -1 },
  ])
    assert.throws(() =>
      validateDirectory({ ...page, ...change }, state, "", 0),
    );
  for (const name of ["..", ".", "a/b", "a\0b", ""])
    assert.throws(() =>
      validateDirectory(
        { ...page, entries: [{ kind: "directory", name, count: 2 }] },
        state,
        "",
        0,
      ),
    );
  assert.throws(() =>
    validateDirectory(
      { ...page, total: 2, entries: [page.entries[0], page.entries[0]] },
      state,
      "",
      0,
    ),
  );
});
