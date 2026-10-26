import {expect,test} from '@playwright/test';
import type {Project,Ref} from '../src/data/source';
import {fixture} from './fixture-data';
const project=fixture<Project[]>('projects')[0];
const release={name:'release/1.2.0',target:fixture<Ref[]>('refs')[0].target};

test('release files show a count, download verified bytes, report errors and show the large-file command',async({page})=>{
 await page.route('**/src/data/fixture-source.ts',async route=>{
  const response=await route.fetch();
  await route.fulfill({response,body:await response.text()+`\nconst originalRefs = FixtureSource.prototype.refs;\nFixtureSource.prototype.refs = async function(project) { return [...await originalRefs.call(this,project), ${JSON.stringify(release)}]; };\nFixtureSource.prototype.releaseFiles = async () => [
   {name:'layr-arm64',size:10485760,sha256:'a'.repeat(64),object:'files/test.age'},
   {name:'corrupt-file',size:1,sha256:'b'.repeat(64),object:'files/other.age'},
   {name:'large-file',size:268435457,sha256:'c'.repeat(64),object:'files/large.age'}];
  FixtureSource.prototype.releaseDownload = async (_project,_reference,name) => {
   if(name==='corrupt-file') throw new Error('release file SHA-256 or size mismatch');
   return new Blob([new Uint8Array(10485760)], {type:'application/octet-stream'});
  };`});
 });
 await page.goto(`/?source=fixture#/project/${project.id}/deploy/releases`);
 const table=page.getByRole('table',{name:'Releases',exact:true});
 await expect(table.getByRole('columnheader',{name:'Files',exact:true})).toBeVisible();
 await page.goto(`/?source=fixture#/project/${project.id}/deploy/release/${encodeURIComponent(release.name)}`);
 const files=page.getByRole('table',{name:'Release files',exact:true});
 await expect(files).toBeVisible();
 const downloadPromise=page.waitForEvent('download');
 await files.getByRole('row').filter({hasText:'layr-arm64'}).getByRole('button',{name:'Download',exact:true}).click();
 const download=await downloadPromise;expect(download.suggestedFilename()).toBe('layr-arm64');
 const stream=await download.createReadStream();let length=0;for await(const bytes of stream!)length+=bytes.length;expect(length).toBe(10485760);
 let saved=0;page.on('download',()=>saved++);
 await files.getByRole('row').filter({hasText:'corrupt-file'}).getByRole('button',{name:'Download',exact:true}).click();
 await expect(page.getByRole('alert')).toContainText('SHA-256');expect(saved).toBe(0);
 const large=files.getByRole('row').filter({hasText:'large-file'});
 await expect(large).toContainText('256 MiB');await expect(large).toContainText(`layr release get ${release.name} large-file`);
 await expect(large.getByRole('button',{name:'Download',exact:true})).toHaveCount(0);
});
