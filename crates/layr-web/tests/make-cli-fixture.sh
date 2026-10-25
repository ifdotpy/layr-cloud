#!/bin/bash
# Run as root in layr-dev. LAYR_BIN names a built CLI binary.
set -euo pipefail
: "${LAYR_BIN:?Set LAYR_BIN to the native layr binary}"
RUN=$(mktemp -d /root/work/layr-web-fixture.XXXXXX)
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
if [[ ${LAYR_WEB_M5_FIXTURE:-0} == 1 ]]; then
  "$LAYR_BIN" config check.release-smoke 'test -f test.txt'
  "$LAYR_BIN" try -q main --check release-smoke
  "$LAYR_BIN" ref set release/0.1.0 main >/dev/null
  "$LAYR_BIN" log --json | python3 -c 'import json,sys; print(json.dumps([json.loads(line) for line in sys.stdin if line.strip()]))' > "$RUN/release-1-log.json"
fi
"$LAYR_BIN" line new feature -q
cd ../feature
printf 'feature bytes\n' > test.txt
"$LAYR_BIN" save -qam feature
if [[ ${LAYR_WEB_M5_FIXTURE:-0} == 1 ]]; then
  "$LAYR_BIN" try -q feature --check release-smoke
  "$LAYR_BIN" ref set release/0.2.0 feature >/dev/null
  "$LAYR_BIN" ref set env/prod feature >/dev/null
  "$LAYR_BIN" log --json | python3 -c 'import json,sys; print(json.dumps([json.loads(line) for line in sys.stdin if line.strip()]))' > "$RUN/release-2-log.json"
  "$LAYR_BIN" line new exp-one main -q
  "$LAYR_BIN" line new exp-two main -q
  "$LAYR_BIN" line new agent main --count 2 -q
  "$LAYR_BIN" attestations feature --json > "$RUN/release-attestations.json"
  "$LAYR_BIN" ref list --json > "$RUN/refs.json"
  "$LAYR_BIN" op log --json > "$RUN/operations.json"
fi
"$LAYR_BIN" line list --json > "$RUN/lines.json"
"$LAYR_BIN" remote add store "file://$RUN/archive" --paper-key-out "$RUN/paper" >/dev/null
"$LAYR_BIN" push store -q
printf 'Fixture: %s\n' "$RUN"
