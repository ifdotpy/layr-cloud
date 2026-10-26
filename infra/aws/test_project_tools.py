"""Check ciphertext-only publication and cleanup of per-command transport grants."""
import importlib.util
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


class ProjectToolsTests(TestCase):
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
