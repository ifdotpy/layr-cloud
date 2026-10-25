#!/bin/bash
# Run as root in layr-dev with LAYR_BIN at the pinned native CLI.
set -euo pipefail
: "${LAYR_BIN:?Set LAYR_BIN to the pinned native CLI}"
RUN=$(mktemp -d /root/work/layr-web-index.XXXXXX)
truncate -s 1G "$RUN/store.img"
mkfs.btrfs -q -f "$RUN/store.img"
mkdir "$RUN/store" "$RUN/src"
mount -o loop,user_subvol_rm_allowed "$RUN/store.img" "$RUN/store"
trap 'cd /; umount "$RUN/store"' EXIT
export LAYR_ROOT="$RUN/store"
export LAYR_AUTHOR_NAME=BrowserTest LAYR_AUTHOR_EMAIL=browser@example.com
printf '# Browser test\nThese bytes come from the CLI store.\n' > "$RUN/src/README.md"
"$LAYR_BIN" init browser-test --from "$RUN/src" -q >/dev/null
cd "$LAYR_ROOT/projects/browser-test/lines/main"
printf 'main bytes\n' > test.txt
"$LAYR_BIN" add test.txt
"$LAYR_BIN" save -qm first
"$LAYR_BIN" line new feature -q
cd ../feature
printf 'feature bytes\n' > test.txt
"$LAYR_BIN" save -qam feature
"$LAYR_BIN" line list --json > "$RUN/lines.json"
"$LAYR_BIN" remote add store "file://$RUN/archive" --paper-key-out "$RUN/paper" >/dev/null
"$LAYR_BIN" push store --index -q
# A later record segment lets the browser prove that the first page precedes reconciliation.
"$LAYR_BIN" ref set env/test feature >/dev/null
"$LAYR_BIN" push store --index -q
for line in main feature; do
  "$LAYR_BIN" log "$line" --first-parent --json -- test.txt > "$RUN/history-$line.jsonl"
  "$LAYR_BIN" blame -s "$line" -- test.txt > "$RUN/blame-$line.txt"
done
printf 'browser-test\n' | "$LAYR_BIN" remote public store on >/dev/null
python3 - "$RUN" <<'PY'
from pathlib import Path
import json, sys
root = Path(sys.argv[1])
for name in ('main', 'feature'):
    rows = [json.loads(line) for line in (root / f'history-{name}.jsonl').read_text().splitlines()]
    (root / f'history-{name}.json').write_text(json.dumps(rows))
PY
printf 'Fixture: %s\n' "$RUN"
