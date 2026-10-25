/** Real worker signatures -> real gateway -> MinIO; real CLI executor -> signed records. */
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile,mkdir,cp} from 'node:fs/promises';
import path from 'node:path';
import {expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import {views} from './views.ts';
import {measureContrast} from './contrast.ts';
export async function checkActions({page,site,project,actionFixture,store,person,root,bucket,env}) {
  const shots = path.join(root,'web/test-results/actions-live'); await mkdir(shots,{recursive:true});
  const {vmRoot}=JSON.parse(await readFile(path.join(actionFixture,'executor.json'),'utf8'));
  const submitted=[]; const delivered=new Map(); let appliedCount=0;
  page.on('request',request => {
    if (request.method()==='PUT' && /\/requests\/[^/]+\.json$/.test(new URL(request.url()).pathname)) submitted.push(JSON.parse(request.postData()));
  });
  page.on('response',response => {
    const request=response.request();
    if(request.method()==='PUT' && /\/requests\/[^/]+\.json$/.test(new URL(request.url()).pathname)) delivered.set(JSON.parse(request.postData()).id,response.status());
  });
  async function publish() {
    await cp(path.join(actionFixture,'store'),store,{recursive:true});
    for (let attempt=0;;attempt++) {
      try { execFileSync(path.join(root,'target/debug/examples/upload_gateway'),[bucket,store,path.join(person,'people')],{env:{...env,LAYR_WEB_APPEND:'1'},stdio:'pipe'}); break; }
      catch(error) { if(attempt>=2 || !String(error.stderr).includes('connection closed before message completed')) throw error; }
    }
  }
  async function apply() {
    await expect.poll(()=>submitted.length).toBe(appliedCount+1);
    const envelope=submitted[appliedCount]; assert(envelope,'browser sent a request');
    await expect.poll(()=>delivered.get(envelope.id)).toBe(201);
    appliedCount++;
    assert.equal(envelope.person,'owner@example.com'); assert.equal(envelope.project,project);
    assert.equal(envelope.v,2); assert.equal(envelope.r,undefined); assert.equal(envelope.action,undefined);
    assert(envelope.ct && envelope.sig, 'Only ciphertext and outer proof cross the gateway');
    await mkdir(path.join(actionFixture,'store/requests'),{recursive:true});
    await writeFile(path.join(actionFixture,'store/requests',envelope.id+'.json'),JSON.stringify(envelope));
    execFileSync(path.join(actionFixture,'apply.sh'),[],{stdio:'pipe'});
    await publish();
  }
  async function capture(name) {
    for (const view of views) {
      await page.setViewportSize({width:view.width,height:view.height});
      await page.emulateMedia({colorScheme:view.colorScheme,reducedMotion:'reduce'});
      const axe = await new AxeBuilder({page}).analyze();
      assert.deepEqual(axe.violations,[],`${name} ${view.name} axe`);
      const buttons = page.locator('main button.btn.primary:enabled');
      if (await buttons.count()) assert((await measureContrast(buttons.first())).ratio>=4.5);
      await writeFile(path.join(shots,`${name}-${view.name}-axe.json`),JSON.stringify(axe,null,2));
      await page.screenshot({path:path.join(shots,`${name}-${view.name}.png`),fullPage:true});
    }
    await page.setViewportSize({width:1280,height:900}); await page.emulateMedia({colorScheme:'light'});
  }
  const changes=JSON.parse(await readFile(path.join(actionFixture,'changes.json'),'utf8'));
  const feature=changes.find(change=>change.message==='feature'); assert(feature);
  const base=`${site}#/project/${project}`;
  await page.goto(`${base}/change/${feature.change_id}/overview`);
  const approve=page.getByRole('button',{name:'Approve v1',exact:true});
  await expect(approve).toBeEnabled(); await capture('approve-before');
  async function action(button,status='applied') {
    await button.click(); const block=button.locator('../..');
    await expect(block).toContainText('waiting for the executor.');
    await apply();
    await expect(block.locator('.request-outcome')).toContainText(status,{timeout:30000});
  }
  await approve.click(); await expect(approve.locator('../..')).toContainText('waiting for the executor.'); await apply();
  await expect(page.locator('.approval-confirmation')).toHaveText('You approved v1',{timeout:30000});
  await expect(approve).toHaveCount(0);
  await expect(page.locator('[aria-label="Merge progress"] .step').nth(2)).toHaveClass(/done/);
  await capture('approve-after');
  const withdraw=page.getByRole('button',{name:'Withdraw approval',exact:true});
  await withdraw.click(); await apply(); await expect(approve).toBeEnabled({timeout:30000});
  await expect(page.locator('.approval-confirmation')).toHaveCount(0);
  await capture('withdraw');
  const conversation=page.getByRole('heading',{name:'Conversation',exact:true}).locator('..');
  await conversation.getByLabel('Comment',{exact:true}).last().fill('Browser review: this version is ready.');
  await conversation.getByRole('button',{name:'Send comment',exact:true}).last().click(); await apply();
  await expect(conversation).toContainText('Browser review: this version is ready.',{timeout:30000});
  const comment=conversation.locator('article.comment').filter({hasText:'Browser review: this version is ready.'});
  await comment.getByRole('button',{name:'Reply',exact:true}).click();
  await comment.getByLabel('Reply',{exact:true}).fill('Confirmed in this thread.');
  await comment.getByRole('button',{name:'Send reply',exact:true}).click(); await apply();
  await expect(conversation).toContainText('Confirmed in this thread.',{timeout:30000});
  await capture('comment-thread');
  await page.goto(`${base}/change/${feature.change_id}/files`);
  const file=page.getByRole('button',{name:'test.txt',exact:true}); await file.click();
  await page.getByRole('button',{name:'Comment on test.txt line 2',exact:true}).click();
  await page.getByLabel('Comment on line 2',{exact:true}).fill('Browser inline review on the second line.');
  await page.getByRole('button',{name:'Send comment',exact:true}).click(); await apply();
  await expect(page.locator('.inline-comment')).toContainText('Browser inline review on the second line.',{timeout:30000});
  await capture('inline-comment');
  await page.goto(`${base}/change/${feature.change_id}/overview`);
  await page.getByRole('link',{name:'test.txt:2',exact:true}).last().click();
  await expect(page.locator('.inline-comment')).toContainText('Browser inline review on the second line.');
  await expect(page.getByRole('combobox',{name:'Version',exact:true})).toHaveValue('v1');
  await page.goto(`${base}/change/${feature.change_id}/overview`);
  await page.getByRole('button',{name:'Add to queue',exact:true}).click(); await apply();
  await expect(page.getByRole('button',{name:'Remove from queue',exact:true})).toBeEnabled({timeout:30000});
  await capture('queue-add');
  await page.goto(`${base}/changes/queue`);
  await expect(page.getByRole('button',{name:'Remove from queue',exact:true})).toBeEnabled();
  await capture('queue');
  const remove=page.getByRole('button',{name:'Remove from queue',exact:true});
  await remove.click(); await apply();
  await expect(page.locator('main')).toContainText('The queue is empty.',{timeout:30000});
  // Native check created env/native-test at feature. Keep this page's expected value stale.
  await page.goto(`${base}/deploy/environment/${encodeURIComponent('env/native-test')}`);
  await page.getByLabel('Target state',{exact:true}).fill(feature.state);
  execFileSync('orb',['-m','layr-dev','-u','root',vmRoot+'/executor.sh','ref','set','env/native-test','main','--expect',feature.state],{stdio:'pipe'});
  execFileSync('orb',['-m','layr-dev','-u','root',vmRoot+'/executor.sh','push','store','-q'],{stdio:'pipe'});
  execFileSync('orb',['-m','layr-dev','-u','root','cp','-R',vmRoot+'/archive/.','/mnt/mac'+path.join(actionFixture,'store/')],{stdio:'pipe'});
  await publish();
  await action(page.getByRole('button',{name:'Move env/native-test',exact:true}),'refused');
  await expect(page.locator('.request-outcome')).toContainText('expected value does not match');
  await capture('refusal');
  await page.goto(`${base}/deploy/environments`);
  await page.goto(`${base}/deploy/environment/${encodeURIComponent('env/native-test')}`);
  await page.getByLabel('Target state',{exact:true}).fill(feature.state);
  await action(page.getByRole('button',{name:'Move env/native-test',exact:true}));
  assert.equal(submitted.length,9);
  assert(submitted.every(row=>row.v===2 && !('r' in row) && !('action' in row)));
  console.log(`PASS browser actions: ${submitted.length} real signed requests, 8 applied, 1 refused; 8 screens x ${views.length} views, axe and contrast; ${shots}`);
}
