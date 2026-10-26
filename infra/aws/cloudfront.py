#!/usr/bin/env python3
"""Plan and deploy the Layr Cloud front. Secrets stay in SDK memory; stdout is public.

Run only after the active gateway/executor deployer confirms its AWS run is done.
Inspect the named change set before executing it. Execution never replaces an instance
or removes a CloudFormation resource. Use AWS_PROFILE=ifdotpy credentials via boto3.
"""
import argparse
import hashlib
import json
from pathlib import Path
import secrets

import boto3
from botocore.exceptions import ClientError

ROOT = Path(__file__).resolve().parent
REGION = "eu-north-1"
ACCOUNT = "134250228509"
ZONE = "Z00094863IAG61NP8YENE"
ORIGIN_SECRET = "/layr-store/gateway/origin-secret"


def current_stack(cf, name):
    try:
        return cf.describe_stacks(StackName=name)["Stacks"][0]
    except ClientError as error:
        if (error.response["Error"]["Code"] == "ValidationError"
                and "does not exist" in error.response["Error"]["Message"]):
            return None
        raise


def prepare_secret(session):
    ssm = session.client("ssm")
    try:
        parameter = ssm.get_parameter(Name=ORIGIN_SECRET, WithDecryption=False)["Parameter"]
        if parameter["Type"] != "SecureString":
            raise ValueError("Existing origin secret must be a SecureString")
    except ssm.exceptions.ParameterNotFound:
        try:
            ssm.put_parameter(Name=ORIGIN_SECRET, Value=secrets.token_hex(32),
                              Type="SecureString", Overwrite=False)
        except ssm.exceptions.ParameterAlreadyExists:
            pass
    print("origin secret SecureString ready")


def plan(session, name, template, values, region=REGION, origin_secret=False):
    cf = session.client("cloudformation", region_name=region)
    current = current_stack(cf, name)
    text = (ROOT / template).read_text()
    digest = hashlib.sha256((text + json.dumps(values, sort_keys=True)).encode()).hexdigest()[:16]
    change = "cloudfront-" + digest
    # Recover the receipt before repeating a possibly completed mutation.
    try:
        previous = cf.describe_change_set(StackName=name, ChangeSetName=change)
        print(json.dumps({"stack": name, "region": region, "changeSet": change,
                          "status": previous["Status"], "executionStatus": previous["ExecutionStatus"]}))
        return change
    except ClientError as error:
        code = error.response["Error"]["Code"]
        absent_stack = (code == "ValidationError" and current is None
                        and "does not exist" in error.response["Error"]["Message"])
        if code != "ChangeSetNotFound" and not absent_stack:
            raise
    parameters = {p["ParameterKey"]: {"ParameterKey": p["ParameterKey"], "UsePreviousValue": True}
                  for p in (current or {}).get("Parameters", [])}
    parameters.update({k: {"ParameterKey": k, "ParameterValue": v} for k, v in values.items()})
    if origin_secret and not current:
        parameter = session.client("ssm").get_parameter(
            Name=ORIGIN_SECRET, WithDecryption=True)["Parameter"]
        value = parameter["Value"]
        if (parameter["Type"] != "SecureString" or len(value) != 64
                or not all(c in "abcdef0123456789" for c in value)):
            raise ValueError("Invalid origin secret SecureString")
        parameters["OriginSecret"] = {"ParameterKey": "OriginSecret", "ParameterValue": value}
    result = cf.create_change_set(
        StackName=name, ChangeSetName=change, ChangeSetType="UPDATE" if current else "CREATE",
        TemplateBody=text, Parameters=list(parameters.values()), Capabilities=["CAPABILITY_NAMED_IAM"],
        ClientToken=name + "-" + digest)
    print(json.dumps({"stack": name, "region": region, "changeSet": change, "id": result["Id"]}))
    return change


def inspect(cf, name, change):
    cf.get_waiter("change_set_create_complete").wait(
        StackName=name, ChangeSetName=change, WaiterConfig={"Delay": 5, "MaxAttempts": 120})
    result = cf.describe_change_set(StackName=name, ChangeSetName=change)
    rows = [{"id": c["ResourceChange"]["LogicalResourceId"],
             "type": c["ResourceChange"]["ResourceType"], "action": c["ResourceChange"]["Action"],
             "replacement": c["ResourceChange"].get("Replacement")} for c in result.get("Changes", [])]
    print(json.dumps({"changeSet": change, "status": result["Status"],
                      "executionStatus": result["ExecutionStatus"], "changes": rows}))
    if any(r["action"] == "Remove" for r in rows):
        raise ValueError("Refusing resource removal")
    if any(r["replacement"] != "False" for r in rows
           if r["type"] == "AWS::EC2::Instance" and r["action"] == "Modify"):
        raise ValueError("Instance replacement requires the lead's decision")
    return result


def execute(cf, name, change):
    result = inspect(cf, name, change)
    if result["ExecutionStatus"] in ("EXECUTE_IN_PROGRESS", "EXECUTE_COMPLETE"):
        print("execution already started", name, change)
        return
    if result["ExecutionStatus"] != "AVAILABLE":
        raise ValueError("Change set is not available")
    cf.execute_change_set(StackName=name, ChangeSetName=change, ClientRequestToken=name + "-" + change)
    print("executed", name, change)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["secret"] + [f"{kind}-{operation}"
                        for kind in ("certificate", "front", "gateway")
                        for operation in ("plan", "inspect", "execute")])
    parser.add_argument("--change")
    parser.add_argument("--certificate")
    parser.add_argument("--dns", choices=["true", "false"], default="false")
    args = parser.parse_args()
    if args.action.endswith(("inspect", "execute")) and not args.change:
        parser.error("--change is required")
    if args.action == "front-plan" and not args.certificate:
        parser.error("--certificate is required")
    session = boto3.Session(profile_name="ifdotpy", region_name=REGION)
    if session.client("sts").get_caller_identity()["Account"] != ACCOUNT:
        raise SystemExit("Refusing AWS account other than " + ACCOUNT)
    if args.action == "secret":
        prepare_secret(session)
        return
    kind, operation = args.action.split("-")
    name = {"certificate": "layr-cloudfront-certificate", "front": "layr-cloudfront",
            "gateway": "layr-gateway"}[kind]
    region = "us-east-1" if kind == "certificate" else REGION
    if operation == "plan":
        if kind == "certificate":
            plan(session, name, "cloudfront-certificate.yaml", {"Host": "cloud.layr.ws", "ZoneId": ZONE}, region)
        elif kind == "front":
            plan(session, name, "cloudfront.yaml", {"ZoneId": ZONE,
                 "CertificateArn": args.certificate, "PublishDns": args.dns}, origin_secret=True)
        else:
            plan(session, name, "gateway.yaml", {"Host": "origin.cloud.layr.ws"})
    else:
        cf = session.client("cloudformation", region_name=region)
        (execute if operation == "execute" else inspect)(cf, name, args.change)


if __name__ == "__main__":
    main()
