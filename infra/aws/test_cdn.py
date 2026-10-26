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
