"""Check the IAM boundaries and the shell/Python programs embedded in the templates."""

import re
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).parent


class Loader(yaml.SafeLoader):
    pass


def intrinsic(loader, tag, node):
    if isinstance(node, yaml.ScalarNode):
        value = loader.construct_scalar(node)
    elif isinstance(node, yaml.SequenceNode):
        value = loader.construct_sequence(node)
    else:
        value = loader.construct_mapping(node)
    return {tag if tag == "Ref" else f"Fn::{tag}": value}


Loader.add_multi_constructor("!", intrinsic)


def template(name):
    return yaml.load((ROOT / name).read_text(), Loader=Loader)


def statements(resource):
    result = []
    for policy in resource["Properties"]["Policies"]:
        result.extend(policy["PolicyDocument"]["Statement"])
    return result


def actions(resource):
    result = set()
    for statement in statements(resource):
        value = statement["Action"]
        result.update(value if isinstance(value, list) else [value])
    return result


class TemplateTests(unittest.TestCase):
    def test_fallback_transfer_applies_requests_and_stops_before_push_or_index_on_failure(self):
        source = template("executor.yaml")["Resources"]["Instance"]["Properties"]["UserData"]["Fn::Base64"]["Fn::Sub"]
        script = re.search(r"cat > /usr/local/sbin/layr-executor-transfer <<'EOF'\n(.*?)\nEOF", source, re.S)[1]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "profile").write_text("PROJECT=example\n")
            binary = root / "layr"
            binary.write_text("#!/usr/bin/env python3\nimport json,os,sys\na=sys.argv[3:]\nwith open(os.environ['TRACE'],'a') as f:f.write(json.dumps(a)+'\\n')\nsys.exit(31 if a[0]==os.environ.get('FAIL') else 0)\n")
            binary.chmod(0o755)
            for kind, failure, expected in [
                ("executor", "", ["fetch", "requests", "push", "index"]),
                ("executor", "requests", ["fetch", "requests"]),
                ("executor", "push", ["fetch", "requests", "push"]),
                ("runner", "", ["fetch", "push"]),
            ]:
                trace = root / "trace"
                trace.unlink(missing_ok=True)
                code = script.replace("${HostKind}", kind).replace("/etc/layr-executor/profile.env", str(root / "profile")).replace("/opt/layr-executor/current/layr", str(binary))
                result = subprocess.run(["bash"], input=code, text=True, capture_output=True, env=dict(os.environ, TRACE=str(trace), FAIL=failure))
                self.assertEqual(result.returncode, 31 if failure else 0, result.stderr)
                calls = [json.loads(line) for line in trace.read_text().splitlines()]
                self.assertEqual([call[0] for call in calls], expected)
                if "requests" in expected:
                    self.assertEqual(calls[1], ["requests", "apply", "--remote", "origin", "--json"])

    def test_project_iam(self):
        resources = template("template.yaml")["Resources"]
        self.assertEqual(actions(resources["Writer"]), {"s3:PutObject", "s3:AbortMultipartUpload", "s3:GetObject", "s3:ListBucket"})
        writer_resources = statements(resources["Writer"])[0]["Resource"]
        self.assertCountEqual(writer_resources, [
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/store.json"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/public.json"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/keys/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/records/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/files/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/states/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/slots/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/requests/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/index/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/overlays/*/keys/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/overlays/*/epochs/*/records/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/overlays/*/epochs/*/states/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/overlays/*/epochs/*/slots/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/store.json"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/public.json"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/records/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/files/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/states/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/slots/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/history/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/index/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/requests/*"},
            {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/claims/*"},
        ])
        self.assertNotIn({"Fn::Sub": "${Bucket.Arn}/${Prefix}/people/*"}, writer_resources)
        writer_get = statements(resources["Writer"])[1]
        self.assertEqual(writer_get["Action"], "s3:GetObject")
        self.assertEqual(writer_get["Resource"], {"Fn::Sub": "${Bucket.Arn}/${Prefix}/overlays/*/keys/*"})
        writer_list = statements(resources["Writer"])[2]
        self.assertEqual(writer_list["Action"], "s3:ListBucket")
        self.assertEqual(writer_list["Resource"], {"Fn::GetAtt": "Bucket.Arn"})
        self.assertEqual(writer_list["Condition"], {"StringLike": {"s3:prefix": [
            {"Fn::Sub": "${Prefix}/records/*"}, {"Fn::Sub": "${Prefix}/states/*"}, {"Fn::Sub": "${Prefix}/requests/*"},
            {"Fn::Sub": "${Prefix}/keys/"}, {"Fn::Sub": "${Prefix}/overlays/*/keys/"},
        ]}})
        self.assertEqual(actions(resources["Reader"]), {"s3:GetObject", "s3:ListBucket"})
        self.assertEqual(actions(resources["Gc"]), {"s3:DeleteObject"})
        self.assertEqual(statements(resources["Gc"])[0]["Resource"],
                         [{"Fn::Sub": "${Bucket.Arn}/${Prefix}/states/*/*.data.age"},
                          {"Fn::Sub": "${Bucket.Arn}/${Prefix}/files/*"},
                          {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/states/*/*.data.age"},
                          {"Fn::Sub": "${Bucket.Arn}/${Prefix}/project/epochs/*/files/*"}])
        for role in ("Writer", "Reader", "Gc"):
            trust = resources[role]["Properties"]["AssumeRolePolicyDocument"]["Statement"][0]
            self.assertEqual(trust["Condition"]["ArnEquals"]["aws:PrincipalArn"], {"Ref": "GatewayRoleArns"})
        policy = resources["BucketPolicy"]["Properties"]["PolicyDocument"]["Statement"]
        conditional = next(item for item in policy if item["Sid"] == "RequireConditionalCreate")
        self.assertEqual(conditional["Principal"], "*")
        self.assertEqual(conditional["Condition"]["Null"]["s3:if-none-match"], "true")
        self.assertEqual(conditional["Condition"]["Bool"]["s3:ObjectCreationOperation"], "true")
        value = next(item for item in policy if item["Sid"] == "RequireCreateOnlyValue")
        self.assertEqual(value["Condition"]["StringNotEquals"]["s3:if-none-match"], "*")
        self.assertEqual(resources["Bucket"]["DeletionPolicy"], "Retain")

    def test_release_file_resources_are_scoped_to_writer_and_gc(self):
        resources = template("template.yaml")["Resources"]
        expected = {"${Bucket.Arn}/${Prefix}/files/*", "${Bucket.Arn}/${Prefix}/project/epochs/*/files/*"}
        for role, permissions in [("Writer", {"s3:PutObject", "s3:AbortMultipartUpload"}), ("Gc", {"s3:DeleteObject"})]:
            grant = statements(resources[role])[0]
            actual = grant["Action"] if isinstance(grant["Action"], list) else [grant["Action"]]
            self.assertEqual(set(actual), permissions)
            paths = {path["Fn::Sub"] for path in grant["Resource"]}
            self.assertEqual({path for path in paths if path.endswith("/files/*")}, expected)
        # A write grant cannot acquire file read or listing permissions.
        self.assertNotIn("s3:GetObject", statements(resources["Writer"])[0]["Action"])
        self.assertNotIn({"Fn::Sub": "${Prefix}/files/*"}, statements(resources["Writer"])[2]["Condition"]["StringLike"]["s3:prefix"])

    def test_gc_removes_noncurrent_bytes_without_expiring_current_objects(self):
        bucket = template("template.yaml")["Resources"]["Bucket"]["Properties"]
        self.assertEqual(bucket["VersioningConfiguration"]["Status"], "Enabled")
        rules = bucket["LifecycleConfiguration"]["Rules"]
        rule = next(item for item in rules if item["Id"] == "RemoveGarbageCollectedStateData")
        self.assertEqual(rule["Status"], "Enabled")
        self.assertEqual(rule["Prefix"], {"Fn::Sub": "${Prefix}/states/"})
        self.assertEqual(rule["NoncurrentVersionExpiration"], {"NoncurrentDays": 30})
        self.assertTrue(rule["ExpiredObjectDeleteMarker"])
        self.assertNotIn("ExpirationInDays", rule)
        self.assertNotIn("ExpirationDate", rule)
        self.assertNotIn("NoncurrentVersionExpiration", rules[0])

    def test_host_roles_do_not_access_project_objects(self):
        for name, kind in (("gateway.yaml", "gateway"), ("executor.yaml", "executor")):
            role = template(name)["Resources"]["Role"]
            for item in statements(role):
                # Optional Route 53 statements use Fn::If.
                if "Action" not in item:
                    continue
                value = item["Action"]
                values = value if isinstance(value, list) else [value]
                if any(action.startswith("s3:") for action in values):
                    self.assertIn("${ArtifactsBucket}", item["Resource"]["Fn::Sub"])
                if "sts:AssumeRole" in values:
                    expected = {"Ref": "StoreRoleArns"} if kind == "gateway" else [
                        {"Ref": "ReaderRoleArn"}, {"Ref": "WriterRoleArn"}]
                    self.assertEqual(item["Resource"], expected)

    def test_public_ipv6_ingress_and_ssm_administration(self):
        for name in ("gateway.yaml","executor.yaml"):
            host=template(name)["Resources"]
            ingress=host["SecurityGroup"]["Properties"]["SecurityGroupIngress"]
            self.assertEqual(ingress,[{"IpProtocol":"tcp","FromPort":443,"ToPort":443,"CidrIpv6":"::/0"}] if name=="gateway.yaml" else [])
            self.assertIn({"Fn::Sub":"arn:${AWS::Partition}:iam::aws:policy/AmazonSSMManagedInstanceCore"},host["Role"]["Properties"]["ManagedPolicyArns"])
            code=host["Instance"]["Properties"]["UserData"]["Fn::Base64"]["Fn::Sub"]
            self.assertIn('"UseDualStackEndpoint":true',code)
            self.assertIn('"network":"public-ipv6"' if name=="executor.yaml" else '"network": "public-ipv6"',code)
            self.assertNotIn("apt-get install -y tailscale",code)
            self.assertNotIn("AuthKeyParameter",code)

    def test_host_security_and_retained_data(self):
        for name in ("gateway.yaml", "executor.yaml"):
            instance = template(name)["Resources"]["Instance"]["Properties"]
            self.assertEqual(instance["MetadataOptions"]["HttpTokens"], "required")
            self.assertEqual(instance["MetadataOptions"]["HttpProtocolIpv6"], "enabled")
            self.assertFalse(instance["NetworkInterfaces"][0]["AssociatePublicIpAddress"])
        executor = template("executor.yaml")["Resources"]["Instance"]["Properties"]["UserData"]["Fn::Base64"]["Fn::Sub"]
        self.assertIn("useradd --system --user-group --no-create-home --home-dir /nonexistent", executor)
        self.assertIn("--shell /usr/sbin/nologin layr-registry", executor)
        self.assertIn("Environment=LAYR_GATEWAY_TOKEN_FILE=/run/layr-executor-token/write-token", executor)
        volume = template("executor.yaml")["Resources"]["DataVolume"]
        self.assertEqual(volume["Properties"]["VolumeType"], "gp3")
        self.assertTrue(volume["Properties"]["Encrypted"])
        self.assertEqual(volume["DeletionPolicy"], "Retain")
        self.assertEqual(volume["UpdateReplacePolicy"], "Retain")

    def test_executor_runtime_token_and_runner_wake_boundary(self):
        host = template("executor.yaml")
        instance = host["Resources"]["Instance"]["Properties"]
        self.assertEqual(instance["InstanceInitiatedShutdownBehavior"], "stop")
        data = instance["UserData"]["Fn::Base64"]["Fn::Sub"]
        self.assertIn("RuntimeDirectory=layr-executor-token", data)
        self.assertIn("RuntimeDirectoryMode=0700", data)
        self.assertIn("PartOf=layr.service", data)
        self.assertIn("ExecStop=/bin/rm -f /run/layr-executor-token/write-token", data)
        self.assertIn("!= 'tmpfs'", data)
        self.assertNotIn("LAYR_GATEWAY_TOKEN_FILE=/etc/", data)
        role = host["Resources"]["Role"]
        wake = next(item["Fn::If"][1] for item in statements(role)
                    if "Fn::If" in item and item["Fn::If"][1]["Action"] == "ec2:StartInstances")
        self.assertEqual(wake["Resource"], {"Fn::Sub": "arn:${AWS::Partition}:ec2:${AWS::Region}:${AWS::AccountId}:instance/${RunnerInstanceId}"})
        self.assertEqual(wake["Condition"], {"StringEquals": {"ec2:ResourceTag/layr:kind": "runner"}})
        self.assertLess(len(data.encode()), 16 * 1024)

    def test_user_data_programs(self):
        for name in ("gateway.yaml", "executor.yaml"):
            code = template(name)["Resources"]["Instance"]["Properties"]["UserData"]["Fn::Base64"]["Fn::Sub"]
            # CloudFormation values cannot affect shell grammar through this substitution.
            code = re.sub(r"\$\{[A-Za-z0-9:.]+\}", "example", code)
            result = subprocess.run(["bash", "-n"], input=code, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            for path, content in re.findall(r"cat > (\S+) <<'EOF'\n(.*?)\nEOF", code, re.S):
                if path.endswith(".py"):
                    compile(content, path, "exec")
                if path.startswith("/usr/local/sbin/"):
                    result = subprocess.run(["bash", "-n"], input=content, text=True, capture_output=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("nodejs", code)

    def test_runbook_shell_and_executor_secrets(self):
        runbook = (ROOT / "README.md").read_text()
        for code in re.findall(r"```sh\n(.*?)\n```", runbook, re.S):
            result = subprocess.run(["bash", "-n"], input=code, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
        executor_profile = runbook.split("## Configure the executor profile", 1)[1].split("## Upgrade", 1)[0]
        self.assertNotIn("LAYR_PAPER_KEY_FILE", executor_profile)
        self.assertNotIn("/run/layr-paper", executor_profile)
        self.assertIn("--public-key", executor_profile)
        self.assertIn("config registry.user layr-registry", executor_profile)


if __name__ == "__main__":
    unittest.main()
