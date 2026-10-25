/** Browser callers against the real signature gateway, including first registration. */
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import path from 'node:path';
import {expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import {views} from './views.ts';
import {observePersistence,expectNoPersistence} from './storage.ts';
export async function checkSignin({browser,site,words,invite,personLogin,root,nativeSession,port,person}){
 const output=path.join(root,'web/test-results/signin-live');await mkdir(output,{recursive:true});
 if(nativeSession){
  execFileSync('orb',['-m','layr-dev','-u','root',nativeSession,`http://localhost:${port}`,personLogin,'/mnt/mac'+path.join(person,'recovery.txt')],{stdio:'pipe'});
  console.log('PASS native Session against real gateway: shared proof, private index, replay and wrong signature refused');
 }
 // The gateway deliberately limits unauthenticated bootstrap attempts to twelve per minute.
 // Native verifier + initial browser unlock consume that allowance; preserve the real limit.
 await new Promise(resolve=>setTimeout(resolve,61000));
 async function capture(page,name,mask=[]){for(const view of views){await page.setViewportSize({width:view.width,height:view.height});await page.emulateMedia({colorScheme:view.colorScheme});const axe=await new AxeBuilder({page}).analyze();assert.deepEqual(axe.violations,[]);await writeFile(path.join(output,`${name}-${view.name}-axe.json`),JSON.stringify(axe,null,2));await page.screenshot({path:path.join(output,`${name}-${view.name}.png`),fullPage:true,mask});}}
 const passkeyContext=await browser.newContext();
 await passkeyContext.addInitScript(()=>{Object.defineProperty(navigator,'credentials',{value:{get:async options=>{
  const salt=new TextDecoder().decode(options.publicKey.extensions.prf.eval.first);
  if(salt!=='layr-person/v1/owner@example.com')throw new Error('Wrong person PRF salt');
  return {rawId:new Uint8Array([1,2,3,4]).buffer,getClientExtensionResults:()=>({prf:{results:{first:new Uint8Array(Array.from({length:32},(_,i)=>i)).buffer}}})};
 }}});});
 const passkeyPage=await passkeyContext.newPage();await observePersistence(passkeyPage);await passkeyPage.goto(site);await passkeyPage.getByLabel('Login',{exact:true}).fill(personLogin);await capture(passkeyPage,'signin-before');
 await passkeyPage.getByRole('button',{name:'Unlock with a passkey',exact:true}).click();await expect(passkeyPage.getByRole('heading',{name:'Projects',exact:true})).toBeVisible();await capture(passkeyPage,'passkey-signed-in');await expectNoPersistence(passkeyPage,'real gateway passkey sign-in');await passkeyContext.close();
 const context=await browser.newContext();const page=await context.newPage();await observePersistence(page);await page.goto(site);
 await page.getByLabel('Login',{exact:true}).fill('new@example.com');await page.getByRole('button',{name:'Join with an invite',exact:true}).click();await page.getByLabel('Invite token',{exact:true}).fill(invite);await page.getByRole('button',{name:'Create person key',exact:true}).click();
 const recovery=page.getByLabel('Your 24 recovery words',{exact:true});await expect(recovery).toBeVisible();const newWords=await recovery.inputValue();assert.equal(newWords.split(' ').length,24);
 await capture(page,'invite-recovery',[recovery,page.getByLabel('Invite token',{exact:true})]);await page.getByLabel('I saved my recovery words',{exact:true}).check();await page.getByRole('button',{name:'Create account and sign in',exact:true}).click();await expect(page.getByRole('heading',{name:'Projects',exact:true})).toBeVisible();await capture(page,'registered');
 await page.getByRole('button',{name:/Account/}).click();await page.getByRole('button',{name:'Lock',exact:true}).click();await expect(page.getByRole('heading',{name:'Sign in',exact:true})).toBeVisible();
 await page.getByLabel('Login',{exact:true}).fill('new@example.com');await page.getByLabel('24 recovery words',{exact:true}).fill(newWords);
 let renewed=false;
 await page.route('**/api/person',async route=>{if(!renewed && route.request().method()==='GET'){renewed=true;await route.fulfill({status:401,contentType:'application/json',body:'{"error":"expired test session"}'});}else await route.continue();});
 await page.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();await expect(page.getByRole('heading',{name:'Projects',exact:true})).toBeVisible();assert(renewed);await capture(page,'recovery-renewed');await expectNoPersistence(page,'registration, lock, recovery and session renewal');await context.close();
 console.log(`PASS real signature browser: passkey PRF caller, invite registration, recovery sign-in, once-only401 renewal; 5 screens x ${views.length} views, zero axe violations; ${output}`);
}
