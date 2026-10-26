"""Check ciphertext-only publication and cleanup of per-command transport grants."""
import importlib.util
import contextlib
import json
import tempfile
from pathlib import Path
from unittest import TestCase
from unittest.mock import Mock, patch


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


publisher = module('publish_project')
grant = module('with_gateway_grant')
enroller = module('enroll_executor')


class ProjectToolsTests(TestCase):
    def test_different_existing_store_refuses_before_writing_any_ciphertext(self):
        session = Mock()
        cf, s3 = Mock(), Mock()
        session.client.side_effect = lambda name: cf if name == 'cloudformation' else s3
        cf.describe_stacks.return_value = {'Stacks': [{'Outputs': [
            {'OutputKey': 'BucketName', 'OutputValue': 'synthetic-bucket'}],
            'Parameters': [{'ParameterKey': 'Prefix', 'ParameterValue': 'layr'}]}]}
        files = [('store.json', b'{"project":"owner-project"}'), ('slots/test.age', b'ciphertext')]
        with patch.object(publisher, 'seed_files', return_value=({'project': 'owner-project'}, files)), patch.object(publisher, 'current', return_value=b'{"project":"different"}'), self.assertRaisesRegex(ValueError, 'descriptor differs'):
            publisher.publish(session, 'layr', Path('/synthetic'))
        s3.put_object.assert_not_called()

    def test_cdn_seed_uploads_at_most_eight_objects_and_publishes_descriptor_last(self):
        import threading
        session, cf, s3 = Mock(), Mock(), Mock()
        session.client.side_effect = lambda name: cf if name == 'cloudformation' else s3
        cf.describe_stacks.return_value = {'Stacks': [{'Outputs': [
            {'OutputKey': key, 'OutputValue': key} for key in
            ('BucketName', 'ReaderRoleArn', 'WriterRoleArn', 'GcRoleArn')],
            'Parameters': [{'ParameterKey': 'Prefix', 'ParameterValue': 'm8-final'}]}]}
        descriptor = {'project': 'synthetic-project', 'recipient': 'synthetic-recipient'}
        files = [('store.json', json.dumps(descriptor).encode()), *[(f'states/{i}.age', b'cipher') for i in range(16)]]
        barrier, mutex = threading.Barrier(8), threading.Lock()
        completed, running, peak = [], 0, 0
        def upload(client, bucket, key, data):
            nonlocal running, peak
            self.assertEqual(bucket, 'BucketName')
            self.assertTrue(key.startswith('m8-final/'))
            if key == 'm8-final/store.json':
                self.assertEqual(len(completed), 16)
                self.assertEqual(running, 0)
                completed.append(key)
                return
            with mutex:
                running += 1
                peak = max(peak, running)
            barrier.wait(timeout=5)
            with mutex:
                running -= 1
                completed.append(key)
        missing = publisher.ClientError({'Error': {'Code': 'NoSuchKey'}}, 'GetObject')
        s3.get_object.side_effect = missing
        with patch.object(publisher, 'seed_files', return_value=(descriptor, files)), patch.object(publisher, 'current', return_value=None), patch.object(publisher, 'immutable', side_effect=upload):
            publisher.publish(session, 'cdn-studio', Path('/synthetic'), rehearsal=True)
        self.assertEqual(peak, 8)
        self.assertEqual(completed[-1], 'm8-final/store.json')
        cf.describe_stacks.assert_called_once_with(StackName='layr-store-m8-final')
        self.assertEqual(s3.put_object.call_args.kwargs['Key'], 'gateway/projects/cdn-studio.json')
        for project, rehearsal in [('cdn-studio', False), ('layr-studio', True)]:
            session.reset_mock()
            with patch.object(publisher, 'seed_files', return_value=(descriptor, files)), self.assertRaisesRegex(ValueError, 'rehearsal'):
                publisher.publish(session, project, Path('/synthetic'), rehearsal=rehearsal)
            session.client.assert_not_called()

    def test_enrollment_marks_ready_only_after_verified_project_and_keeps_grants_separate(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source.json'
            source.write_text('{"hostKind":"executor","region":"eu-north-1"}')
            ready = root / 'ready'
            profile = {'project': 'layr', 'writeTokenParameter': '/layr-store/executor/projects/layr/write-token'}
            @contextlib.contextmanager
            def token(source, own):
                self.assertEqual(own, profile)
                yield root / 'private-token'
            with patch.object(enroller, 'HOME', root), patch.object(enroller, 'READY', ready), patch.object(enroller.os, 'geteuid', return_value=0), patch.object(enroller.checks, 'SOURCE', source), patch.object(enroller.checks, 'project_profiles', return_value=[profile]), patch.object(enroller.checks, 'project_token', token), patch.object(enroller.subprocess, 'run') as run:
                run.side_effect = RuntimeError('clone failed')
                with self.assertRaises(RuntimeError):
                    enroller.enroll('layr', 'igor@example.test', 'a' * 64)
                self.assertFalse(ready.exists())
                run.side_effect = None
                enroller.enroll('layr', 'igor@example.test', 'a' * 64)
                self.assertTrue(ready.exists())
                self.assertEqual(run.call_args.args[0][-1], 'fsck')
                for call in run.call_args_list:
                    self.assertEqual(call.kwargs['env']['LAYR_GATEWAY_TOKEN_FILE'], str(root / 'private-token'))

    def test_plaintext_seed_is_refused_before_any_aws_call(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'store.json').write_text('{}')
            (root / 'paper').write_text('synthetic plaintext')
            session = Mock()
            with self.assertRaises(ValueError):
                publisher.publish(session, 'layr', root)
            session.client.assert_not_called()

    def test_age_suffix_does_not_allow_plaintext_or_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            path = root / 'slot.age'
            path.write_text('synthetic plaintext')
            with self.assertRaises(ValueError):
                publisher.seed_files(root)
            path.unlink()
            path.symlink_to(root / 'other')
            with self.assertRaises(ValueError):
                publisher.seed_files(root)

    def test_failed_cli_removes_private_transport_file(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ssm = Mock()
            ssm.get_parameter.return_value = {'Parameter': {'Type': 'SecureString', 'Value': 'synthetic-grant'}}
            session = Mock()
            session.client.return_value = ssm
            paths = []
            def run(args, env):
                path = Path(env['LAYR_GATEWAY_TOKEN_FILE'])
                paths.append(path)
                self.assertEqual(path.read_text(), 'synthetic-grant')
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                raise RuntimeError('failed caller')
            with patch.object(grant, 'verify_ram'), patch.object(grant.boto3, 'Session', return_value=session), patch.object(grant.subprocess, 'run', side_effect=run), self.assertRaises(RuntimeError):
                grant.run('/layr-store/owner/projects/layr/write-token', root, ['layr', 'fetch', 'origin'])
            self.assertEqual(len(paths), 1)
            self.assertFalse(paths[0].exists())

    def test_transport_loader_refuses_gateway_session_secret(self):
        with patch.object(grant.boto3, 'Session') as session, self.assertRaises(ValueError):
            grant.run('/layr-store/gateway/session-secret', Path('/unused'), ['layr'])
        session.assert_not_called()
