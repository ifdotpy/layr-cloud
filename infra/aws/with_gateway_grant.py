#!/usr/bin/env python3
"""Run a CLI with an SSM transport grant in a private RAM-only file, then remove it.

This accepts transport parameter names only. It never accepts a paper key.
The caller creates and later detaches the Mac RAM disk, or supplies a Linux tmpfs directory.
"""
import argparse
import os
import plistlib
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

import boto3


def verify_ram(directory):
    info = directory.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise ValueError('Token directory must be caller-owned, mode 0700, without a symlink')
    if sys.platform == 'darwin':
        mount = directory.resolve()
        while not mount.is_mount():
            mount = mount.parent
        volume = plistlib.loads(subprocess.check_output(['diskutil', 'info', '-plist', str(mount)]))
        device = '/dev/' + volume['DeviceIdentifier']
        images = plistlib.loads(subprocess.check_output(['hdiutil', 'info', '-plist']))['images']
        if not any(str(image.get('image-path', '')).startswith('ram://') and any(
                device == entry.get('dev-entry') or device.startswith(entry.get('dev-entry', '') + 's')
                for entry in image.get('system-entities', []) if entry.get('dev-entry')) for image in images):
            raise ValueError('Token directory must be on an attached RAM disk')
    elif subprocess.check_output(['findmnt', '-n', '-o', 'FSTYPE', '-T', str(directory)], text=True).strip() != 'tmpfs':
        raise ValueError('Token directory must be on tmpfs')


def run(parameter, directory, command):
    if not re.fullmatch(r'/layr-store/(owner|runner|executor)/projects/[A-Za-z0-9][A-Za-z0-9_-]{0,43}/write-token', parameter):
        raise ValueError('Expected an exact project transport parameter')
    verify_ram(directory)
    session = boto3.Session(profile_name='ifdotpy', region_name='eu-north-1')
    value = session.client('ssm').get_parameter(Name=parameter, WithDecryption=True)['Parameter']
    token = value['Value']
    if value['Type'] != 'SecureString' or not token or len(token) > 4096 or any(c.isspace() for c in token):
        raise ValueError('Invalid transport grant')
    path = None
    try:
        with tempfile.NamedTemporaryFile('w', dir=directory, delete=False) as file:
            path = Path(file.name)
            os.fchmod(file.fileno(), 0o600)
            file.write(token)
        return subprocess.run(command, env=dict(os.environ, LAYR_GATEWAY_TOKEN_FILE=str(path))).returncode
    finally:
        if path is not None:
            path.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parameter', required=True)
    parser.add_argument('--token-dir', required=True, type=Path)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('Supply a command after --')
    return run(args.parameter, args.token_dir, command)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        sys.exit('Gateway command failed: ' + type(error).__name__)
