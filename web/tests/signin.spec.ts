import {test,expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import {views} from './views';
import {mkdir,writeFile} from 'node:fs/promises';
import path from 'node:path';
import {observePersistence,expectNoPersistence} from './storage';
const output=path.resolve('test-results/signin');
test('sign-in and invitation controls are accessible and keep generated recovery material out of storage',async({page})=>{
 await observePersistence(page); await page.goto('/');
 await expect(page.getByRole('heading',{name:'Sign in',exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'Unlock with a passkey',exact:true})).toBeDisabled();
 await page.getByLabel('Login',{exact:true}).fill('new@example.com');
 await expect(page.getByRole('button',{name:'Unlock with a passkey',exact:true})).toBeEnabled();
 await mkdir(output,{recursive:true});
 async function capture(name:string,mask=false){for(const view of views){await page.setViewportSize({width:view.width,height:view.height});await page.emulateMedia({colorScheme:view.colorScheme});const axe=await new AxeBuilder({page}).analyze();expect(axe.violations).toEqual([]);await writeFile(path.join(output,`${name}-${view.name}-axe.json`),JSON.stringify(axe,null,2));await page.screenshot({path:path.join(output,`${name}-${view.name}.png`),fullPage:true,mask:mask?[page.getByLabel('Your 24 recovery words',{exact:true})]:[]});}}
 await capture('signin');
 await page.getByRole('button',{name:'Join with an invite',exact:true}).click();
 await page.getByLabel('Invite token',{exact:true}).fill('disposable-test-invite');
 await page.getByRole('button',{name:'Create person key',exact:true}).click();
 const recovery=page.getByLabel('Your 24 recovery words',{exact:true});await expect(recovery).toBeVisible();
 expect((await recovery.inputValue()).split(' ')).toHaveLength(24);
 await expect(page.getByRole('button',{name:'Create account and sign in',exact:true})).toBeDisabled();
 await page.getByLabel('I saved my recovery words',{exact:true}).check();
 await expect(page.getByRole('button',{name:'Create account and sign in',exact:true})).toBeEnabled();
 await capture('invite-recovery',true);
 await expectNoPersistence(page,'new recovery words');
 await page.getByRole('button',{name:'Use an existing person key',exact:true}).click();
 await expect(recovery).toHaveCount(0);await expect(page.getByLabel('24 recovery words',{exact:true})).toHaveValue('');
});
