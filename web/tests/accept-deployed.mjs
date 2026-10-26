/** Accept one synthetic project on the published site, then verify a fresh sign-in. */
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,stat} from 'node:fs/promises';
import path from 'node:path';
import {resolve4} from 'node:dns/promises';
import {chromium,expect} from '@playwright/test';
import {observePersistence,expectNoPersistence} from './storage.ts';

assert.equal(process.env.LAYR_WEB_LIVE_DISPOSABLE,'1');
const fixture=JSON.parse(await readFile(process.env.LAYR_WEB_LIVE_FIXTURE,'utf8'));
assert.match(fixture.login,/^m1-ui-[a-z0-9]+@example\.test$/);
assert(fixture.words_file.startsWith('/Volumes/LayrM1Smoke/'));
const words=(await readFile(fixture.words_file,'utf8')).trim();
const output=process.env.LAYR_WEB_LIVE_OUTPUT;
await mkdir(output,{recursive:true});
const receipt=path.join(output,'acceptance.json');
assert.equal(await stat(receipt).catch(()=>null),null,'Inspect the existing mutation receipt before any retry');
const origin='https://cloud.layr.ws',ipv4=(await resolve4('cloud.layr.ws'))[0];
const browser=await chromium.launch({headless:true,args:[`--host-resolver-rules=MAP cloud.layr.ws ${ipv4}`]});
let page;
try{
 async function signIn(context){
  page=await context.newPage();await observePersistence(page);
  await page.goto(origin+'/#/you');
  await page.getByLabel('Login',{exact:true}).fill(fixture.login);
  await page.getByLabel('24 recovery words',{exact:true}).fill(words);
  await page.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
  await expect(page.getByRole('heading',{name:'You',exact:true})).toBeVisible({timeout:60000});
 }
 let context=await browser.newContext();await signIn(context);
 const accept=page.getByRole('button',{name:'Accept',exact:true});
 await expect(accept).toHaveCount(1);
 const index=page.waitForResponse(r=>r.request().method()==='PUT'&&new URL(r.url()).pathname==='/api/person/index');
 const removed=page.waitForResponse(r=>r.request().method()==='DELETE'&&new URL(r.url()).pathname.startsWith('/api/person/inbox/'));
 await writeFile(receipt,JSON.stringify({login:fixture.login,project:fixture.project,status:'attempted'}),{flag:'wx'});
 await accept.click();
 const updated=await index;assert.equal(updated.status(),200);
 const body=JSON.parse(updated.request().postData());assert.equal(typeof body.data,'string');
 assert(!JSON.stringify(body).includes(words));
 assert.equal((await removed).status(),200);
 await expect(page).toHaveURL(new RegExp(`/project/${fixture.project}`));
 await page.goto(`${origin}/#/project/${fixture.project}/code?line=main&path=README.md`);
 await expect(page.locator('#main-content')).toContainText('three',{timeout:60000});
 await expect(page.getByRole('alert')).toHaveCount(0);
 await expectNoPersistence(page,'project acceptance');
 await page.screenshot({path:path.join(output,'accepted.png'),fullPage:true});
 await context.close();context=await browser.newContext();await signIn(context);
 await expect(page.getByRole('button',{name:'Accept',exact:true})).toHaveCount(0);
 await page.goto(`${origin}/#/project/${fixture.project}/code?line=main&path=README.md`);
 await expect(page.locator('#main-content')).toContainText('three',{timeout:60000});
 await expectNoPersistence(page,'fresh sign-in after acceptance');
 await writeFile(receipt,JSON.stringify({login:fixture.login,project:fixture.project,status:'PASS',indexPut:200,inboxDelete:200,freshSignIn:true,decryptedFile:'README.md',persistentSecrets:false,transport:'Mac Chromium IPv4 CloudFront'},null,2));
 console.log('PASS published signed-in Accept: encrypted index, inbox removal, decrypted project file and fresh sign-in');
}catch(error){
 if(page)await writeFile(path.join(output,'failure.json'),JSON.stringify({error:String(error),alerts:await page.getByRole('alert').allTextContents().catch(()=>[])}));
 throw error;
}finally{await browser.close();}
