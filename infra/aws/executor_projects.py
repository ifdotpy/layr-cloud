#!/usr/bin/env python3
"""Render public executor profiles and systemd units for post-enrollment installation.

This command writes a reviewable directory. It does not install or start services.
"""
import argparse
import json
import re
from pathlib import Path

UNITS = {
    'layr.service.d/projects.conf': '''[Unit]
Requires=
Requires=layr-data.service
After=
After=layr-data.service network-online.target
[Service]
UnsetEnvironment=LAYR_GATEWAY_TOKEN_FILE
''',
    'layr-projects.service': '''[Unit]
Description=Sync each enrolled Layr executor project
Requires=layr.service
After=layr.service network-online.target
ConditionPathExists=/etc/layr-executor/ready
[Service]
Type=oneshot
Environment=AWS_EC2_METADATA_SERVICE_ENDPOINT_MODE=IPv6
Environment=AWS_USE_DUALSTACK_ENDPOINT=true
RuntimeDirectory=layr-executor-projects
RuntimeDirectoryMode=0700
ExecStart=/usr/bin/python3 /opt/layr-executor/current/checks.py tick
TimeoutStartSec=0
''',
    'layr-projects.timer': '''[Timer]
OnBootSec=3min
OnUnitInactiveSec=60s
[Install]
WantedBy=timers.target
''',
}


def render(root, projects):
    if not projects or len(set(projects)) != len(projects):
        raise ValueError('Supply each enrolled project once')
    for project in projects:
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,43}', project):
            raise ValueError('Invalid project name')
    files = {'systemd/' + name: text for name, text in UNITS.items()}
    files.update({'projects.d/' + project + '.json': json.dumps({
        'project': project,
        'writeTokenParameter': '/layr-store/executor/projects/' + project + '/write-token',
    }, sort_keys=True) + '\n' for project in projects})
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        if path.is_symlink() or path.exists():
            raise ValueError('Render destination already exists: ' + name)
        with path.open('x') as file:
            file.write(text)
        path.chmod(0o644)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--projects', nargs='+', required=True)
    args = parser.parse_args()
    render(args.output, args.projects)
    print('Rendered public profiles and units; install only after owner enrollment')


if __name__ == '__main__':
    main()
