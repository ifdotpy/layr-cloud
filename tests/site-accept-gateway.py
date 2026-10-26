#!/usr/bin/env python3
"""Run as root in disposable layr-sa after building core c1d7229 and gateway.

Requires MinIO at /usr/local/bin/minio and boto3/btrfs-progs/openssl. Fixture data
and synthetic person words stay in /run/site-accept. Exposes the TLS test gateway
on 19003 until /run/site-accept/finish exists; delete the VM after acceptance.
"""
import os,json,time,secrets,subprocess,socket,ssl,re,http.server,http.client,threading
from pathlib import Path
import boto3
from botocore.config import Config
home=Path('/run/site-accept');home.mkdir(exist_ok=False);os.chmod(home,0o755)
layr='/root/site-accept/core/target/debug/layr';gateway='/root/site-accept/core/target/debug/layr-gateway'
# Gateway drops uid; binaries and static files must be traversable.
os.chmod('/root',0o755)
def run(args,env=None,cwd=None):
 r=subprocess.run(args,env=env,cwd=cwd,text=True,capture_output=True)
 if r.returncode: raise RuntimeError('command '+args[0]+' failed: '+r.stderr[-1800:])
 return r.stdout
root=home/'store';root.mkdir(exist_ok=True)
image=home/'store.img'
with image.open('wb') as f:f.truncate(1024**3)
run(['mkfs.btrfs','-q',str(image)]);run(['mount','-o','loop,user_subvol_rm_allowed',str(image),str(root)])
access,secret=secrets.token_hex(12),secrets.token_hex(24)
env={k:v for k,v in os.environ.items() if not k.startswith(('LAYR_','AWS_'))}
env.update(LAYR_ROOT=str(root),LAYR_AUTHOR_NAME='Acceptance',LAYR_AUTHOR_EMAIL='accept@example.test',AWS_ACCESS_KEY_ID=access,AWS_SECRET_ACCESS_KEY=secret,AWS_ENDPOINT_URL_S3='http://127.0.0.1:19000',AWS_DEFAULT_REGION='us-east-1')
minenv=dict(env,MINIO_ROOT_USER=access,MINIO_ROOT_PASSWORD=secret)
log=(home/'minio.log').open('w');minio=subprocess.Popen(['/usr/local/bin/minio','server',str(home/'minio'),'--address','127.0.0.1:19000','--console-address','127.0.0.1:19001'],env=minenv,stdout=log,stderr=log)
s3=boto3.client('s3',endpoint_url=env['AWS_ENDPOINT_URL_S3'],aws_access_key_id=access,aws_secret_access_key=secret,region_name='us-east-1',config=Config(signature_version='s3v4',retries={'max_attempts':0}))
for _ in range(100):
 try:s3.list_buckets();break
 except Exception:time.sleep(.1)
bucket='site-accept';s3.create_bucket(Bucket=bucket)
source=home/'source';source.mkdir();(source/'notes.txt').write_text('browser accepted this project\n')
run([layr,'init','browser-project','--from',str(source),'-q'],env,home)
line=root/'projects/browser-project/lines/main'
run([layr,'remote','add','owner','s3://'+bucket+'/project','--paper-key-out',str(home/'store-words')],env,line)
run([layr,'push','owner','-q'],env,line)
people={}
for name in ['owner','browser']:
 login=name+'@example.test';directory=home/(name+'-person');words=home/(name+'-words')
 personenv=dict(env,LAYR_PERSON_DIR=str(directory),LAYR_PERSON_LOGIN=login)
 run([layr,'person','keys','--paper-key-out',str(words)],personenv,home)
 recipient=(directory/'recipient.txt').read_text().strip();public=(directory/'signing-public.txt').read_text().strip()
 for key,body in [('person.json',json.dumps({'format':1,'recipient':recipient,'created':0,'signing_key':public}).encode()),('slots/paper.age',(directory/'paper.age').read_bytes()),('signing.age',(directory/'signing.age').read_bytes())]:
  s3.put_object(Bucket=bucket,Key='people/'+login+'/'+key,Body=body)
 people[name]={'login':login,'directory':str(directory),'words':str(words),'recipient':recipient}
# Establish the owner's pin, then issue the browser grant with the actual gateway caller.
owner=people['owner'];ownenv=dict(env,LAYR_PERSON_DIR=owner['directory'],LAYR_PERSON_LOGIN=owner['login'],LAYR_PERSON_KEY_FILE=owner['words'])
run([layr,'remote','grant','owner','--person',owner['login'],'--recipient',owner['recipient'],'--invite-out',str(home/'owner-invite.age')],ownenv,line)
run([layr,'person','accept','--invite-file',str(home/'owner-invite.age')],ownenv,home)
s3.put_object(Bucket=bucket,Key='people/'+owner['login']+'/index.age',Body=(Path(owner['directory'])/'index.age').read_bytes())
descriptor=json.loads(s3.get_object(Bucket=bucket,Key='project/store.json')['Body'].read())
role='arn:aws:iam::134250228509:role/test-only'
config={'host':'localhost','listen':'127.0.0.1:19002','people':{'bucket':bucket,'prefix':'people/','region':'us-east-1'},'projects':{'browser':{'bucket':bucket,'prefix':'project','region':'us-east-1','recipient':descriptor['recipient'],'readerRoleArn':role,'writerRoleArn':role,'gcRoleArn':role}}}
configpath=home/'gateway.json';configpath.write_text(json.dumps(config));configpath.chmod(0o644)
token=run([gateway,'token','browser','--write','--config',str(configpath)]).strip();tokenpath=home/'write-token';tokenpath.write_text(token);tokenpath.chmod(0o600)
# Rebuild writes config atomically, so restore permissions before the service reads it.
configpath.chmod(0o644)
glog=(home/'gateway.log').open('w');gw=subprocess.Popen(['setpriv','--reuid=nobody','--regid=nogroup','--clear-groups',gateway,'serve','--tls','none','--test-static-credentials','--config',str(configpath)],env=env,stdout=glog,stderr=glog)
for _ in range(100):
 try:
  with socket.create_connection(('127.0.0.1',19002),timeout=1):break
 except OSError:time.sleep(.1)
pem=subprocess.check_output(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-subj','/CN=localhost','-addext','subjectAltName=DNS:localhost,IP:127.0.0.1','-addext','basicConstraints=critical,CA:FALSE','-keyout','/dev/stdout','-out','/dev/stdout'],stderr=subprocess.DEVNULL)
cert=re.search(rb'-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----',pem,re.S)[0];key=re.search(rb'-----BEGIN PRIVATE KEY-----.*?-----END PRIVATE KEY-----',pem,re.S)[0]
certfd,keyfd=os.memfd_create('test-cert'),os.memfd_create('test-key');os.write(certfd,cert);os.write(keyfd,key)
ca=home/'ca.pem';ca.write_bytes(cert)
context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain('/proc/self/fd/'+str(certfd),'/proc/self/fd/'+str(keyfd))
trace=[]
class Proxy(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def forward(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')))
  c=http.client.HTTPConnection('127.0.0.1',19002,timeout=30)
  h={k:v for k,v in self.headers.items() if k.lower() not in ('host','connection','content-length')};h['Host']='localhost'
  c.request(self.command,self.path,body,h);r=c.getresponse();data=r.read()
  trace.append({'method':self.command,'path':self.path,'status':r.status})
  self.send_response(r.status)
  for k,v in r.getheaders():
   if k.lower() not in ('content-length','connection','transfer-encoding'):self.send_header(k,v)
  self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data);c.close()
 do_GET=do_POST=do_PUT=do_DELETE=forward
proxy=http.server.ThreadingHTTPServer(('0.0.0.0',19003),Proxy);proxy.socket=context.wrap_socket(proxy.socket,server_side=True);threading.Thread(target=proxy.serve_forever,daemon=True).start()
site='https://localhost:19003';url='layr+'+site+'/browser'
ownenv.update(LAYR_GATEWAY_CA_FILE=str(ca),LAYR_GATEWAY_TOKEN_FILE=str(tokenpath))
run([layr,'remote','add','gateway',url],ownenv,line)
reader=people['browser'];run([layr,'remote','grant','gateway','--person',reader['login'],'--recipient',reader['recipient']],ownenv,line)
run([layr,'push','gateway','-q'],ownenv,line)
# Non-secret control file. Disposable words remain in tmpfs.
state={'site':site,'project':descriptor['project'],'people':people,'line':str(line),'root':str(root),'url':url,'trace':trace}
(home/'state.json').write_text(json.dumps(state));(home/'env.json').write_text(json.dumps(ownenv));(home/'env.json').chmod(0o600)
print('READY gateway/minio and CLI-created browser invite',flush=True)
(home/'ready').touch()
try:
 while not (home/'finish').exists():time.sleep(1)
finally:
 (home/'trace.json').write_text(json.dumps(trace));proxy.shutdown();gw.terminate();minio.terminate();gw.wait();minio.wait()
