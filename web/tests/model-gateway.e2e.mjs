/** Real CLI store -> MinIO -> layr-gateway -> worker -> ModelSource -> page. */
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdtemp, cp, readFile, writeFile, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';
import { createServer as createVite } from 'vite';
import { observePersistence, expectNoPersistence } from './storage.ts';

const root = fileURLToPath(new URL('../../', import.meta.url));
const storeInput = process.env.LAYR_WEB_TEST_STORE;
const paperInput = process.env.LAYR_WEB_TEST_PAPER;
const linesInput = process.env.LAYR_WEB_TEST_LINES;
assert(storeInput && paperInput && linesInput, 'Set LAYR_WEB_TEST_STORE, LAYR_WEB_TEST_PAPER, and LAYR_WEB_TEST_LINES from the real t37 export.');
const home = await mkdtemp(path.join(tmpdir(), 'layr-web-gateway-'));
const runId = randomUUID();
const vmHome = `/root/work/layr-web-${runId}`;
const vmPort = 20000 + randomBytes(2).readUInt16BE() % 20000;
const access = `web${randomBytes(8).toString('hex')}`;
const secret = randomBytes(24).toString('hex');
const children = [];
let minio, whois, gateway, vite, browser;
const logs = [];
function start(command, args, options = {}) {
  const child = spawn(command, args, { ...options, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout?.on('data', bytes => logs.push(bytes.toString()));
  child.stderr?.on('data', bytes => logs.push(bytes.toString()));
  children.push(child); return child;
}
async function listen(server) { await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); }); return server.address().port; }
async function waitFor(url, child) {
  const end = Date.now() + 30000;
  let last = 'no response';
  while (Date.now() < end) {
    if (child?.exitCode != null) throw new Error(`Server exit ${child.exitCode}`);
    try { const response = await fetch(url, {signal: AbortSignal.timeout(2000)}); if (response.ok) return; last = `HTTP ${response.status}: ${(await response.text()).slice(0,120)}`; } catch (error) { last = error.cause?.code || error.message; }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Server timeout: ${new URL(url).pathname}: ${last}`);
}
try {
  const store = path.join(home, 'store'); const person = path.join(home, 'person');
  await cp(storeInput, store, { recursive: true });
  execFileSync(path.join(root, 'target/debug/examples/prepare_gateway'), [store, paperInput, person], { stdio: 'pipe' });
  await cp(linesInput, path.join(person, 'lines.json'));
  const descriptor = JSON.parse(await readFile(path.join(store, 'store.json'), 'utf8'));
  const words = (await readFile(path.join(person, 'recovery.txt'), 'utf8')).trim();
  const lines = JSON.parse(await readFile(linesInput, 'utf8'));
  execFileSync('orb', ['-m', 'layr-dev', '-u', 'root', 'mkdir', '-m', '700', vmHome]);
  // The PID file belongs to this run. Cleanup sends a signal only to that process.
  const launch = `echo $$ > '${vmHome}/pid'\nexec env MINIO_ROOT_USER='${access}' MINIO_ROOT_PASSWORD='${secret}' /root/work/remote-store-tools/minio server '${vmHome}/data' --address '127.0.0.1:${vmPort}' --console-address '127.0.0.1:0' --quiet`;
  minio = start('orb', ['-m', 'layr-dev', '-u', 'root', 'sh', '-c', launch]);
  const reserveMinio = createServer(); const minioPort = await listen(reserveMinio);
  await new Promise(resolve => reserveMinio.close(resolve));
  const tunnel = start('ssh', ['-o', 'BatchMode=yes', '-o', 'ExitOnForwardFailure=yes', '-N', '-L', `127.0.0.1:${minioPort}:127.0.0.1:${vmPort}`, 'root@layr-dev@orb']);
  const endpoint = `http://127.0.0.1:${minioPort}`;
  await waitFor(`${endpoint}/minio/health/ready`, tunnel);
  const env = { ...process.env, AWS_ACCESS_KEY_ID: access, AWS_SECRET_ACCESS_KEY: secret, AWS_ENDPOINT_URL_S3: endpoint };
  const bucket = `layr-web-${runId}`;
  try { execFileSync(path.join(root, 'target/debug/examples/upload_gateway'), [bucket, store, path.join(person, 'people')], { env, stdio: 'pipe' }); }
  catch (error) { throw new Error(`Fixture upload: ${error.stderr?.toString().replaceAll(access, '[test access]').replaceAll(secret, '[test secret]').trim() || error.message}`); }
  whois = createServer(socket => {
    let request = '';
    socket.on('data', bytes => {
      request += bytes;
      if (!request.includes('\r\n\r\n')) return;
      assert(request.startsWith('GET /localapi/v0/whois?addr='));
      const body = JSON.stringify({ Node: {}, UserProfile: { LoginName: 'owner@example.com' } });
      socket.end(`HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\nConnection: close\r\n\r\n${body}`);
    });
  });
  const socketPath = path.join(home, 'whois.sock');
  await new Promise((resolve, reject) => { whois.once('error', reject); whois.listen(socketPath, resolve); });
  const reserve = createServer(); const port = await listen(reserve); await new Promise(resolve => reserve.close(resolve));
  const role = 'arn:aws:iam::123456789012:role/web-test';
  const config = { host: 'localhost', listen: `127.0.0.1:${port}`, projects: { example: { bucket, region: 'us-east-1', prefix: 'fixture', readerRoleArn: role, writerRoleArn: role, gcRoleArn: role, recipient: descriptor.recipient, writeTokenHashes: [], gcTokenHashes: [] } }, people: { bucket, region: 'us-east-1', prefix: 'people/' } };
  const configPath = path.join(home, 'gateway.json'); await writeFile(configPath, JSON.stringify(config), { mode: 0o600 });
  gateway = start(path.join(root, 'target/debug/layr-gateway'), ['serve', '--config', configPath, '--tls', 'none', '--test-static-credentials', '--tailscale-socket', socketPath], { env });
  await waitFor(`http://localhost:${port}/api/person`, gateway);
  vite = await createVite({ configFile: false, root: path.join(root, 'web'), server: { host: '127.0.0.1', port: 0, proxy: { '/api': { target: `http://localhost:${port}`, changeOrigin: true, headers: { origin: `http://localhost:${port}` } }, '/v1': { target: `http://localhost:${port}`, changeOrigin: true, headers: { origin: `http://localhost:${port}` } } } }, worker: { format: 'es' } });
  await vite.listen(); const site = vite.resolvedUrls.local[0];
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage(); const errors = []; const writes = []; const dataRequests = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (request.postData()) writes.push(request.postData()); if (request.url().includes('.data.age')) dataRequests.push(request.url()); });
  await observePersistence(page);
  await page.goto(site);
  await page.getByLabel('24 recovery words').fill(words);
  await page.getByRole('button', { name: 'Unlock with recovery words', exact: true }).click();
  await page.locator('#main-content').getByRole('heading', { name: 'Projects', exact: true }).waitFor({ timeout: 30000 });
  await page.goto(`${site}#/project/${descriptor.project}/lines`);
  await page.getByRole('heading', { name: 'Lines', exact: true }).waitFor();
  for (const line of lines) await page.getByRole('link', { name: line.name, exact: true }).first().waitFor();
  assert.equal(dataRequests.length, 0, 'The lines page loads no file data parts.');
  const content = await page.evaluate(async words => {
    const { ModelSource } = await import('/src/data/model-source.ts');
    const { loadChangeIndex } = await import('/src/data/change-index.ts');
    const source = new ModelSource();
    try {
      await source.unlockRecovery(words);
      const [project] = await source.projects();
      const lines = await source.lines(project.id);
      const base = lines.find(line => line.name === 'main').head;
      const head = lines.find(line => line.name === 'feature').head;
      const summary = await source.diff(project.id, base, head);
      const selected = await source.diff(project.id, base, head, 'test.txt');
      const file = await source.file(project.id, base, 'README.md');
      const index = await loadChangeIndex(source, project);
      return { summary, selected, file, open: index.open, changes: index.changes.length };
    } finally { await source.lock(); }
  }, words);
  assert(content.summary.files.some(file => file.path === 'test.txt'));
  assert(content.summary.files.every(file => file.before === undefined && file.after === undefined && file.added === undefined));
  assert.equal(content.selected.files[0].before, 'main bytes\n');
  assert.equal(content.selected.files[0].after, 'feature bytes\n');
  assert.equal(content.selected.files[0].added, 1); assert.equal(content.selected.files[0].removed, 1);
  assert.equal(content.file.text, '# Browser test\nThese bytes come from the CLI store.\n');
  await page.waitForFunction(expected => document.querySelector('[aria-label="Open changes"]')?.textContent === String(expected), content.open);
  assert.equal(content.changes, 3, 'The CLI fixture has three recorded changes.');
  assert.equal(content.open, 1, 'Only the feature change is open; two states are already in main.');
  console.log(`PASS header Changes: ${content.open} open of ${content.changes} recorded changes`);
  const gap = await page.getByRole('button', {name:'New line',exact:true}).evaluate(button => button.nextElementSibling.getBoundingClientRect().left - button.getBoundingClientRect().right);
  assert.equal(gap, 8, 'The disabled action and CLI chip use the normal 8px gap.');
  if (process.env.LAYR_WEB_TEST_SCREENSHOT) await page.screenshot({path: process.env.LAYR_WEB_TEST_SCREENSHOT, fullPage: true});
  const rows = await page.evaluate(async () => {
    const { ModelSource } = await import('/src/data/model-source.ts');
    // A separate locked worker cannot reuse the unlocked page's person identity.
    const locked = new ModelSource();
    try { await locked.projects(); return false; } catch { return true; } finally { await locked.lock(); }
  });
  assert(rows, 'A new worker must start locked');
  assert.deepEqual(await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage), cookies: document.cookie })), { local: [], session: [], cookies: '' });
  await expectNoPersistence(page, 'real recovery unlock');
  assert(writes.every(body => !body.includes(words) && !body.includes('AGE-SECRET-KEY-')), 'Unlock material must not reach the gateway');
  await page.getByRole('button', { name: /Account/ }).click();
  await page.getByRole('button', { name: 'Lock', exact: true }).click();
  await page.getByRole('heading', { name: 'Open your person key' }).waitFor();
  assert.equal(await page.getByRole('heading', { name: 'Lines', exact: true }).count(), 0);
  assert.deepEqual(errors, []);
  console.log(`PASS ModelSource: real CLI store, ${lines.length} lines, MinIO, gateway whois and read proof, recovery unlock, worker isolation, lock, no persistent secrets`);
} catch (error) {
  // Server logs contain no keys. Suppress disposable MinIO credentials if its startup fails.
  const tail = logs.join('').slice(-6000).replaceAll(access, '[test access]').replaceAll(secret, '[test secret]');
  if (tail) console.error(tail);
  throw error;
} finally {
  await browser?.close(); await vite?.close();
  for (const child of children) child.kill('SIGTERM');
  try {
    execFileSync('orb', ['-m', 'layr-dev', '-u', 'root', 'sh', '-c', `if test -f '${vmHome}/pid'; then pid=$(cat '${vmHome}/pid'); if test -r /proc/$pid/cmdline && tr '\\0' ' ' < /proc/$pid/cmdline | grep -Fq '${vmHome}/data'; then kill -TERM "$pid" 2>/dev/null || true; fi; fi`], { stdio: 'ignore' });
  } catch {}
  await new Promise(resolve => whois ? whois.close(resolve) : resolve());
  await rm(home, { recursive: true, force: true });
  try { execFileSync('orb', ['-m', 'layr-dev', '-u', 'root', 'rm', '-rf', vmHome], { stdio: 'ignore' }); } catch {}
}
