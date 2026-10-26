import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
import {HttpError} from '../src/data/gateway';
import {epochRoute} from '../src/data/epochs';
const code=ts.transpileModule(readFileSync(new URL('../src/data/worker.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace(/^import .*;\n/gm,'').replace(/^export \{\};\n/gm,'');
function worker(mode='ok') {
 let saved:string|null=null, removed=false, writes=0, refreshed=false;
 let receive:(v:{id?:number;value?:unknown;error?:string;updated?:boolean})=>void=()=>{};
 const events:string[]=[];
 class Session {
  unlock_recovery(){} refresh_person(view:string){refreshed=JSON.parse(view).index==='sealed';} slot_name(){return 'person.age'} sign_session(){return 'signature'}
  invite_project(){if(mode==='stale'||mode==='foreign')throw Error(mode);return 'project'}
  prepare_invite_accept(){if(mode==='slot')throw Error('slot mismatch');return '{"data":"sealed","previous":null}'}
  query(_name:string,method:string){return method==='pins'?(refreshed?'["project"]':'[]'):'[]'}
  open_store(){assert(refreshed);events.push('open');return 'project'} finish_records(){if(mode==='records'&&refreshed)throw Error('invalid project records')} close_store(){events.push('close')} overlay_slots(){return '[]'} lock(){} free(){}
 }
 class Gateway {
  unlock(){return {recipient:'person'}}signIn(){}person(){return {index:saved,inbox:removed?[]:[{id:'invite'}]}}projects(){return ['store']}descriptor(){return {project:'project'}}slot(){return new Uint8Array()} discovery(){return {}}setEpoch(){}list(){return []}
  putPersonIndex(){writes++;if(mode==='conflict'){saved='concurrent';throw Error('index changed')}saved='sealed';events.push('write');if(mode==='lost')throw Error('reply lost')}
  removeInvite(){assert(refreshed);removed=true;events.push('delete');if(mode==='lost-delete')throw Error('delete reply lost')}lock(){}
 }
 const scope={onmessage:(_e:unknown)=>{},postMessage:(v:Parameters<typeof receive>[0])=>{if(v.updated)events.push('updated');else receive(v)},close(){}};
 vm.runInNewContext(code,{self:scope,Session,Gateway,epochRoute,HttpError,init:async()=>{},Uint8Array,Date,Error,setTimeout:()=>1,clearTimeout(){}});
 let id=0;
 return {events,get writes(){return writes},get removed(){return removed},call:(method:string)=>new Promise<Parameters<typeof receive>[0]>(resolve=>{receive=resolve;scope.onmessage({data:{id:++id,method,args:method==='unlockRecovery'?['words','accept@example.test']:['invite']}})})};
}
for(const mode of ['ok','lost','lost-delete'])test(`accept ${mode}: persists, reads back, opens and removes invite, then refreshes the page`,async()=>{
 const w=worker(mode);await w.call('unlockRecovery');assert.equal((await w.call('acceptInvite')).value,'project');assert.equal(w.writes,1);assert(w.removed);assert.deepEqual(w.events,['write','open','delete','updated']);
});
for(const mode of ['stale','foreign','slot','conflict','records'])test(`refuses ${mode} without activating the project or deleting the invite`,async()=>{
 const w=worker(mode);await w.call('unlockRecovery');assert((await w.call('acceptInvite')).error);assert(!w.removed);if(mode==='records')assert(w.events.includes('close'));else assert(!w.events.includes('open')); assert.equal(w.writes,['conflict','records'].includes(mode)?1:0);
});
test('locked worker refuses acceptance',async()=>{const w=worker();assert.match((await w.call('acceptInvite')).error!,/locked/);assert.equal(w.writes,0)});
