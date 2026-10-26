/** Rehearse registration with disposable recovery material on an owned Mac RAM disk. */
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,stat} from 'node:fs/promises';
import {resolve4} from 'node:dns/promises';
import path from 'node:path';
import {chromium,expect} from '@playwright/test';
import {observePersistence,expectNoPersistence} from './storage.ts';

assert.equal(process.env.LAYR_WEB_LIVE_DISPOSABLE,'1','Only disposable registration is allowed');
const fixture=JSON.parse(await readFile(process.env.LAYR_WEB_LIVE_REGISTRATION,'utf8'));
assert.match(fixture.login,/^m1-ui-[a-z0-9]+@example\.test$/);
assert(fixture.words_file.startsWith('/Volumes/LayrM1Smoke/'));
assert(fixture.invite_file.startsWith('/Volumes/LayrM1Smoke/'));
const output=process.env.LAYR_WEB_LIVE_OUTPUT;
await mkdir(output,{recursive:true});
const receipt=path.join(output,'registration.json');
assert.equal(await stat(receipt).catch(()=>null),null,'Inspect the existing registration receipt before any retry');
const origin='https://cloud.layr.ws';
const ipv4=(await resolve4('cloud.layr.ws'))[0];
const browser=await chromium.launch({headless:true,args:[`--host-resolver-rules=MAP cloud.layr.ws ${ipv4}`]});
try{
 const context=await browser.newContext();
 const page=await context.newPage();
 await observePersistence(page);
 await page.goto(origin);
 await page.getByRole('button',{name:'Join with an invite',exact:true}).click();
 await page.getByLabel('Login',{exact:true}).fill(fixture.login);
 await page.getByLabel('Invite token',{exact:true}).fill((await readFile(fixture.invite_file,'utf8')).trim());
 await page.getByRole('button',{name:'Create person key',exact:true}).click();
 const recovery=page.getByLabel('Your 24 recovery words',{exact:true});
 await expect(recovery).toBeVisible({timeout:60000});
 const words=await recovery.inputValue();
 assert.equal(words.trim().split(/\s+/).length,24);
 await writeFile(fixture.words_file,words,{mode:0o600,flag:'wx'});
 await page.getByRole('checkbox',{name:'I saved my recovery words',exact:true}).check();
 const delivered=page.waitForResponse(r=>r.request().method()==='PUT'&&new URL(r.url()).pathname==='/api/person');
 // Save an attempt receipt before the mutation; a connection loss must not cause blind replay.
 await writeFile(receipt,JSON.stringify({login:fixture.login,status:'attempted'}),{flag:'wx'});
 await page.getByRole('button',{name:'Create account and sign in',exact:true}).click();
 const response=await delivered;
 await writeFile(receipt,JSON.stringify({login:fixture.login,status:response.status(),transport:'Mac Chromium IPv4 CloudFront'}));
 assert.equal(response.status(),200);
 await expect(page.locator('#main-content').getByRole('heading',{name:'Projects',exact:true})).toBeVisible({timeout:60000});
 await expectNoPersistence(page,'disposable site registration');
 await page.getByRole('button',{name:/Account/}).click();
 await page.getByRole('button',{name:'Lock',exact:true}).click();
 await expect(page.getByRole('heading',{name:'Sign in',exact:true})).toBeVisible();
 console.log('PASS disposable site registration, signature sign-in, lock and no persistent secrets over Mac IPv4');
}finally{await browser.close();}
