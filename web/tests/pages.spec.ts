import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { openChangeCount, pages } from './pages';
import { fixture } from './fixture-data';
import type { Project } from '../src/data/source';
import { expectNoPersistence, observePersistence } from './storage';
import { views } from './views';
import { measureContrast } from './contrast';

for (const route of pages) {
  test(`${route.name} renders fixture data without browser or accessibility errors`, async ({ page }, testInfo) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.goto(`/?source=fixture#${route.path}`);
    await expect(page.locator('main')).toBeVisible();
    if (route.heading) await expect(page.locator('main h1')).toContainText(route.heading);
    await route.check(page);
    if (route.path.startsWith('/project/')) {
      await expect(page.getByLabel('Open changes', { exact: true })).toHaveText(String(openChangeCount));
    }
    await expect(page.getByRole('alert')).toHaveCount(0);
    const readOnlyNotes = page.locator('main .readonly-note');
    expect(await readOnlyNotes.count()).toBeLessThanOrEqual(1);
    if (await page.locator('main .readonly-action').count()) {
      await expect(readOnlyNotes).toHaveCount(1);
      await expect(readOnlyNotes).toHaveText('Read only here. Use the CLI command.');
      await expect(page.locator('main .pagehead, main .chead').first().locator('.readonly-note')).toHaveCount(1);
    }


    for (const view of views) {
      await test.step(view.name, async () => {
        await page.setViewportSize({ width: view.width, height: view.height });
        await page.emulateMedia({ colorScheme: view.colorScheme, reducedMotion: 'reduce' });
        expect(await page.evaluate(() => matchMedia('(prefers-color-scheme: dark)').matches))
          .toBe(view.colorScheme === 'dark');
        if (route.name === 'Lines') {
          const action = page.locator('main .readonly-action').filter({ has: page.getByRole('button', { name: 'New line', exact: true }) });
          const button = await action.getByRole('button', { name: 'New line', exact: true }).boundingBox();
          const command = await action.locator('.cmd').boundingBox();
          expect(button).not.toBeNull();
          expect(command).not.toBeNull();
          const gap = Math.max(command!.x - button!.x - button!.width, command!.y - button!.y - button!.height);
          expect(gap, `New line command gap (${view.name})`).toBeGreaterThanOrEqual(8);
        }
        if (route.name === 'Checks behavior' && view.colorScheme === 'dark') {
          const primary = page.locator('main').getByRole('button', { name: 'Compare', exact: true });
          const measurements = [];
          for (const state of ['normal', 'hover']) {
            if (state === 'normal') await page.mouse.move(0, 0);
            else await primary.hover();
            await primary.evaluate(element => { for (const animation of element.getAnimations()) animation.finish(); });
            const contrast = await measureContrast(primary);
            measurements.push({ state, ...contrast });
            expect.soft(contrast.ratio, `dark primary button (${state}) contrast`).toBeGreaterThanOrEqual(4.5);
          }
          await testInfo.attach('primary-dark-contrast', { body: JSON.stringify(measurements, null, 2), contentType: 'application/json' });
          await page.mouse.move(0, 0);
        }
        if (route.name === 'Overlay') {
          const contrast = await measureContrast(page.locator('main .badge.ok').first());
          await testInfo.attach(`overlay-access-contrast-${view.name}`, { body: JSON.stringify(contrast, null, 2), contentType: 'application/json' });
          expect.soft(contrast.ratio, `overlay access badge (${view.name}) contrast`).toBeGreaterThanOrEqual(4.5);
        }
        if (route.name === 'Activity') {
          const badge = page.locator('main tbody .badge.ok').first();
          await page.mouse.move(0, 0);
          const normal = await measureContrast(badge);
          await badge.hover();
          const hover = await measureContrast(badge);
          expect(hover.backgroundRgb, 'badge background must not change with its row').toBe(normal.backgroundRgb);
          expect.soft(normal.ratio).toBeGreaterThanOrEqual(4.5);
          expect.soft(hover.ratio).toBeGreaterThanOrEqual(4.5);
          await testInfo.attach(`badge-row-contrast-${view.name}`, { body: JSON.stringify({normal, hover}, null, 2), contentType: 'application/json' });
          await page.mouse.move(0, 0);
          await badge.evaluate(element => { const frame = element.closest('.tbl-frame'); if (frame) frame.scrollLeft = 0; });
        }
        const screenshot = testInfo.outputPath(`${view.name}.png`);
        await page.screenshot({
          path: screenshot,
          fullPage: true,
          animations: 'disabled',
        });
        await testInfo.attach(view.name, { path: screenshot, contentType: 'image/png' });
        const accessibility = await new AxeBuilder({ page }).analyze();
        await testInfo.attach(`axe-${view.name}`, {
          body: JSON.stringify(accessibility.violations, null, 2), contentType: 'application/json',
        });
        const violations = accessibility.violations.map(({ id, impact, nodes }) => ({
          rule: id, impact, targets: nodes.map(node => node.target),
        }));
        expect.soft(violations, `${route.name}: ${view.name} accessibility violations`).toEqual([]);
      });
    }
    expect(errors, `${route.name}: browser errors`).toEqual([]);
  });
}

test('all fixture pages keep secrets out of browser storage and cookies', async ({ page }) => {
  test.setTimeout(60_000);
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await observePersistence(page);
  for (const route of pages) {
    await test.step(route.name, async () => {
      await page.goto(`/?source=fixture#${route.path}`);
      await expect(page.locator('main')).toBeVisible();
      if (route.heading) await expect(page.locator('main h1')).toContainText(route.heading);
      await expect(page.locator('main [role="status"]')).toHaveCount(0);
      await expectNoPersistence(page, route.name);
    });
  }
  await test.step('recovery input', async () => {
    await page.goto('/');
    await expect(page.locator('main h1')).toHaveText('Open your person key');
    const recovery = page.getByRole('textbox', { name: '24 recovery words' });
    const fakeSecret = Array.from({ length: 24 }, (_, index) => `test-secret-${index}`).join(' ');
    await recovery.fill(fakeSecret);
    await expectNoPersistence(page, 'recovery input before submit');
    await page.getByRole('button', { name: 'Unlock with recovery words', exact: true }).click();
    await expect(page.getByRole('alert')).toBeVisible();
    await expect(recovery).toHaveValue('');
    await expectNoPersistence(page, 'recovery input after rejection');
  });
  expect(errors, 'storage checks must not hide browser errors').toEqual([]);
});

test('the persistence observer detects transient writes in all four stores', async ({ page }) => {
  await observePersistence(page);
  await page.goto('/?source=fixture#/projects');
  await expect(page.locator('main')).toBeVisible();
  const writes = await page.evaluate(`(async () => {
    localStorage.setItem('observer-probe', 'test');
    localStorage.removeItem('observer-probe');
    sessionStorage.setItem('observer-probe', 'test');
    sessionStorage.removeItem('observer-probe');
    document.cookie = 'observer-probe=test; SameSite=Strict; path=/';
    document.cookie = 'observer-probe=; Max-Age=0; path=/';
    const database = await new Promise((resolve, reject) => {
      const request = indexedDB.open('observer-probe', 1);
      request.onupgradeneeded = () => request.result.createObjectStore('probe');
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    await new Promise((resolve, reject) => {
      const transaction = database.transaction('probe', 'readwrite');
      const store = transaction.objectStore('probe');
      store.add('test', 'first');
      store.put('test', 'second');
      transaction.oncomplete = resolve;
      transaction.onerror = () => reject(transaction.error);
    });
    database.close();
    await new Promise((resolve, reject) => {
      const request = indexedDB.deleteDatabase('observer-probe');
      request.onsuccess = resolve;
      request.onerror = () => reject(request.error);
    });
    const writes = [...window.__persistenceWrites];
    window.__persistenceWrites = [];
    return writes;
  })()`);
  expect(writes).toEqual([
    'localStorage.setItem', 'sessionStorage.setItem', 'cookie', 'cookie', 'IndexedDB.add', 'IndexedDB.put',
  ]);
  await expectNoPersistence(page, 'observer probe after cleanup');
});

test('account, clone and state dialogs support their controls and Escape', async ({ page }) => {
  const project = fixture<Project[]>('projects')[0];
  const lines = pages.find(route => route.name === 'Lines')!;
  await page.goto(`/?source=fixture#${lines.path}`);
  await lines.check(page);
  const account = page.getByRole('button', { name: /^Account/ });
  await account.click();
  await page.getByRole('button', { name: 'Show CLI commands', exact: true }).click();
  await expect(page.locator('html')).toHaveClass(/\bcli\b/);
  await expect(page.locator('main .cmd .t').first()).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(account).toHaveAttribute('aria-expanded', 'false');

  await page.getByRole('button', { name: 'Clone', exact: true }).click();
  const clone = page.getByRole('dialog', { name: `Clone ${project.name}`, exact: true });
  await expect(clone).toBeVisible();
  for (const name of ['Clone', 'Mount', 'Git']) {
    const tab = clone.getByRole('tab', { name, exact: true });
    await tab.click();
    await expect(tab).toHaveAttribute('aria-selected', 'true');
    await expect(clone.getByRole('tabpanel', { name, exact: true })).toBeVisible();
  }
  await page.keyboard.press('Escape');
  await expect(clone).not.toBeVisible();

  const states = pages.find(route => route.name === 'States')!;
  await page.goto(`/?source=fixture#${states.path}`);
  await states.check(page);
  await page.getByRole('button', { name: 'Find the state that broke it', exact: true }).click();
  const bisect = page.getByRole('dialog', { name: 'Find the state that broke it', exact: true });
  await expect(bisect).toBeVisible();
  const runs = fixture<import("../src/data/source").BisectResult[]>("bisect", []);
  if (runs.length) {
    await expect(bisect.locator('tbody tr')).toHaveCount(runs.reduce((n, run) => n + run.steps.length, 0));
  } else await expect(bisect).toContainText('No bisect results recorded. Use the CLI to search exact states.');
  await page.keyboard.press('Escape');
  await expect(bisect).not.toBeVisible();
});

test('stored screen controls preserve pixels, regions and escaped structure', async ({ page }) => {
  const route = pages.find(route => route.name === 'Change behavior screens')!;
  await page.goto(`/?source=fixture#${route.path}`);
  await route.check(page);
  const modes = page.getByRole('group', {name:'UI diff view'});
  await modes.getByRole('button', {name:'Swipe',exact:true}).click();
  const swipe = page.getByRole('slider', {name:'Swipe position'});
  await swipe.fill('25');
  await expect(page.locator('.vd-stack img.vd-top')).toHaveAttribute('style', /25%/);
  await modes.getByRole('button', {name:'Onion skin',exact:true}).click();
  await page.getByRole('slider', {name:'Head opacity'}).fill('75');
  await expect(page.locator('.vd-stack img.vd-top')).toHaveAttribute('style', /0.75/);
  await modes.getByRole('button', {name:'Difference',exact:true}).click();
  await expect(page.getByRole('img', {name:'1 changed pixel region, 0 changed nodes',exact:true})).toBeVisible();
  await expect(page.locator('.diff-regions rect')).toHaveCount(1);
  await page.getByRole('button', {name:'Native@Phone',exact:true}).click();
  await expect(page.locator('main')).toContainText('No PNG captured. Open Structure to review this screen.');
  await modes.getByRole('button', {name:'Structure',exact:true}).click();
  await expect(page.getByRole('region', {name:'Structure diff of Native@Phone',exact:true})).toContainText('+<node text="110">');
  await expect(page.locator('main iframe, main .structure-diff node')).toHaveCount(0);
  await page.getByRole('button', {name:'Stable@Phone',exact:true}).click();
  await expect(page.locator('main')).toContainText('No structure changes.');
  await expect(page.locator('main .structure-diff')).toHaveCount(0);
});
