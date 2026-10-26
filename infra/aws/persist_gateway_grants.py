#!/usr/bin/env python3
"""Persist only a gateway alias's public grant hashes after local token issuance."""
import argparse
import json
import re
from pathlib import Path

import boto3


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', required=True)
    parser.add_argument('--artifacts-bucket', required=True)
    parser.add_argument('--issued-project', type=Path, required=True,
                        help='Public alias configuration read from the gateway through SSM after issuance')
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,43}', args.project):
        raise ValueError('Invalid project alias')
    local = json.loads(args.issued_project.read_text())
    session = boto3.Session(profile_name='ifdotpy', region_name='eu-north-1')
    if session.client('sts').get_caller_identity()['Account'] != '134250228509':
        raise ValueError('Unexpected AWS account')
    s3 = session.client('s3')
    key = 'gateway/projects/' + args.project + '.json'
    response = s3.get_object(Bucket=args.artifacts_bucket, Key=key)
    public = json.loads(response['Body'].read())
    for name in ('bucket', 'region', 'prefix', 'recipient', 'readerRoleArn', 'writerRoleArn', 'gcRoleArn'):
        if public.get(name) != local.get(name):
            raise ValueError('Local and published project configuration differ')
    for name in ('writeTokenHashes', 'gcTokenHashes'):
        if (not isinstance(local[name], list) or len(local[name]) > 4096
                or any(not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{64}', value)
                       for value in local[name])):
            raise ValueError('Expected only public grant hashes')
        public[name] = local[name]
    s3.put_object(Bucket=args.artifacts_bucket, Key=key,
                  Body=json.dumps(public, sort_keys=True).encode(), IfMatch=response['ETag'])
    print('Persisted public grant hashes for ' + args.project)


if __name__ == '__main__':
    main()
