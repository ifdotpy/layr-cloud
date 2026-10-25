/** VM-only browser caller against the deployed HTTPS gateway through an AWS SSM relay.
 * The reviewed client build is served from local files; every API call retains real Host/SNI/TLS.
 */
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,stat} from 'node:fs/promises';
import path from 'node:path';
import {chromium,expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import {observePersistence,expectNoPersistence} from './storage.ts';
import {views} from './views.ts';
const origin='https://cloud.layr.ws';
const fixture=JSON.parse(await readFile(process.env.LAYR_WEB_LIVE_FIXTURE,'utf8'));
const words=(await readFile(fixture.words_file,'utf8')).trim();
const dist=path.resolve(process.env.LAYR_WEB_LIVE_DIST || 'dist');
const output=path.resolve(process.env.LAYR_WEB_LIVE_OUTPUT || 'test-results/signin-deployed');await mkdir(output,{recursive:true});
const browser=await chromium.launch({headless:true,args:[`--host-resolver-rules=MAP cloud.layr.ws ${process.env.LAYR_WEB_LIVE_RELAY || '127.0.0.1:443'}`]});
try{
 const context=await browser.newContext();
 await context.route(`${origin}/**`,async route=>{
  const request=route.request(),url=new URL(request.url());
  if(request.method()!=='GET' || /^\/(api|v1|p)\//.test(url.pathname)){await route.continue();return;}
  const relative=url.pathname==='/'?'index.html':decodeURIComponent(url.pathname.slice(1));
  const file=path.resolve(dist,relative);
  if(!file.startsWith(dist+'/') || !(await stat(file).catch(()=>null))?.isFile()){await route.continue();return;}
  const ext=path.extname(file),mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm','.woff2':'font/woff2','.svg':'image/svg+xml','.png':'image/png'}[ext]||'application/octet-stream';
  await route.fulfill({status:200,contentType:mime,body:await readFile(file)});
 });
 const page=await context.newPage();await observePersistence(page);const errors=[];let exchanges=0;let requestId;let wire;
 page.on('pageerror',error=>errors.push(error.message));
 page.on('response',response=>{const request=response.request();if(new URL(response.url()).pathname==='/api/session'&&request.method()==='POST'&&response.status()===200)exchanges++;});
 await page.goto(origin);await page.getByLabel('Login',{exact:true}).fill(fixture.login);
 await page.getByLabel('24 recovery words',{exact:true}).fill(words);await page.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
 await expect(page.getByRole('heading',{name:'Projects',exact:true})).toBeVisible({timeout:60000});assert.equal(exchanges,1);
 await page.goto(`${origin}/#/project/${fixture.project}/change/${fixture.change}/overview`);
 await expect(page.getByRole('heading',{name:'Conversation',exact:true})).toBeVisible({timeout:60000});
 const text=`Deployed browser review ${new Date().toISOString()}`;
 const conversation=page.getByRole('heading',{name:'Conversation',exact:true}).locator('..');
 await conversation.getByLabel('Comment',{exact:true}).last().fill(text);
 const delivered=page.waitForResponse(response=>response.request().method()==='PUT'&&/\/objects\/requests\/[^/]+\.json$/.test(new URL(response.url()).pathname));
 await conversation.getByRole('button',{name:'Send comment',exact:true}).last().click();
 const response=await delivered;assert.equal(response.status(),201);wire=JSON.parse(response.request().postData());requestId=wire.id;
 assert.equal(wire.v,2);assert.equal(wire.r,undefined);assert.equal(wire.action,undefined);assert(!JSON.stringify(wire).includes(text));
 await expect(conversation.locator('article.comment').filter({hasText:text})).toBeVisible({timeout:180000});
 for(const view of views){await page.setViewportSize({width:view.width,height:view.height});await page.emulateMedia({colorScheme:view.colorScheme});const axe=await new AxeBuilder({page}).analyze();assert.deepEqual(axe.violations,[]);await writeFile(path.join(output,`comment-${view.name}-axe.json`),JSON.stringify(axe,null,2));await page.screenshot({path:path.join(output,`comment-${view.name}.png`),fullPage:true});}
 await expectNoPersistence(page,'deployed signature sign-in and encrypted comment');assert.deepEqual(errors,[]);
 await page.getByRole('button',{name:/Account/}).click();await page.getByRole('button',{name:'Lock',exact:true}).click();await expect(page.getByRole('heading',{name:'Sign in',exact:true})).toBeVisible();
 await writeFile(path.join(output,'result.json'),JSON.stringify({origin,transport:'VM browser to SSM relay through AWS executor, then public IPv6 TLS gateway',client:'reviewed local build in VM; API calls use deployed HTTPS',requestId,version:wire.v,applied:true,exchanges,screenshots:3,axeViolations:0,persistentSecrets:false},null,2));
 console.log(`PASS deployed HTTPS browser: recovery, shared signature session, encrypted v2 comment applied, lock, no persistent secrets; ${requestId}`);
}finally{await browser.close();}
