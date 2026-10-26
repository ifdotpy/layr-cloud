import {test,expect} from '@playwright/test';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import type {PersonView} from '../src/data/gateway';
import {observePersistence,expectNoPersistence} from './storage';
const root=fileURLToPath(new URL('../../',import.meta.url));
const fixture=JSON.parse(execFileSync('cargo',['run','--quiet','--locked','-p','layr-web','--example','invite_fixture'],{cwd:root,encoding:'utf8'}));
for(const mode of ['accept','lost reply','lost delete','conflict','missing','foreign','descriptor','revoked slot','records','locked']) {
 test(`project invite: ${mode}`,async({page})=>{
  let view:PersonView=structuredClone(fixture.view), writes=0, deletes=0, refuse=false;
  if(mode==='foreign')view.inbox[0].data=fixture.foreign;
  await observePersistence(page);
  await page.route('**/api/**',async route=>{
   const request=route.request(),url=new URL(request.url()),pathname=url.pathname;
   if(['PUT','DELETE'].includes(request.method()))expect(request.headers()['content-type']).toBe('application/json');
   const json=(body:unknown,status=200)=>route.fulfill({status,contentType:'application/json',body:JSON.stringify(body)});
   if(pathname==='/api/person/unlock') {
    const {index:_,inbox:__,...bootstrap}=view;
    const bytes=Buffer.alloc(65536,32);bytes.write(JSON.stringify(bootstrap));return json({unlock:bytes.toString('base64')});
   }
   if(pathname==='/api/session/challenge')return json({challenge:Buffer.alloc(32,9).toString('base64')});
   if(pathname==='/api/session')return json({login:view.login,token:'person-session',expires:Date.now()+3600000});
   if(pathname==='/api/person')return refuse?json({error:'Your session is locked.'},401):json(view);
   if(pathname==='/api/projects')return json({projects:['store']});
   if(pathname==='/api/person/index') {
    writes++;expect(request.headers().authorization).toBe('Bearer person-session');
    const body=request.postDataJSON();expect(body.previous).toBe(view.index===null?null:createHash('sha256').update(Buffer.from(view.index,'base64')).digest('hex'));
    if(mode==='conflict')return json({error:'the index changed; read it again'},409);
    view.index=body.data;
    if(mode==='lost reply')return route.abort('connectionreset');
    return json({saved:'index'});
   }
   if(pathname.startsWith('/api/person/inbox/')){deletes++;view.inbox=[];if(mode==='lost delete')return route.abort('connectionreset');return json({removed:true});}
   return json({error:'unexpected route'},404);
  });
  await page.route('**/v1/p/store/**',async route=>{
   const url=new URL(route.request().url()),p=url.pathname;
   const json=(body:unknown,status=200)=>route.fulfill({status,contentType:'application/json',body:JSON.stringify(body)});
   if(p.endsWith('/store.json'))return json(mode==='descriptor'?{...fixture.descriptor,recipient:fixture.view.recipient}:fixture.descriptor);
   if(p.includes('/slots/'))return mode==='revoked slot'?json({error:'slot is gone'},404):route.fulfill({body:Buffer.from(fixture.slot,'base64')});
   if(p.endsWith('/challenge'))return json({challenge:fixture.challenge});
   if(p.endsWith('/token'))return json({token:'reader-token',expires:Date.now()+3600000});
   if(p.endsWith('/objects'))return json({objects:url.searchParams.get('prefix')==='records/'?[{key:fixture.segment}]:[]});
   if(p.endsWith('/objects/'+fixture.segment))return route.fulfill({body:mode==='records'?Buffer.from('invalid ciphertext'):Buffer.from(fixture.records,'base64')});
   return json({error:'unexpected route'},404);
  });
  await page.goto('/#/you');
  await page.getByLabel('Login',{exact:true}).fill(view.login);
  await page.getByLabel('24 recovery words',{exact:true}).fill(fixture.words);
  await page.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
  await expect(page.getByRole('heading',{name:'You',exact:true})).toBeVisible();
  if(mode==='foreign') {
   await expect(page.getByText(/Cannot open invite/)).toBeVisible();await expect(page.getByRole('button',{name:'Accept',exact:true})).toHaveCount(0);
  } else {
   if(mode==='missing')view.inbox=[];
   if(mode==='locked')refuse=true;
   await page.getByRole('button',{name:'Accept',exact:true}).click();
   if(mode==='accept'||mode==='lost reply'||mode==='lost delete') {
    await expect(page.getByRole('button',{name:'Accept',exact:true})).toHaveCount(0);
    await expect(page.getByRole('navigation',{name:'Breadcrumb'}).getByRole('link',{name:'accepted-project',exact:true})).toBeVisible();
    expect(writes).toBe(1);expect(deletes).toBe(1);
   } else if(mode==='locked')await expect(page.getByRole('heading',{name:'Sign in',exact:true})).toBeVisible();
   else {await expect(page.getByRole('alert')).toBeVisible();expect(deletes).toBe(0);}
  }
  if(!['accept','lost reply','lost delete','conflict','records'].includes(mode))expect(writes).toBe(0);
  await expectNoPersistence(page,`invite ${mode}`);
 });
}
