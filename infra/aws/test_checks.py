"""Check the host boundary, including failed-check work and idle shutdown exclusion."""
import importlib.util
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
