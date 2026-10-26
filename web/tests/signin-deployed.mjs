/** Browser caller against the deployed HTTPS gateway over direct IPv4 or an AWS SSM relay.
 * An explicit staging option serves local static files; the default uses the published site.
 */
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,stat} from 'node:fs/promises';
import path from 'node:path';
import {resolve4} from 'node:dns/promises';
import {chromium,expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import {observePersistence,expectNoPersistence} from './storage.ts';
import {views} from './views.ts';
const origin='https://cloud.layr.ws';
const phase=process.env.LAYR_WEB_LIVE_PHASE || 'verification';
const readOnly=process.env.LAYR_WEB_LIVE_READ_ONLY==='1';
const fixture=JSON.parse(await readFile(process.env.LAYR_WEB_LIVE_FIXTURE,'utf8'));
const words=(await readFile(fixture.words_file,'utf8')).trim();
const dist=path.resolve(process.env.LAYR_WEB_LIVE_DIST || 'dist');
const output=path.resolve(process.env.LAYR_WEB_LIVE_OUTPUT || 'test-results/signin-deployed');await mkdir(output,{recursive:true});
const directIpv4=process.env.LAYR_WEB_LIVE_IPV4==='1';
assert(!(directIpv4 && (process.env.LAYR_WEB_LIVE_PROXY || process.env.LAYR_WEB_LIVE_RELAY)), 'Direct IPv4 cannot use a proxy or relay');
const viewerIpv4=directIpv4?(await resolve4(new URL(origin).hostname))[0]:undefined;
const browser=await chromium.launch({headless:true,...(process.env.LAYR_WEB_LIVE_PROXY ? {proxy:{server:process.env.LAYR_WEB_LIVE_PROXY}} : {args:[`--host-resolver-rules=MAP cloud.layr.ws ${viewerIpv4 || process.env.LAYR_WEB_LIVE_RELAY || '127.0.0.1:443'}`]})});
let page;
try{
 const context=await browser.newContext();
 if(process.env.LAYR_WEB_LIVE_STAGED_CLIENT)await context.route(`${origin}/**`,async route=>{
  const request=route.request(),url=new URL(request.url());
  if(request.method()!=='GET' || /^\/(api|v1|p)\//.test(url.pathname)){await route.continue();return;}
  const relative=url.pathname==='/'?'index.html':decodeURIComponent(url.pathname.slice(1));
  const file=path.resolve(dist,relative);
  if(!file.startsWith(dist+'/') || !(await stat(file).catch(()=>null))?.isFile()){await route.continue();return;}
  const ext=path.extname(file),mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm','.woff2':'font/woff2','.svg':'image/svg+xml','.png':'image/png'}[ext]||'application/octet-stream';
  await route.fulfill({status:200,contentType:mime,body:await readFile(file)});
 });
 page=await context.newPage();await observePersistence(page);const errors=[];let exchanges=0;let requestId;let wire;
 page.on('pageerror',error=>errors.push(error.message));
 page.on('response',response=>{const request=response.request();if(new URL(response.url()).pathname==='/api/session'&&request.method()==='POST'&&response.status()===200)exchanges++;});
 await page.goto(origin);await page.getByLabel('Login',{exact:true}).fill(fixture.login);
 await page.getByLabel('24 recovery words',{exact:true}).fill(words);await page.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
 await expect(page.locator('#main-content').getByRole('heading',{name:'Projects',exact:true})).toBeVisible({timeout:60000});assert.equal(exchanges,1);
 await page.goto(`${origin}/#/project/${fixture.project}/change/${fixture.change}/overview`);
 await expect(page.getByRole('heading',{name:'Conversation',exact:true})).toBeVisible({timeout:60000});
 if(readOnly && process.env.LAYR_WEB_LIVE_DRAFT==='1'){
  const conversation=page.getByRole('heading',{name:'Conversation',exact:true}).locator('..');
  await conversation.getByLabel('Comment',{exact:true}).last().fill(`Unsent diagnostic draft (${phase})`);
  await expect(conversation.getByRole('button',{name:'Send comment',exact:true}).last()).toBeEnabled({timeout:15000});
  await expect(page.getByText('protected main',{exact:true})).toBeVisible();
  await expect(page.getByText('Index discarded. Showing verified records.',{exact:true})).toHaveCount(0);
 }
 if(!readOnly){
 assert.equal(await stat(path.join(output,'delivery.json')).catch(()=>null),null,'This output already records a delivered request; inspect it before submitting again.');
 const text=`Deployed browser review (${phase}) ${new Date().toISOString()}`;
 const conversation=page.getByRole('heading',{name:'Conversation',exact:true}).locator('..');
 await conversation.getByLabel('Comment',{exact:true}).last().fill(text);
 const send=conversation.getByRole('button',{name:'Send comment',exact:true}).last();
 await expect(send).toBeEnabled({timeout:15000});
 const delivered=page.waitForResponse(response=>response.request().method()==='PUT'&&/\/objects\/requests\/[^/]+\.json$/.test(new URL(response.url()).pathname));
 await send.click();
 const response=await delivered;assert.equal(response.status(),201);wire=JSON.parse(response.request().postData());requestId=wire.id;
 assert.equal(wire.v,2);assert.equal(wire.r,undefined);assert.equal(wire.action,undefined);assert(!JSON.stringify(wire).includes(text));
 await writeFile(path.join(output,'delivery.json'),JSON.stringify({origin,phase,requestId,version:wire.v,comment:text,delivered:true,applied:false},null,2));
 await expect(conversation.locator('article.comment').filter({hasText:text})).toBeVisible({timeout:180000});
 }
 const screen=readOnly?'project-read':'comment';
 for(const view of views){await page.setViewportSize({width:view.width,height:view.height});await page.emulateMedia({colorScheme:view.colorScheme});const axe=await new AxeBuilder({page}).analyze();assert.deepEqual(axe.violations,[]);await writeFile(path.join(output,`${screen}-${view.name}-axe.json`),JSON.stringify(axe,null,2));await page.screenshot({path:path.join(output,`${screen}-${view.name}.png`),fullPage:true});}
 await expectNoPersistence(page,readOnly?'deployed signature sign-in and project read':'deployed signature sign-in and encrypted comment');assert.deepEqual(errors,[]);
 await page.getByRole('button',{name:/Account/}).click();await page.getByRole('button',{name:'Lock',exact:true}).click();await expect(page.getByRole('heading',{name:'Sign in',exact:true})).toBeVisible();
 await writeFile(path.join(output,'result.json'),JSON.stringify({origin,transport:directIpv4?'Mac Chromium over public IPv4 to CloudFront, then IPv6 HTTPS origin':'VM browser to SSM relay through AWS executor, then public IPv6 TLS gateway',viewerIpv4,client:process.env.LAYR_WEB_LIVE_STAGED_CLIENT?'reviewed local build in VM; API calls use deployed HTTPS':'published site and deployed HTTPS gateway',mode:readOnly?'read-only':'comment',phase,requestId,version:wire?.v,applied:readOnly?undefined:true,exchanges,screenshots:3,axeViolations:0,persistentSecrets:false},null,2));
 console.log(`PASS deployed HTTPS browser: recovery, shared signature session, ${readOnly?'project read':'encrypted v2 comment applied'}, lock, no persistent secrets${requestId?`; ${requestId}`:''}`);
}catch(error){
 if(page){
  await page.screenshot({path:path.join(output,'failure.png'),fullPage:true,mask:[page.getByLabel('24 recovery words',{exact:true}),page.getByLabel('Your 24 recovery words',{exact:true}),page.getByLabel('Invite token',{exact:true})]}).catch(()=>{});
  await writeFile(path.join(output,'failure.json'),JSON.stringify({phase,error:String(error),alerts:await page.locator('[role=alert],.request-action .note').allTextContents().catch(()=>[])},null,2));
 }
 throw error;
}finally{await browser.close();}
