#!/usr/bin/env python3
"""Enroll one owner-trusted executor project, with its own tmpfs transport grant.

Run on the executor only after the owner has published its encrypted machine slot.
The JSON profile and person identity are public. This command never accepts a paper key.
"""
import argparse
import json
import os
import re
import subprocess
from pathlib import Path

import checks

HOME = Path('/var/lib/layr')
READY = Path('/etc/layr-executor/ready')


def enroll(project, login, public):
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,43}', project):
        raise ValueError('Invalid project')
    if not re.fullmatch(r'[^\s/]+@[^\s/]+', login) or not re.fullmatch(r'[0-9a-f]{64}', public):
        raise ValueError('Invalid person public identity')
    if os.geteuid() != 0:
        raise ValueError('Run as executor root')
    source = json.loads(checks.SOURCE.read_text())
    if source.get('hostKind') != 'executor':
        raise ValueError('This is not an executor host')
    profile = next(p for p in checks.project_profiles() if p['project'] == project)
    os.environ['AWS_EC2_METADATA_SERVICE_ENDPOINT_MODE'] = 'IPv6'
    os.environ['AWS_USE_DUALSTACK_ENDPOINT'] = 'true'
    with checks.project_token(source, profile) as token:
        env = dict(os.environ, LAYR_ROOT=str(HOME), LAYR_GATEWAY_TOKEN_FILE=str(token))
        def run(*args):
            subprocess.run([checks.BINARY, *args], env=env, check=True)
        if not (HOME / 'projects' / project).exists():
            run('clone', 'layr+https://cloud.layr.ws/' + project, project)
        for args in (
                ('config', 'queue.run', ''),
                ('access', 'person-key', login, public),
                ('access', 'grant', 'person:' + login, 'maintainer'),
                ('access', 'grant', 'layr-checks', 'reader'),
                ('config', '--local', 'checks.runner', 'false'),
                ('config', '--local', 'checks.privileged-runner', 'false')):
            run('--project', project, *args)
        run('--project', project, 'push', 'origin')
        run('--project', project, 'index', 'write', 'origin', '--json')
        run('--project', project, 'fsck')
        READY.touch(mode=0o600)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', required=True)
    parser.add_argument('--person-login', required=True)
    parser.add_argument('--person-public', required=True)
    args = parser.parse_args()
    enroll(args.project, args.person_login, args.person_public)


if __name__ == '__main__':
    main()
