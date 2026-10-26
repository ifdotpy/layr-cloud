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

test('epoch read proofs and requests use the current namespace and renewal clears the old proof',async()=>{
 const previous=globalThis.fetch;const paths:string[]=[];let exchanges=0;
 globalThis.fetch=async(input)=>{
  const path=String(input);paths.push(path);
  if(path.startsWith('/api/session/challenge'))return Response.json({challenge:'session'});
  if(path==='/api/session')return Response.json({login:'ann@example.com',token:'person',expires:Date.now()+60000});
  if(path.includes('/challenge'))return Response.json({challenge:btoa('sealed')});
  if(path.includes('/token')){exchanges++;return Response.json({token:'read',expires:Date.now()+60000});}
  return new Response(new Uint8Array([1]),{status:200});
 };
 try{
  const gateway=new Gateway();await gateway.signIn('ann@example.com',()=> 'signature');
  const epoch='01a12290-0000-7000-8000-000000000008';gateway.setEpoch('p',epoch);
  await gateway.object('p',`project/epochs/${epoch}/records/a.age`,()=> 'proof');
  await gateway.putRequest('p','id','encrypted');
  assert(paths.includes(`/v1/p/p/challenge?epoch=${epoch}`));
  assert(paths.includes(`/v1/p/p/token?epoch=${epoch}`));
  assert(paths.includes(`/v1/p/p/objects/project/epochs/${epoch}/requests/id.json`));
  gateway.setEpoch('p','root');await gateway.object('p','records/b.age',()=> 'root proof');
  assert.equal(exchanges,2);assert(paths.includes('/v1/p/p/challenge'));
 }finally{globalThis.fetch=previous;}
});

test('acceptance index and inbox writes use the person session and preserve the CAS body',async()=>{
 const previous=globalThis.fetch;const calls:{path:string;method?:string;body?:BodyInit|null}[]=[];
 globalThis.fetch=async(input,init={})=>{
  const path=String(input);
  if(path.startsWith('/api/session/challenge'))return Response.json({challenge:'session'});
  if(path==='/api/session')return Response.json({login:'ann@example.com',token:'person',expires:Date.now()+60000});
  assert.equal(new Headers(init.headers).get('authorization'),'Bearer person');
  assert.equal(new Headers(init.headers).get('content-type'),'application/json');
  calls.push({path,method:init.method,body:init.body});return Response.json({saved:true});
 };
 try{
  const gateway=new Gateway();await gateway.signIn('ann@example.com',()=> 'signature');
  const body=JSON.stringify({data:'ciphertext',previous:'sha256'});
  await gateway.putPersonIndex(body);await gateway.removeInvite('1793010000000-00000001');
  assert.deepEqual(calls,[{path:'/api/person/index',method:'PUT',body},{path:'/api/person/inbox/1793010000000-00000001',method:'DELETE',body:undefined}]);
  gateway.lock();await assert.rejects(gateway.putPersonIndex(body),/Sign in/);
 }finally{globalThis.fetch=previous;}
});

test('CDN reads carry no gateway authority and retry verification directly once', async () => {
  const previous=globalThis.fetch, key='files/01a177aa-a7a5-716c-8774-7f165724c16b.age';
  for (const mode of ['corrupt','blocked','foreign','direct-corrupt']) {
    let links=0, cdn=0, direct=0, verifies=0;
    globalThis.fetch=async(input,init={})=>{
      const path=String(input), headers=new Headers(init.headers);
      if(path.startsWith('/api/session/challenge'))return Response.json({challenge:'person'});
      if(path==='/api/session')return Response.json({login:'ann@example.com',token:'person-token',expires:Date.now()+60000});
      if(path.endsWith('/challenge'))return Response.json({challenge:btoa('cipher')});
      if(path.endsWith('/token'))return Response.json({token:'read-token',expires:Date.now()+60000});
      if(path.startsWith('https://')) {
        cdn++;assert.equal(headers.get('authorization'),null);assert.equal(init.credentials,'omit');assert.equal(init.redirect,'error');assert.equal(init.referrerPolicy,'no-referrer');
        if(mode==='blocked')return new Response('blocked',{status:403});
        return new Response(new Uint8Array([0,0,0]));
      }
      if(path==='/v1/p/project/cdn') {
        assert.equal(init.method,'POST');assert.equal(headers.get('authorization'),'Bearer read-token');
        assert.deepEqual(JSON.parse(String(init.body)),{epoch:'root',keys:[key]});
        links++;const expires=Math.floor(Date.now()/1000)+300,host=mode==='foreign'?'other.example':'layr.example';
        return Response.json({objects:[{status:200,url:`https://${host}/o/project/${key}?Expires=${expires}&Signature=abc&Key-Pair-Id=K123&Hash-Algorithm=SHA256`,expires,bytes:3}]});
      }
      assert.equal(path,`/v1/p/project/objects/${key}`);assert.equal(headers.get('authorization'),'Bearer read-token');
      if(headers.get('x-layr-cdn')==='1') {
        links++;const expires=Math.floor(Date.now()/1000)+300;
        const host=mode==='foreign'?'other.example':'layr.example';
        return Response.json({url:`https://${host}/o/project/${key}?Expires=${expires}&Signature=abc&Key-Pair-Id=K123&Hash-Algorithm=SHA256`,expires,bytes:3},{headers:{'content-type':'application/vnd.layr.cdn+json'}});
      }
      direct++;return new Response(new Uint8Array(mode==='direct-corrupt'?[0,0,0]:[1,2,3]));
    };
    try {
      const gateway=new Gateway();await gateway.signIn('ann@example.com',()=> 'signature');
      const result=gateway.verifiedObjects('project',[key],()=> 'proof',bodies=>{verifies++;assert.deepEqual([...bodies[0]],[1,2,3]);return 'verified';});
      if(mode==='direct-corrupt')await assert.rejects(result);else assert.equal(await result,'verified');
      assert.equal(links,1);assert.equal(direct,1);assert.equal(cdn,mode==='foreign'?0:1);
      assert.equal(verifies,mode==='corrupt'||mode==='direct-corrupt'?2:1);
    } finally {globalThis.fetch=previous;}
  }
});

test('CDN object batches use at most eight parallel reads and preserve caller order', async()=>{
  const previous=globalThis.fetch;let active=0,peak=0,exchanges=0;
  globalThis.fetch=async(input)=>{
    const path=String(input);
    if(path.startsWith('/api/session/challenge'))return Response.json({challenge:'person'});
    if(path==='/api/session')return Response.json({login:'ann@example.com',token:'person',expires:Date.now()+60000});
    if(path.endsWith('/challenge'))return Response.json({challenge:btoa('cipher')});
    if(path.endsWith('/token')){exchanges++;return Response.json({token:'read',expires:Date.now()+60000});}
    active++;peak=Math.max(peak,active);await new Promise(resolve=>setImmediate(resolve));active--;
    return new Response(new Uint8Array([Number(path.split('/').at(-1)!.split('.')[0])]));
  };
  try {
    const gateway=new Gateway();await gateway.signIn('ann@example.com',()=> 'signature');
    const result=await gateway.verifiedObjects('project',Array.from({length:19},(_,i)=>`records/${i}.age`),()=> 'proof',bodies=>bodies.map(body=>body[0]));
    assert.deepEqual(result,Array.from({length:19},(_,i)=>i));assert.equal(peak,8);assert.equal(active,0);assert.equal(exchanges,1);
  }finally{globalThis.fetch=previous;}
});

test('busy responses retry the identical envelope and stop without another burst',async()=>{
 const previous=globalThis.fetch,random=Math.random;let calls=0;
 Math.random=()=>0;
 globalThis.fetch=async(input,init={})=>{
  assert.equal(String(input),'/api/person?login=ann%40example.com');
  assert.equal(init.method,'PUT');assert.equal(init.body,'same encrypted registration');
  calls++;return Response.json({error:'gateway is busy; retry later'},{status:503,headers:{'Retry-After':'0'}});
 };
 try {const gateway=new Gateway();await assert.rejects(gateway.register('ann@example.com','invite','same encrypted registration'),/busy/);assert.equal(calls,4);}
 finally {globalThis.fetch=previous;Math.random=random;}
});

test('batch signing uses 256-key requests and eight CDN reads with one proof',async()=>{
 const previous=globalThis.fetch;let batches:number[]=[],active=0,peak=0,proofs=0;
 const keys=Array.from({length:300},(_,i)=>`files/01a177aa-a7a5-716c-8774-${i.toString(16).padStart(12,'0')}.age`),positions=new Map(keys.map((key,i)=>[key,i]));
 globalThis.fetch=async(input,init={})=>{
  const path=String(input),headers=new Headers(init.headers);
  if(path.startsWith('/api/session/challenge'))return Response.json({challenge:'person'});
  if(path==='/api/session')return Response.json({login:'ann@example.com',token:'person',expires:Date.now()+60000});
  if(path.endsWith('/challenge'))return Response.json({challenge:btoa('cipher')});
  if(path.endsWith('/token')){proofs++;return Response.json({token:'read',expires:Date.now()+60000});}
  if(path==='/v1/p/project/cdn'){
   assert.equal(init.method,'POST');assert.equal(headers.get('authorization'),'Bearer read');
   const body=JSON.parse(String(init.body));assert.equal(body.epoch,'root');assert.ok(body.keys.length<=256);batches.push(body.keys.length);
   const expires=Math.floor(Date.now()/1000)+300;
   return Response.json({objects:body.keys.map((key:string)=>({status:200,bytes:1,expires,url:`https://layr.example/o/project/${key}?Expires=${expires}&Signature=abc&Key-Pair-Id=K123&Hash-Algorithm=SHA256`}))});
  }
  assert.ok(path.startsWith('https://layr.example/o/project/'));assert.equal(headers.get('authorization'),null);
  active++;peak=Math.max(peak,active);await new Promise(resolve=>setImmediate(resolve));active--;
  const key=new URL(path).pathname.slice('/o/project/'.length);return new Response(new Uint8Array([positions.get(key)!%256]));
 };
 try {
  const gateway=new Gateway();await gateway.signIn('ann@example.com',()=> 'signature');
  const result=await gateway.verifiedObjects('project',keys,()=> 'proof',bodies=>bodies.map(body=>body[0]));
  assert.deepEqual(result,keys.map((_,i)=>i%256));assert.deepEqual(batches,[256,44]);assert.equal(proofs,1);assert.equal(peak,8);
 }finally{globalThis.fetch=previous;}
});
