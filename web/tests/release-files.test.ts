import {mockVerification} from './worker-transport';
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { Gateway } from '../src/data/gateway';
import { releaseDownloadCommand } from '../src/data/releases';
import {epochRoute} from '../src/data/epochs';
Object.defineProperty(globalThis,'location',{value:{origin:'https://layr.example'},configurable:true});

test('release fetch bounds ciphertext headers and streamed bodies before WASM',async()=>{
  const previous=globalThis.fetch;
  const key='files/01a12290-0000-7000-8000-000000000001.age';
  try {
    globalThis.fetch=async()=>new Response(new Uint8Array([1,2,3]),{headers:{'content-length':'30'}});
    await assert.rejects(new Gateway().releaseObject('project',key,4),/size limit/);
    let cancelled=false;
    globalThis.fetch=async()=>new Response(new ReadableStream({pull(controller){controller.enqueue(new Uint8Array(5));},cancel(){cancelled=true;}}));
    await assert.rejects(new Gateway().releaseObject('project',key,4),/size limit/);
    assert(cancelled);
    globalThis.fetch=async(input,init)=>{assert.equal(String(input),`/p/project/${key}`);assert.equal(new Headers(init?.headers).get('authorization'),null);return new Response(new Uint8Array([1,2,3]));};
    assert.deepEqual([...await new Gateway().releaseObject('project',key,4)],[1,2,3]);
  } finally {globalThis.fetch=previous;}
});

const code=ts.transpileModule(readFileSync(new URL('../src/data/worker.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace(/^import .*;\n/gm,'').replace(/^export \{\};\n/gm,'');
function worker(size:number,valid=true) {
 let fetched=0,blobbed=0;let receive:(value:{value?:Blob;error?:string})=>void;
 const file={name:'layr',size,object:'files/01a12290-0000-7000-8000-000000000001.age'};
 class Session {
    clear_record_segments() {} verify_file() {}
  open_public_store(){return 'project';} add_segment(){} finish_records(){} lock(){} free(){}
  query(_name:string,method:string){assert.equal(method,'releaseFiles');return JSON.stringify([file]);}
  release_file(){if(!valid)throw Error('release file SHA-256 or size mismatch');return new Uint8Array([1,2,3]);}
 }
 class Gateway {
    verifiedObjects = mockVerification;
  discovery(){return {};}setEpoch(){} publicDescriptor(){return {project:'project'};}publicManifest(){return new Uint8Array();}publicObject(){return new Uint8Array();}publicList(){return [];}lock(){}
  releaseObject(){fetched++;return new Uint8Array();}
 }
 class CountedBlob extends Blob {constructor(parts:BlobPart[],options:BlobPropertyBag){super(parts,options);blobbed++;}}
 const scope={onmessage:(_event:unknown)=>{},postMessage:(value:Parameters<typeof receive>[0])=>receive(value),close(){}};
 vm.runInNewContext(code,{self:scope,Session,Gateway,epochRoute,HttpError:class extends Error{},init:async()=>{},setTimeout:()=>1,clearTimeout(){},Uint8Array,Date,Error,Blob:CountedBlob});
 return {counts:()=>({fetched,blobbed}),call:(method:string,args:unknown[]=[])=>new Promise<Parameters<typeof receive>[0]>(resolve=>{receive=resolve;scope.onmessage({data:{id:1,method,args}});})};
}
test('worker creates a Blob only after WASM verification and rejects >256 MiB before fetch',async()=>{
 const good=worker(10<<20);await good.call('openPublic',['store']);assert.equal((await good.call('releaseDownload',['project','release/1','layr'])).value?.size,3);assert.deepEqual(good.counts(),{fetched:1,blobbed:1});
 const corrupt=worker(10<<20,false);await corrupt.call('openPublic',['store']);assert.match((await corrupt.call('releaseDownload',['project','release/1','layr'])).error!,/SHA-256/);assert.deepEqual(corrupt.counts(),{fetched:1,blobbed:0});
 const large=worker((256<<20)+1);await large.call('openPublic',['store']);assert.match((await large.call('releaseDownload',['project','release/1','layr'])).error!,/256 MiB/);assert.deepEqual(large.counts(),{fetched:0,blobbed:0});
});

test('CLI download hints preserve shell syntax and option-like filenames',()=>{
 assert.equal(releaseDownloadCommand('release/1','layr-arm64'),'layr release get release/1 layr-arm64');
 assert.equal(releaseDownloadCommand('release/1','a $(echo x)'),'layr release get release/1 \'a $(echo x)\'');
 assert.equal(releaseDownloadCommand('release/1','-hidden'),'layr release get release/1 -- \'-hidden\'');
});
