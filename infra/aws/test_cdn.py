import copy
import json
import unittest
from unittest.mock import Mock

import cdn


class CdnTests(unittest.TestCase):
    def test_signing_iam_changes_only_the_role_and_two_exact_read_permissions(self):
        base = cdn.source_template('gateway.yaml')
        old = copy.deepcopy(base)
        # Reproduce the current live role before CDN was enabled.
        rows = base['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
        rows[:] = [row for row in rows if row.get('Sid') != 'CdnSigningRead']
        deny = next(row for row in rows if row.get('Effect') == 'Deny' and 'ssm:GetParameter' in row.get('Action', []))
        deny['NotResource'] = deny['NotResource'][:2]
        changed = cdn.gateway_role(base)
        for name, resource in base['Resources'].items():
            if name != 'Role': self.assertEqual(changed['Resources'][name], resource)
        statements = changed['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
        reads = next(row for row in statements if row.get('Sid') == 'CdnSigningRead')
        self.assertEqual(reads['Action'], 'ssm:GetParameter')
        self.assertEqual(len(reads['Resource']), 2)
        self.assertTrue(all('cdn-signing-' in row['Fn::Sub'] and '*' not in row['Fn::Sub'] for row in reads['Resource']))
        self.assertEqual(cdn.gateway_role(changed), changed)
        self.assertEqual(changed, old)
        deny['NotResource'].append({'Fn::Sub': 'unexpected'})
        with self.assertRaises(ValueError): cdn.gateway_role(base)
        changes = {'Changes': [{'ResourceChange': {'LogicalResourceId':'Role','Action':'Modify','Replacement':'True'}}]}
        with self.assertRaises(ValueError): cdn.validate_changes(Mock(), 'layr-gateway', 'change', changes)

    def test_rotation_adds_a_fresh_key_resource_and_keeps_two_trusted_keys(self):
        stores = {'cdn-studio': {'origin':'cdn-test','bucket':'test','prefix':'m8-final'}}
        old = cdn.front_template(cdn.source_template('cloudfront.yaml'),stores,{'a':'public-a','b':'public-b'})
        rotated = cdn.rotation_template(old,'b','new-public-b')
        self.assertEqual(rotated['Resources']['CdnPublicKeyB'],old['Resources']['CdnPublicKeyB'])
        self.assertEqual(rotated['Resources']['CdnPublicKeyA'],old['Resources']['CdnPublicKeyA'])
        new_id = rotated['Outputs']['CdnPublicKeyB']['Value']['Ref']
        self.assertNotEqual(new_id,'CdnPublicKeyB')
        self.assertEqual(rotated['Resources'][new_id]['Properties']['PublicKeyConfig']['EncodedKey'],'new-public-b')
        group = rotated['Resources']['CdnKeys']['Properties']['KeyGroupConfig']['Items']
        self.assertEqual(group,[{'Ref':'CdnPublicKeyA'},{'Ref':new_id}])
        self.assertEqual(cdn.rotation_template(rotated,'b','new-public-b'),rotated)
        self.assertEqual(rotated['Resources']['Distribution'],old['Resources']['Distribution'])
        with self.assertRaises(ValueError): cdn.rotation_template(old,'other','pem')

    def test_inactive_slot_rotation_recovers_a_lost_ssm_reply_without_a_private_key_file(self):
        import datetime
        import io
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric import rsa
        previous = rsa.generate_private_key(public_exponent=65537,key_size=2048)
        value = {'Type':'SecureString','Value':previous.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()).decode()}
        session, ssm, s3, cf = Mock(), Mock(), Mock(), Mock()
        session.client.side_effect = lambda name: {'ssm':ssm,'s3':s3,'cloudformation':cf}[name]
        ssm.get_parameter.side_effect = lambda **_: {'Parameter':dict(value)}
        source = {'cdn':{'keyParameter':cdn.KEY_PARAMETERS['a']}}
        s3.get_object.side_effect = lambda **_: {'Body':io.BytesIO(json.dumps(source).encode()),'LastModified':datetime.datetime.now(datetime.timezone.utc)-datetime.timedelta(minutes=20)}
        front = cdn.front_template(cdn.source_template('cloudfront.yaml'),{'cdn-studio':{'origin':'cdn-test','bucket':'test','prefix':'m8-final'}},{'a':'public-a','b':'public-b'})
        cf.get_template.return_value = {'TemplateBody':front}
        def lost_reply(**kwargs):
            value['Value'] = kwargs['Value']
            self.assertEqual(kwargs['Name'],cdn.KEY_PARAMETERS['b'])
            self.assertEqual(kwargs['Type'],'SecureString')
            self.assertTrue(kwargs['Overwrite'])
            raise RuntimeError('response lost after write')
        ssm.put_parameter.side_effect = lost_reply
        with tempfile.TemporaryDirectory() as temp:
            receipt=Path(temp)/'rotation.json'
            with self.assertRaisesRegex(ValueError,'inactive'):
                cdn.plan_rotation(session,'a',receipt)
            ssm.put_parameter.assert_not_called()
            with self.assertRaisesRegex(RuntimeError,'response lost'):
                cdn.plan_rotation(session,'b',receipt)
            self.assertNotIn('PRIVATE KEY',receipt.read_text())
            with patch.object(cdn,'plan',return_value={'change':'rotation-fixture'}) as plan:
                self.assertEqual(cdn.plan_rotation(session,'b',receipt)['change'],'rotation-fixture')
                planned=plan.call_args.args[2]
                self.assertEqual(len(planned['Resources']['CdnKeys']['Properties']['KeyGroupConfig']['Items']),2)
            self.assertEqual(ssm.put_parameter.call_count,1)
            cf.describe_change_set.return_value={'Status':'CREATE_COMPLETE','ExecutionStatus':'AVAILABLE'}
            self.assertEqual(cdn.plan_rotation(session,'b',receipt)['executionStatus'],'AVAILABLE')
            self.assertEqual(ssm.put_parameter.call_count,1)

    def test_public_source_cas_recovers_a_lost_reply_without_another_write(self):
        import io
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        session, s3 = Mock(), Mock(); session.client.return_value = s3
        config = {'host':'origin.cloud.layr.ws','cloudfront':True,'publicHost':'cloud.layr.ws','zone':'unchanged'}
        value = {'Body':json.dumps(config).encode(),'VersionId':'before','ETag':'before-etag'}
        s3.get_object.side_effect = lambda **_: {**value,'Body':io.BytesIO(value['Body'])}
        settings = {'host':'cloud.layr.ws','keyId':'PUBLIC','keyParameter':cdn.KEY_PARAMETERS['a']}
        def write(**kwargs):
            self.assertEqual(kwargs['IfMatch'],'before-etag')
            value.update(Body=kwargs['Body'],VersionId='after',ETag='after-etag')
            raise RuntimeError('response lost after write')
        s3.put_object.side_effect = write
        with tempfile.TemporaryDirectory() as temp, patch.object(cdn,'runtime_settings',return_value=settings):
            receipt=Path(temp)/'source.json'
            with self.assertRaisesRegex(RuntimeError,'response lost'):
                cdn.configure_source(session,'a',receipt)
            self.assertEqual(cdn.configure_source(session,'a',receipt)['version'],'after')
            self.assertEqual(s3.put_object.call_count,1)
            self.assertEqual(json.loads(receipt.read_text())['previousConfig'],config)
            self.assertEqual(json.loads(value['Body'])['zone'],'unchanged')

    def test_retirement_refuses_trusted_or_referenced_keys_and_unrelated_template_changes(self):
        old = cdn.front_template(cdn.source_template('cloudfront.yaml'),{'cdn-studio':{'origin':'cdn-test','bucket':'test','prefix':'m8-final'}},{'a':'public-a','b':'public-b'})
        with self.assertRaises(ValueError): cdn.retirement_template(old,'CdnPublicKeyB')
        rotated=cdn.rotation_template(old,'b','new-public-b')
        retired=cdn.retirement_template(rotated,'CdnPublicKeyB')
        self.assertNotIn('CdnPublicKeyB',retired['Resources'])
        cf=Mock();cf.get_template.side_effect=[{'TemplateBody':rotated},{'TemplateBody':retired}]
        changes={'Changes':[{'ResourceChange':{'LogicalResourceId':'CdnPublicKeyB','ResourceType':'AWS::CloudFront::PublicKey','Action':'Remove'}}]}
        cdn.validate_changes(cf,'layr-cloudfront','change',changes,'CdnPublicKeyB')
        with self.assertRaises(ValueError): cdn.validate_changes(Mock(),'layr-cloudfront','change',changes)
        altered=copy.deepcopy(retired);altered['Resources']['Distribution']['Properties']['DistributionConfig']['Enabled']=False
        cf.get_template.side_effect=[{'TemplateBody':rotated},{'TemplateBody':altered}]
        with self.assertRaises(ValueError): cdn.validate_changes(cf,'layr-cloudfront','change',changes,'CdnPublicKeyB')

    def test_dns_dependency_updates_require_identical_definitions(self):
        cf = Mock()
        base = cdn.source_template('cloudfront.yaml')
        cf.get_template.return_value = {'TemplateBody': base}
        changes = {'Changes': [{'ResourceChange': {'LogicalResourceId': 'A',
            'Action': 'Modify', 'Replacement': 'False'}}]}
        cdn.validate_changes(cf, 'layr-cloudfront', 'change', changes)
        other = copy.deepcopy(base)
        other['Resources']['A']['Properties']['Name'] = 'other.example'
        cf.get_template.side_effect = [{'TemplateBody': base}, {'TemplateBody': other}]
        with self.assertRaises(ValueError):
            cdn.validate_changes(cf, 'layr-cloudfront', 'change', changes)
        changes['Changes'][0]['ResourceChange']['LogicalResourceId'] = 'Instance'
        with self.assertRaises(ValueError):
            cdn.validate_changes(cf, 'layr-cloudfront', 'change', changes)

    def test_canned_signature_is_path_exact_and_verifies_with_library_rsa_sha256(self):
        import base64
        from urllib.parse import parse_qs, urlsplit
        from cryptography.hazmat.primitives.asymmetric import rsa, padding
        from cryptography.hazmat.primitives import hashes
        key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        url = 'https://cloud.layr.ws/o/cdn-studio/states/id/12345678.tree.age'
        query = parse_qs(urlsplit(cdn.signed_url(url, key, 'PUBLICKEY', 2000)).query)
        self.assertEqual(query['Hash-Algorithm'], ['SHA256'])
        self.assertEqual(query['Expires'], ['2000'])
        signature = base64.b64decode(query['Signature'][0].translate(str.maketrans('-_~', '+=/')))
        policy = json.dumps({'Statement': [{'Resource': url, 'Condition': {
            'DateLessThan': {'AWS:EpochTime': 2000}}}]}, separators=(',', ':')).encode()
        key.public_key().verify(signature, policy, padding.PKCS1v15(), hashes.SHA256())
        from cryptography.exceptions import InvalidSignature
        with self.assertRaises(InvalidSignature):
            key.public_key().verify(signature, policy.replace(b'tree.age', b'data.age'), padding.PKCS1v15(), hashes.SHA256())

    def test_front_keeps_existing_origin_and_behavior_and_isolates_cache_paths(self):
        base = cdn.source_template('cloudfront.yaml')
        original = copy.deepcopy(base)
        stores = {'layr': {'origin': 'cdn-layr', 'bucket': 'bucket-layr', 'prefix': 'layr'},
                  'cdn-studio': {'origin': 'cdn-test', 'bucket': 'bucket-test', 'prefix': 'm8-final'}}
        front = cdn.front_template(base, stores, {'a': 'public-a', 'b': 'public-b'})
        self.assertEqual(base, original)
        config = front['Resources']['Distribution']['Properties']['DistributionConfig']
        old = original['Resources']['Distribution']['Properties']['DistributionConfig']
        self.assertEqual(config['DefaultCacheBehavior'], old['DefaultCacheBehavior'])
        self.assertEqual(config['Origins'][0], old['Origins'][0])
        behavior, = config['CacheBehaviors']
        self.assertEqual(behavior['PathPattern'], '/o/*')
        self.assertEqual(behavior['TrustedKeyGroups'], [{'Ref': 'CdnKeys'}])
        self.assertNotIn('OriginRequestPolicyId', behavior)
        router = front['Resources']['CdnRouter']['Properties']['FunctionCode']
        self.assertIn("request.uri = '/' + route[1] + '/' + match[2]", router)
        stores['cdn-studio']['prefix'] = 'layr'
        with self.assertRaises(ValueError):
            cdn.origin_router(stores)

    def test_distribution_read_policy_excludes_mutable_classes_and_preserves_create_guards(self):
        base = cdn.source_template('template.yaml')
        template = cdn.store_policy(base, f'arn:aws:cloudfront::{cdn.ACCOUNT}:distribution/{cdn.DISTRIBUTION}')
        old = base['Resources']['BucketPolicy']['Properties']['PolicyDocument']['Statement']
        rows = template['Resources']['BucketPolicy']['Properties']['PolicyDocument']['Statement']
        self.assertEqual(rows[:-1], old)
        self.assertEqual(rows[-1]['Action'], 's3:GetObject')
        text = json.dumps(rows[-1]['Resource'])
        for mutable in ['heads/', 'keys/', 'requests/', 'claims/', 'store.json', 'public.json', 'people/']:
            self.assertNotIn(mutable, text)
        self.assertEqual(template['Resources']['Bucket'], base['Resources']['Bucket'])
        self.assertEqual(cdn.store_policy(template, f'arn:aws:cloudfront::{cdn.ACCOUNT}:distribution/{cdn.DISTRIBUTION}'), template)


if __name__ == '__main__':
    unittest.main()
