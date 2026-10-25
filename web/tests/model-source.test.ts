import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ModelSource } from '../src/data/model-source';

class WorkerStub {
  onmessage?: (event: { data: unknown }) => void;
  onerror?: (event: { message: string }) => void;
  messages: { id: number; method: string; args: unknown[] }[] = [];
  terminated = false;
  postMessage(message: { id: number; method: string; args: unknown[] }) { this.messages.push(message); }
  terminate() { this.terminated = true; }
  reply(id: number, value: unknown) { this.onmessage?.({ data: { id, value } }); }
}
const events = new EventTarget();
Object.assign(globalThis, {
  addEventListener: events.addEventListener.bind(events),
  removeEventListener: events.removeEventListener.bind(events),
});

test('lock terminates the worker, rejects pending queries, and notifies the page', async () => {
  const workers: WorkerStub[] = []; let locked = 0;
  const source = new ModelSource({ worker: () => { const worker = new WorkerStub(); workers.push(worker); return worker as unknown as Worker; }, onLock: () => locked++ });
  const first = source.projects();
  workers[0].reply(workers[0].messages[0].id, [{ id: 'p', name: 'project' }]);
  assert.deepEqual(await first, [{ id: 'p', name: 'project' }]);
  const pending = source.lines('p');
  const rejected = assert.rejects(pending, /locked/);
  await source.lock(); await rejected;
  assert.equal(workers[0].terminated, true); assert.equal(locked, 1);
  const next = source.projects();
  assert.equal(workers.length, 2);
  workers[1].reply(workers[1].messages[0].id, []); await next;
  events.dispatchEvent(new Event('pagehide'));
  assert.equal(workers[1].terminated, true); assert.equal(locked, 2);
  source.dispose();
});

test('worker idle notification removes the worker and clears the page state', async () => {
  const worker = new WorkerStub(); let locked = false;
  const source = new ModelSource({ worker: () => worker as unknown as Worker, onLock: () => { locked = true; } });
  const pending = source.projects(); const rejected = assert.rejects(pending, /locked/);
  worker.onmessage?.({ data: { locked: true } });
  await rejected; assert.equal(worker.terminated, true); assert.equal(locked, true);
  source.dispose();
});

test('page activity keeps an open session active without a model query', async () => {
  const worker = new WorkerStub();
  const source = new ModelSource({ worker: () => worker as unknown as Worker });
  events.dispatchEvent(new Event('scroll'));
  assert.equal(worker.messages.length, 0, 'activity must not open a locked worker');
  const unlock = source.unlockRecovery('disposable test words','ann@example.com');
  worker.reply(worker.messages[0].id, undefined); await unlock;
  events.dispatchEvent(new Event('scroll'));
  assert.equal(worker.messages.at(-1)?.method, 'touch');
  const count = worker.messages.length;
  events.dispatchEvent(new Event('pointerdown'));
  assert.equal(worker.messages.length, count, 'activity messages are limited to one each 15 seconds');
  worker.reply(worker.messages.at(-1)!.id, undefined);
  source.dispose();
  events.dispatchEvent(new Event('keydown'));
  assert.equal(worker.messages.length, count);
});
