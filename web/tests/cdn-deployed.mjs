/** Read one disposable full-history project from the published HTTPS site, with a blocked-CDN variant. */
import assert from 'node:assert/strict';
import {mkdir, readFile, stat, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {resolve4} from 'node:dns/promises';
import {chromium, expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import {observePersistence, expectNoPersistence} from './storage.ts';
import {views} from './views.ts';

assert.equal(process.env.LAYR_WEB_LIVE_DISPOSABLE, '1');
const fixture = JSON.parse(await readFile(process.env.LAYR_WEB_LIVE_FIXTURE, 'utf8'));
assert.equal(fixture.login, 'cdn-studio-7d619c@example.test');
assert.equal(fixture.alias, 'cdn-studio');
assert.equal(fixture.project, '01a17979-d63f-7058-b6d5-f663339ec328');
assert.equal(fixture.words_file, '/Volumes/LayrCdnRead/fixture/person/recovery.txt');
const words = (await readFile(fixture.words_file, 'utf8')).trim();
const output = path.resolve(process.env.LAYR_WEB_LIVE_OUTPUT);
await mkdir(output, {recursive: true});
assert.equal(await stat(path.join(output, 'result.json')).catch(() => null), null);
const blocked = process.env.LAYR_WEB_LIVE_BLOCK_CDN === '1';
const origin = 'https://cloud.layr.ws';
const ipv4 = (await resolve4('cloud.layr.ws'))[0];
const browser = await chromium.launch({headless: true, args: [`--host-resolver-rules=MAP cloud.layr.ws ${ipv4}`]});
const context = await browser.newContext();
const page = await context.newPage();
const errors = [];
const stats = {cdnAttempts: 0, cdnResponses: 0, cdnHits: 0, cdnMisses: 0, directObjectReads: 0, sessionExchanges: 0, authorityLeaks: 0};
const pending = new Set();
function observe(promise) {
  const observed = promise.catch(error => errors.push(error.message));
  pending.add(observed);
  observed.finally(() => pending.delete(observed));
}
try {
  await observePersistence(page);
  page.on('pageerror', error => errors.push(error.message));
  await context.route(`${origin}/o/**`, async route => {
    stats.cdnAttempts++;
    const headers = route.request().headers();
    if (['authorization', 'cookie', 'x-layr-cdn', 'x-layr-origin'].some(name => name in headers)) stats.authorityLeaks++;
    if (blocked) await route.abort('blockedbyclient');
    else await route.continue();
  });
  context.on('request', request => {
    const url = new URL(request.url());
    if (request.method() === 'GET' && url.pathname.startsWith('/v1/p/cdn-studio/objects/') && !request.headers()['x-layr-cdn']) stats.directObjectReads++;
  });
  context.on('response', response => {
    observe((async () => {
      const url = new URL(response.url());
      if (url.pathname.startsWith('/o/')) {
        assert.equal(response.status(), 200);
        stats.cdnResponses++;
        const cache = await response.headerValue('x-cache');
        if (cache === 'Hit from cloudfront') stats.cdnHits++;
        if (cache === 'Miss from cloudfront') stats.cdnMisses++;
      }
      if (url.pathname === '/api/session' && response.request().method() === 'POST' && response.status() === 200) stats.sessionExchanges++;
    })());
  });
  const started = Date.now();
  await page.goto(origin);
  await page.getByLabel('Login', {exact: true}).fill(fixture.login);
  await page.getByLabel('24 recovery words', {exact: true}).fill(words);
  await page.getByRole('button', {name: 'Unlock with recovery words', exact: true}).click();
  await expect(page.locator('#main-content').getByRole('heading', {name: 'Projects', exact: true})).toBeVisible({timeout: 180000});
  const file = 'workspaces/runtime/packages/runtime-http/src/routes/linux_vm_settings.rs';
  await page.goto(`${origin}/#/project/${fixture.project}/code?line=main&path=${encodeURIComponent(file)}`);
  await expect(page.locator('#main-content pre.code').first()).toContainText('Linux VM resource settings routes.', {timeout: 600000});
  await expect(page.locator('#main-content pre.code').first()).toContainText('studio_runtime_process::blocking_pool::spawn_blocking');
  await expect(page.getByRole('alert')).toHaveCount(0);
  const loadSeconds = (Date.now() - started) / 1000;
  for (const view of views) {
    await page.setViewportSize({width: view.width, height: view.height});
    await page.emulateMedia({colorScheme: view.colorScheme});
    const axe = await new AxeBuilder({page}).analyze();
    assert.deepEqual(axe.violations, []);
    await writeFile(path.join(output, `${view.name}-axe.json`), JSON.stringify(axe, null, 2));
    await page.screenshot({path: path.join(output, `${view.name}.png`), fullPage: true});
  }
  await expectNoPersistence(page, 'full-history project read');
  await page.getByRole('button', {name: /Account/}).click();
  await page.getByRole('button', {name: 'Lock', exact: true}).click();
  await expect(page.getByRole('heading', {name: 'Sign in', exact: true})).toBeVisible();
  await expect(page.getByLabel('24 recovery words', {exact: true})).toHaveValue('');
  await expectNoPersistence(page, 'locked full-history reader');
  await Promise.all([...pending]);
  assert.equal(stats.authorityLeaks, 0);
  assert(stats.cdnAttempts > 0, 'The published client must attempt immutable CDN reads');
  if (blocked) {
    assert.equal(stats.cdnResponses, 0);
    assert(stats.directObjectReads > 0);
  } else assert(stats.cdnResponses > 0);
  assert.deepEqual(errors, []);
  const result = {status: 'PASS', transport: 'Mac Chromium over public IPv4 HTTPS CloudFront', publishedSite: true, blockedCdn: blocked, project: fixture.project, fixtureGitCommits: 7965, loadSeconds, file, screenshots: views.length, axeViolations: 0, persistentWrites: 0, locked: true, ...stats};
  await writeFile(path.join(output, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} catch (error) {
  await writeFile(path.join(output, 'failure.json'), JSON.stringify({blockedCdn: blocked, error: String(error), ...stats}, null, 2));
  throw error;
} finally {
  await context.close();
  await browser.close();
}
