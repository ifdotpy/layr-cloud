import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
import {epochRoute} from '../src/data/epochs';
const source=ts.transpileModule(readFileSync(new URL('../src/data/worker.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace(/^import .*;\n/gm,'').replace(/^export \{\};\n/gm,'');
const machine='01a12290-0000-7000-8000-000000000002';
const epoch='01a12290-0000-7000-8000-000000000008';
const record='01a12290-0000-7000-8000-000000000009';
const state='01a12290-0000-7000-8000-000000000010';
const oldState='01a12290-0000-7000-8000-000000000011';
const prefix=`project/epochs/${epoch}/`;
const segment=`records/${machine}/00000000000000000001-00000000000000000009.age`;
const tree=`states/${state}/12345678.tree.age`, oldTree=`states/${oldState}/87654321.tree.age`;
const part=`states/${state}/12345678.0000.data.age`, oldPart=`states/${oldState}/87654321.0000.data.age`;
const releaseObject='files/01a12290-0000-7000-8000-000000000021.age', oldRelease='files/01a12290-0000-7000-8000-000000000022.age';
const root={format:2,project:'project',recipient:'root recipient'};
const current={...root,recipient:'new recipient'};
const discovery={[`keys/${epoch}-${record}.json`]:JSON.stringify({r:{id:record,op:'project.key',project:root.project,data:{project:root.project,epoch,parent:'root',parent_recipient:root.recipient,recipient:current.recipient}}})};
class HttpError extends Error {constructor(public status:number){super(`HTTP ${status}`)}}
function worker(options:{removed?:boolean;incomplete?:boolean}={}){
 const calls:string[]=[];let locked=false;
 let resolve:(message:{id?:number;value?:unknown;error?:string})=>void=()=>{};
 class Session {
  unlock_recovery(){} refresh_person(){} sign_session(){return 'signature'} slot_name(){return 'person-reader.age'}
  open_store(){throw Error('old epoch fallback')}
  open_epoch_store(){calls.push('open-current');return 'project'}
  open_public_epoch_store(){calls.push('open-current-public');return 'project'}
  add_epoch_history(_name:string,id:string){calls.push(`history:${id}`)}
  add_root_segment(_name:string,object:string){calls.push(`root:${object}`)}
  add_segment(_name:string,object:string){calls.push(`segment:${object}`)}
  finish_records(){calls.push('finish');if(options.incomplete)throw Error('rotation in progress')}
  challenge(){calls.push('current-proof');return 'proof'}
  overlay_slots(){return '[]'} overlay_inputs(){return '[]'}
  query(_name:string,method:string,args='[]'){if(method==='releaseFiles')return JSON.stringify([{name:'layr',size:3,object:JSON.parse(args)[0]==='release/old'?oldRelease:releaseObject}]);if(method==='pins')return '["project"]';if(method==='projects')return '[{"id":"project"}]';if(method==='layers')return '[]';throw Error(method)}
  release_file(_name:string,_reference:string,_filename:string,object:string){calls.push(`release:${object}`);return new Uint8Array([1,2,3]);}
  tree_base(_name:string,object:string){calls.push(`base:${object}`);return JSON.stringify(object===prefix+tree?oldTree:null)}
  install_tree(_name:string,object:string){calls.push(`tree:${object}`)}
  file_location(name:string,object:string){return JSON.stringify({name,object})}
  file_parts(){return JSON.stringify([oldPart,part])}
  add_part(_name:string,object:string){calls.push(`part:${object}`)}
  file(){return '{"text":"joined history"}'}
  sign_request(){return JSON.stringify({v:2,epoch,ct:'sealed action'})}
  lock(){locked=true} free(){}
 }
 class Gateway {
  selected='root';
  unlock(){return {recipient:'person',login:'reader@example.test'}} person(){return this.unlock()} signIn(){}
  projects(){return ['store']} discovery(){calls.push('discovery');return discovery}
  setEpoch(_name:string,id:string){this.selected=id;calls.push(`epoch:${id}`)}
  descriptor(_name:string,scope=''){calls.push(`descriptor:${scope}`);return scope?current:root}
  publicDescriptor(name:string,scope=''){return this.descriptor(name,scope)}
  publicManifest(){return new Uint8Array()}
  slot(_name:string,_slot:string,scope=''){calls.push(`slot:${scope}`);if(options.removed)throw new HttpError(404);return new Uint8Array()}
  async list(_name:string,path:string,proof?: (bytes:Uint8Array)=>string){
   proof?.(new Uint8Array());calls.push(`list:${path}`);
   if(path==='records/')return [{key:segment}];
   if(path===prefix+'records/')return [{key:prefix+segment}];
   if(path===prefix+`states/${state}/`)return [{key:prefix+tree}];
   return [];
  }
  publicList(name:string,path:string){return this.list(name,path)}
  async object(_name:string,path:string,proof?: (bytes:Uint8Array)=>string){
   proof?.(new Uint8Array());calls.push(`object:${path}`);
   if(path===prefix+oldTree || path===prefix+oldPart || path===prefix+oldRelease)throw new HttpError(404);
   return new Uint8Array();
  }
  releaseObject(name:string,path:string,maximum:number,proof?: (bytes:Uint8Array)=>string){assert(maximum<5000);return this.object(name,path,proof);}
  publicObject(name:string,path:string){return this.object(name,path)}
  putRequest(_name:string,_id:string,envelope:string){assert.equal(this.selected,epoch);assert.equal(JSON.parse(envelope).epoch,epoch);calls.push('request:current')}
  lock(){}
 }
 const scope={onmessage:(_event:unknown)=>{},postMessage:(message:Parameters<typeof resolve>[0])=>resolve(message),close(){}};
 vm.runInNewContext(source,{self:scope,Session,Gateway,epochRoute,HttpError,uuidV7:()=>record,init:async()=>{},setTimeout:()=>1,clearTimeout(){},Uint8Array,Date,Error,Blob});
 let id=0;
 function request(method:string,args:unknown[]=[]){return new Promise<Parameters<typeof resolve>[0]>(done=>{const wanted=++id;resolve=message=>{if(message.id===wanted)done(message)};scope.onmessage({data:{id:wanted,method,args}})})}
 return {calls,request,locked:()=>locked};
}
test('epoch worker opens the current slot, verifies sealed root history, and reads files across epochs',async()=>{
 const w=worker();assert.equal((await w.request('unlockRecovery',['words','reader@example.test'])).error,undefined);
 assert(w.calls.indexOf(`epoch:${epoch}`)<w.calls.indexOf(`slot:${prefix}`));
 assert(w.calls.indexOf('open-current')<w.calls.indexOf('history:root'));
 assert(w.calls.indexOf(`root:${segment}`)<w.calls.indexOf(`segment:${prefix+segment}`));
 assert(w.calls.indexOf(`segment:${prefix+segment}`)<w.calls.indexOf('finish'));
 assert(!w.calls.some(call=>call.includes('index/heads')));
 assert.equal(JSON.stringify((await w.request('file',['project',state,'test.txt'])).value),'{"text":"joined history"}');
 assert(w.calls.indexOf(`tree:${oldTree}`)<w.calls.indexOf(`tree:${prefix+tree}`));
 assert(w.calls.includes(`part:${oldPart}`));assert(w.calls.includes(`part:${prefix+part}`));
 assert.equal((await w.request('submitRequest',['project',{kind:'comment'}])).error,undefined);
 assert(w.calls.includes('request:current'));await w.request('lock');assert(w.locked());
});
test('a removed epoch reader never falls back to the root slot or loads history',async()=>{
 const w=worker({removed:true});assert.match((await w.request('unlockRecovery',['words'])).error!,/404/);
 assert(w.locked());assert(!w.calls.includes('slot:'));assert(!w.calls.some(call=>call.startsWith('history:')||call.startsWith('segment:')));
});
test('an incomplete epoch clears the worker and preserves the rotation-in-progress error',async()=>{
 const w=worker({incomplete:true});assert.match((await w.request('unlockRecovery',['words'])).error!,/rotation in progress/);
 assert(w.locked());assert.match((await w.request('file',['project',state,'test.txt'])).error!,/locked/);
});
test('anonymous epoch loading uses only public history and no private proof',async()=>{
 const w=worker();assert.equal((await w.request('openPublic',['store'])).value,'project');
 assert(w.calls.includes('open-current-public'));assert(w.calls.includes('history:root'));assert(w.calls.includes(`root:${segment}`));
 assert(!w.calls.includes('current-proof'));await w.request('lock');
});

test('release download passes the fetched current or historical source to the session verifier',async()=>{
 const w=worker();await w.request('openPublic',['store']);
 assert.equal(((await w.request('releaseDownload',['project','release/current','layr'])).value as Blob).size,3);
 assert(w.calls.includes(`release:${prefix}${releaseObject}`));
 assert.equal(((await w.request('releaseDownload',['project','release/old','layr'])).value as Blob).size,3);
 assert(w.calls.includes(`object:${prefix}${oldRelease}`));
 assert(w.calls.includes(`release:${oldRelease}`));
});
