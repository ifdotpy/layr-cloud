import {epochRoute} from '../src/data/epochs';
import assert from 'node:assert/strict';
import {test} from 'node:test';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import {HttpError} from '../src/data/gateway';
const code=ts.transpileModule(readFileSync(new URL('../src/data/worker.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace(/^import .*;\n/gm,'').replace(/^export \{\};\n/gm,'');
function setup(){
 const calls:string[]=[];const buffers:Uint8Array[]=[];const bodies:string[]=[];let signing=false,refused=false;let receive:(v:{id?:number;value?:unknown;error?:string;locked?:boolean})=>void=()=>{};
 class Session{
  unlock_recovery(){calls.push('open person');} open_signing_key(){signing=true;calls.push('open signing');}
  sign_session(challenge:string){assert(signing);calls.push(`sign ${challenge}`);return 'proof';}
  refresh_person(){calls.push('private index');}slot_name(){return 'person.age';}query(_name:string,method:string){return method==='pins'?'["project"]':'[]';}
  open_store(){return 'project';}add_segment(){}finish_records(){}overlay_slots(){return '[]';}overlay_inputs(){return '[]';}
  prepare_person(_login:string,entropy:Uint8Array){buffers.push(entropy);signing=true;return JSON.stringify({words:'recovery words',recipient:'recipient',paper:'sealed paper',signing_key:'public',signing:'sealed signing'});}
  sign_request(){return 'sealed request';}lock(){signing=false;calls.push('lock');}free(){}
 }
 class Gateway{
  discovery(){return {};} setEpoch(){}
  unlock(login:string){calls.push(`bootstrap ${login}`);return {login,recipient:'recipient',signing:'sealed',signing_key:'public'};}
  signIn(login:string,sign:(s:string)=>string){if(refused)throw new HttpError(401,'wrong proof');calls.push(`session ${login}`);assert.equal(sign('challenge'),'proof');}
  person(){calls.push('person');return {login:'ann@example.com'};}projects(){if(refused)throw new HttpError(401,'sign in again');calls.push('projects');return ['store'];}descriptor(){return {project:'project'};}slot(){return new Uint8Array();}list(){return [];}
  register(_login:string,invite:string,body:string){bodies.push(body);refused=invite==='invalid';if(refused)throw new HttpError(401,'invalid invite');throw new Error('reply lost');}
  putRequest(){if(refused)throw new HttpError(401,'sign in again');}lock(){}
 }
 const scope={onmessage:(_event:unknown)=>{},postMessage:(v:Parameters<typeof receive>[0])=>receive(v),close(){}};
 vm.runInNewContext(code,{self:scope,Session,Gateway,epochRoute,HttpError,init:async()=>{},fromBase64:()=>new Uint8Array([1]),crypto:{getRandomValues:(v:Uint8Array)=>{v.fill(37);return v;}},uuidV7:()=> 'id',Uint8Array,Date,Error,setTimeout:()=>1,clearTimeout(){}});
 let id=0;
 return{calls,buffers,bodies,refuse:()=>{refused=true;},call:(method:string,args:unknown[]=[])=>new Promise<Parameters<typeof receive>[0]>(resolve=>{receive=v=>{if(v.id)resolve(v)};scope.onmessage({data:{id:++id,method,args}});})};
}
test('worker signs only after opening the person key, then loads authenticated private data',async()=>{
 const w=setup();assert.equal((await w.call('unlockRecovery',['words','ann@example.com'])).error,undefined);
 assert.deepEqual(w.calls,['bootstrap ann@example.com','open person','open signing','session ann@example.com','sign challenge','person','private index','projects']);
});
test('registration returns only recovery words, clears entropy and recovers a lost registration reply with the same key',async()=>{
 const w=setup();const prepared=await w.call('prepareRegistration',['ann@example.com']);assert.equal(JSON.stringify(prepared.value),JSON.stringify({words:'recovery words'}));assert(w.buffers[0].every(b=>b===0));
 assert.equal((await w.call('registerPerson',['invite'])).error,undefined);assert.equal(w.bodies.length,1);assert(!w.bodies[0].includes('recovery words'));assert(w.calls.includes('private index'));
 await w.call('lock');assert.match((await w.call('registerPerson',['invite'])).error!,/locked/);
});

test('an invalid invite keeps the prepared key for an authorized retry',async()=>{
 const w=setup();await w.call('prepareRegistration',['ann@example.com']);assert.match((await w.call('registerPerson',['invalid'])).error!,/invalid invite/);assert.equal((await w.call('registerPerson',['valid'])).error,undefined);assert.equal(w.bodies[0],w.bodies[1]);
});

test('a final person-session refusal clears the worker key and requires sign-in',async()=>{
 const w=setup();await w.call('unlockRecovery',['words','ann@example.com']);w.refuse();assert.match((await w.call('submitRequest',['project',{kind:'approve',state:'state',into:'main'}])).error!,/sign in again/);assert.equal(w.calls.at(-1),'lock');assert.match((await w.call('projects')).error!,/locked/);
});
