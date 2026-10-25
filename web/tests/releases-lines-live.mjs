/** Real CLI-created refs and lines, through the gateway and verified model worker. */
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { views } from './views.ts';
import { measureContrast } from './contrast.ts';

export async function checkReleasesAndLines(page, site, project, fixture) {
  const load = async name => JSON.parse(await readFile(path.join(fixture, name + '.json'), 'utf8'));
  const [before, after, refs, lines] = await Promise.all(['release-1-log', 'release-2-log', 'refs', 'lines'].map(load));
  const attestations = await load('release-attestations').catch(error => { if (error.code === 'ENOENT') return []; throw error; });
  const checkCount = attestations.filter(attestation => attestation.kind === 'check').length;
  const base = refs.find(ref => ref.name === 'release/0.1.0');
  const release = refs.find(ref => ref.name === 'release/0.2.0');
  const beforeIds = new Set(before.map(state => state.id));
  const notes = [...new Map(after.filter(state => state.change_id && !beforeIds.has(state.id)).map(state => [state.change_id, state])).values()];
  const output = process.env.LAYR_WEB_TEST_SCREENSHOT_DIR ?? new URL('../test-results/live-m5/', import.meta.url).pathname;
  await mkdir(output, { recursive: true });
  const main = page.locator('main');
  const route = suffix => `${site}#/project/${project}/${suffix}`;
  const capture = async name => {
    for (const view of views) {
      await page.setViewportSize({ width: view.width, height: view.height });
      await page.emulateMedia({ colorScheme: view.colorScheme, reducedMotion: 'reduce' });
      const violations = (await new AxeBuilder({ page }).analyze()).violations;
      assert.deepEqual(violations.map(({id, nodes}) => ({id, targets: nodes.map(node => node.target)})), [], `${name} ${view.name} axe`);
      const latest = main.locator('.badge.blue').first();
      if (await latest.count()) {
        assert((await measureContrast(latest)).ratio >= 4.5, 'Latest badge must meet AA contrast');
        const background = (await measureContrast(latest)).backgroundRgb;
        await latest.hover();
        assert.equal((await measureContrast(latest)).backgroundRgb, background, 'Badge fill stays opaque on row hover');
        await page.mouse.move(0, 0);
      }
      await page.screenshot({ path: path.join(output, `${name}-${view.name}.png`), fullPage: true, animations: 'disabled' });
    }
  };
  await page.goto(route('deploy/releases'));
  await expect(main.getByRole('heading', { name: 'Releases', exact: true })).toBeVisible();
  const table = main.getByRole('table', { name: 'Releases', exact: true });
  await expect(table.locator('tbody tr')).toHaveCount(2);
  const latest = table.locator('tbody tr').first();
  await expect(latest).toContainText(release.name);
  await expect(latest.getByRole('cell').nth(1)).toHaveText(String(notes.length));
  await expect(latest).toContainText('env/prod');
  await expect(latest.getByRole('cell').nth(2)).toHaveText(checkCount ? `${checkCount} of ${checkCount} passed` : 'No checks recorded');
  await expect(main.getByRole('button', { name: 'New release', exact: true })).toBeDisabled();
  await expect(main.locator('.readonly-note')).toHaveCount(1);
  await capture('releases');
  await main.getByRole('link', { name: release.name, exact: true }).click();
  await expect(main.getByRole('heading', { level: 1 })).toContainText(release.name);
  await expect(main.getByRole('table', { name: 'Release notes', exact: true }).locator('tbody tr')).toHaveCount(notes.length);
  for (const state of notes) await expect(main).toContainText(state.message);
  await expect(main.getByRole('table', { name: 'Release checks', exact: true }).locator('tbody tr')).toHaveCount(checkCount);
  if (!checkCount) await expect(main).toContainText('No checks recorded for this state.');
  else await expect(main.getByRole('table', { name: 'Release checks', exact: true }).locator('tbody tr').first().getByRole('cell').first()).toHaveText('release-smoke');
  await capture('release');
  await main.getByRole('button', { name: `Compare files with ${base.name}`, exact: true }).click();
  await expect(main.getByRole('complementary', { name: 'Compared files' })).toContainText('test.txt');
  await expect(main.locator('.diffs')).toContainText('feature bytes');
  await capture('release-files');
  // Exercise real filter memberships and collapsible prefix groups.
  await page.goto(route('lines'));
  await expect(main.getByRole('heading', { name: 'Lines', exact: true })).toBeVisible();
  for (const filter of ['All', 'Yours', 'Agents', 'Stale']) {
    await main.getByRole('group', { name: 'Line filters' }).getByRole('button', { name: filter, exact: true }).click();
    const groups = main.locator('.line-group-toggle');
    if (filter === 'All') {
      await expect(main.getByRole('table', { name: 'Lines', exact: true }).locator('tbody tr').first()).toContainText('main');
      for (const button of await groups.all()) {
        await expect(button).toHaveAttribute('aria-expanded', 'false');
        await button.click();
      }
    } else for (const button of await groups.all()) await expect(button).toHaveAttribute('aria-expanded', 'true');
    const visibleNames = main.getByRole('table', { name: 'Lines', exact: true }).locator('tbody tr:not(.line-group) td:first-child a');
    const expected = filter === 'All' ? lines : filter === 'Agents' ? lines.filter(line => line.name.startsWith('agent-')) : [];
    await expect(visibleNames).toHaveCount(expected.length);
    for (const line of expected) await expect(main.locator(`a[href$="line=${encodeURIComponent(line.name)}"]`)).toHaveText(line.name);
    await expect(main.getByRole('columnheader', { name: 'Cache', exact: true })).toHaveCount(0);
    if (filter === 'Stale') await expect(main).toContainText('No save for 21 days.');
    if (filter === 'All') for (const button of await groups.all()) await button.click();
    await capture(`lines-${filter.toLowerCase()}`);
  }
  await main.getByRole('group', { name: 'Line filters' }).getByRole('button', { name: 'All', exact: true }).click();
  const exp = main.getByRole('button', { name: /exp-… \(2 lines\)/ });
  await expect(exp).toHaveAttribute('aria-expanded', 'false');
  await expect(main.getByTitle('exp-one', { exact: true })).toHaveCount(0);
  await exp.click();
  await expect(main.getByTitle('exp-one', { exact: true })).toBeVisible();
  await main.getByRole('searchbox', { name: 'Filter lines' }).fill('exp-one');
  await expect(main.locator('.line-group-toggle')).toHaveCount(0);
  await expect(main.getByRole('table', { name: 'Lines', exact: true }).locator('tbody tr')).toHaveCount(1);
  await main.getByRole('searchbox', { name: 'Filter lines' }).fill('');
  await expect(main.locator('.line-group-toggle')).toHaveCount(2);
  for (const button of await main.locator('.line-group-toggle').all()) await expect(button).toHaveAttribute('aria-expanded', 'false');
  await main.getByRole('searchbox', { name: 'Filter lines' }).fill('exp-');
  await expect(exp).toHaveAttribute('aria-expanded', 'true');
  await expect(main.getByTitle('exp-one', { exact: true })).toHaveText('exp-one');
  await main.getByRole('searchbox', { name: 'Filter lines' }).fill('');
  // Advance only the page clock to test the age boundary on the real signed records.
  await page.clock.setFixedTime(new Date(Date.now() + 22 * 24 * 60 * 60 * 1000));
  await main.getByRole('group', { name: 'Line filters' }).getByRole('button', { name: 'Stale', exact: true }).click();
  for (const button of await main.locator('.line-group-toggle').all()) if (await button.getAttribute('aria-expanded') === 'false') await button.click();
  await expect(main.getByRole('table', { name: 'Lines', exact: true }).locator('tbody tr:not(.line-group)')).toHaveCount(lines.length);
  await expect(main.locator('tbody .cmd')).toHaveCount(lines.length);
  await page.clock.setFixedTime(new Date());
  await main.getByRole('group', { name: 'Line filters' }).getByRole('button', { name: 'All', exact: true }).click();
  await page.setViewportSize({ width: 1280, height: 900 });
  console.log(`PASS M5 live: 2 releases, ${notes.length} release changes, ${checkCount} exact-state checks, file compare, ${lines.length} lines, 4 filters, hyphen groups and singleton search, 21-day age boundary; screenshots ${output}`);
}
