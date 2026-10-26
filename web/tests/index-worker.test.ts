import {epochRoute} from '../src/data/epochs';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('../src/data/worker.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace(/^import .*;\n/gm,'').replace(/^export \{\};\n/gm,'');
const machine='01a12290-0000-7000-8000-000000000002';
const first=`records/${machine}/00000000000000000001-00000000000000000002.age`;
const second=`records/${machine}/00000000000000000003-00000000000000000004.age`;
const head=`index/heads/${machine}/00000000000000000001.age`;
function worker(tamper:boolean|'laterTrust'=false) {
  const calls:string[]=[];let provisional=false;let finishes=0;let release:()=>void=()=>{};
  const reconcile=new Promise<void>(resolve=>release=resolve);
  let onReply:(message:{id?:number;value?:unknown;error?:string;updated?:boolean})=>void=()=>{};
  class Session {
    refresh_person() {} sign_session(){return "signature";}
    unlock_recovery(){} slot_name(){return 'person.age'} open_store(){return 'project'} challenge(){return 'proof'} lock(){} free(){}
    query(_name:string,method:string){
      if(method==='pins')return '["project"]';if(method==='projects')return '[{"id":"project"}]';
      if(method==='loadStatus')return JSON.stringify({provisional});if(method==='lines')return '[{"name":"main"}]';throw Error(method);
    }
    add_segment(_name:string,key:string){calls.push(key)}
    finish_records(){finishes++;if(finishes>1)provisional=false;calls.push('finish')}
    add_heads(){calls.push('heads');if(tamper===true || (tamper==='laterTrust'&&!calls.includes(second)))throw Error('untrusted signer')}
    use_heads(){provisional=true;return true} discard_index(){provisional=false;calls.push('discard')}
    overlay_slots(){return '[]'}overlay_inputs(){return '[]'}
  }
  class Gateway {
    discovery(){return {};} setEpoch(){}
    unlock(){return this.person();} signIn(){}
    async person(){return {recipient:'person'}}async projects(){return ['store']}async descriptor(){return {project:'project'}}async slot(){return new Uint8Array()}
    async list(_name:string,prefix:string){if(prefix==='index/heads/')return [{key:head}];if(prefix==='records/')return [{key:first},{key:second}];return []}
    async object(_name:string,key:string){if(key===second&&!tamper)await reconcile;return new Uint8Array()}
    lock(){}
  }
  const scope={onmessage:(_event:unknown)=>{},postMessage:(message:Parameters<typeof onReply>[0])=>onReply(message),close(){}};
  vm.runInNewContext(source,{self:scope,Session,Gateway,epochRoute,HttpError:class extends Error{},init:async()=>{},setTimeout:(fn:()=>void,ms:number)=>ms===0?setTimeout(fn,0):undefined,clearTimeout,Uint8Array,Date,Error});
  let id=0;
  function request(method:string,args:unknown[]=[]){return new Promise<Parameters<typeof onReply>[0]>(resolve=>{const wanted=++id;onReply=message=>{if(message.id===wanted)resolve(message)};scope.onmessage({data:{id:wanted,method,args}})})}
  return {calls,request,release,finishes:()=>finishes,updated:()=>new Promise<void>(resolve=>onReply=message=>{if(message.updated)resolve()})};
}
test('index first page returns before background records and then publishes verified update',async()=>{
  const w=worker();assert.equal((await w.request('unlockRecovery',['words'])).error,undefined);
  assert.equal(w.finishes(),1);assert.deepEqual(w.calls,[first,'finish','heads']);
  assert.equal((await w.request('loadStatus',['project'])).value && ((await w.request('loadStatus',['project'])).value as {provisional:boolean}).provisional,true);
  assert.equal(JSON.stringify((await w.request('lines',['project'])).value),JSON.stringify([{name:'main'}]));
  const updated=w.updated();w.release();await updated;assert.equal(w.finishes(),2);
  assert.equal(((await w.request('loadStatus',['project'])).value as {provisional:boolean}).provisional,false);await w.request('lock');
});
test('untrusted snapshot discards index and completes full verified loading before unlock',async()=>{
  const w=worker(true);assert.equal((await w.request('unlockRecovery',['words'])).error,undefined);
  assert.equal(w.finishes(),2);assert(w.calls.includes('discard'));assert(w.calls.includes(second));
  assert.equal(((await w.request('loadStatus',['project'])).value as {provisional:boolean}).provisional,false);await w.request('lock');
});

test('a later creator trust record authorizes the executor index before any discard',async()=>{
  const w=worker('laterTrust');assert.equal((await w.request('unlockRecovery',['words'])).error,undefined);
  assert.equal(w.finishes(),3);assert(w.calls.includes(second));assert(!w.calls.includes('discard'));
  assert.equal(w.calls.filter(call=>call==='heads').length,2);
  assert.equal(((await w.request('loadStatus',['project'])).value as {provisional:boolean}).provisional,false);
  await w.request('lock');
});
