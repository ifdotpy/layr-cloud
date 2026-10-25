import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Gateway } from '../src/data/gateway';
Object.defineProperty(globalThis, 'location', { value: { origin: 'https://layr.example' }, configurable: true });

test('gateway refuses another origin and refreshes a refused read token once', async () => {
  assert.throws(() => new Gateway('https://other.example'), /origin/);
  const requests: { path: string; init: RequestInit }[] = [];
  let challenges = 0; let gets = 0;
  const previous = globalThis.fetch;
  globalThis.fetch = async (input, init = {}) => {
    const path = String(input); requests.push({ path, init });
    if (path.endsWith('/challenge')) { challenges++; return Response.json({ challenge: btoa('ciphertext challenge') }); }
    if (path.endsWith('/token')) {
      assert.deepEqual(JSON.parse(init.body as string), { challenge: 'decrypted proof' });
      return Response.json({ token: `token-${challenges}`, expires: Date.now() + 60000 });
    }
    gets++;
    if (gets === 1) return Response.json({ error: 'expired' }, { status: 401 });
    return new Response(new Uint8Array([1, 2, 3]));
  };
  try {
    const gateway = new Gateway(); let decrypts = 0;
    const decrypt = (bytes: Uint8Array) => { assert.equal(new TextDecoder().decode(bytes), 'ciphertext challenge'); decrypts++; return 'decrypted proof'; };
    const first = await gateway.object('project', 'records/machine/segment.age', decrypt);
    assert.deepEqual([...first], [1, 2, 3]); assert.equal(challenges, 2); assert.equal(decrypts, 2);
    const count = requests.length;
    await gateway.object('project', 'records/machine/segment.age', decrypt);
    assert.equal(requests.length, count, 'a repeated ciphertext object comes from the worker cache');
    gateway.lock();
    await gateway.object('project', 'records/machine/segment.age', decrypt);
    assert.equal(challenges, 3, 'lock removes read tokens and ciphertext cache entries');
    assert(requests.every(request => request.init.cache === 'no-store' && request.init.credentials === 'same-origin'));
    assert.equal(requests.filter(request => request.init.method === 'POST').length, 3);
  } finally { globalThis.fetch = previous; }
});

test('person request delivery uses its existing identity and never asks for a machine bearer', async () => {
  const previous = globalThis.fetch;
  const calls: {path:string;init:RequestInit}[]=[];
  globalThis.fetch = async (input,init={}) => { calls.push({path:String(input),init}); return new Response(null,{status:201}); };
  try {
    const gateway=new Gateway();
    const id='01a1734c-0000-7000-8000-000000000001';
    await gateway.putRequest('example',id,'signed envelope');
    assert.equal(calls.length,1);
    assert.equal(calls[0].path,`/v1/p/example/objects/requests/${id}.json`);
    assert.equal(calls[0].init.method,'PUT');
    assert.equal(calls[0].init.body,'signed envelope');
    assert.equal(new Headers(calls[0].init.headers).get('authorization'),null);
    assert.equal(calls[0].init.credentials,'same-origin');
  } finally { globalThis.fetch=previous; }
});
