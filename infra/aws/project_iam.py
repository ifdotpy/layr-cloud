#!/usr/bin/env python3
"""Extend project IAM on the live templates without changing host definitions."""
import argparse
import copy
import hashlib
import json
from pathlib import Path

import boto3
import yaml
from botocore.exceptions import ClientError

ACCOUNT = '134250228509'
REGION = 'eu-north-1'
PROJECTS = ('layr', 'layr-cloud', 'layr-studio')


class Loader(yaml.SafeLoader):
    pass


def intrinsic(loader, tag, node):
    key = 'Ref' if tag == 'Ref' else 'Fn::' + tag
    value = (loader.construct_scalar(node) if isinstance(node, yaml.ScalarNode)
             else loader.construct_sequence(node) if isinstance(node, yaml.SequenceNode)
             else loader.construct_mapping(node))
    return {key: value}


Loader.add_multi_constructor('!', intrinsic)


def executor_template(live, source):
    result = copy.deepcopy(live)
    for section, names in (
            ('Parameters', ('AdditionalStoreRoleArns', 'ProjectWriteTokenParameterArns')),
            ('Conditions', ('HasAdditionalStores', 'HasProjectTokens'))):
        for name in names:
            result.setdefault(section, {})[name] = source[section][name]
    statements = result['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
    additions = source['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
    for statement in additions:
        if statement.get('Fn::If', [None])[0] == 'HasAdditionalStores':
            if statement not in statements:
                statements.append(statement)
        elif statement.get('Action') in ('ssm:GetParameter', ['ssm:GetParameter', 'ssm:GetParameters', 'ssm:GetParametersByPath']):
            matches = [i for i, old in enumerate(statements)
                       if old.get('Effect') == statement['Effect'] and old.get('Action') == statement['Action']]
            if len(matches) != 1:
                raise ValueError('Live SSM policy differs; review before updating')
            statements[matches[0]] = statement
    for name in live['Resources']:
        if name != 'Role' and result['Resources'][name] != live['Resources'][name]:
            raise ValueError('Only Role may change')
    return result


def role_arns(kinds):
    return ['arn:aws:iam::' + ACCOUNT + ':role/layr-store-' + kind + '-' + project
            for project in PROJECTS for kind in kinds]


def gateway_template(live, source):
    result = copy.deepcopy(live)
    result.setdefault('Parameters', {})['ProjectWriteTokenParameterArns'] = source['Parameters']['ProjectWriteTokenParameterArns']
    result.setdefault('Conditions', {})['HasProjectTokens'] = source['Conditions']['HasProjectTokens']
    statements = result['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
    source_statements = source['Resources']['Role']['Properties']['Policies'][0]['PolicyDocument']['Statement']
    statement = next(row for row in source_statements if row.get('Fn::If', [None])[0] == 'HasProjectTokens')
    if statement not in statements:
        statements.append(statement)
    return result


def plan(cf, name):
    stack = cf.describe_stacks(StackName=name)['Stacks'][0]
    body = cf.get_template(StackName=name, TemplateStage='Original')['TemplateBody']
    live = yaml.load(body, Loader=Loader) if isinstance(body, str) else body
    parameters = {p['ParameterKey']: {'ParameterKey': p['ParameterKey'], 'UsePreviousValue': True}
                  for p in stack['Parameters']}
    if name == 'layr-gateway':
        old = next(p['ParameterValue'] for p in stack['Parameters'] if p['ParameterKey'] == 'StoreRoleArns')
        values = {'StoreRoleArns': ','.join(dict.fromkeys(old.split(',') + role_arns(('reader', 'writer', 'gc')))),
                  'ProjectWriteTokenParameterArns': ','.join(
                      'arn:aws:ssm:' + REGION + ':' + ACCOUNT + ':parameter/layr-store/' + kind
                      + '/projects/' + project + '/write-token'
                      for project in PROJECTS for kind in ('owner', 'executor', 'runner'))}
        source = yaml.load(Path(__file__).with_name('gateway.yaml').read_text(), Loader=Loader)
        template = gateway_template(live, source)
    else:
        source = yaml.load(Path(__file__).with_name('executor.yaml').read_text(), Loader=Loader)
        template = executor_template(live, source)
        values = {'AdditionalStoreRoleArns': ','.join(role_arns(('reader', 'writer'))),
                  'ProjectWriteTokenParameterArns': ','.join(
                      'arn:aws:ssm:' + REGION + ':' + ACCOUNT + ':parameter/layr-store/executor/projects/'
                      + project + '/write-token' for project in PROJECTS)}
    parameters.update({k: {'ParameterKey': k, 'ParameterValue': v} for k, v in values.items()})
    encoded = json.dumps(template, sort_keys=True)
    digest = hashlib.sha256((name + encoded + json.dumps(values, sort_keys=True)).encode()).hexdigest()[:16]
    change = 'm1-iam-' + digest
    try:
        previous = cf.describe_change_set(StackName=name, ChangeSetName=change)
        print(json.dumps({'stack': name, 'changeSet': change, 'status': previous['Status'],
                          'executionStatus': previous['ExecutionStatus']}))
        return
    except ClientError as error:
        if error.response['Error']['Code'] != 'ChangeSetNotFound':
            raise
    response = cf.create_change_set(StackName=name, ChangeSetName=change, ChangeSetType='UPDATE',
                                   TemplateBody=encoded, Parameters=list(parameters.values()),
                                   Capabilities=['CAPABILITY_NAMED_IAM'], ClientToken=name + '-' + change)
    print(json.dumps({'stack': name, 'changeSet': change, 'id': response['Id']}))


def inspect(cf, name, change):
    cf.get_waiter('change_set_create_complete').wait(StackName=name, ChangeSetName=change)
    result = cf.describe_change_set(StackName=name, ChangeSetName=change)
    rows = [item['ResourceChange'] for item in result.get('Changes', [])]
    if not rows or any(row['LogicalResourceId'] != 'Role' or row['ResourceType'] != 'AWS::IAM::Role'
                       or row['Action'] != 'Modify' or row.get('Replacement') != 'False' for row in rows):
        raise ValueError('Refusing change set other than Role Modify Replacement=False')
    print(json.dumps({'stack': name, 'changeSet': change, 'executionStatus': result['ExecutionStatus'],
                      'changes': [{k: row.get(k) for k in ('LogicalResourceId', 'Action', 'Replacement')} for row in rows]}))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('plan', 'inspect', 'execute'))
    parser.add_argument('--stack', choices=('layr-gateway', 'layr-executor-layr'), required=True)
    parser.add_argument('--change')
    args = parser.parse_args()
    if args.action != 'plan' and not args.change:
        parser.error('--change is required')
    session = boto3.Session(profile_name='ifdotpy', region_name=REGION)
    if session.client('sts').get_caller_identity()['Account'] != ACCOUNT:
        raise ValueError('Unexpected AWS account')
    cf = session.client('cloudformation')
    if args.action == 'plan':
        plan(cf, args.stack)
    else:
        result = inspect(cf, args.stack, args.change)
        if args.action == 'execute':
            if result['ExecutionStatus'] in ('EXECUTE_IN_PROGRESS', 'EXECUTE_COMPLETE'):
                print('Execution already started')
            elif result['ExecutionStatus'] == 'AVAILABLE':
                cf.execute_change_set(StackName=args.stack, ChangeSetName=args.change,
                                      ClientRequestToken=args.stack + '-' + args.change)
                print('Execution started')
            else:
                raise ValueError('Change set is not available')


if __name__ == '__main__':
    main()
