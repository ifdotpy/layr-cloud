#!/usr/bin/env python3
"""Publish an owner-initialized encrypted seed and its public gateway alias.

Run only after the owner confirms initialization. Read only a public store.json and
age ciphertext from the seed directory. No paper-key input is accepted.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import time
import uuid

import boto3
from botocore.exceptions import ClientError

ACCOUNT = '134250228509'
REGION = 'eu-north-1'
ARTIFACTS = 'layr-bootstrap-artifacts-ik3kxdaezcnd'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def current(s3, bucket, key):
    try:
        return s3.get_object(Bucket=bucket, Key=key)['Body'].read()
    except ClientError as error:
        if error.response['Error']['Code'] not in ('NoSuchKey', '404'):
            raise
        return None


def immutable(s3, bucket, key, data):
    previous = current(s3, bucket, key)
    if previous is not None:
        if digest(previous) != digest(data):
            raise ValueError('Existing different object: ' + key)
        return
    s3.put_object(Bucket=bucket, Key=key, Body=data, IfNoneMatch='*')


def seed_files(seed):
    if seed.is_symlink() or not seed.is_dir():
        raise ValueError('Seed must be a directory, without symlinks')
    files = []
    for path in sorted(seed.rglob('*')):
        if path.is_symlink():
            raise ValueError('Seed contains a symlink')
        if path.is_dir():
            continue
        relative = path.relative_to(seed).as_posix()
        if not path.is_file() or (relative != 'store.json' and not relative.endswith('.age')):
            raise ValueError('Seed contains a file other than store.json or age ciphertext')
        data = path.read_bytes()
        if relative != 'store.json' and not data.startswith(b'age-encryption.org/v1\n'):
            raise ValueError('Seed contains a file without an age ciphertext header')
        files.append((relative, data))
    descriptor = json.loads((seed / 'store.json').read_bytes())
    if (descriptor.get('format') != 2 or str(uuid.UUID(descriptor['project'])) != descriptor['project']
            or not re.fullmatch(r'age1[0-9a-z]{58}', descriptor.get('recipient', ''))):
        raise ValueError('Invalid public seed descriptor')
    return descriptor, files


def publish(session, project, seed, login=None, invite=None, rehearsal=False):
    descriptor, files = seed_files(seed)
    if rehearsal and project != 'm1-move':
        raise ValueError('Rehearsal is limited to m1-move')
    prefix = 'm8-final' if rehearsal else project
    stack = session.client('cloudformation').describe_stacks(StackName='layr-store-' + prefix)['Stacks'][0]
    outputs = {row['OutputKey']: row['OutputValue'] for row in stack['Outputs']}
    parameters = {row['ParameterKey']: row['ParameterValue'] for row in stack['Parameters']}
    if parameters['Prefix'] != prefix:
        raise ValueError('Store prefix differs from project')
    s3 = session.client('s3')
    bucket = outputs['BucketName']
    key = 'gateway/projects/' + project + '.json'
    previous = current(s3, ARTIFACTS, key)
    config = json.loads(previous) if previous is not None else {'writeTokenHashes': [], 'gcTokenHashes': []}
    fields = {'bucket': bucket, 'prefix': prefix, 'region': REGION, 'recipient': descriptor['recipient'],
              **{name: outputs[field] for name, field in (
                  ('readerRoleArn', 'ReaderRoleArn'), ('writerRoleArn', 'WriterRoleArn'), ('gcRoleArn', 'GcRoleArn'))}}
    if previous is not None and any(config.get(name) != value for name, value in fields.items()):
        raise ValueError('Existing gateway alias differs from this store')
    config.update(fields)
    if login is not None:
        if not re.fullmatch(r'[^\s/]+@[^\s/]+', login) or invite is None or invite.is_symlink():
            raise ValueError('Invalid encrypted person invite')
        sealed = invite.read_bytes()
        if not sealed.startswith(b'age-encryption.org/v1\n') or len(sealed) > 4096:
            raise ValueError('Person invite must be age ciphertext, at most 4096 bytes')
        person = current(s3, ARTIFACTS, 'people/' + login + '/person.json')
        if person is None:
            raise ValueError('Person must register before publishing its invite')
    # The public descriptor goes last: it is the seed publication boundary.
    for relative, data in sorted(files, key=lambda item: item[0] == 'store.json'):
        immutable(s3, bucket, prefix + '/' + relative, data)
    # Re-read after uploading ciphertext so another grant issuer cannot be overwritten.
    try:
        response = s3.get_object(Bucket=ARTIFACTS, Key=key)
        latest = json.loads(response['Body'].read())
        if any(latest.get(name) != value for name, value in fields.items()):
            raise ValueError('Gateway alias changed during publication')
        s3.put_object(Bucket=ARTIFACTS, Key=key, Body=json.dumps(latest, sort_keys=True).encode(),
                      IfMatch=response['ETag'])
    except ClientError as error:
        if error.response['Error']['Code'] not in ('NoSuchKey', '404'):
            raise
        s3.put_object(Bucket=ARTIFACTS, Key=key, Body=json.dumps(config, sort_keys=True).encode(), IfNoneMatch='*')
    if login is not None:
        # Deterministic id makes a lost response recoverable without another inbox entry.
        invite_id = '0000000000000-' + digest(sealed)[:8]
        invite_key = 'people/' + login + '/inbox/' + invite_id + '.json'
        existing = current(s3, ARTIFACTS, invite_key)
        body = json.dumps({'from': login, 'time': int(time.time() * 1000),
                           'data': base64.b64encode(sealed).decode()}, sort_keys=True).encode()
        if existing is None:
            immutable(s3, ARTIFACTS, invite_key, body)
        elif json.loads(existing)['data'] != base64.b64encode(sealed).decode():
            raise ValueError('Existing inbox entry differs')
    print(json.dumps({'alias': project, 'project': descriptor['project'], 'objects': len(files),
                      'bucket': bucket, 'personInvitePublished': login is not None}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', choices=('layr', 'layr-cloud', 'layr-studio', 'm1-move'), required=True)
    parser.add_argument('--rehearsal', action='store_true')
    parser.add_argument('--seed', type=Path, required=True)
    parser.add_argument('--person-login')
    parser.add_argument('--person-invite', type=Path)
    args = parser.parse_args()
    if bool(args.person_login) != bool(args.person_invite):
        parser.error('Supply both --person-login and --person-invite')
    if (args.project == 'm1-move') != args.rehearsal:
        parser.error('m1-move requires --rehearsal; real projects forbid it')
    session = boto3.Session(profile_name='ifdotpy', region_name=REGION)
    if session.client('sts').get_caller_identity()['Account'] != ACCOUNT:
        raise ValueError('Unexpected AWS account')
    publish(session, args.project, args.seed, args.person_login, args.person_invite, args.rehearsal)


if __name__ == '__main__':
    main()
