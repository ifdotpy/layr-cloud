import { expect, test } from '@playwright/test';
import { FixtureSource } from '../src/data/fixture-source';
import type { Change, Project, Tree } from '../src/data/source';
import { fixture, fixtures } from './fixture-data';

test('the t37 store retains every earlier change version tree', async () => {
  const project = fixture<Project[]>('projects')[0];
  const change = fixture<Change>('change');
  const earlier = change.versions.slice(0, -1);
  // The b1a23c1 recipe checks all version trees before and after remote GC.
  test.info().annotations.push({ type: 'provenance', description: 'Native fixture b1a23c100aa1e0b941afc424c14eb155295cbf50, tests/t37-model.sh, all version trees checked before and after remote GC.' });
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
