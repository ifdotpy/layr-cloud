import assert from 'node:assert/strict';
import { test } from 'node:test';
import { textDiff } from '../src/text-diff';

test('text diff preserves unchanged lines', () => {
  assert.deepEqual(textDiff('alpha\nbeta\n', 'alpha\nbeta\n'), [
    { kind: 'same', text: 'alpha', before: 1, after: 1 },
    { kind: 'same', text: 'beta', before: 2, after: 2 },
  ]);
});

test('text diff records an inserted line with its new position', () => {
  assert.deepEqual(textDiff('alpha\ngamma\n', 'alpha\nbeta\ngamma\n'), [
    { kind: 'same', text: 'alpha', before: 1, after: 1 },
    { kind: 'add', text: 'beta', after: 2 },
    { kind: 'same', text: 'gamma', before: 2, after: 3 },
  ]);
});

test('text diff records a deleted line with its old position', () => {
  assert.deepEqual(textDiff('alpha\nbeta\ngamma\n', 'alpha\ngamma\n'), [
    { kind: 'same', text: 'alpha', before: 1, after: 1 },
    { kind: 'remove', text: 'beta', before: 2 },
    { kind: 'same', text: 'gamma', before: 3, after: 2 },
  ]);
});

test('text diff supports empty files and empty lines', () => {
  assert.deepEqual(textDiff('', ''), []);
  assert.deepEqual(textDiff('', '\n'), [{ kind: 'add', text: '', after: 1 }]);
  assert.deepEqual(textDiff('\n', ''), [{ kind: 'remove', text: '', before: 1 }]);
});

test('text diff reconstructs both inputs and their line positions', () => {
  const cases = [
    ['one\ntwo\nthree\nfour', 'zero\none\nthree\nfive'],
    ['same\nrepeat\nsame\n', 'same\nsame\nrepeat\n'],
    [Array.from({ length: 2100 }, (_, index) => `before-${index}`).join('\n'),
      Array.from({ length: 2100 }, (_, index) => `after-${index}`).join('\n')],
  ];
  for (const [before, after] of cases) {
    const diff = textDiff(before, after);
    const old = diff.filter(line => line.kind !== 'add');
    const current = diff.filter(line => line.kind !== 'remove');
    assert.equal(old.map(line => line.text).join('\n'), before.replace(/\n$/, ''));
    assert.equal(current.map(line => line.text).join('\n'), after.replace(/\n$/, ''));
    assert.deepEqual(old.map(line => line.before), old.map((_, index) => index + 1));
    assert.deepEqual(current.map(line => line.after), current.map((_, index) => index + 1));
  }
});
