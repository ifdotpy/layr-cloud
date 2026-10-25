#!/usr/bin/env python3
"""Host loop for the local checks watcher. No SSH or credentials in command arguments."""
from __future__ import annotations
import fcntl
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/run/layr-checks')
BINARY = '/opt/layr-executor/current/layr'
SOURCE = Path('/etc/layr-executor/source.json')
PROFILE = Path('/etc/layr-executor/profile.env')
IDLE_SECONDS = 1800


def watcher_result(result: subprocess.CompletedProcess) -> dict:
    if result.returncode not in (0, 1):
        raise RuntimeError('check scan failed operationally')
    lines = result.stdout.splitlines()
    if len(lines) != 1:
        raise ValueError('watcher must return one JSON line')
    value = json.loads(lines[0])
    if (type(value.get('work_done')) is not bool or type(value.get('count')) is not int
            or value['count'] < 0 or value['work_done'] != (value['count'] > 0)
            or not isinstance(value.get('checks'), list)):
        raise ValueError('invalid watcher result')
    pending = value.get('pending')
    privileged = value.get('pending_privileged')
    if type(pending) is not int or type(privileged) is not int or not 0 <= privileged <= pending:
        raise ValueError('invalid privileged pending count')
    return value


def wake_runner(source: dict, pending: int) -> None:
    instance = source.get('runnerInstanceId', '')
    if pending <= 0 or not instance:
        return
    import boto3
    ec2 = boto3.client('ec2', region_name=source['region'])
    rows = ec2.describe_instances(InstanceIds=[instance])['Reservations']
    state = rows[0]['Instances'][0]['State']['Name']
    if state == 'stopped':
        ec2.start_instances(InstanceIds=[instance])
        print('started check runner', instance, flush=True)
    elif state not in ('pending', 'running', 'stopping'):
        raise RuntimeError('runner instance is unavailable')


def tick(source: dict) -> None:
    text = PROFILE.read_text().strip()
    match = re.fullmatch(r'PROJECT=([A-Za-z0-9][A-Za-z0-9_-]{0,43})', text)
    if not match:
        raise ValueError('invalid executor project profile')
    project = match[1]
    env = dict(os.environ, LAYR_ROOT='/var/lib/layr',
               LAYR_GATEWAY_TOKEN_FILE='/run/layr-executor-token/write-token')
    subprocess.run([BINARY, '--project', project, 'fetch', 'origin'], env=env, check=True,
                   stdout=subprocess.DEVNULL)
    if source.get('hostKind') != 'runner':
        queue = subprocess.run([BINARY, '--project', project, 'config', '--get', 'queue.run'],
                               env=env, text=True, capture_output=True)
        if queue.returncode not in (0, 1) or (queue.returncode == 0 and queue.stdout.strip()):
            raise RuntimeError('executor queue.run must be empty; queues belong on the runner')
        subprocess.run([BINARY, '--project', project, 'requests', 'apply', '--json'], env=env,
                       check=True, stdout=subprocess.DEVNULL)
        # The executor's machine-local policy is reporter-only. It never executes checks.
        for flag in ('checks.runner', 'checks.privileged-runner'):
            subprocess.run([BINARY, '--project', project, 'config', '--local', flag, 'false'],
                           env=env, check=True, stdout=subprocess.DEVNULL)
    args = ['runuser', '-u', 'layr-checks', '--', BINARY, '--project', project,
            'checks', 'watch', '--once', '--json']
    # The reader uses daemon IPC. It receives no gateway token or AWS credentials.
    reader_env = {'PATH': '/usr/local/bin:/usr/bin:/bin', 'LAYR_ROOT': '/var/lib/layr',
                  'RUSTUP_HOME': '/usr/local/share/layr-checks/rustup', 'LAYR_SOCKET': '/run/layr.sock'}
    result = subprocess.run(args, env=reader_env, text=True, capture_output=True)
    value = watcher_result(result)
    if source.get('hostKind') != 'runner' and value['work_done']:
        raise RuntimeError('executor must not run checks')
    if value['work_done']:
        (ROOT / 'last-work').write_text(str(time.monotonic()))
    subprocess.run([BINARY, '--project', project, 'push', 'origin'], env=env, check=True,
                   stdout=subprocess.DEVNULL)
    if source.get('hostKind') != 'runner':
        subprocess.run([BINARY, '--project', project, 'index', 'write', 'origin'],
                       env=env, check=True, stdout=subprocess.DEVNULL)
        wake_runner(source, value['pending'])
    print(json.dumps(value), flush=True)


def idle(source: dict) -> None:
    if source.get('hostKind') != 'runner':
        return
    marker = ROOT / 'last-work'
    if not marker.exists():
        marker.write_text(str(time.monotonic()))
        return
    if time.monotonic() - float(marker.read_text()) >= IDLE_SECONDS:
        # EC2 InstanceInitiatedShutdownBehavior=stop retains the machine/data disks.
        subprocess.run(['systemctl', 'poweroff'], check=True)


def main() -> None:
    os.environ['AWS_EC2_METADATA_SERVICE_ENDPOINT_MODE'] = 'IPv6'
    os.environ['AWS_USE_DUALSTACK_ENDPOINT'] = 'true'
    ROOT.mkdir(mode=0o700, exist_ok=True)
    source = json.loads(SOURCE.read_text())
    with (ROOT / 'active.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        if sys.argv[1:] == ['idle']:
            idle(source)
        elif sys.argv[1:] == ['tick']:
            tick(source)
        else:
            raise ValueError('expected tick or idle')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Do not log subprocess output, SSM responses, or credential contents.
        print('checks host loop failed:', type(error).__name__, file=sys.stderr)
        sys.exit(1)
