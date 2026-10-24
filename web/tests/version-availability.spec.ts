import { expect, test } from '@playwright/test';
import { FixtureSource } from '../src/data/fixture-source';
import type { Change, Project, Tree } from '../src/data/source';
import { fixture, fixtures } from './fixture-data';

test('the single t37 push after v3 retains every earlier change version tree', async () => {
  const project = fixture<Project[]>('projects')[0];
  const change = fixture<Change>('change');
  const earlier = change.versions.slice(0, -1);
  // Fixture revision 7b0a165: tests/t37-model.sh:201 pushes once after v3.
  // Native src/store_remote/writer.rs:190 selects src/db.rs:595 present states without version pins.
  // The records task must make push send every change version, including its tree and file bytes.
  test.info().annotations.push({ type: 'provenance', description: 'Native fixture 7b0a165, tests/t37-model.sh:201, src/store_remote/writer.rs:190, src/db.rs:595.' });
  expect(earlier.length).toBeGreaterThan(0);
  const source = new FixtureSource(fixtures);
  for (const version of earlier) {
    const captured = fixture<Tree[]>('trees').find(tree => tree.state === version.state.id && tree.path === '');
    expect(captured, `${version.version}: encrypted-store root tree`).toBeDefined();
    const tree = await source.tree(project.id, version.state.id);
    expect(tree.state).toBe(version.state.id);
    expect(tree.entries).toEqual(captured!.entries);
    for (const entry of tree.entries.filter(entry => entry.kind === 'file')) {
      const file = await source.file(project.id, version.state.id, entry.path);
      expect(file.state).toBe(version.state.id);
      if (entry.size !== undefined) expect(file.size).toBe(entry.size);
    }
  }
});
