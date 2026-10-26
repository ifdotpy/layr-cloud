import AxeBuilder from '@axe-core/playwright';
import {expect,test} from '@playwright/test';
import type {Project} from '../src/data/source';
import {fixture} from './fixture-data';
import {views} from './views';
import {observePersistence,expectNoPersistence} from './storage';

test('Access shows the current mode and retains each earlier epoch',async({page},testInfo)=>{
 const epoch='01a12290-0000-7000-8000-000000000008';
 const previous='01a12290-0000-7000-8000-000000000007';
 const projects=structuredClone(fixture<Project[]>('projects'));
 projects[0].access_mode='private';
 projects[0].epochs=[
  {epoch:'root',mode:'public',current:false},
  {epoch:previous,parent:'root',mode:'service',time:1792968000000,current:false},
  {epoch,parent:previous,mode:'private',time:1792968600000,current:true},
 ];
 await page.route('**/fixtures/projects.json*',route=>route.fulfill({contentType:'application/javascript',body:`export default ${JSON.stringify(projects)};`}));
 await observePersistence(page);
 await page.goto(`/?source=fixture#/project/${projects[0].id}/settings/access`);
 const access=page.locator('main section').filter({has:page.getByRole('heading',{name:'Access mode',exact:true})});
 await expect(access.locator('p').filter({hasText:/^private$/})).toBeVisible();
 const table=page.getByRole('table',{name:'Project epochs',exact:true});
 await expect(table.locator('tbody tr')).toHaveCount(3);
 for(const [id,mode,status] of [['root','public','History'],[previous,'service','History'],[epoch,'private','Current']]){
  const row=table.getByRole('row').filter({hasText:id});
  await expect(row.getByRole('cell').nth(1)).toHaveText(mode);
  await expect(row.getByRole('cell').nth(3)).toHaveText(status);
 }
 await expect(table.getByRole('cell',{name:'Current',exact:true})).toHaveCount(1);
 for(const view of views){
  await page.setViewportSize({width:view.width,height:view.height});await page.emulateMedia({colorScheme:view.colorScheme,reducedMotion:'reduce'});
  expect((await new AxeBuilder({page}).analyze()).violations).toEqual([]);
  const screenshot=testInfo.outputPath(`${view.name}.png`);await page.screenshot({path:screenshot,fullPage:true});
  await testInfo.attach(view.name,{path:screenshot,contentType:'image/png'});
 }
 await expectNoPersistence(page,'epoch access history');
});
