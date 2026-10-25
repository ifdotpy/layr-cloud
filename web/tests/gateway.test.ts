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
    if (path.startsWith('/api/session/challenge')) return Response.json({challenge:'session challenge'});
    if (path === '/api/session') return Response.json({login:'ann@example.com',token:'person-token',expires:Date.now()+60000});
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
    const gateway = new Gateway(); await gateway.signIn('ann@example.com',()=> 'signature'); let decrypts = 0;
    const decrypt = (bytes: Uint8Array) => { assert.equal(new TextDecoder().decode(bytes), 'ciphertext challenge'); decrypts++; return 'decrypted proof'; };
    const first = await gateway.object('project', 'records/machine/segment.age', decrypt);
    assert.deepEqual([...first], [1, 2, 3]); assert.equal(challenges, 2); assert.equal(decrypts, 2);
    const count = requests.length;
    await gateway.object('project', 'records/machine/segment.age', decrypt);
    assert.equal(requests.length, count, 'a repeated ciphertext object comes from the worker cache');
    gateway.lock();
    await gateway.signIn('ann@example.com',()=> 'signature');
    await gateway.object('project', 'records/machine/segment.age', decrypt);
    assert.equal(challenges, 3, 'lock removes read tokens and ciphertext cache entries');
    assert(requests.every(request => request.init.cache === 'no-store' && request.init.credentials === 'same-origin'));
    assert.equal(requests.filter(request => request.init.method === 'POST' && request.path.startsWith('/v1')).length, 3);
  } finally { globalThis.fetch = previous; }
});

test('person actions carry the worker session and renew once with the identical encrypted envelope', async () => {
  const previous=globalThis.fetch; let sessions=0, puts=0;
  const bodies:string[]=[]; const proofs:string[]=[];
  globalThis.fetch=async(input,init={})=>{
    const path=String(input);
    if(path.startsWith('/api/session/challenge'))return Response.json({challenge:`challenge-${sessions}`});
    if(path==='/api/session'){sessions++;return Response.json({login:'ann@example.com',token:`session-${sessions}`,expires:Date.now()+60000});}
    assert(path.includes('/objects/requests/'));puts++;
    bodies.push(String(init.body));proofs.push(new Headers(init.headers).get('authorization')!);
    return puts===1?Response.json({error:'expired'},{status:401}):new Response(null,{status:201});
  };
  try {
    const gateway=new Gateway(); const signed:string[]=[];
    await gateway.signIn('ann@example.com',challenge=>{signed.push(challenge);return 'signature';});
    await gateway.putRequest('example','id','sealed envelope');
    assert.equal(sessions,2);assert.deepEqual(signed,['challenge-0','challenge-1']);
    assert.deepEqual(bodies,['sealed envelope','sealed envelope']);assert.deepEqual(proofs,['Bearer session-1','Bearer session-2']);
    gateway.lock();await assert.rejects(gateway.person(),/Sign in/);
  }finally{globalThis.fetch=previous;}
});

test('unlock is padded and never exposes the private index or inbox before proof',async()=>{
 const previous=globalThis.fetch;
 const view={login:'ann@example.com',recipient:'age1person',slots:[{name:'paper',data:'sealed'}],signing_key:'key',signing:'sealed signing'};
 globalThis.fetch=async input=>{assert.equal(String(input),'/api/person/unlock?login=ann%40example.com');return Response.json({unlock:btoa(JSON.stringify(view).padEnd(65536,' '))});};
 try{const value=await new Gateway().unlock('ann@example.com');assert.equal(value.login,view.login);assert.equal(value.index,null);assert.deepEqual(value.inbox,[]);
 globalThis.fetch=async()=>Response.json({unlock:btoa('{}')});await assert.rejects(new Gateway().unlock('ann@example.com'),/Invalid/);
 }finally{globalThis.fetch=previous;}
});

test('a second session refusal ends retry and registration uses only the invite bearer',async()=>{
 const previous=globalThis.fetch;let exchanges=0,reads=0;
 globalThis.fetch=async(input,init={})=>{
  const path=String(input);
  if(path==='/api/person?login=ann%40example.com'){assert.equal(new Headers(init.headers).get('authorization'),'Bearer invite');assert.equal(init.body,'sealed person');return Response.json({registered:true});}
  if(path.startsWith('/api/session/challenge'))return Response.json({challenge:'challenge'});
  if(path==='/api/session'){exchanges++;return Response.json({login:'ann@example.com',token:'token',expires:Date.now()+60000});}
  reads++;return Response.json({error:'expired'},{status:401});
 };
 try{const gateway=new Gateway();await gateway.register('ann@example.com','invite','sealed person');await gateway.signIn('ann@example.com',()=> 'signature');await assert.rejects(gateway.person(),/expired/);assert.equal(exchanges,2);assert.equal(reads,2);}
 finally{globalThis.fetch=previous;}
});

test('locking an in-flight exchange cannot restore its session',async()=>{
 const previous=globalThis.fetch;let release:(value:Response)=>void=()=>{};
 globalThis.fetch=async(input)=>String(input).startsWith('/api/session/challenge')?Response.json({challenge:'challenge'}):new Promise<Response>(resolve=>{release=resolve;});
 try{const gateway=new Gateway();const login=gateway.signIn('ann@example.com',()=> 'signature');await new Promise(resolve=>setTimeout(resolve,0));gateway.lock();release(Response.json({login:'ann@example.com',token:'secret',expires:Date.now()+60000}));await assert.rejects(login,/locked/);await assert.rejects(gateway.person(),/Sign in/);}
 finally{globalThis.fetch=previous;}
});
