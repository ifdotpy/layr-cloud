"""Check that live-template updates preserve hosts and reject broader changes."""
import copy
import importlib.util
from pathlib import Path
from unittest import TestCase
from unittest.mock import Mock
import yaml

spec = importlib.util.spec_from_file_location('project_iam', Path(__file__).with_name('project_iam.py'))
iam = importlib.util.module_from_spec(spec)
spec.loader.exec_module(iam)


class ProjectIamTests(TestCase):
    def test_gateway_parameter_write_allowlist_does_not_change_the_origin_or_host(self):
        source = yaml.load(Path(__file__).with_name('gateway.yaml').read_text(), Loader=iam.Loader)
        live = copy.deepcopy(source)
        live['Resources']['Instance']['Properties']['UserData'] = {'live': 'existing gateway'}
        live['Parameters'].pop('ProjectWriteTokenParameterArns')
        result = iam.gateway_template(live, source)
        for name in live['Resources']:
            if name != 'Role':
                self.assertEqual(result['Resources'][name], live['Resources'][name])
        self.assertEqual(result['Resources']['Role'], source['Resources']['Role'])

    def test_live_host_drift_is_preserved_and_only_role_and_parameters_change(self):
        source = yaml.load(Path(__file__).with_name('executor.yaml').read_text(), Loader=iam.Loader)
        live = copy.deepcopy(source)
        live['Resources']['Instance']['Properties']['UserData'] = {'live': 'existing deployment'}
        for name in ('AdditionalStoreRoleArns', 'ProjectWriteTokenParameterArns'):
            live['Parameters'].pop(name)
        result = iam.executor_template(live, source)
        self.assertEqual(result['Resources']['Instance'], live['Resources']['Instance'])
        self.assertEqual(result['Resources']['DataAttachment'], live['Resources']['DataAttachment'])
        self.assertEqual(result['Resources']['Role'], source['Resources']['Role'])
        self.assertIn('ProjectWriteTokenParameterArns', result['Parameters'])

    def test_inspection_rejects_instance_modification_even_without_replacement(self):
        cf = Mock()
        cf.describe_change_set.return_value = {'ExecutionStatus': 'AVAILABLE', 'Changes': [
            {'ResourceChange': {'LogicalResourceId': 'Instance', 'ResourceType': 'AWS::EC2::Instance',
                                'Action': 'Modify', 'Replacement': 'False'}}]}
        with self.assertRaises(ValueError):
            iam.inspect(cf, 'layr-executor-layr', 'test')
