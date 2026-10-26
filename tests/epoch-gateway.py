#!/usr/bin/env python3
"""Actual CLI rotation order through HTTPS -> gateway -> MinIO.

Run as root in the disposable Linux VM with boto3, btrfs-progs, MinIO and OpenSSL.
Set LAYR_BIN and LAYR_GATEWAY_BINARY to the exact binaries under test. All test
data and recovery/token files stay under /run; TLS private keys use memfd only.
The trace contains methods, paths, status codes, and credential presence only.
"""
import contextlib
import http.client
import http.server
import json
import hashlib
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
import time

import boto3
from botocore.config import Config


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def command(args, **kwargs):
    return subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=120, **kwargs)


def main():
    assert os.geteuid() == 0, 'run in the disposable root Linux VM'
    assert subprocess.check_output(['findmnt', '-nro', 'FSTYPE', '-T', '/run'], text=True).strip() == 'tmpfs'
    layr = os.environ['LAYR_BIN']
    gateway = os.environ['LAYR_GATEWAY_BINARY']
    home = Path(tempfile.mkdtemp(prefix='epoch-gateway-', dir='/run'))
    home.chmod(0o755)
    children, mounts, descriptors = [], [], []
    proxy = None
    trace = []
    interrupt = {'armed':os.environ.get('LAYR_TEST_INTERRUPT_ROTATION')=='1','hit':False}
    try:
        root = home / 'owner'
        root.mkdir()
        image = home / 'owner.img'
        with image.open('wb') as out:
            out.truncate(1024 * 1024 * 1024)
        assert command(['mkfs.btrfs', '-q', str(image)]).returncode == 0
        assert command(['mount', '-o', 'loop,user_subvol_rm_allowed', str(image), str(root)]).returncode == 0
        mounts.append(root)
        minio_port, console_port, gateway_port = port(), port(), port()
        endpoint = f'http://127.0.0.1:{minio_port}'
        access, secret = secrets.token_hex(12), secrets.token_hex(24)
        redactions = [access, secret]
        env = {k: v for k, v in os.environ.items() if not k.startswith(('AWS_', 'LAYR_'))}
        env.update(LAYR_ROOT=str(root), LAYR_AUTHOR_NAME='M8 Test', LAYR_AUTHOR_EMAIL='m8@example.test',
                   AWS_ACCESS_KEY_ID=access, AWS_SECRET_ACCESS_KEY=secret,
                   AWS_ENDPOINT_URL_S3=endpoint, AWS_DEFAULT_REGION='us-east-1')
        minio_env = dict(env, MINIO_ROOT_USER=access, MINIO_ROOT_PASSWORD=secret)
        children.append(subprocess.Popen([os.environ.get('MINIO_BINARY', '/usr/local/bin/minio'), 'server',
            '--address', f'127.0.0.1:{minio_port}', '--console-address', f'127.0.0.1:{console_port}',
            str(home / 'minio')], env=minio_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        s3 = boto3.client('s3', endpoint_url=endpoint, aws_access_key_id=access,
            aws_secret_access_key=secret, region_name='us-east-1',
            config=Config(signature_version='s3v4', retries={'max_attempts': 0}, connect_timeout=1, read_timeout=2))
        deadline = time.monotonic() + 30
        while True:
            try:
                s3.list_buckets()
                break
            except Exception:
                assert time.monotonic() < deadline, 'MinIO did not start'
                time.sleep(0.1)
        if os.environ.get('LAYR_TEST_T27') == '1':
            caller = dict(env, TMPDIR='/run', LAYR_BIN=layr, LAYR_GATEWAY_BINARY=gateway,
                LAYR_MINIO_ENDPOINT=endpoint, LAYR_MINIO_ACCESS_KEY=access, LAYR_MINIO_SECRET_KEY=secret)
            result = subprocess.run(['bash', str(Path(__file__).with_name('t27-gateway.sh'))],
                env=caller, text=True, capture_output=True, timeout=300)
            output = result.stdout + result.stderr
            for value in redactions:
                output = output.replace(value, '[redacted]')
            print(output)
            assert result.returncode == 0, 'actual gateway MinIO t27 failed'
            return
        bucket = 'm8-public-writer'
        s3.create_bucket(Bucket=bucket)
        source = home / 'source'
        source.mkdir()
        (source / 'notes.txt').write_text('before publication\n')
        line = root / 'projects/gateway-project/lines/main'

        def cli(args, *, stdin=None, aws=True, ok=True):
            caller = env.copy()
            if not aws:
                caller = {k: v for k, v in caller.items() if not k.startswith('AWS_')}
            result = command([layr] + args, cwd=line if line.exists() else home,
                             env=caller, input=stdin, text=True)
            if ok and result.returncode:
                error = result.stderr
                for value in redactions:
                    error = error.replace(value, '[redacted]')
                error = re.sub(r'AGE-SECRET-KEY-\S+', '[redacted]', error)
                print(json.dumps({'setup': args[0], 'error': error, 'requests': trace}))
            assert not ok or result.returncode == 0, f'CLI setup {args[0]} failed (exit {result.returncode})'
            return result

        cli(['init', 'gateway-project', '--from', str(source), '-q'])
        cli(['remote', 'add', 'owner', f's3://{bucket}/project', '--paper-key-out', str(home / 'paper')])
        redactions.append((home / 'paper').read_text().strip())
        cli(['push', 'owner', '-q'])
        descriptor = json.loads(s3.get_object(Bucket=bucket, Key='project/store.json')['Body'].read())
        own = json.loads(cli(['remote', 'self', '--json']).stdout)
        s3.head_object(Bucket=bucket, Key=f"project/slots/machine-{own['id']}.age")
        role = 'arn:aws:iam::134250228509:role/test-only'
        config = {'host': 'localhost', 'listen': f'127.0.0.1:{gateway_port}', 'projects': {'public-test': {
            'bucket': bucket, 'prefix': 'project', 'region': 'us-east-1', 'recipient': descriptor['recipient'],
            'readerRoleArn': role, 'writerRoleArn': role, 'gcRoleArn': role}}}
        if os.environ.get('LAYR_TEST_REQUEST') == '1':
            config['people'] = {'bucket': bucket, 'region': 'us-east-1', 'prefix': 'people/'}
        config_path = home / 'gateway.json'
        config_path.write_text(json.dumps(config))
        config_path.chmod(0o644)
        issued = command([gateway, 'token', 'public-test', '--write', '--config', str(config_path)])
        assert issued.returncode == 0, 'write token issue failed'
        redactions.append(issued.stdout.decode().strip())
        token = home / 'write-token'
        fd = os.open(token, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, 'wb') as out:
            out.write(issued.stdout.strip())
        env['LAYR_GATEWAY_TOKEN_FILE'] = str(token)
        children.append(subprocess.Popen(['setpriv', '--reuid=nobody', '--regid=nogroup', '--clear-groups',
            gateway, 'serve', '--tls', 'none', '--test-static-credentials', '--config', str(config_path),
            ], env=env,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        deadline = time.monotonic() + 30
        while True:
            try:
                with socket.create_connection(('127.0.0.1', gateway_port), timeout=1):
                    break
            except OSError:
                assert time.monotonic() < deadline, 'gateway did not start'
                time.sleep(0.1)

        # Capture the PEM output in memory. No test TLS private key reaches a file.
        pem = command(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
            '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1',
            '-addext', 'basicConstraints=critical,CA:FALSE',
            '-keyout', '/dev/stdout', '-out', '/dev/stdout'])
        assert pem.returncode == 0
        cert = re.search(rb'-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----', pem.stdout, re.S)[0]
        key = re.search(rb'-----BEGIN PRIVATE KEY-----.*?-----END PRIVATE KEY-----', pem.stdout, re.S)[0]
        cert_fd, key_fd = os.memfd_create('test-cert'), os.memfd_create('test-key')
        descriptors += [cert_fd, key_fd]
        os.write(cert_fd, cert)
        os.write(key_fd, key)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(f'/proc/self/fd/{cert_fd}', f'/proc/self/fd/{key_fd}')
        ca = home / 'test-ca.pem'
        ca.write_bytes(cert)
        env['LAYR_GATEWAY_CA_FILE'] = str(ca)

        class Trace(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def forward(self):
                body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
                headers = {name: value for name, value in self.headers.items() if name.lower() != 'host'}
                headers['Host'] = 'localhost'
                connection = http.client.HTTPConnection('127.0.0.1', gateway_port, timeout=30)
                connection.request(self.command, self.path, body, headers)
                reply = connection.getresponse()
                data = reply.read()
                trace.append({'method': self.command, 'path': self.path, 'status': reply.status,
                              'credential_present': 'Authorization' in self.headers,
                              'write_bearer': self.headers.get('Authorization') == 'Bearer ' + issued.stdout.decode().strip()})
                if interrupt['armed'] and self.command=='PUT' and '/objects/keys/' in self.path and reply.status==201:
                    interrupt['armed']=False;interrupt['hit']=True
                    # The discovery did land. Simulate an interruption before its caller can continue.
                    self.send_response(503);self.send_header('Content-Length','0');self.end_headers();connection.close();return
                self.send_response(reply.status)
                for name, value in reply.getheaders():
                    if name.lower() not in ('connection', 'transfer-encoding', 'content-length'):
                        self.send_header(name, value)
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                connection.close()

            do_GET = do_POST = do_PUT = do_DELETE = forward

        proxy = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Trace)
        proxy.socket = context.wrap_socket(proxy.socket, server_side=True)
        threading.Thread(target=proxy.serve_forever, daemon=True).start()
        url = f'layr+https://localhost:{proxy.server_port}/public-test'
        cli(['remote', 'add', 'gateway', url], aws=False)
        # Give the project a private discovery boundary before exercising the writer.
        env['LAYR_PERSON_LOGIN']='hidden@example.test'
        cli(['remote','grant','owner','--person','hidden@example.test','--recipient',descriptor['recipient'],'--invite-out',str(home/'hidden-invite.age')])
        env.pop('LAYR_PERSON_LOGIN')
        callers={}
        for name in ['executor','removed']:
            caller=home/name;caller.mkdir();image=home/(name+'.img')
            with image.open('wb') as out:out.truncate(1024*1024*1024)
            assert command(['mkfs.btrfs','-q',str(image)]).returncode==0
            assert command(['mount','-o','loop,user_subvol_rm_allowed',str(image),str(caller)]).returncode==0
            mounts.append(caller)
            caller_env={k:v for k,v in env.items() if not k.startswith('AWS_')}
            caller_env['LAYR_ROOT']=str(caller)
            result=command([layr,'remote','self','--json'],env=caller_env,cwd=home,text=True)
            assert result.returncode==0,result.stderr
            machine=json.loads(result.stdout);callers[name]=(caller_env,machine)
            cli(['remote','trust','owner',machine['id'],'--public-key',machine['public_key']])
        cli(['push','owner','-q'])
        removed_env,removed=callers['removed']
        result=command([layr,'clone',url,'removed','-q'],env=removed_env,cwd=home,text=True)
        assert result.returncode==0,result.stderr
        trace.clear()
        cli(['push','gateway','-q'],aws=False)
        assert any(row['path']=='/v1/p/public-test/objects?prefix=keys%2F' and row['status']==200 for row in trace)
        print('PASS ddcce23-or-later CLI pushes a private root store through authenticated keys discovery')
        env['LAYR_PAPER_KEY_FILE']=str(home/'paper')
        trace.clear()
        rotated=cli(['remote','rotate','gateway','--remove','machine:'+removed['id'],'--reason','gateway order regression'],aws=False,ok=False)
        print(json.dumps({'rotate_exit':rotated.returncode,'requests':trace}))
        if os.environ.get('LAYR_EXPECT_ROTATE_FAILURE')=='1':
            assert rotated.returncode!=0,'baseline unexpectedly passed; regression did not reproduce'
            assert any(row['method']=='PUT' and '/project/epochs/' in row['path'] and '/slots/' in row['path'] and row['status']==404 for row in trace),'failure was not the staged slot proof boundary'
            print('EXPECTED FAILURE: ddcce23 sends an old proof on staged epoch slot PUT')
            return
        if os.environ.get('LAYR_TEST_INTERRUPT_ROTATION')=='1':
            assert rotated.returncode!=0 and interrupt['hit'],'interruption did not follow discovery publication'
            before=s3.list_objects_v2(Bucket=bucket,Prefix='project/keys/')['Contents']
            assert len(before)==1
            pending_files=list(root.rglob('project-keys/*.age'));assert len(pending_files)==1
            assert pending_files[0].stat().st_mode & 0o777 == 0o600
            executor_env,_=callers['executor']
            blocked=command([layr,'clone',url,'rotation-pending','-q'],env=executor_env,cwd=home,text=True)
            assert blocked.returncode!=0 and 'rotation in progress' in blocked.stderr,blocked.stderr
            # Repeating rotate without another remove list resumes the exact saved audience.
            rotated=cli(['remote','rotate','gateway'],aws=False,ok=False)
            assert rotated.returncode==0,rotated.stderr
            after=s3.list_objects_v2(Bucket=bucket,Prefix='project/keys/')['Contents']
            assert [r['Key'] for r in before]==[r['Key'] for r in after]
            assert not list(root.rglob('project-keys/*.age')),'pending private identity remains after completion'
            print('PASS interruption after discovery fails readers closed with rotation in progress; retry resumes one epoch and deletes its local sealed identity')
        assert rotated.returncode==0,rotated.stderr
        keys=s3.list_objects_v2(Bucket=bucket,Prefix='project/keys/')['Contents']
        assert len(keys)==1
        publication=json.loads(s3.get_object(Bucket=bucket,Key=keys[0]['Key'])['Body'].read())
        epoch=publication['r']['data']['epoch']
        assert not any(row['write_bearer'] and row['method']=='GET' and '/objects?prefix=project' in row['path'] for row in trace)
        discovery=next(i for i,row in enumerate(trace) if row['method']=='PUT' and '/objects/keys/' in row['path'])
        proof=next(i for i,row in enumerate(trace) if row['method']=='GET' and row['path'].endswith('/challenge?epoch='+epoch))
        assert discovery<proof,'new proof must follow signed discovery'
        assert all(i>proof for i,row in enumerate(trace) if row['method']=='PUT' and '/project/epochs/' in row['path'] and any(p in row['path'] for p in ['/records/','/states/'])),'new epoch data was pushed before its read proof'
        assert all(i<discovery for i,row in enumerate(trace) if row['method']=='PUT' and '/slots/' in row['path'] and row['status']==201)
        assert all(row['write_bearer'] for row in trace if row['method']=='PUT' and ('/slots/' in row['path'] or '/history/' in row['path']))
        executor_env,executor=callers['executor']
        result=command([layr,'clone',url,'epoch-executor','-q'],env=executor_env,cwd=home,text=True)
        assert result.returncode==0,result.stderr
        exported=command([layr,'--project','epoch-executor','op','export'],env=executor_env,cwd=home,text=True)
        assert exported.returncode==0,exported.stderr
        records=[json.loads(line)['r'] for line in exported.stdout.splitlines()]
        audiences=[r['data']['readers'] for r in records if r['op']=='project.readers' and r['data']['epoch']==epoch]
        assert len(audiences)==1
        assert set(audiences[0])=={'machine:'+own['id'],'machine:'+executor['id'],'paper','person:hidden@example.test'}
        modes=[r for r in records if r['op']=='project.mode' and r['data'].get('epoch')==epoch]
        assert len(modes)==1 and modes[0]['data']['reason']=='gateway order regression'
        assert not list(root.rglob('project-keys/*.age'))
        denied=command([layr,'--project','removed','fetch','origin','-q'],env=removed_env,cwd=home,text=True)
        assert denied.returncode!=0,'removed machine fetched the current epoch'
        print('PASS real gateway rotation stages grant-only objects, publishes discovery, then gets the new read proof')
        print('PASS machine-only clone after rotation; removed reader cannot fetch current data')
    finally:
        if proxy:
            proxy.shutdown()
            proxy.server_close()
        for child in reversed(children):
            child.terminate()
            with contextlib.suppress(subprocess.TimeoutExpired):
                child.wait(timeout=10)
            if child.poll() is None:
                child.kill()
                child.wait()
        for mount in reversed(mounts):
            subprocess.run(['umount', str(mount)], check=True, capture_output=True)
        for fd in descriptors:
            os.close(fd)
        shutil.rmtree(home)
        assert not home.exists()
        print('CLEANUP all disposable recovery/token/test material removed from /run')


if __name__ == '__main__':
    main()
