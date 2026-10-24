import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { shapeErrors } from './shape';

const directory = new URL('../fixtures/', import.meta.url);
const result = (method: string) => `Awaited<ReturnType<Source.DataSource['${method}']>>`;
const shapes: Record<string, string> = {
  'lines.json': result('lines'),
  'log.json': result('log'),
  'log-auto.json': result('log'),
  'change.json': result('changeShow'),
  'refs.json': result('refs'),
  'ref-log.json': result('refLog'),
  'queue.json': result('queue'),
  'attestations.json': result('attestations'),
  'operations.json': result('opLog'),
  'probe-diff.json': result('probeDiff'),
  'projects.json': result('projects'),
  'states.json': result('log'),
  'changes.json': result('changes'),
  'operations-all.json': result('opLog'),
  'attestations-all.json': result('attestations'),
  'trees.json': 'Source.Tree[]',
  'files.json': 'Source.FileContent[]',
  'diff.json': result('diff'),
  'config.json': `{
    project: Source.Id;
    machine: Source.Id;
    values: NonNullable<Source.Project['config']>;
    policy: Source.Policy;
    path_rules: Source.Json;
    line_owners: Source.Json;
  }`,
  'overlays.json': 'Source.Overlay[]',
  'policy.json': 'Source.Policy',
  'copies.json': result('copies'),
  'people.json': result('people'),
  'machines.json': result('machines'),
  'stores.json': result('stores'),
  'you.json': result('you'),
  'screens.json': '{ base: Source.Id; head: Source.Id; screens: Source.ScreenDiff[] }',
  'bisect.json': 'Source.BisectResult[]',
  'probe-same.json': result('probeDiff'),
  'hidden-reader.json': '{ name: string; hidden: boolean }[]',
  'hidden-nonreader.json': '{ name: string; hidden: boolean }[]',
  'merge_status.json': result('mergeStatus'),
  'merge_status_browser.json': `(${result('mergeStatus')})[]`,
  'merge-status.json': `(${result('mergeStatus')})[]`,
  'merge-refusals.json': '{ merge: boolean; ref: boolean }',
  'merge-refusal-details.json': '{ merge: Source.MergeStatus; ref: Source.MergeStatus }',
  'interdiff.json': result('changeDiff'),
  'interdiff-replay.json': result('changeDiff'),
  'request.json': `{
    line: Parameters<Source.DataSource['log']>[1];
    change: Parameters<Source.DataSource['changeShow']>[1];
    ref: Parameters<Source.DataSource['refLog']>[1];
    attestation_state: Parameters<Source.DataSource['attestations']>[1];
    probe_base: Parameters<Source.DataSource['probeDiff']>[1];
    probe_head: Parameters<Source.DataSource['probeDiff']>[2];
    merge_source: Parameters<Source.DataSource['mergeStatus']>[1];
    merge_candidate: Parameters<Source.DataSource['mergeStatus']>[1];
  }`,
};
const files = readdirSync(directory, { recursive: true })
  .filter(name => String(name).endsWith('.json')).map(String).sort();

function assertNativeInputs(value: unknown) {
  assert.ok(Array.isArray(value), 'Native snapshot inputs must be an array.');
  const states: string[] = [];
  for (const [index, item] of value.entries()) {
    assert.ok(item && typeof item === 'object' && !Array.isArray(item));
    assert.deepEqual(Object.keys(item).sort(), ['parent', 'state', 'tree']);
    states.push(`const state${index}: Source.State = ${JSON.stringify(item.state)};`);
    for (const name of ['parent', 'tree']) {
      const tree = item[name];
      assert.ok(tree && typeof tree === 'object' && !Array.isArray(tree));
      for (const [path, file] of Object.entries(tree)) {
        assert.ok(file && typeof file === 'object' && !Array.isArray(file), `${name}/${path}`);
        const entry = file as { mode: number; bytes: number[] };
        assert.deepEqual(Object.keys(entry).sort(), ['bytes', 'mode']);
        assert.ok(Number.isInteger(entry.mode), `${name}/${path}: mode must be an integer.`);
        assert.ok(Array.isArray(entry.bytes), `${name}/${path}: bytes must be an array.`);
        assert.ok(entry.bytes.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255), `${name}/${path}: invalid byte.`);
      }
    }
  }
  assert.deepEqual(shapeErrors(states.join('\n')), []);
}

test('fixture directory contains JSON data', () => assert.ok(files.length > 0));

for (const file of files) {
  test(`${file} matches the ${file === 'interdiff-input.json' ? 'native snapshot input and State' : 'DataSource'} contract`, () => {
    const fixture = JSON.parse(readFileSync(new URL(file, directory), 'utf8'));
    if (file === 'interdiff-input.json') {
      assertNativeInputs(fixture);
      return;
    }
    assert.ok(shapes[file], `Declare a DataSource shape for ${file}`);
    const errors = shapeErrors(`const fixture: ${shapes[file]} = ${JSON.stringify(fixture)};`);
    assert.deepEqual(errors, []);
  });
}

test('fixture validator rejects wrong types and missing required fields', () => {
  const errors = shapeErrors(`
    const wrongType: Source.Line = { id: 'state', name: 'main', head: 'head', owner: 'root', machine: 'machine' };
    const missingField: Source.State = { id: 'state', kind: 'commit', parents: [], subvol: 'volume', author: { name: 'Test', email: 'test@example.com' }, time: 1 };
  `);
  assert.equal(errors.length, 2, errors.join('\n'));
  assert.ok(errors.some(message => message.includes("Type 'string' is not assignable to type 'number'")));
  assert.ok(errors.some(message => message.includes("Property 'message' is missing")));
});
