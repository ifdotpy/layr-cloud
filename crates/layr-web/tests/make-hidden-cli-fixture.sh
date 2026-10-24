#!/bin/bash
# Run as root in layr-dev with the b1a23c1 CLI, using the web-core btrfs recipe.
set -euo pipefail
: "${LAYR_BIN:?Set LAYR_BIN to the native layr binary}"
RUN=$(mktemp -d /root/work/layr-web-hidden.XXXXXX)
truncate -s 1G "$RUN/store.img"
mkfs.btrfs -q -f "$RUN/store.img"
mkdir "$RUN/store" "$RUN/src"
mount -o loop,user_subvol_rm_allowed "$RUN/store.img" "$RUN/store"
trap 'cd /; umount "$RUN/store"' EXIT
export LAYR_ROOT="$RUN/store" HIDDEN_FIXTURE_ROOT="$RUN"
export LAYR_AUTHOR_NAME=BrowserTest LAYR_AUTHOR_EMAIL=browser@example.com
python3 - <<'PY'
import base64, json, os, pathlib, shutil, subprocess
root = pathlib.Path(os.environ['HIDDEN_FIXTURE_ROOT'])
binary = os.environ['LAYR_BIN']
base = dict(os.environ, LAYR_PROJECT='hidden-browser')
for key in ['LAYR_SOCKET', 'LAYR_PERSON_KEY_FILE', 'LAYR_PERSON_LOGIN', 'LAYR_PERSON_DIR', 'LAYR_LINE']:
    base.pop(key, None)
def run(args, env=base):
    result = subprocess.run([binary, *args], env=env, cwd=root, capture_output=True, text=True, timeout=120)
    assert result.returncode == 0, (args, result.stdout, result.stderr)
    return result.stdout
(root/'src/public.txt').write_text('shared browser bytes\n')
run(['init', 'hidden-browser', '--from', str(root/'src'), '-q'])
run(['remote', 'add', 'archive', 'file://' + str(root/'archive'), '--paper-key-out', str(root/'vault-paper')])
people = []
for label in ['reader', 'other']:
    login = 'hidden-' + label + '@example.com'
    directory = root/(label + '-person')
    paper = root/(label + '-paper')
    env = dict(base, LAYR_PERSON_DIR=str(directory), LAYR_PERSON_LOGIN=login)
    run(['person', 'keys', '--paper-key-out', str(paper)], env)
    env['LAYR_PERSON_KEY_FILE'] = str(paper)
    person = json.loads(run(['person', 'show', '--json'], env))
    invite = root/(label + '-invite.age')
    run(['remote', 'grant', 'archive', '--person', login, '--recipient', person['recipient'], '--invite-out', str(invite)], dict(base, LAYR_PERSON_LOGIN=login))
    run(['person', 'accept', '--invite-file', str(invite)], env)
    def b64(path): return base64.b64encode(path.read_bytes()).decode()
    view = dict(login=login, recipient=person['recipient'], slots=[dict(name='paper', data=b64(directory/'paper.age'))], index=b64(directory/'index.age'), inbox=[])
    (root/(label + '-view.json')).write_text(json.dumps(view))
    people.append(person)
machine = json.loads(run(['remote', 'self', '--json']))
readers = 'machine:' + machine['id'] + ',person:hidden-reader@example.com=' + people[0]['recipient']
run(['--line', 'main', 'overlay', 'create', 'hidden-browser-secret', '--hidden', '--readers', readers, '-q'])
line = root/'store/projects/hidden-browser/lines/main'
(line/'hidden.txt').write_text('hidden browser bytes\n')
run(['--line', 'main', 'overlay', 'move', 'hidden-browser-secret', 'hidden.txt', '-q'])
run(['--line', 'main', 'save', '-qm', 'hidden browser state'])
(root/'overlays.json').write_text(run(['overlay', 'list', '--json']))
(root/'lines.json').write_text(run(['line', 'list', '--json']))
run(['push', 'archive', '-q'])
shutil.copytree(root/'archive', root/'without-hidden', ignore=shutil.ignore_patterns('overlays'))
assert len(list((root/'archive').glob('overlays/*/keys/*.age'))) == 1
assert not list((root/'archive').glob('overlays/*/keys/*.json'))
PY
printf 'Fixture: %s\n' "$RUN"
