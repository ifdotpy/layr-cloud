/** The real MinIO/gateway worker, against the pinned CLI index recipe. */
import assert from 'node:assert/strict';
import {mkdir,readFile,readdir,writeFile} from 'node:fs/promises';
import path from 'node:path';
import AxeBuilder from '@axe-core/playwright';
import {views} from './views.ts';
import {measureContrast} from './contrast.ts';
export async function checkIndexPublic({browser,page,site,project,fixture,words,output}) {
  await mkdir(output,{recursive:true});const screenshots=[];let axe=0;
  const url=(section='code',query='')=>`${site}#/project/${project}/${section}${query}`;
  async function capture(page,name) {
    for(const view of views){await page.setViewportSize({width:view.width,height:view.height});await page.emulateMedia({colorScheme:view.colorScheme,reducedMotion:'reduce'});await page.mouse.move(0,0);
      const scan=await new AxeBuilder({page}).analyze();assert.deepEqual(scan.violations.map(({id,impact})=>({id,impact})),[],name);axe++;
      for(const badge of await page.locator('.projhead .badge').all())assert((await measureContrast(badge)).ratio>=4.5,'Header badges meet AA contrast');
      const screenshot=path.join(output,`${name}-${view.name}.png`);await page.screenshot({path:screenshot,fullPage:true,animations:'disabled'});screenshots.push(screenshot);
    }
  }
  const recordRoot=path.join(fixture,'archive','records');
  const machine=(await readdir(recordRoot))[0];
  const segments=(await readdir(path.join(recordRoot,machine))).filter(name=>name.endsWith('.age')).sort();
  assert(segments.length>=2,'Provisional test needs separate bootstrap and later record segments');
  let releaseRecords;const recordsGate=new Promise(resolve=>releaseRecords=resolve);
  const earlyContext=await browser.newContext();const early=await earlyContext.newPage();
  await early.route(`**/v1/p/example/objects/records/${machine}/${segments.at(-1)}`,async route=>{await recordsGate;await route.continue()});
  await early.goto(url('lines'));await early.getByLabel('24 recovery words').fill(words);
  const opening=performance.now();await early.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
  await early.getByText('Provisional · checking signed records…',{exact:true}).waitFor();
  await early.locator('main').getByTitle('main',{exact:true}).waitFor();const provisionalMs=performance.now()-opening;
  await capture(early,'provisional-first-page');
  // Code and open changes remain usable while the remaining record segment is withheld.
  await early.goto(url('code'));await early.getByRole('table',{name:'Files',exact:true}).waitFor();
  await early.goto(url('changes'));await early.getByRole('table',{name:'Open changes',exact:true}).waitFor();
  releaseRecords();await early.getByText('Provisional · checking signed records…',{exact:true}).waitFor({state:'detached'});
  await earlyContext.close();
  const tamperedContext=await browser.newContext();const tampered=await tamperedContext.newPage();
  await tampered.route('**/v1/p/example/objects/index/heads/**',async route=>{const response=await route.fetch();const bytes=await response.body();bytes[bytes.length-1]^=1;await route.fulfill({response,body:bytes})});
  await tampered.goto(url('lines'));await tampered.getByLabel('24 recovery words').fill(words);await tampered.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
  await tampered.getByText('Index discarded. Showing verified records.',{exact:true}).waitFor();assert.equal(await tampered.getByText('Provisional · checking signed records…',{exact:true}).count(),0);await tamperedContext.close();
  const actual=await page.evaluate(async words=>{
    const {ModelSource}=await import('/src/data/model-source.ts');const source=new ModelSource();
    try{const start=performance.now();await source.unlockRecovery(words,'owner@example.com');const [p]=await source.projects();const lines=await source.lines(p.id);const firstPageMs=performance.now()-start;const main=lines.find(l=>l.name==='main');const feature=lines.find(l=>l.name==='feature');
      const history=await source.fileHistory(p.id,feature.head,'test.txt');const blame=await source.blame(p.id,feature.head,'test.txt');const opens=await source.openChanges(p.id);return {firstPageMs,lines,history,blame,open:opens.length,head:feature.head};
    }finally{await source.lock()}
  },words);
  const oracle=JSON.parse(await readFile(path.join(fixture,'history-feature.json'),'utf8'));assert.deepEqual(actual.history,oracle);
  const cli=(await readFile(path.join(fixture,'blame-feature.txt'),'utf8')).trimEnd().split('\n');assert.deepEqual(actual.blame.lines.map(l=>l.text),cli.map(l=>l.split(') ').slice(1).join(') ')));
  const feature=actual.head;
  await page.goto(url('code',`?state=${feature}&path=test.txt`));await page.getByRole('group',{name:'File view'}).waitFor();
  await page.getByRole('button',{name:'Blame',exact:true}).click();await page.getByRole('region',{name:'Blame',exact:true}).waitFor();await capture(page,'blame');
  await page.getByRole('button',{name:'File history',exact:true}).click();await page.getByRole('table',{name:'File history',exact:true}).waitFor();await capture(page,'file-history');
  const pathContext=await browser.newContext();const corruptedPath=await pathContext.newPage();
  await corruptedPath.route('**/v1/p/example/objects/index/paths/**',async route=>{const response=await route.fetch();const bytes=await response.body();bytes[bytes.length-1]^=1;await route.fulfill({response,body:bytes})});
  await corruptedPath.goto(url('code',`?state=${feature}&path=test.txt`));await corruptedPath.getByLabel('24 recovery words').fill(words);await corruptedPath.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
  await corruptedPath.getByRole('button',{name:'Blame',exact:true}).click();await corruptedPath.getByRole('region',{name:'Blame',exact:true}).waitFor();
  assert.equal(await corruptedPath.getByText('Index discarded. Showing verified records.',{exact:true}).count(),1,'A rejected path cache has one muted note and verified blame remains available');
  await pathContext.close();
  // Use a fresh browser session with no whois or signed-in account endpoints.
  const context=await browser.newContext();const anonymous=await context.newPage();let accountRequests=0;let writes=0;
  anonymous.on('request',r=>{if(new URL(r.url()).pathname.startsWith('/api/'))accountRequests++;if(!['GET','HEAD'].includes(r.method()))writes++});
  await anonymous.goto(`${site}?project=example`);await anonymous.locator('.projhead').getByText('browser-test',{exact:true}).waitFor();
  await anonymous.locator('.projhead .badge').getByText('Public',{exact:true}).waitFor();assert.equal(await anonymous.getByLabel('24 recovery words').count(),0);assert.equal(accountRequests,0);assert.equal(writes,0);
  await capture(anonymous,'public-project');
  const publicValues=await anonymous.evaluate(async project=>{const {ModelSource}=await import('/src/data/model-source.ts');const s=new ModelSource();try{const id=await s.openPublic('example');const lines=await s.lines(id);const f=lines.find(l=>l.name==='feature');return {lines,file:await s.file(id,f.head,'test.txt'),you:await s.you()}}finally{await s.lock()}},project);
  assert.equal(publicValues.you.login,'');assert.equal(publicValues.file.text,'feature bytes\n');assert.deepEqual(publicValues.lines,actual.lines);
  await anonymous.getByRole('button',{name:/Account/}).click();await anonymous.getByRole('button',{name:'Sign in',exact:true}).click();
  await anonymous.getByLabel('24 recovery words').fill(words);await anonymous.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
  await anonymous.locator('.projhead .badge').getByText('Public',{exact:true}).waitFor();await anonymous.getByRole('button',{name:'Account of owner@example.com',exact:true}).waitFor();
  const invalid=await context.newPage();await invalid.route('**/p/example/public.json',route=>route.fulfill({status:200,contentType:'application/json',body:'{}'}));
  await invalid.goto(`${site}?project=example`);await invalid.getByRole('heading',{name:'This project is not public',exact:true}).waitFor();assert.equal(await invalid.locator('#main-content').count(),0);await invalid.close();
  // An absent manifest yields a closed page with sign-in and no project DOM.
  await anonymous.goto(`${site}?project=missing`);await anonymous.getByRole('heading',{name:'This project is not public',exact:true}).waitFor();assert.equal(await anonymous.locator('#main-content').count(),0);await anonymous.getByRole('button',{name:'Sign in',exact:true}).click();await anonymous.getByLabel('24 recovery words').waitFor();
  await context.close();
  await writeFile(path.join(output,'index-public-results.json'),JSON.stringify({firstPageMs:actual.firstPageMs,provisionalMs,lines:actual.lines.length,history:actual.history.length,blame:actual.blame.lines.length,open:actual.open,axe,screenshots},null,2));
  await page.goto(url('lines'));await page.getByRole('heading',{name:'Lines',exact:true}).waitFor();
  console.log(`PASS index/public live: ${actual.firstPageMs.toFixed(2)}ms first page, ${actual.history.length} CLI history states, ${actual.blame.lines.length} blame lines, anonymous bytes, fail-closed missing manifest, ${axe} axe checks, ${screenshots.length} screenshots`);
}
