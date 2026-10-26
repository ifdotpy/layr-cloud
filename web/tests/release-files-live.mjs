/** Real public release: CLI archive -> MinIO -> gateway stream -> worker SHA-256 -> browser download. */
import assert from 'node:assert/strict';
import {spawn,execFileSync} from 'node:child_process';
import {randomBytes,createHash} from 'node:crypto';
import {mkdtemp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createServer} from 'node:net';
import {createServer as createVite} from 'vite';
import {chromium} from '@playwright/test';
const root=fileURLToPath(new URL('../../',import.meta.url));
const fixture=process.env.LAYR_RELEASE_FIXTURE;
assert(fixture,'Set LAYR_RELEASE_FIXTURE to the t47 real CLI export.');
const metadata=JSON.parse(await readFile(path.join(fixture,'fixture.json'),'utf8'));
const descriptor=JSON.parse(await readFile(path.join(fixture,'store/store.json'),'utf8'));
const home=await mkdtemp(path.join(tmpdir(),'layr-release-browser-'));
const vmHome='/root/work/'+path.basename(home);
const access=randomBytes(12).toString('hex'),secret=randomBytes(24).toString('hex');
const vmPort=19472;
const vm=process.env.LAYR_RELEASE_VM || 'layr-rf';
const quote=value=>"'"+value.replaceAll("'","'\\''")+"'";
const children=[];let vite,browser;const logs=[];
function start(command,args,options={}){const child=spawn(command,args,{...options,stdio:['ignore','pipe','pipe']});child.stdout.on('data',b=>logs.push(b.toString()));child.stderr.on('data',b=>logs.push(b.toString()));child.on('error',error=>{child.spawnError=error;});children.push(child);return child;}
async function port(){const server=createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const p=server.address().port;await new Promise(r=>server.close(r));return p;}
async function ready(url){const deadline=Date.now()+30000;while(Date.now()<deadline){const failed=children.find(child=>child.spawnError);if(failed)throw failed.spawnError;try{const r=await fetch(url);if(r.ok){await r.arrayBuffer();return;}}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Server did not become ready.');}
try {
 execFileSync(path.join(root,'target/debug/examples/release_fixture'),[fixture],{stdio:'pipe'});
 await mkdir(path.join(home,'people'));
 execFileSync('orb',['-m',vm,'-u','root','mkdir','-m','700',vmHome]);
 const launch=`echo $$ > ${quote(vmHome+'/pid')}\nexec env MINIO_ROOT_USER=${quote(access)} MINIO_ROOT_PASSWORD=${quote(secret)} /root/work/remote-store-tools/minio server ${quote(vmHome+'/data')} --address 127.0.0.1:${vmPort} --console-address 127.0.0.1:0 --quiet`;
 execFileSync('orb',['-m',vm,'-u','root','bash','-c',`setsid nohup bash -c ${quote(launch)} >${quote(vmHome+'/minio.log')} 2>&1 </dev/null & disown`]);
 const localMinio=await port();start('ssh',['-o','BatchMode=yes','-o','ExitOnForwardFailure=yes','-N','-L',`127.0.0.1:${localMinio}:127.0.0.1:${vmPort}`,`root@${vm}@orb`]);
 const endpoint=`http://127.0.0.1:${localMinio}`;await ready(endpoint+'/minio/health/ready');
 const env={...process.env,AWS_ACCESS_KEY_ID:access,AWS_SECRET_ACCESS_KEY:secret,AWS_ENDPOINT_URL_S3:endpoint};
 const bucket='release-'+randomBytes(8).toString('hex');
 const uploadDeadline=Date.now()+30000;
 for(;;){try{execFileSync(path.join(root,'target/debug/examples/upload_gateway'),[bucket,path.join(fixture,'store'),path.join(home,'people')],{env,stdio:'pipe',timeout:120000});break;}catch(error){const message=error.stderr?.toString()||'';if(!message.includes('HTTP 503 Service Unavailable, XMinioServerNotInitialized')||Date.now()>=uploadDeadline)throw error;await new Promise(r=>setTimeout(r,100));}}

 const gatewayPort=await port(),role='arn:aws:iam::123456789012:role/release-test';
 const config={host:'localhost',listen:`127.0.0.1:${gatewayPort}`,projects:{example:{bucket,region:'us-east-1',prefix:'fixture',readerRoleArn:role,writerRoleArn:role,gcRoleArn:role,recipient:descriptor.recipient,writeTokenHashes:[],gcTokenHashes:[]}}};
 await writeFile(path.join(home,'gateway.json'),JSON.stringify(config),{mode:0o600});
 start(path.join(root,'target/debug/layr-gateway'),['serve','--config',path.join(home,'gateway.json'),'--tls','none','--test-static-credentials'],{env});
 await ready(`http://localhost:${gatewayPort}/p/example/store.json`);
 const proxy=Object.fromEntries(['/p','/api','/v1'].map(prefix=>[prefix,{target:`http://localhost:${gatewayPort}`,changeOrigin:true,headers:{origin:`http://localhost:${gatewayPort}`}}]));
 vite=await createVite({configFile:false,root:path.join(root,'web'),server:{host:'127.0.0.1',port:0,watch:null,hmr:false,proxy},worker:{format:'es'}});await vite.listen();
 const site=vite.resolvedUrls.local[0];
 const url=`${site}?project=example#/project/${descriptor.project}/deploy/release/${encodeURIComponent(metadata.reference)}`;
 await writeFile(path.join(home,'runtime.json'),JSON.stringify({url,home,fixture}));console.log(JSON.stringify({url,home}));
 browser=await chromium.launch({headless:true});const page=await browser.newPage();const errors=[];const fileRequests=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>{if(r.url().includes('/files/'))fileRequests.push(r.url());});
 await page.goto(url);await page.getByRole('table',{name:'Release files',exact:true}).waitFor({timeout:30000});
 const table=page.getByRole('table',{name:'Release files',exact:true});
 const good=table.getByRole('row').filter({hasText:'layr-linux-arm64'});
 const pending=page.waitForEvent('download');await good.getByRole('button',{name:'Download',exact:true}).click();let download;try{download=await pending;}catch(error){throw new Error('Download failed: '+JSON.stringify({alerts:await page.getByRole('alert').allTextContents(),errors})+'; '+error.message);}
 const sha=createHash('sha256');let size=0;for await(const part of await download.createReadStream()){sha.update(part);size+=part.length;}
 assert.equal(size,10<<20);assert.equal(sha.digest('hex'),metadata.sha256);assert.equal(download.suggestedFilename(),'layr-linux-arm64');
 console.log('PASS real public browser download: 10 MiB, expected SHA-256 and filename');
 let saved=0;page.on('download',()=>saved++);
 await table.getByRole('row').filter({hasText:'sha-mismatch'}).getByRole('button',{name:'Download',exact:true}).click();
 await page.getByRole('alert').filter({hasText:'SHA-256'}).waitFor();assert.equal(saved,0);
 console.log('PASS valid-age wrong plaintext shows SHA-256 error and saves nothing');
 execFileSync(path.join(root,'target/debug/examples/release_fixture'),[fixture,'truncate',bucket],{env,stdio:'pipe'});
 await table.getByRole('row').filter({hasText:'sha-mismatch'}).getByRole('button',{name:'Download',exact:true}).click();
 await page.waitForFunction(()=>{const alert=document.querySelector('[role=alert]');return alert && !alert.textContent.includes('SHA-256');});
 assert.equal(saved,0);console.log('PASS truncated age object shows an error and saves nothing');

 const large=table.getByRole('row').filter({hasText:'large-file'});assert.match(await large.textContent(),/256 MiB/);assert.match(await large.textContent(),/layr release get/);
 assert.equal(await large.getByRole('button',{name:'Download',exact:true}).count(),0);
 assert.equal(fileRequests.length,3,'large files must not be requested');assert.deepEqual(errors,[]);
 await page.setViewportSize({width:390,height:844});await page.screenshot({path:path.join(home,'release-files-mobile.png'),fullPage:true});
 await page.setViewportSize({width:1280,height:900});await page.screenshot({path:path.join(home,'release-files-desktop.png'),fullPage:true});
 console.log('PASS over-limit CLI note and no fetch; public reader uses no login');
 if(metadata.prefix) {
  assert(fileRequests.slice(0,3).every(url=>url.includes('/'+metadata.prefix)), 'current release files must be fetched from the current epoch');
  console.log('PASS current-epoch release file uses its canonical epoch source and session identity');
  await page.goto(url.replace(encodeURIComponent(metadata.reference),encodeURIComponent(metadata.previous)));
  const previous=page.getByRole('table',{name:'Release files',exact:true});await previous.waitFor();
  const earlier=page.waitForEvent('download');await previous.getByRole('button',{name:'Download',exact:true}).click();
  const savedEarlier=await earlier;const oldHash=createHash('sha256');let oldSize=0;
  for await(const part of await savedEarlier.createReadStream()){oldHash.update(part);oldSize+=part.length;}
  assert.equal(oldSize,10<<20);assert.equal(oldHash.digest('hex'),metadata.sha256);
  assert(fileRequests.slice(3).some(url=>!url.includes('/project/epochs/')), 'historical release must resolve its root source');
  console.log('PASS historical release resolves its retained root object and verifies with the sealed history identity');
 }
 await writeFile(path.join(home,'result.json'),JSON.stringify({size,sha256:metadata.sha256,fileRequests:fileRequests.length,checks:metadata.prefix?6:4,epochPrefix:metadata.prefix||null}));
 if(process.env.LAYR_RELEASE_KEEP_SERVERS){console.log('READY_FOR_VISUAL_CHECK');await new Promise(resolve=>{const interval=setInterval(async()=>{try{await readFile(path.join(home,'done'));clearInterval(interval);resolve();}catch{}},1000);});}
} catch(error){console.error(logs.join('').replaceAll(access,'[test access]').replaceAll(secret,'[test secret]'));throw error;}
finally {await browser?.close();await vite?.close();for(const child of children)child.kill('SIGTERM');try{execFileSync('orb',['-m',vm,'-u','root','bash','-c',`if [ -f ${quote(vmHome+'/pid')} ]; then kill "$(cat ${quote(vmHome+'/pid')})" 2>/dev/null || true; fi`]);}catch{}}
