#!/bin/bash
# Export the existing real CLI release suite after its public epoch rotation.
set -eu
: "${LAYR_CLI_SOURCE:?Set the shared CLI source folder}"
: "${LAYR_BIN:?Set the real CLI binary}"
: "${LAYR_RELEASE_EXPORT:?Set a new disposable fixture output folder}"
export LAYR="$LAYR_BIN"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cp "$LAYR_CLI_SOURCE/tests/lib.sh" "$scratch/lib.sh"
python3 - "$LAYR_CLI_SOURCE/tests/t47-release-files.sh" "$scratch/t47-release-files.sh" <<'PY'
from pathlib import Path
import sys
source=Path(sys.argv[1]).read_text()
start=source.index('# An optional disposable export feeds the real-gateway browser tests.')
end=source.index('# Tightening to a private epoch',start)
export=source[start:end]
source=source[:start]+source[end:]
needle="run(['ref','delete',name]);run(['ref','delete','release/previous'])"
assert source.count(needle)==1
source=source.replace(needle,export+needle)
source=source.replace("run(['remote','mode','origin','private'])", "run(['remote','mode','origin','private'],dict(base,LAYR_PAPER_KEY_FILE=str(root/'store-paper')))")
Path(sys.argv[2]).write_text(source)
PY
bash "$scratch/t47-release-files.sh"
python3 - "$LAYR_RELEASE_EXPORT" <<'PY'
from pathlib import Path
import json,sys
home=Path(sys.argv[1]); metadata=json.loads((home/'fixture.json').read_text())
store=home/'store'; parents={'root'}; children={}
for path in (store/'keys').glob('*.json'):
 record=json.loads(path.read_text())['r']; key=record['data']
 assert key['parent'] not in children
 children[key['parent']]=key['epoch']
epoch='root'
while epoch in children:
 epoch=children[epoch]
 assert epoch not in parents
 parents.add(epoch)
assert epoch!='root', 'fixture must have a current epoch'
metadata['prefix']=f'project/epochs/{epoch}/'
metadata['previous']='release/previous'
assert (store/metadata['prefix']/metadata['files'][0]['object']).is_file()
(home/'fixture.json').write_text(json.dumps(metadata))
PY
