import {epochRoute} from '../src/data/epochs';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

// Exercise the real worker message handler while replacing its two I/O boundaries.
const source = ts.transpileModule(readFileSync(new URL('../src/data/worker.ts', import.meta.url), 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText.replace(/^import .*;\n/gm, '').replace(/^export \{\};\n/gm, '');
const overlay = '01a12290-0000-7000-8000-000000000004';
const epoch = '01a12290-0000-7000-8000-000000000005';
const manifest = `overlays/${overlay}/keys/${epoch}-01a12290-0000-7000-8000-000000000006.age`;
const prefix = `overlays/${overlay}/epochs/${epoch}`;
const slot = `${prefix}/slots/person-reader.age`;

function worker(options: { hidden?: boolean; slot?: boolean; foreign?: boolean; corrupt?: boolean; visible?: boolean }) {
  const calls: string[] = [];
  let opened = false;
  let locked = false;
  let resolve: (message: { id: number; value?: unknown; error?: string }) => void;
  class Session {
    refresh_person() {} sign_session(){return "signature";}
    unlock_recovery() { calls.push('unlock'); }
    slot_name() { return 'person-reader.age'; }
    open_store() { return 'project'; }
    challenge() { return 'proof'; }
    lock() { locked = true; }
    free() {}
    query(_name: string, method: string) {
      if (method === 'pins') return '["project"]';
      if (method === 'projects') return JSON.stringify([{ id: 'project', overlays: opened ? [{ name: 'secret', hidden: true }] : [] }]);
      throw new Error(`unexpected query: ${method}`);
    }
    add_segment(name: string) { calls.push(`segment:${name}`); }
    finish_records(name: string) { calls.push(`finish:${name}`); }
    overlay_slot_opens() { calls.push('slot-opens'); return !options.foreign; }
    open_hidden_overlay() {
      calls.push('open-hidden');
      if (options.corrupt) throw new Error('hidden signature is invalid');
      opened = true;
      return 'store::secret';
    }
    overlay_key() { calls.push('visible-key'); }
    overlay_slots() { return '[]'; }
    overlay_inputs() { return '[]'; }
  }
  class Gateway {
    discovery(){return {};} setEpoch(){}
    unlock(){return this.person();} signIn(){}
    async person() { return { recipient: 'person' }; }
    async projects() { return ['store']; }
    async descriptor() { return { project: 'project' }; }
    async slot() { return new Uint8Array([0]); }
    async list(_name: string, path: string) {
      calls.push(`list:${path}`);
      if (path === 'overlays/') return [
        ...(options.hidden ? [{ key: manifest }] : []),
        ...(options.slot ? [{ key: slot }] : []),
        ...(options.visible ? [{ key: `overlays/${overlay}/keys/${epoch}-record.json` }] : []),
      ];
      if (path.endsWith('/records/')) return [{ key: path + 'machine.age' }];
      return [];
    }
    async object(_name: string, key: string) { calls.push(`object:${key}`); return new Uint8Array([1]); }
    async signedObject(_name: string, key: string) { calls.push(`signed:${key}`); return new Uint8Array([2]); }
    lock() {}
  }
  const scope = { onmessage: (_event: unknown) => {}, postMessage: (message: Parameters<typeof resolve>[0]) => resolve(message), close() {} };
  vm.runInNewContext(source, { self: scope, Session, Gateway, epochRoute, HttpError:class extends Error{}, init: async () => {}, setTimeout: () => 1, clearTimeout() {}, Uint8Array, Date, Error });
  let id = 0;
  const request = (method: string) => new Promise<Parameters<typeof resolve>[0]>(done => {
    resolve = done;
    scope.onmessage({ data: { id: ++id, method, args: method === 'unlockRecovery' ? ['test words'] : [] } });
  });
  return { calls, request, isLocked: () => locked };
}

test('worker discovers hidden age manifests via own person slot and verifies their scoped records', async () => {
  const w = worker({ hidden: true, slot: true });
  assert.equal((await w.request('unlockRecovery')).error, undefined);
  assert.ok(w.calls.indexOf('slot-opens') < w.calls.indexOf(`object:${manifest}`));
  assert.ok(w.calls.indexOf('open-hidden') < w.calls.indexOf('segment:store::secret'));
  assert.ok(w.calls.includes(`object:${prefix}/records/machine.age`));
  assert.ok(w.calls.includes('finish:store::secret'));
  assert.equal(JSON.stringify((await w.request('projects')).value), '[{"id":"project","overlays":[{"name":"secret","hidden":true}]}]');
});

test('worker without a hidden person slot produces the same response as a store without overlays', async () => {
  const absent = worker({});
  const other = worker({ hidden: true });
  assert.equal(JSON.stringify(await other.request('unlockRecovery')), JSON.stringify(await absent.request('unlockRecovery')));
  assert.equal(JSON.stringify(await other.request('projects')), JSON.stringify(await absent.request('projects')));
  assert.ok(!other.calls.some(call => call.startsWith('object:overlays/')));
  assert.ok(!other.calls.includes('open-hidden'));
});

test('a foreign slot is silent and does not fetch or validate its hidden manifest', async () => {
  const absent = worker({});
  const other = worker({ hidden: true, slot: true, foreign: true, corrupt: true });
  assert.equal(JSON.stringify(await other.request('unlockRecovery')), JSON.stringify(await absent.request('unlockRecovery')));
  assert.equal(JSON.stringify(await other.request('projects')), JSON.stringify(await absent.request('projects')));
  assert.ok(other.calls.includes(`object:${slot}`));
  assert.ok(!other.calls.includes(`object:${manifest}`));
  assert.ok(!other.calls.includes('open-hidden'));
});

test('an owned slot with corrupt hidden records fails unlock and clears the session', async () => {
  const w = worker({ hidden: true, slot: true, corrupt: true });
  assert.equal((await w.request('unlockRecovery')).error, 'hidden signature is invalid');
  assert.equal(w.isLocked(), true);
  assert.equal((await w.request('projects')).error, 'Your session is locked.');
});

test('visible manifests continue through the signed plaintext object path', async () => {
  const w = worker({ visible: true });
  assert.equal((await w.request('unlockRecovery')).error, undefined);
  assert.ok(w.calls.includes('visible-key'));
  assert.ok(w.calls.some(call => call.startsWith('signed:overlays/')));
  assert.ok(!w.calls.includes('slot-opens'));
});
