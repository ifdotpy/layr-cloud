#!/bin/bash
# Private fixture material stays in VM tmpfs. The caller owns cleanup after inspection.
set -euo pipefail
: "${LAYR_BIN:?Set the exact native binary}"
: "${LAYR_PREPARE_PERSON:?Set the prepare_gateway example binary}"
: "${LAYR_FIXTURE_LOGIN:?Set a unique disposable login}"
: "${LAYR_FIXTURE_ALIAS:?Set a unique disposable project alias}"
[ "$(findmnt -nro FSTYPE -T /run)" = tmpfs ]
umask 077
RUN=$(mktemp -d /run/layr-web-epoch.XXXXXX)
cleanup_failed() {
    rc=$?
    if [ "$rc" -ne 0 ]; then
        cd /
        if mountpoint -q "$RUN/store"; then umount "$RUN/store"; fi
        rm -rf "$RUN"
    fi
    exit "$rc"
}
trap cleanup_failed EXIT
truncate -s 1G "$RUN/store.img"
mkfs.btrfs -q -f "$RUN/store.img"
mkdir "$RUN/store" "$RUN/src"
mount -o loop,user_subvol_rm_allowed "$RUN/store.img" "$RUN/store"
export LAYR_ROOT="$RUN/store" LAYR_AUTHOR_NAME=EpochTest LAYR_AUTHOR_EMAIL=epoch@example.test
printf '# Epoch fixture\n' > "$RUN/src/README.md"
"$LAYR_BIN" init "$LAYR_FIXTURE_ALIAS" --from "$RUN/src" -q
cd "$LAYR_ROOT/projects/$LAYR_FIXTURE_ALIAS/lines/main"
printf 'root history\n' > test.txt
"$LAYR_BIN" add test.txt
"$LAYR_BIN" save -qm first
"$LAYR_BIN" line new feature -q
cd ../feature
printf 'root history\nfeature before rotation\n' > test.txt
"$LAYR_BIN" save -qam feature
"$LAYR_BIN" remote add store "file://$RUN/archive" --paper-key-out "$RUN/paper" >/dev/null
"$LAYR_BIN" push store -q
"$LAYR_PREPARE_PERSON" "$RUN/archive" "$RUN/paper" "$RUN/person" "$LAYR_FIXTURE_LOGIN"
recipient=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["recipient"])' "$RUN/person/person-view.json")
LAYR_PERSON_LOGIN="$LAYR_FIXTURE_LOGIN" "$LAYR_BIN" remote grant store --person "$LAYR_FIXTURE_LOGIN" --recipient "$recipient" --invite-out "$RUN/invite.age" >/dev/null
public=$(cat "$RUN/person/signing-public.txt")
"$LAYR_BIN" access person-key "$LAYR_FIXTURE_LOGIN" "$public"
"$LAYR_BIN" access grant "person:$LAYR_FIXTURE_LOGIN" maintainer
"$LAYR_BIN" protect main --approvers "person:$LAYR_FIXTURE_LOGIN" --approvals 1
if [ -n "${LAYR_FIXTURE_EXECUTOR_ID:-}" ]; then
    : "${LAYR_FIXTURE_EXECUTOR_PUBLIC:?Set the executor public key}"
    "$LAYR_BIN" remote trust store "$LAYR_FIXTURE_EXECUTOR_ID" --public-key "$LAYR_FIXTURE_EXECUTOR_PUBLIC"
fi
if [ "${LAYR_FIXTURE_REMOVED_READER:-0}" = 1 ]; then
    LAYR_ROOT="$RUN/store/removed-root" "$LAYR_BIN" remote self --json > "$RUN/removed-machine.json"
    read -r removed_id removed_public < <(python3 - "$RUN/removed-machine.json" <<'PY'
import json,sys
value=json.load(open(sys.argv[1]));print(value['id'],value['public_key'])
PY
    )
    "$LAYR_BIN" remote trust store "$removed_id" --public-key "$removed_public"
fi
"$LAYR_BIN" push store -q
if [ "${LAYR_FIXTURE_ROTATE:-0}" = 1 ]; then
    LAYR_PAPER_KEY_FILE="$RUN/paper" "$LAYR_BIN" remote rotate store --reason 'rotated browser fixture' >/dev/null
    printf 'root history\nfeature before rotation\nnew epoch bytes\n' > test.txt
    "$LAYR_BIN" save -qam 'after rotation'
    "$LAYR_BIN" push store -q
fi
"$LAYR_BIN" change list --json > "$RUN/changes.json"
"$LAYR_BIN" line list --json > "$RUN/lines.json"
python3 - "$RUN" "$LAYR_FIXTURE_LOGIN" "$LAYR_FIXTURE_ALIAS" <<'PY'
import json,os,pathlib,subprocess,sys
root=pathlib.Path(sys.argv[1]);login,alias=sys.argv[2:]
lines=json.loads((root/'lines.json').read_text())
for name in ['main','feature']:
 state=next(line['head'] for line in lines if line['name']==name)
 result=subprocess.run([os.environ['LAYR_BIN'],'show',state+':test.txt'],check=True,capture_output=True,text=True)
 (root/(name+'.txt')).write_text(result.stdout)
view=json.loads((root/'person/person-view.json').read_text())
registration={k:view[k] for k in ['recipient','signing_key','signing']};registration['paper']=view['slots'][0]['data']
(root/'registration.json').write_text(json.dumps(registration))
project=json.loads((root/'archive/store.json').read_text())['project']
change=next(c['change_id'] for c in json.loads((root/'changes.json').read_text()) if c['message']=='feature')
epochs=list((root/'archive/keys').glob('*.json'))
epoch=json.loads(epochs[-1].read_text())['r']['data']['epoch'] if epochs else None
(root/'browser.json').write_text(json.dumps(dict(login=login,alias=alias,project=project,change=change,epoch=epoch,code_lines={n:(root/(n+'.txt')).read_text() for n in ['main','feature']},words_file=str(root/'person/recovery.txt'))))
PY
printf '%s\n' "$RUN"
