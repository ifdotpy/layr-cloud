#!/usr/bin/env python3
"""Verify the browser-written index, unchanged CLI accept, and content read.

Run in the fixture created by site-accept-gateway.py after invites-live.mjs.
Only synthetic fixture credentials in the disposable VM are read.
"""
import json,subprocess
from pathlib import Path
import boto3
from botocore.config import Config
h=Path('/run/site-accept');state=json.loads((h/'state.json').read_text());env=json.loads((h/'env.json').read_text());binary='/root/site-accept/core/target/debug/layr'
def run(args,e=env,cwd=state['line']):
 r=subprocess.run([binary]+args,env=e,cwd=cwd,capture_output=True,text=True)
 if r.returncode:raise RuntimeError(args[0]+': '+r.stderr[-1000:])
 return r.stdout
reader=state['people']['browser']
readenv=dict(env,LAYR_PERSON_DIR=reader['directory'],LAYR_PERSON_LOGIN=reader['login'],LAYR_PERSON_KEY_FILE=reader['words'])
# Read the browser-written index using the unchanged CLI, then accept a second grant with it.
view=json.loads(run(['person','show','--site',state['site'],'--json'],readenv))
assert any(pin['project']==state['project'] for pin in view['stores']),'browser pin absent'
s3=boto3.client('s3',endpoint_url=env['AWS_ENDPOINT_URL_S3'],aws_access_key_id=env['AWS_ACCESS_KEY_ID'],aws_secret_access_key=env['AWS_SECRET_ACCESS_KEY'],region_name='us-east-1',config=Config(signature_version='s3v4'))
prefix='people/'+reader['login']+'/inbox/'
assert not s3.list_objects_v2(Bucket='site-accept',Prefix=prefix).get('Contents',[]),'browser did not remove invite'
print('PASS unchanged CLI reads browser-written encrypted index; inbox empty')
run(['remote','grant','gateway','--person',reader['login'],'--recipient',reader['recipient']])
invites=s3.list_objects_v2(Bucket='site-accept',Prefix=prefix)['Contents']
assert len(invites)==1
invite_id=Path(invites[0]['Key']).stem
run(['person','accept','--site',state['site'],'--invite',invite_id],readenv)
print('PASS unchanged CLI accepts next gateway invite after browser acceptance')
# Machine slot gives this synthetic machine access; person pin still comes from the browser-written index.
run(['clone',state['url'],'browser-copy','-q'],readenv,cwd=str(h))
copy=Path(state['root'])/'projects/browser-copy/lines/main/notes.txt'
assert copy.read_text()=='browser accepted this project\n'
print('PASS person CLI reads the project content through gateway and MinIO')
