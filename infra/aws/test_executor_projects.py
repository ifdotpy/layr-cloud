"""Exercise rendered profiles through the runtime and parse the actual systemd units."""
import importlib.util
import shutil
import subprocess
import tempfile
from pathlib import Path
from unittest import TestCase, skipUnless
from unittest.mock import patch


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


render = module('executor_projects')
checks = module('checks')


class ExecutorProjectsTests(TestCase):
    def test_rendered_profiles_are_accepted_by_the_runtime(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            render.render(root, ['layr', 'layr-cloud', 'layr-studio'])
            with patch.object(checks, 'PROJECTS', root / 'projects.d'):
                profiles = checks.project_profiles()
            self.assertEqual({p['project'] for p in profiles}, {'layr', 'layr-cloud', 'layr-studio'})

    @skipUnless(shutil.which('systemd-analyze'), 'requires Linux systemd')
    def test_systemd_accepts_units_without_legacy_grant_dependency(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            render.render(root, ['layr'])
            units = root / 'systemd'
            for name in ('layr', 'layr-data'):
                (units / (name + '.service')).write_text('[Service]\nExecStart=/bin/true\n')
            subprocess.run(['systemd-analyze', 'verify', '--man=no',
                            str(units / 'layr.service'), str(units / 'layr-data.service'),
                            str(units / 'layr-projects.service'), str(units / 'layr-projects.timer')],
                           check=True, capture_output=True, text=True)
            drop = (units / 'layr.service.d/projects.conf').read_text()
            self.assertIn('UnsetEnvironment=LAYR_GATEWAY_TOKEN_FILE', drop)
            self.assertNotIn('layr-executor-token.service', drop)
