#!/bin/bash
# Run as root in layr-dev. Clone a frozen real CLI archive and capture its overlay read outputs.
set -euo pipefail
: "${LAYR_BIN:?Set LAYR_BIN to the tested CLI binary}"
: "${LAYR_ORACLE_ARCHIVE:?Set LAYR_ORACLE_ARCHIVE to the archive path}"
: "${LAYR_ORACLE_PAPER:?Set LAYR_ORACLE_PAPER to its disposable recovery file}"
: "${LAYR_ORACLE_PROJECT:?Set LAYR_ORACLE_PROJECT to the CLI project name}"
: "${LAYR_ORACLE_OVERLAY:?Set LAYR_ORACLE_OVERLAY to the overlay name}"
RUN=$(mktemp -d /root/work/layr-web-overlay-oracle.XXXXXX)
truncate -s 2G "$RUN/store.img"
mkfs.btrfs -q -f "$RUN/store.img"
mkdir "$RUN/store"
mount -o loop,user_subvol_rm_allowed "$RUN/store.img" "$RUN/store"
trap 'cd /; umount "$RUN/store"' EXIT
export LAYR_ROOT="$RUN/store" LAYR_PAPER_KEY_FILE="$LAYR_ORACLE_PAPER"
unset LAYR_PROJECT LAYR_LINE LAYR_SOCKET LAYR_PERSON_KEY_FILE
"$LAYR_BIN" clone "file://$LAYR_ORACLE_ARCHIVE" "$LAYR_ORACLE_PROJECT" -q
cd "$LAYR_ROOT/projects/$LAYR_ORACLE_PROJECT/lines/main"
# b1a23c1 show has no --json; log's first state is its exact current head.
"$LAYR_BIN" overlay log "$LAYR_ORACLE_OVERLAY" --json -n 100000 > "$RUN/log.json"
"$LAYR_BIN" overlay ls-files "$LAYR_ORACLE_OVERLAY" > "$RUN/paths.txt"
python3 - "$RUN" <<'PY'
import json,os,pathlib,sqlite3,subprocess,sys
root=pathlib.Path(sys.argv[1])
project=root/'store/projects'/os.environ['LAYR_ORACLE_PROJECT']
name=os.environ['LAYR_ORACLE_OVERLAY']
db=sqlite3.connect((project/'meta/overlays'/name/'meta/db/layr.sqlite').as_uri()+'?mode=ro',uri=True)
local_machine=json.loads(subprocess.check_output([os.environ['LAYR_BIN'],'remote','self','--json']))['id']
# Clone records its own machine/trust locally. They were never in the frozen archive.
raw=[row[0] for row in db.execute('select raw from records order by time desc,seq desc')]
raw=[line for line in raw if json.loads(line)['r']['machine'] != local_machine]
(root/'signed-history.jsonl').write_text('\n'.join(raw)+'\n')
records=[json.loads(line)['r'] for line in raw]
(root/'history.json').write_text(json.dumps(records))
settings={}
for record in reversed(records): settings.update(record.get('data',{}).get('config',{}))
parent=sqlite3.connect((project/'meta/db/layr.sqlite').as_uri()+'?mode=ro',uri=True)
for raw, in parent.execute('select raw from records order by time,seq'):
    settings.update(json.loads(raw)['r'].get('data',{}).get('config',{}))
(root/'patterns.json').write_text(json.dumps(settings.get('overlay.'+name,{}).get('patterns',[])))
keys=sorted((r for r in records if r['op']=='overlay.key'),key=lambda r:(r['seq'],r['machine'],r['id']))
epochs=[]
seen=set()
for key in keys:
    epoch=key['data']['epoch']
    if epoch in seen: continue
    seen.add(epoch)
    item=dict(id=epoch,number=len(epochs)+1,time=key['time'],machine=key['machine'])
    if 'reason' in key['data']: item['reason']=key['data']['reason']
    epochs.append(item)
(root/'epochs.json').write_text(json.dumps(epochs))
PY
printf 'Oracle: %s\n' "$RUN"
