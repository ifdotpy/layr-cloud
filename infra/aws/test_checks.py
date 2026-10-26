"""Check the host boundary, including failed-check work and idle shutdown exclusion."""
import importlib.util
import contextlib
import io
import json
import os
import fcntl
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('host_checks', Path(__file__).with_name('checks.py'))
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)


def result(code=0, **changes):
    value = {'work_done': False, 'count': 0, 'checks': [], 'pending': 0, 'pending_privileged': 0}
    value.update(changes)
    return Mock(returncode=code, stdout=json.dumps(value))


class ChecksTests(unittest.TestCase):
    def test_failed_checks_are_work_but_operational_errors_are_not_success(self):
        value = checks.watcher_result(result(1, work_done=True, count=1, checks=[{'status':'failed'}]))
        self.assertTrue(value['work_done'])
        for bad in [result(128), Mock(returncode=0,stdout='{}\n{}'), result(count=1),
                    result(pending_privileged=-1), result(count=True)]:
            with self.assertRaises((ValueError, RuntimeError)):
                checks.watcher_result(bad)

    def test_only_pending_work_starts_the_exact_stopped_runner(self):
        import types, sys
        ec2 = Mock()
        ec2.describe_instances.return_value = {'Reservations':[{'Instances':[{'State':{'Name':'stopped'}}]}]}
        boto = Mock(client=Mock(return_value=ec2))
        source = {'region':'eu-north-1', 'runnerInstanceId':'i-0123456789abcdef0'}
        with patch.dict(sys.modules, {'boto3':boto}):
            checks.wake_runner(source, 0)
            ec2.start_instances.assert_not_called()
            checks.wake_runner(source, 2)
            ec2.start_instances.assert_called_once_with(InstanceIds=[source['runnerInstanceId']])
            ec2.start_instances.reset_mock()
            ec2.describe_instances.return_value['Reservations'][0]['Instances'][0]['State']['Name']='running'
            checks.wake_runner(source, 2)
            ec2.start_instances.assert_not_called()

    def test_idle_clock_and_active_lock_prevent_premature_stop(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source.json'
            source.write_text('{"hostKind":"runner"}')
            with patch.object(checks,'ROOT',root), patch.object(checks,'SOURCE',source), patch.object(checks.time,'monotonic',return_value=2000), patch.object(checks.subprocess,'run') as run:
                checks.idle({'hostKind':'runner'})
                run.assert_not_called()
                (root/'last-work').write_text('300')
                checks.idle({'hostKind':'runner'})
                run.assert_not_called()
                (root/'last-work').write_text('199')
                with (root/'active.lock').open('a') as active:
                    fcntl.flock(active,fcntl.LOCK_EX)
                    with patch('sys.argv',['checks.py','idle']):
                        checks.main()
                    run.assert_not_called()
                checks.idle({'hostKind':'runner'})
                run.assert_called_once_with(['systemctl','poweroff'],check=True)

    def test_tick_uses_nonroot_ipc_and_pushes_failed_results(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            profile = root/'profile.env'
            profile.write_text('PROJECT=layr\n')
            calls=[]
            def run(args, **kwargs):
                calls.append((args,kwargs))
                return result(1,work_done=True,count=1,checks=[{'status':'failed'}]) if args[0]==checks.RUNUSER else Mock(returncode=0,stdout="")
            with patch.object(checks,'ROOT',root), patch.object(checks,'PROFILE',profile), patch.object(checks.subprocess,'run',side_effect=run), patch.object(checks.time,'monotonic',return_value=42), patch('sys.stdout',new_callable=io.StringIO):
                checks.tick({'hostKind':'runner'})
            self.assertEqual((root/'last-work').read_text(),'42')
            watcher=next((args,kw) for args,kw in calls if args[0]==checks.RUNUSER)
            self.assertNotIn('--privileged-only',watcher[0])
            self.assertIn('layr-checks',watcher[0])
            self.assertEqual(watcher[1]['env']['LAYR_SOCKET'],'/run/layr.sock')
            self.assertEqual(watcher[0][0], '/usr/sbin/runuser')
            self.assertNotIn('LAYR_GATEWAY_TOKEN_FILE',watcher[1]['env'])
            self.assertNotIn('AWS_ACCESS_KEY_ID',watcher[1]['env'])
            self.assertEqual(calls[-1][0][-2:],['push','origin'])

    def test_executor_applies_requests_reports_pending_and_never_runs_checks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            profile = root/'profile.env'
            profile.write_text('PROJECT=layr\n')
            calls=[]
            def run(args, **kwargs):
                calls.append(args)
                return result(pending=2,pending_privileged=1) if args[0]==checks.RUNUSER else Mock(returncode=0,stdout="")
            with patch.object(checks,'ROOT',root), patch.object(checks,'PROFILE',profile), patch.object(checks.subprocess,'run',side_effect=run), patch.object(checks,'wake_runner') as wake, patch('sys.stdout',new_callable=io.StringIO):
                source={'hostKind':'executor','runnerInstanceId':'i-0123456789abcdef0'}
                checks.tick(source)
                wake.assert_called_once_with(source,2)
            self.assertIn(['requests','apply','--json'],[c[-3:] for c in calls])
            self.assertIn(['--local','checks.runner','false'],[c[-3:] for c in calls])
            self.assertIn(['--local','checks.privileged-runner','false'],[c[-3:] for c in calls])
            self.assertFalse((root/'last-work').exists())
            index = [checks.BINARY, '--project', 'layr', 'index', 'write', 'origin']
            self.assertEqual(calls.count(index), 1)
            self.assertEqual(calls[calls.index(index)-1][-2:], ['push','origin'])

    def test_failed_push_never_publishes_an_index(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            profile=root/'profile.env'
            profile.write_text('PROJECT=layr\n')
            calls=[]
            def run(args, **kwargs):
                calls.append(args)
                if args[-2:]==['push','origin']:
                    raise checks.subprocess.CalledProcessError(128,args)
                return result() if args[0]==checks.RUNUSER else Mock(returncode=0,stdout='')
            with patch.object(checks,'ROOT',root), patch.object(checks,'PROFILE',profile), patch.object(checks.subprocess,'run',side_effect=run):
                with self.assertRaises(checks.subprocess.CalledProcessError):
                    checks.tick({'hostKind':'executor'})
            self.assertNotIn(['index','write','origin'],[c[-3:] for c in calls])

    def profiles(self, root):
        folder = root / 'projects.d'
        folder.mkdir()
        for name in ['layr', 'layr-cloud', 'layr-studio']:
            (folder / (name + '.json')).write_text(json.dumps({
                'project': name, 'writeTokenParameter': '/layr-store/executor/projects/' + name + '/write-token'}))
        return folder

    def test_three_projects_get_separate_tokens_and_each_fetch_apply_push_index(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            folder = self.profiles(root)
            calls = []
            @contextlib.contextmanager
            def token(source, profile):
                yield root / (profile['project'] + '.token')
            def run(args, **kwargs):
                calls.append((args, kwargs))
                return result() if args[0] == checks.RUNUSER else Mock(returncode=0, stdout='')
            with patch.object(checks, 'PROJECTS', folder), patch.object(checks, 'project_token', token), patch.object(checks.subprocess, 'run', side_effect=run), patch('sys.stdout', new_callable=io.StringIO):
                checks.tick({'hostKind': 'executor'})
            for project in ['layr', 'layr-cloud', 'layr-studio']:
                own = [(a, kw) for a, kw in calls if a[0] == checks.BINARY and a[2] == project]
                self.assertEqual(own[0][0][-2:], ['fetch', 'origin'])
                self.assertIn(['requests', 'apply', '--json'], [a[-3:] for a, _ in own])
                self.assertEqual(own[-2][0][-2:], ['push', 'origin'])
                self.assertEqual(own[-1][0][-3:], ['index', 'write', 'origin'])
                self.assertTrue(all(kw['env']['LAYR_GATEWAY_TOKEN_FILE'] == str(root / (project + '.token')) for _, kw in own))

    def test_one_project_failure_does_not_skip_other_projects_or_publish_its_index(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            folder = self.profiles(root)
            calls = []
            @contextlib.contextmanager
            def token(source, profile):
                yield root / (profile['project'] + '.token')
            def run(args, **kwargs):
                calls.append(args)
                if args[0] == checks.BINARY and args[2] == 'layr-cloud' and args[-2:] == ['push', 'origin']:
                    raise checks.subprocess.CalledProcessError(128, args)
                return result() if args[0] == checks.RUNUSER else Mock(returncode=0, stdout='')
            with patch.object(checks, 'PROJECTS', folder), patch.object(checks, 'project_token', token), patch.object(checks.subprocess, 'run', side_effect=run), patch('sys.stdout', new_callable=io.StringIO), self.assertRaisesRegex(RuntimeError, 'layr-cloud'):
                checks.tick({'hostKind': 'executor'})
            self.assertNotIn([checks.BINARY, '--project', 'layr-cloud', 'index', 'write', 'origin'], calls)
            for name in ['layr', 'layr-studio']:
                self.assertIn([checks.BINARY, '--project', name, 'index', 'write', 'origin'], calls)

    def test_project_token_requires_tmpfs_exact_ssm_name_and_removes_grant(self):
        import sys
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'tokens'
            profile = {'project': 'layr', 'writeTokenParameter': '/layr-store/executor/projects/layr/write-token'}
            ssm = Mock()
            ssm.get_parameter.return_value = {'Parameter': {'Type': 'SecureString', 'Value': 'synthetic-grant'}}
            with patch.object(checks, 'TOKEN_ROOT', root), patch.object(checks.subprocess, 'run', return_value=Mock(stdout='tmpfs\n')), patch.dict(sys.modules, {'boto3': Mock(client=Mock(return_value=ssm))}):
                with checks.project_token({'region': 'eu-north-1'}, profile) as path:
                    self.assertEqual(path.read_text(), 'synthetic-grant')
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertFalse(path.exists())
                with self.assertRaises(RuntimeError):
                    with checks.project_token({'region': 'eu-north-1'}, profile) as path:
                        raise RuntimeError('project failed')
                self.assertFalse(path.exists())
            ssm.get_parameter.assert_called_with(Name=profile['writeTokenParameter'], WithDecryption=True)
            with patch.object(checks, 'TOKEN_ROOT', root), patch.object(checks.subprocess, 'run', return_value=Mock(stdout='ext4\n')), patch.dict(sys.modules, {'boto3': Mock(client=Mock(return_value=ssm))}), self.assertRaisesRegex(ValueError, 'tmpfs'):
                with checks.project_token({'region': 'eu-north-1'}, profile):
                    self.fail('persistent token was accepted')

    def test_project_profile_refuses_another_parameter_or_symlink(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            folder = self.profiles(root)
            path = folder / 'layr.json'
            path.write_text(json.dumps({'project': 'layr', 'writeTokenParameter': '/layr-store/gateway/session-secret'}))
            with patch.object(checks, 'PROJECTS', folder), self.assertRaises(ValueError):
                checks.project_profiles()
            path.unlink()
            path.symlink_to(folder / 'layr-cloud.json')
            with patch.object(checks, 'PROJECTS', folder), self.assertRaises(ValueError):
                checks.project_profiles()

    def test_malformed_project_profile_does_not_skip_valid_projects_or_use_legacy(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            folder = self.profiles(root)
            (folder / 'layr-cloud.json').write_text('{')
            @contextlib.contextmanager
            def token(source, profile):
                yield root / (profile['project'] + '.token')
            with patch.object(checks, 'PROJECTS', folder), patch.object(checks, 'project_token', token), patch.object(checks, 'tick_project') as tick, patch('sys.stdout', new_callable=io.StringIO), self.assertRaisesRegex(RuntimeError, 'layr-cloud'):
                checks.tick({'hostKind': 'executor'})
            self.assertEqual([call.args[1] for call in tick.call_args_list], ['layr-studio', 'layr'])

    @unittest.skipUnless(Path('/usr/sbin/runuser').is_file(), 'requires Linux runuser')
    def test_reader_launcher_runs_with_the_restricted_path(self):
        # The reader deliberately excludes sbin. Resolve its launcher before
        # entering that environment, as on the deployed Debian executor.
        env = {'PATH': '/usr/local/bin:/usr/bin:/bin'}
        process = checks.subprocess.run([checks.RUNUSER, '--version'], env=env,
                                        text=True, capture_output=True, check=True)
        self.assertIn('runuser', process.stdout)


if __name__ == '__main__':
    unittest.main()
