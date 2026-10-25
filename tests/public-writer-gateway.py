#!/usr/bin/env python3
"""Live CLI -> HTTPS trace proxy -> gateway -> MinIO public-writer regression.

Run as root in the disposable Linux VM with boto3, btrfs-progs, MinIO and OpenSSL.
Set LAYR_BIN and LAYR_GATEWAY_BINARY to the exact binaries under test. All test
data and recovery/token files stay under /run; TLS private keys use memfd only.
The trace contains methods, paths, status codes, and credential presence only.
"""
import contextlib
import http.client
import http.server
import json
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
    home = Path(tempfile.mkdtemp(prefix='m8-public-writer-', dir='/run'))
    home.chmod(0o755)
    children, mounts, descriptors = [], [], []
    proxy = None
    trace = []
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
        cli(['remote', 'public', 'owner', 'on'], stdin='gateway-project\n')
        descriptor = json.loads(s3.get_object(Bucket=bucket, Key='project/store.json')['Body'].read())
        own = json.loads(cli(['remote', 'self', '--json']).stdout)
        manifest = json.loads(s3.get_object(Bucket=bucket, Key='project/public.json')['Body'].read())
        assert manifest['machine'] == own['id']
        s3.head_object(Bucket=bucket, Key=f"project/slots/machine-{own['id']}.age")
        role = 'arn:aws:iam::134250228509:role/test-only'
        config = {'host': 'localhost', 'listen': f'127.0.0.1:{gateway_port}', 'projects': {'public-test': {
            'bucket': bucket, 'prefix': 'project', 'region': 'us-east-1', 'recipient': descriptor['recipient'],
            'readerRoleArn': role, 'writerRoleArn': role, 'gcRoleArn': role}}}
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
            '--tailscale-socket', str(home / 'absent-whois.sock')], env=env,
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
        (line / 'notes.txt').write_text('writer edit after publication\n')
        cli(['save', '-am', 'Writer change after publication', '-q'], aws=False)
        before = s3.list_objects_v2(Bucket=bucket, Prefix='project/states/').get('KeyCount', 0)
        trace.clear()
        pushed = cli(['push', 'gateway', '-q'], aws=False, ok=False)
        print(json.dumps({'push_exit': pushed.returncode, 'requests': trace}))
        if b'gateway reads require a machine slot or paper key' in pushed.stderr.encode():
            print('CLI error: gateway reads require a machine slot or paper key')
        public_requests = [row for row in trace if row['path'].startswith('/p/')]
        assert [(row['method'], row['path']) for row in public_requests] == [
            ('GET', '/p/public-test/public.json')], 'writer may read only the public manifest anonymously, once'
        assert not public_requests[0]['credential_present'], 'public manifest probe must not carry a bearer'
        assert all(row['path'].startswith(('/v1/p/', '/p/')) for row in trace)
        assert pushed.returncode == 0, 'trusted writer push to a public project failed'
        writes = [row for row in trace if row['method'] in ('PUT', 'DELETE', 'PATCH')]
        assert writes and all(row['path'].startswith('/v1/p/') and row['write_bearer'] for row in writes), \
            'every object write must use the authenticated route and exact writer bearer'
        assert any(row['method'] == 'PUT' and row['status'] == 201 for row in writes)
        after = s3.list_objects_v2(Bucket=bucket, Prefix='project/states/').get('KeyCount', 0)
        assert after > before, 'gateway push added no state objects'
        print('PASS trusted writer pushed a new public-project state through authenticated gateway transport')
        if os.environ.get('LAYR_TEST_INDEX') == '1':
            trace.clear()
            indexed = cli(['index', 'write', 'gateway'], aws=False, ok=False)
            print(json.dumps({'index_exit': indexed.returncode, 'requests': trace}))
            assert indexed.returncode == 0, 'executor index write through gateway failed'
            writes = [row for row in trace if row['method'] == 'PUT']
            assert writes and all(row['path'].startswith('/v1/p/public-test/objects/index/')
                                  and row['write_bearer'] and row['status'] == 201 for row in writes)
            assert s3.list_objects_v2(Bucket=bucket, Prefix='project/index/').get('KeyCount', 0) > 0
            print('PASS executor index write published immutable signed index objects through gateway')
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
