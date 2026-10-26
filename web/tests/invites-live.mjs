/** Real CLI-created invite through production WASM, gateway and MinIO in disposable layr-sa. */
import {execFileSync} from 'node:child_process';
import {mkdir} from 'node:fs/promises';
import {chromium,expect} from '@playwright/test';
const orb=(...args)=>execFileSync('orb',['-m','layr-sa','-u','root',...args],{encoding:'utf8'});
const state=JSON.parse(orb('cat','/run/site-accept/state.json'));
const browser=await chromium.launch();
try {
 const context=await browser.newContext();const page=await context.newPage();
 await page.goto('http://localhost:4200/#/you');
 await page.getByLabel('Login',{exact:true}).fill(state.people.browser.login);
 await page.getByLabel('24 recovery words',{exact:true}).fill(orb('cat',state.people.browser.words).trim());
 await page.getByRole('button',{name:'Unlock with recovery words',exact:true}).click();
 await expect(page.getByRole('heading',{name:'You',exact:true})).toBeVisible();
 if(process.argv.includes('--read-only')) await page.goto(`http://localhost:4200/#/project/${state.project}/code`);
 else {
 await expect(page.getByRole('button',{name:'Accept',exact:true})).toBeVisible();
 await page.getByRole('button',{name:'Accept',exact:true}).click();
 }
 await expect(page.getByRole('navigation',{name:'Breadcrumb'}).getByRole('link',{name:'browser-project',exact:true})).toBeVisible();
 await expect(page.getByRole('alert')).toHaveCount(0);
 await mkdir('test-results/invites-live',{recursive:true});
 await page.screenshot({path:'test-results/invites-live/accepted.png',fullPage:true});
 await page.getByRole('link',{name:'notes.txt',exact:true}).click();
 await expect(page.getByText('browser accepted this project',{exact:false})).toBeVisible();
 await page.screenshot({path:'test-results/invites-live/project-file.png',fullPage:true});
 console.log(process.argv.includes('--read-only')?'PASS fresh Playwright Chromium: accepted pin persists across sign-in; project file decrypted':'PASS fresh Playwright Chromium: live CLI-created grant accepted; project appears without reload; project file decrypted');
 await context.close();
} finally {await browser.close();}
