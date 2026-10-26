#!/usr/bin/env python3
"""Render the cache resources and keep signing private keys exclusively in SSM."""
import copy
import argparse
import base64
import hashlib
import json
import re
from pathlib import Path
import time
import urllib.error
import urllib.request

import yaml
import boto3
from botocore.exceptions import ClientError

from project_iam import Loader

ROOT = Path(__file__).resolve().parent
ACCOUNT = '134250228509'
REGION = 'eu-north-1'
DISTRIBUTION = 'E3SU1MYTKJEE06'
KEY_PARAMETERS = {slot: '/layr-store/gateway/cdn-signing-' + slot for slot in ('a', 'b')}
IMMUTABLE = ('states/*', 'records/*/*.age', 'index/paths/*.age', 'index/graph/*.age',
             'files/*.age', 'project/epochs/*/states/*',
             'project/epochs/*/records/*/*.age', 'project/epochs/*/index/paths/*.age',
             'project/epochs/*/index/graph/*.age', 'project/epochs/*/files/*.age',
             'project/epochs/*/history/*')


def store_policy(template, distribution_arn):
    """Add distribution-bound object reads without changing the bucket or its IAM roles."""
    if distribution_arn != f'arn:aws:cloudfront::{ACCOUNT}:distribution/{DISTRIBUTION}':
        raise ValueError('Unexpected distribution ARN')
    result = copy.deepcopy(template)
    statements = result['Resources']['BucketPolicy']['Properties']['PolicyDocument']['Statement']
    statements[:] = [row for row in statements if row.get('Sid') != 'CloudFrontImmutableReads']
    statements.append({
        'Sid': 'CloudFrontImmutableReads', 'Effect': 'Allow',
        'Principal': {'Service': 'cloudfront.amazonaws.com'}, 'Action': 's3:GetObject',
        'Resource': [{'Fn::Sub': '${Bucket.Arn}/${Prefix}/' + key} for key in IMMUTABLE],
        'Condition': {'StringEquals': {'AWS:SourceArn': distribution_arn}},
    })
    return result


def origin_router(stores):
    """Preserve the store prefix in the cache key when selecting a different S3 origin."""
    prefixes = [row['prefix'] for row in stores.values()]
    if len(prefixes) != len(set(prefixes)):
        raise ValueError('Store prefixes must be distinct to isolate cache entries')
    routes = {alias: [row['origin'], row['prefix']] for alias, row in stores.items()}
    return "import cf from 'cloudfront';\nconst routes = " + json.dumps(routes, sort_keys=True) + r""";
function handler(event) {
    var request = event.request;
    var match = /^\/o\/([a-zA-Z0-9_-]+)\/([a-zA-Z0-9_./-]+)$/.exec(request.uri);
    var route = match && routes[match[1]];
    if (!route || match[2].split('/').some(p => !p || p === '.' || p === '..')) {
        return {statusCode: 403, statusDescription: 'Forbidden'};
    }
    cf.selectRequestOriginById(route[0]);
    request.uri = '/' + route[1] + '/' + match[2];
    return request;
}
"""


def front_template(base, stores, public_keys):
    result = copy.deepcopy(base)
    resources = result['Resources']
    config = resources['Distribution']['Properties']['DistributionConfig']
    if len(public_keys) != 2 or set(public_keys) != {'a', 'b'}:
        raise ValueError('Both rotation slots are required')
    for slot, pem in public_keys.items():
        resources['CdnPublicKey' + slot.upper()] = {
            'Type': 'AWS::CloudFront::PublicKey', 'Properties': {'PublicKeyConfig': {
                'Name': 'layr-cdn-' + slot + '-' + hashlib.sha256(pem.encode()).hexdigest()[:12],
                'CallerReference': 'layr-cdn-' + slot + '-' + hashlib.sha256(pem.encode()).hexdigest(),
                'EncodedKey': pem, 'Comment': 'Layr CDN rotation slot ' + slot,
            }}}
    resources['CdnKeys'] = {'Type': 'AWS::CloudFront::KeyGroup', 'Properties': {
        'KeyGroupConfig': {'Name': 'layr-cdn', 'Items': [
            {'Ref': 'CdnPublicKeyA'}, {'Ref': 'CdnPublicKeyB'}]}}}
    resources['CdnOac'] = {'Type': 'AWS::CloudFront::OriginAccessControl', 'Properties': {
        'OriginAccessControlConfig': {'Name': 'layr-cdn-s3', 'OriginAccessControlOriginType': 's3',
                                     'SigningBehavior': 'always', 'SigningProtocol': 'sigv4'}}}
    resources['CdnCache'] = {'Type': 'AWS::CloudFront::CachePolicy', 'Properties': {
        'CachePolicyConfig': {'Name': 'layr-immutable-ciphertext', 'MinTTL': 31536000,
            'DefaultTTL': 31536000, 'MaxTTL': 31536000,
            'ParametersInCacheKeyAndForwardedToOrigin': {
                'EnableAcceptEncodingGzip': False, 'EnableAcceptEncodingBrotli': False,
                'CookiesConfig': {'CookieBehavior': 'none'},
                'HeadersConfig': {'HeaderBehavior': 'none'},
                'QueryStringsConfig': {'QueryStringBehavior': 'none'}}}}}
    resources['CdnRouter'] = {'Type': 'AWS::CloudFront::Function', 'Properties': {
        'Name': 'layr-cdn-origin', 'AutoPublish': True, 'FunctionCode': origin_router(stores),
        'FunctionConfig': {'Comment': 'Route signed ciphertext to its store', 'Runtime': 'cloudfront-js-2.0'}}}
    for row in stores.values():
        config['Origins'].append({'Id': row['origin'],
            'DomainName': row['bucket'] + '.s3.' + REGION + '.amazonaws.com',
            'OriginAccessControlId': {'Ref': 'CdnOac'},
            'S3OriginConfig': {'OriginAccessIdentity': ''}})
    if config.get('CacheBehaviors'):
        raise ValueError('Review existing cache behaviors before adding CDN delivery')
    config['CacheBehaviors'] = [{
        'PathPattern': '/o/*', 'TargetOriginId': next(iter(stores.values()))['origin'],
        'ViewerProtocolPolicy': 'https-only', 'AllowedMethods': ['GET', 'HEAD'],
        'CachedMethods': ['GET', 'HEAD'], 'Compress': False,
        'CachePolicyId': {'Ref': 'CdnCache'}, 'TrustedKeyGroups': [{'Ref': 'CdnKeys'}],
        'FunctionAssociations': [{'EventType': 'viewer-request',
                                  'FunctionARN': {'Fn::GetAtt': ['CdnRouter', 'FunctionARN']}}],
    }]
    result.setdefault('Outputs', {}).update({
        'CdnPublicKey' + slot.upper(): {'Value': {'Ref': 'CdnPublicKey' + slot.upper()}}
        for slot in ('a', 'b')})
    return result


def rotation_template(live, slot, public_pem):
    """Add a new public-key resource; CloudFront cannot change an existing key's PEM."""
    if slot not in KEY_PARAMETERS:
        raise ValueError('Unknown rotation slot')
    result = copy.deepcopy(live)
    resources = result['Resources']
    output = result['Outputs']['CdnPublicKey' + slot.upper()]
    previous = output['Value']['Ref']
    if previous not in resources or resources[previous]['Type'] != 'AWS::CloudFront::PublicKey':
        raise ValueError('Signing output does not name a managed public key')
    digest = hashlib.sha256(public_pem.encode()).hexdigest()
    name = 'CdnPublicKey' + slot.upper() + digest[:12].upper()
    if name == previous:
        return result
    properties = copy.deepcopy(resources[previous]['Properties'])
    properties['PublicKeyConfig'].update({'Name': 'layr-cdn-' + slot + '-' + digest[:12],
        'CallerReference': 'layr-cdn-' + slot + '-' + digest, 'EncodedKey': public_pem})
    resource = {'Type': 'AWS::CloudFront::PublicKey', 'Properties': properties}
    if name in resources and resources[name] != resource:
        raise ValueError('Public-key resource hash collision')
    group = resources['CdnKeys']['Properties']['KeyGroupConfig']['Items']
    if len(group) != 2 or group.count({'Ref': previous}) != 1:
        raise ValueError('Trusted group does not contain both managed rotation slots')
    resources[name] = resource
    group[group.index({'Ref': previous})] = {'Ref': name}
    output['Value'] = {'Ref': name}
    # Preserve the old resource until a separate, reviewed retirement after the URL lifetime.
    return result


def retirement_template(live, logical):
    """Remove only an unreferenced managed public key, never a stack or an active key."""
    if not re.fullmatch(r'CdnPublicKey[AB](?:[0-9A-F]{12})?', logical):
        raise ValueError('Not a managed CDN public key')
    result = copy.deepcopy(live)
    resource = result['Resources'].get(logical)
    if not resource or resource['Type'] != 'AWS::CloudFront::PublicKey':
        raise ValueError('Managed public key is absent')
    del result['Resources'][logical]
    if {'Ref': logical} in result['Resources']['CdnKeys']['Properties']['KeyGroupConfig']['Items']:
        raise ValueError('Cannot retire a trusted public key')
    def references(value):
        if isinstance(value, dict):
            return value.get('Ref') == logical or any(references(item) for item in value.values())
        return isinstance(value, list) and any(references(item) for item in value)
    if references(result):
        raise ValueError('Cannot retire a referenced public key')
    return result


def signing_public_keys(session):
    """Create each private key once; recover it from the same exact SSM name on retries."""
    from cryptography.hazmat.primitives.asymmetric import rsa
    from cryptography.hazmat.primitives import serialization
    ssm = session.client('ssm')
    public = {}
    for slot, name in KEY_PARAMETERS.items():
        try:
            parameter = ssm.get_parameter(Name=name, WithDecryption=True)['Parameter']
        except ssm.exceptions.ParameterNotFound:
            key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
            pem = key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                    serialization.NoEncryption()).decode()
            try:
                ssm.put_parameter(Name=name, Type='SecureString', Value=pem, Overwrite=False)
            except ssm.exceptions.ParameterAlreadyExists:
                pass
            parameter = ssm.get_parameter(Name=name, WithDecryption=True)['Parameter']
        if parameter['Type'] != 'SecureString':
            raise ValueError('Signing key must be a SecureString')
        key = serialization.load_pem_private_key(parameter['Value'].encode(), password=None)
        if not isinstance(key, rsa.RSAPrivateKey) or key.key_size != 2048:
            raise ValueError('CloudFront signing key must be RSA 2048')
        public[slot] = key.public_key().public_bytes(
            serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo).decode()
    return public


def source_template(name):
    return yaml.load((ROOT / name).read_text(), Loader=Loader)


def signed_url(url, private_key, public_key_id, expires):
    """Use CloudFront's canned policy and library RSA PKCS#1 v1.5 / SHA256."""
    from cryptography.hazmat.primitives.asymmetric import padding
    from cryptography.hazmat.primitives import hashes
    policy = json.dumps({'Statement': [{'Resource': url, 'Condition': {
        'DateLessThan': {'AWS:EpochTime': expires}}}]}, separators=(',', ':')).encode()
    signature = private_key.sign(policy, padding.PKCS1v15(), hashes.SHA256())
    encoded = base64.b64encode(signature).decode().translate(str.maketrans('+=/', '-_~'))
    return f'{url}?Expires={expires}&Signature={encoded}&Key-Pair-Id={public_key_id}&Hash-Algorithm=SHA256'


def probe(session, receipt, slot='a'):
    """Probe an owned object; never write a bearer URL or a private key to the receipt."""
    import uuid
    from cryptography.hazmat.primitives import serialization
    if slot not in KEY_PARAMETERS:
        raise ValueError('Unknown signing slot')
    receipt = Path(receipt)
    stores = stores_from_stacks(session)
    store = stores['cdn-studio']
    if receipt.exists():
        state = json.loads(receipt.read_text())
        if state.get('keySlot','a') != slot:
            raise ValueError('Probe receipt belongs to another signing slot')
        if state.get('complete'):
            print(json.dumps(state)); return
        if state['bucket'] != store['bucket'] or state['prefix'] != store['prefix']:
            raise ValueError('Probe receipt belongs to another store')
    else:
        state = {'bucket': store['bucket'], 'prefix': store['prefix'], 'keySlot':slot,
                 'key': 'states/' + str(uuid.uuid4()) + '/12345678.0000.data.age'}
        receipt.write_text(json.dumps(state))
    cf = session.client('cloudformation')
    outputs = {row['OutputKey']: row['OutputValue'] for row in
               cf.describe_stacks(StackName='layr-cloudfront')['Stacks'][0]['Outputs']}
    parameter = session.client('ssm').get_parameter(Name=KEY_PARAMETERS[slot], WithDecryption=True)['Parameter']
    if parameter['Type'] != 'SecureString':
        raise ValueError('Signing key is not a SecureString')
    private = serialization.load_pem_private_key(parameter['Value'].encode(), password=None)
    body = hashlib.sha256(b'Public disposable CloudFront RSA SHA256 probe').digest() * 128
    s3 = session.client('s3'); key = state['prefix'] + '/' + state['key']
    try:
        result = s3.put_object(Bucket=state['bucket'], Key=key, Body=body, IfNoneMatch='*')
        state['version'] = result['VersionId']; receipt.write_text(json.dumps(state))
    except ClientError as error:
        if error.response['Error']['Code'] != 'PreconditionFailed':
            raise
        existing = s3.get_object(Bucket=state['bucket'], Key=key)
        if existing['Body'].read() != body:
            raise ValueError('Existing probe object differs')
        state['version'] = existing['VersionId']; receipt.write_text(json.dumps(state))
    base = 'https://cloud.layr.ws/o/cdn-studio/' + state['key']
    def fetch(url):
        try:
            with urllib.request.urlopen(url, timeout=30) as response:
                return response.status, response.read(), response.headers.get('X-Cache', '')
        except urllib.error.HTTPError as error:
            return error.code, b'', ''
    try:
        unsigned = fetch(base)[0]
        url = signed_url(base, private, outputs['CdnPublicKey'+slot.upper()], int(time.time()) + 300)
        status, received, _ = fetch(url)
        if unsigned != 403 or status != 200 or received != body:
            raise ValueError('SHA256 probe failed: unsigned=' + str(unsigned) + ' signed=' + str(status))
        second_status, received, cache = fetch(url)
        if second_status != 200 or received != body:
            raise ValueError('Second signed probe read failed')
        changed = fetch(url.replace('12345678.0000.data.age', '87654321.0000.data.age'))[0]
        expired = fetch(signed_url(base, private, outputs['CdnPublicKey'+slot.upper()], int(time.time()) - 5))[0]
        expiring = signed_url(base, private, outputs['CdnPublicKey'+slot.upper()], int(time.time()) + 5)
        first_expiring = fetch(expiring)[0]
        time.sleep(6)
        reused = fetch(expiring)[0]
        if (changed, expired, first_expiring, reused) != (403, 403, 200, 403):
            raise ValueError('Signed URL path or expiry enforcement failed')
        state.update({'complete': True, 'algorithm': 'RSA2048-PKCS1v15-SHA256',
                      'unsigned': unsigned, 'signed': status, 'otherPath': changed,
                      'expired': expired, 'reusedAfterExpiry': reused,
                      'secondReadCacheHit': cache == 'Hit from cloudfront',
                      'bytes': len(body), 'sha256': hashlib.sha256(body).hexdigest(),
                      'verifiedAt': int(time.time())})
    finally:
        if state.get('version'):
            s3.delete_object(Bucket=state['bucket'], Key=key, VersionId=state['version'])
            state['objectRemoved'] = True
        receipt.write_text(json.dumps(state, indent=2))
    print(json.dumps(state))


def stores_from_stacks(session):
    result = {}
    cf = session.client('cloudformation')
    for alias, stack in [('layr', 'layr-store-layr'), ('layr-cloud', 'layr-store-layr-cloud'),
                         ('layr-studio', 'layr-store-layr-studio'), ('cdn-studio', 'layr-store-m8-final')]:
        current = cf.describe_stacks(StackName=stack)['Stacks'][0]
        if current['StackStatus'] not in ('CREATE_COMPLETE', 'UPDATE_COMPLETE'):
            raise ValueError('Store stack is not stable: ' + stack)
        outputs = {row['OutputKey']: row['OutputValue'] for row in current['Outputs']}
        prefix = next(row['ParameterValue'] for row in current['Parameters'] if row['ParameterKey'] == 'Prefix')
        result[alias] = {'origin': 'cdn-' + alias, 'bucket': outputs['BucketName'], 'prefix': prefix}
    return result


def gateway_role(template):
    """Change only the role's exact signing-key read permissions."""
    result = copy.deepcopy(template)
    rows = result['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
    source = source_template('gateway.yaml')['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
    desired = next(row for row in source if row.get('Sid') == 'CdnSigningRead')
    old = [row for row in rows if row.get('Sid') == 'CdnSigningRead']
    if old and old != [desired]:
        raise ValueError('Existing CDN signing permission differs')
    if not old:
        position = next((i for i, row in enumerate(rows) if row.get('Effect') == 'Deny'), len(rows))
        rows.insert(position, desired)
    deny = [row for row in rows if row.get('Effect') == 'Deny'
            and row.get('Action') == ['ssm:GetParameter', 'ssm:GetParameters', 'ssm:GetParametersByPath']]
    if len(deny) != 1 or not isinstance(deny[0].get('NotResource'), list):
        raise ValueError('Gateway SSM deny boundary differs')
    required = [{'Fn::Sub': 'arn:${AWS::Partition}:ssm:${AWS::Region}:${AWS::AccountId}:parameter/layr-store/gateway/' + name}
                for name in ('session-secret', 'origin-secret', 'cdn-signing-a', 'cdn-signing-b')]
    if any(row not in required for row in deny[0]['NotResource']) or not all(row in deny[0]['NotResource'] for row in required[:2]):
        raise ValueError('Gateway secret read exceptions differ')
    deny[0]['NotResource'] = required
    return result


def runtime_settings(session, slot='a'):
    """Return only public routing metadata after matching the SSM key to CloudFront."""
    from cryptography.hazmat.primitives import serialization
    if slot not in KEY_PARAMETERS:
        raise ValueError('Unknown signing slot')
    outputs = {row['OutputKey']: row['OutputValue'] for row in session.client('cloudformation').describe_stacks(
        StackName='layr-cloudfront')['Stacks'][0]['Outputs']}
    public_id = outputs['CdnPublicKey' + slot.upper()]
    encoded = session.client('cloudfront').get_public_key(Id=public_id)['PublicKey']['PublicKeyConfig']['EncodedKey']
    parameter = session.client('ssm').get_parameter(Name=KEY_PARAMETERS[slot], WithDecryption=True)['Parameter']
    if parameter['Type'] != 'SecureString':
        raise ValueError('Signing key must be a SecureString')
    key = serialization.load_pem_private_key(parameter['Value'].encode(), password=None)
    public = key.public_key().public_bytes(serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo).decode()
    if public.strip() != encoded.strip():
        raise ValueError('SSM signing key does not match the trusted CloudFront public key')
    return {'host': 'cloud.layr.ws', 'keyId': public_id, 'keyParameter': KEY_PARAMETERS[slot], 'region': REGION,
            'stores': {alias: {'bucket': row['bucket'], 'prefix': row['prefix'], 'region': REGION}
                       for alias, row in stores_from_stacks(session).items()}}


def plan_rotation(session, slot, receipt):
    """Recover an inactive-slot preparation through its public receipt, never a key file."""
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    if slot not in KEY_PARAMETERS:
        raise ValueError('Supply an inactive rotation slot')
    receipt = Path(receipt)
    state = json.loads(receipt.read_text()) if receipt.exists() else None
    if state is not None and state.get('slot') != slot:
        raise ValueError('Receipt belongs to another signing slot')
    cf = session.client('cloudformation')
    if state and state.get('change'):
        previous = cf.describe_change_set(StackName='layr-cloudfront', ChangeSetName=state['change'])
        return {'slot':slot, 'change':state['change'], 'status':previous['Status'], 'executionStatus':previous['ExecutionStatus']}
    source = session.client('s3').get_object(Bucket='layr-bootstrap-artifacts-ik3kxdaezcnd', Key='gateway/gateway.json')
    config = json.loads(source['Body'].read())
    active = config.get('cdn', {}).get('keyParameter')
    if active not in KEY_PARAMETERS.values() or active == KEY_PARAMETERS[slot]:
        raise ValueError('Only the inactive configured signing slot may be prepared')
    if time.time() - source['LastModified'].timestamp() < 600:
        raise ValueError('Wait ten minutes after the last signer switch before replacing its inactive slot')
    ssm = session.client('ssm')
    current = ssm.get_parameter(Name=KEY_PARAMETERS[slot], WithDecryption=True)['Parameter']
    if current['Type'] != 'SecureString':
        raise ValueError('Signing key must be a SecureString')
    private = serialization.load_pem_private_key(current['Value'].encode(), password=None)
    public = private.public_key().public_bytes(serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo).decode()
    public_hash = hashlib.sha256(public.encode()).hexdigest()
    if state and public_hash not in (state['previousPublicSha256'], state['newPublicSha256']):
        raise ValueError('Signing slot changed outside this rotation receipt')
    if state is None or public_hash != state['newPublicSha256']:
        # If a previous put had an unknown outcome, the read above settles which key exists.
        # An unapplied preparation may generate a new key; no issued URL used the inactive slot.
        private = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        public = private.public_key().public_bytes(serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo).decode()
        state = {'slot':slot, 'previousPublicSha256':public_hash,
                 'newPublicSha256':hashlib.sha256(public.encode()).hexdigest(), 'newPublicPem':public}
        receipt.parent.mkdir(parents=True, exist_ok=True)
        receipt.write_text(json.dumps(state, indent=2))
        pem = private.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()).decode()
        ssm.put_parameter(Name=KEY_PARAMETERS[slot], Type='SecureString', Value=pem, Overwrite=True)
    live = cf.get_template(StackName='layr-cloudfront')['TemplateBody']
    live = yaml.load(live, Loader=Loader) if isinstance(live, str) else live
    state.setdefault('previousLogicalKey',live['Outputs']['CdnPublicKey'+slot.upper()]['Value']['Ref'])
    template = rotation_template(live, slot, state['newPublicPem'])
    planned = plan(session, 'layr-cloudfront', template)
    state['change'] = planned['change']; receipt.write_text(json.dumps(state, indent=2))
    return planned


def configure_source(session, slot, receipt, disable=False):
    """Publish only public metadata with CAS; recover a lost response by exact body hash."""
    desired = None if disable else runtime_settings(session, slot)
    receipt = Path(receipt)
    s3 = session.client('s3')
    bucket, key = 'layr-bootstrap-artifacts-ik3kxdaezcnd', 'gateway/gateway.json'
    current = s3.get_object(Bucket=bucket, Key=key)
    body = current['Body'].read(); config = json.loads(body)
    if not config.get('cloudfront') or config.get('publicHost') != 'cloud.layr.ws':
        raise ValueError('CDN requires the existing guarded public host')
    if 'PRIVATE KEY' in body.decode() or any(name in config for name in ('originSecret','sessionSecret','privateKey','signingKey')):
        raise ValueError('Gateway source must contain only public metadata')
    proposed = dict(config)
    if desired is None: proposed.pop('cdn', None)
    else: proposed['cdn'] = desired
    encoded = json.dumps(proposed, sort_keys=True).encode()
    digest = hashlib.sha256(encoded).hexdigest()
    if receipt.exists():
        state = json.loads(receipt.read_text())
        if state['desiredSha256'] != digest:
            raise ValueError('Source receipt belongs to a different configuration')
    else:
        state = {'previousSha256':hashlib.sha256(body).hexdigest(), 'previousVersion':current.get('VersionId'),
                 'previousConfig':config, 'desiredSha256':digest, 'slot':None if disable else slot}
        receipt.parent.mkdir(parents=True, exist_ok=True); receipt.write_text(json.dumps(state, indent=2))
    if config != proposed:
        if hashlib.sha256(body).hexdigest() != state['previousSha256']:
            raise ValueError('Gateway source changed outside this receipt')
        result = s3.put_object(Bucket=bucket, Key=key, Body=encoded, ContentType='application/json', IfMatch=current['ETag'])
        state['version'] = result.get('VersionId')
    else:
        state['version'] = current.get('VersionId')
    state['complete'] = True; receipt.write_text(json.dumps(state, indent=2))
    return {'sourceSha256':digest, 'version':state['version'], 'cdnEnabled':desired is not None}


def plan(session, stack, template):
    cf = session.client('cloudformation')
    current = cf.describe_stacks(StackName=stack)['Stacks'][0]
    if current['StackStatus'] not in ('CREATE_COMPLETE', 'UPDATE_COMPLETE'):
        raise ValueError('Stack is not stable: ' + stack)
    body = json.dumps(template, sort_keys=True)
    change = 'cdn-' + hashlib.sha256(body.encode()).hexdigest()[:20]
    try:
        previous = cf.describe_change_set(StackName=stack, ChangeSetName=change)
        return {'stack': stack, 'change': change, 'status': previous['Status']}
    except ClientError as error:
        if error.response['Error']['Code'] != 'ChangeSetNotFound':
            raise
    cf.create_change_set(StackName=stack, ChangeSetName=change, ChangeSetType='UPDATE',
                        ClientToken=stack + '-' + change, TemplateBody=body,
                        Parameters=[{'ParameterKey': row['ParameterKey'], 'UsePreviousValue': True}
                                    for row in current.get('Parameters', [])],
                        Capabilities=['CAPABILITY_NAMED_IAM'])
    return {'stack': stack, 'change': change, 'status': 'PLANNED'}


def validate_changes(cf, stack, change, result, retire_key=None):
    allowed = {'layr-cloudfront': {'Distribution', 'CdnOac', 'CdnCache', 'CdnRouter',
                                  'CdnKeys', 'CdnPublicKeyA', 'CdnPublicKeyB'}}
    names = {'Role'} if stack == 'layr-gateway' else allowed.get(stack, {'BucketPolicy'} if stack in
        ['layr-store-' + name for name in ('layr', 'layr-cloud', 'layr-studio', 'm8-final')] else set())
    for row in result.get('Changes', []):
        resource = row['ResourceChange']
        name = resource['LogicalResourceId']
        if stack == 'layr-cloudfront' and retire_key == name and resource['Action'] == 'Remove':
            before = cf.get_template(StackName=stack)['TemplateBody']
            after = cf.get_template(StackName=stack, ChangeSetName=change)['TemplateBody']
            before = yaml.load(before, Loader=Loader) if isinstance(before, str) else before
            after = yaml.load(after, Loader=Loader) if isinstance(after, str) else after
            if resource.get('ResourceType') != 'AWS::CloudFront::PublicKey' or retirement_template(before, name) != after:
                raise ValueError('Retirement may remove only the unreferenced public key')
            continue
        # Distribution dependencies can flag aliases even when their definitions do not change.
        if stack == 'layr-cloudfront' and name in ('A', 'AAAA'):
            before = cf.get_template(StackName=stack)['TemplateBody']
            after = cf.get_template(StackName=stack, ChangeSetName=change)['TemplateBody']
            before = yaml.load(before, Loader=Loader) if isinstance(before, str) else before
            after = yaml.load(after, Loader=Loader) if isinstance(after, str) else after
            if (resource['Action'] != 'Modify' or resource.get('Replacement') != 'False'
                    or before['Resources'][name] != after['Resources'][name]):
                raise ValueError('DNS resource definition changed')
            continue
        if stack == 'layr-cloudfront' and re.fullmatch(r'CdnPublicKey[AB][0-9A-F]{12}', name):
            if resource['Action'] != 'Add' or resource.get('ResourceType') != 'AWS::CloudFront::PublicKey':
                raise ValueError('Rotation must add a new immutable public-key resource')
            continue
        if name not in names or resource['Action'] == 'Remove':
            raise ValueError('Unexpected resource change')
        if name in ('Distribution', 'BucketPolicy', 'Role') and resource['Action'] == 'Modify' and resource.get('Replacement') != 'False':
            raise ValueError('Refusing replacement of an existing distribution or bucket policy')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['plan-front', 'plan-store', 'inspect', 'execute', 'probe', 'plan-role', 'settings', 'plan-rotation', 'configure-source', 'plan-retirement'])
    parser.add_argument('--stack', default='layr-cloudfront')
    parser.add_argument('--change')
    parser.add_argument('--retire-key')
    parser.add_argument('--slot', choices=['a', 'b'])
    parser.add_argument('--rotation-receipt')
    parser.add_argument('--source-receipt')
    parser.add_argument('--disable-cdn', action='store_true')
    parser.add_argument('--receipt', default='/tmp/layr-cdn-build/sha256-probe.json')
    args = parser.parse_args()
    session = boto3.Session(region_name=REGION)
    if session.client('sts').get_caller_identity()['Account'] != ACCOUNT:
        raise ValueError('Unexpected AWS account')
    if args.operation == 'plan-retirement':
        if not args.retire_key:
            parser.error('--retire-key is required')
        live = session.client('cloudformation').get_template(StackName='layr-cloudfront')['TemplateBody']
        live = yaml.load(live, Loader=Loader) if isinstance(live, str) else live
        print(json.dumps(plan(session,'layr-cloudfront',retirement_template(live,args.retire_key))))
    elif args.operation == 'configure-source':
        if not args.source_receipt or (not args.disable_cdn and not args.slot):
            parser.error('--source-receipt and either --slot or --disable-cdn are required')
        print(json.dumps(configure_source(session,args.slot,args.source_receipt,args.disable_cdn)))
    elif args.operation == 'plan-rotation':
        if not args.slot or not args.rotation_receipt:
            parser.error('--slot and --rotation-receipt are required')
        print(json.dumps(plan_rotation(session, args.slot, args.rotation_receipt)))
    elif args.operation == 'settings':
        print(json.dumps(runtime_settings(session, args.slot or 'a'), sort_keys=True))
    elif args.operation == 'plan-role':
        if args.stack != 'layr-gateway':
            raise ValueError('Unexpected gateway stack')
        live = session.client('cloudformation').get_template(StackName=args.stack)['TemplateBody']
        live = yaml.load(live, Loader=Loader) if isinstance(live, str) else live
        print(json.dumps(plan(session, args.stack, gateway_role(live))))
    elif args.operation == 'probe':
        probe(session, args.receipt, args.slot or 'a')
    elif args.operation == 'plan-front':
        if args.stack != 'layr-cloudfront':
            raise ValueError('Unexpected front stack')
        template = front_template(source_template('cloudfront.yaml'), stores_from_stacks(session),
                                  signing_public_keys(session))
        print(json.dumps(plan(session, args.stack, template)))
    elif args.operation == 'plan-store':
        if args.stack not in ['layr-store-' + name for name in ('layr', 'layr-cloud', 'layr-studio', 'm8-final')]:
            raise ValueError('Unexpected store stack')
        live = session.client('cloudformation').get_template(StackName=args.stack)['TemplateBody']
        live = yaml.load(live, Loader=Loader) if isinstance(live, str) else live
        template = store_policy(live, f'arn:aws:cloudfront::{ACCOUNT}:distribution/{DISTRIBUTION}')
        print(json.dumps(plan(session, args.stack, template)))
    else:
        from cloudfront import execute, inspect
        if not args.change:
            parser.error('--change is required')
        cf = session.client('cloudformation')
        result = inspect(cf, args.stack, args.change)
        validate_changes(cf, args.stack, args.change, result, args.retire_key)
        if args.operation == 'execute':
            execute(cf, args.stack, args.change)


if __name__ == '__main__':
    main()
