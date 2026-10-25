import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
import {uuidV7} from '../src/data/requests';
const code=ts.transpileModule(readFileSync(new URL('../src/data/worker.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace(/^import .*;\n/gm,'').replace(/^export \{\};\n/gm,'');
function setup(mode:'lost-success'|'offline') {
  const writes:{id:string;envelope:string}[]=[];
  const opened:Uint8Array[]=[];
  let online=false, locked=false;
  let received:(value:{id:number;value?:unknown;error?:string})=>void;
  class Session {
    refresh_person() {} sign_session(){return "signature";}
    unlock_recovery() {} slot_name(){return 'person.age';} open_store(){return 'project';}
    open_signing_key(bytes:Uint8Array){opened.push(bytes);}
    add_segment(){} finish_records(){} overlay_slots(){return '[]';} overlay_inputs(){return '[]';}
    sign_request(_name:string,id:string,action:string){assert(!locked);return JSON.stringify({id,ct:'ciphertext',action:JSON.parse(action)});}
    query(_name:string,method:string){if(method==='pins')return '["project"]';if(method==='requestStatus')return JSON.stringify({id:writes[0].id,status:'pending'});throw Error(method);}
    challenge(){return 'proof';} lock(){locked=true;} free(){}
  }
  class Gateway {
    unlock(){return this.person();} signIn(){}
    person(){return {recipient:'person',signing:'sealed',signing_key:'public'};}
    projects(){return ['store'];} descriptor(){return {project:'project'};} slot(){return new Uint8Array();}
    list(){return [];} lock(){}
    async putRequest(_project:string,id:string,envelope:string){writes.push({id,envelope});if(!online)throw Error('lost response');}
    async requestObject(){if(mode==='offline'&&!online)throw Error('offline');return writes[0].envelope;}
  }
  const scope={onmessage:(_event:unknown)=>{},postMessage:(value:Parameters<typeof received>[0])=>received(value),close(){}};
  vm.runInNewContext(code,{self:scope,Session,Gateway,HttpError:class extends Error{},init:async()=>{},uuidV7,fromBase64:()=>new Uint8Array([1,2,3]),setTimeout:()=>1,clearTimeout(){},Uint8Array,Date,Error});
  let id=0;
  return {writes,opened,online:()=>{online=true;},call:(method:string,args:unknown[]=[])=>new Promise<Parameters<typeof received>[0]>(resolve=>{received=resolve;scope.onmessage({data:{id:++id,method,args}});})};
}
test('lost success reads back the same envelope and keeps one request identity',async()=>{
  const worker=setup('lost-success');await worker.call('unlockRecovery',['test words']);
  assert.deepEqual([...worker.opened[0]],[0,0,0],'sealed key buffer is cleared after the WASM call');
  const response=await worker.call('submitRequest',['project',{kind:'approve',state:'state',into:'main'}]);
  assert.equal(response.error,undefined);assert.equal((response.value as {status:string}).status,'pending');
  assert.equal((response.value as {reason?:string}).reason,undefined);assert.equal(worker.writes.length,1);
});
test('uncertain delivery retries the exact envelope and lock discards its retry material',async()=>{
  const worker=setup('offline');await worker.call('unlockRecovery',['test words']);
  const response=await worker.call('submitRequest',['project',{kind:'comment',text:'review'}]);
  const request=response.value as {id:string;reason:string};assert.match(request.reason,/could not be confirmed/);
  worker.online();await worker.call('retryRequest',['project',request.id]);
  assert.equal(worker.writes.length,2);assert.deepEqual(worker.writes[0],worker.writes[1]);
  await worker.call('lock');
  assert.match((await worker.call('retryRequest',['project',request.id])).error!,/locked/);
});
