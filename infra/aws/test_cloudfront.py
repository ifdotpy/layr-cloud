"""Deployment safety and secret-handling boundaries, without AWS requests."""
import contextlib
import importlib.util
import io
import unittest
from pathlib import Path
from unittest.mock import Mock

from botocore.exceptions import ClientError

spec = importlib.util.spec_from_file_location("cloudfront_deploy", Path(__file__).with_name("cloudfront.py"))
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


def missing(code="ChangeSetNotFound", message="Change set does not exist"):
    return ClientError({"Error": {"Code": code, "Message": message}}, "DescribeChangeSet")


def change(resource="Distribution", kind="AWS::CloudFront::Distribution", action="Modify", replacement="False"):
    return {"ResourceChange": {"LogicalResourceId": resource, "ResourceType": kind,
                               "Action": action, "Replacement": replacement}}


class CloudFrontDeployTests(unittest.TestCase):
    def session(self):
        session = Mock()
        cf, ssm = Mock(), Mock()
        session.client.side_effect = lambda name, **kwargs: {"cloudformation": cf, "ssm": ssm}[name]
        return session, cf, ssm

    def test_new_front_secret_is_only_in_sdk_parameter_and_receipt_is_public(self):
        session, cf, ssm = self.session()
        cf.describe_stacks.side_effect = missing("ValidationError", "Stack does not exist")
        cf.describe_change_set.side_effect = missing()
        secret = "ab" * 32
        ssm.get_parameter.return_value = {"Parameter": {"Value": secret, "Type": "SecureString"}}
        cf.create_change_set.return_value = {"Id": "public-change-id"}
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            deploy.plan(session, "layr-cloudfront", "cloudfront.yaml", {"ZoneId": "ZEXAMPLE"}, origin_secret=True)
        args = cf.create_change_set.call_args.kwargs
        self.assertEqual(args["ChangeSetType"], "CREATE")
        self.assertIn({"ParameterKey": "OriginSecret", "ParameterValue": secret}, args["Parameters"])
        self.assertNotIn(secret, stdout.getvalue())
        ssm.get_parameter.assert_called_once_with(Name=deploy.ORIGIN_SECRET, WithDecryption=True)
        self.assertTrue(args["ClientToken"].startswith("layr-cloudfront-"))

    def test_update_preserves_secret_and_unrelated_parameters(self):
        session, cf, ssm = self.session()
        cf.describe_stacks.return_value = {"Stacks": [{"Parameters": [{"ParameterKey": key} for key in ("Host", "OriginSecret")]}]}
        cf.describe_change_set.side_effect = missing()
        cf.create_change_set.return_value = {"Id": "public-change-id"}
        with contextlib.redirect_stdout(io.StringIO()):
            deploy.plan(session, "layr-cloudfront", "cloudfront.yaml", {"PublishDns": "true"}, origin_secret=True)
        self.assertEqual(cf.create_change_set.call_args.kwargs["Parameters"], [
            {"ParameterKey": "Host", "UsePreviousValue": True},
            {"ParameterKey": "OriginSecret", "UsePreviousValue": True},
            {"ParameterKey": "PublishDns", "ParameterValue": "true"}])
        ssm.get_parameter.assert_not_called()

    def test_existing_receipt_prevents_a_second_mutation(self):
        session, cf, ssm = self.session()
        cf.describe_stacks.return_value = {"Stacks": [{"Parameters": []}]}
        cf.describe_change_set.return_value = {"Status": "CREATE_COMPLETE", "ExecutionStatus": "EXECUTE_COMPLETE"}
        with contextlib.redirect_stdout(io.StringIO()):
            deploy.plan(session, "layr-cloudfront", "cloudfront.yaml", {}, origin_secret=True)
        cf.create_change_set.assert_not_called()
        ssm.get_parameter.assert_not_called()

    def test_execution_refuses_instance_replacement_or_resource_removal(self):
        for row in [change("Instance", "AWS::EC2::Instance", replacement="True"),
                    change("Instance", "AWS::EC2::Instance", replacement="Conditional"),
                    change(action="Remove")]:
            cf = Mock()
            cf.describe_change_set.return_value = {"Status": "CREATE_COMPLETE", "ExecutionStatus": "AVAILABLE", "Changes": [row]}
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(ValueError):
                deploy.execute(cf, "layr-gateway", "changes")
            cf.execute_change_set.assert_not_called()

    def test_execution_recovery_does_not_execute_twice(self):
        cf = Mock()
        cf.describe_change_set.return_value = {"Status": "CREATE_COMPLETE", "ExecutionStatus": "EXECUTE_IN_PROGRESS", "Changes": [change()]}
        with contextlib.redirect_stdout(io.StringIO()):
            deploy.execute(cf, "layr-cloudfront", "changes")
        cf.execute_change_set.assert_not_called()


if __name__ == "__main__":
    unittest.main()
