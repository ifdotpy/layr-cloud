#!/bin/bash
# Disposable executor project. Keep the mount until native and browser checks finish.
set -euo pipefail
: "${LAYR_BIN:?Set LAYR_BIN to the pinned native binary}"
RUN=$(mktemp -d /root/work/layr-web-actions.XXXXXX)
truncate -s 1G "$RUN/store.img"
mkfs.btrfs -q -f "$RUN/store.img"
mkdir "$RUN/store" "$RUN/src"
mount -o loop,user_subvol_rm_allowed "$RUN/store.img" "$RUN/store"
export LAYR_ROOT="$RUN/store" LAYR_AUTHOR_NAME=BrowserTest LAYR_AUTHOR_EMAIL=browser@example.com
printf '# Browser actions\nReal executor project.\n' > "$RUN/src/README.md"
"$LAYR_BIN" init browser-actions --from "$RUN/src" -q >/dev/null
cd "$LAYR_ROOT/projects/browser-actions/lines/main"
printf 'main bytes\n' > test.txt
"$LAYR_BIN" add test.txt
"$LAYR_BIN" save -qm first
"$LAYR_BIN" line new feature -q
cd ../feature
printf 'feature bytes\nsecond line\n' > test.txt
"$LAYR_BIN" save -qam feature
"$LAYR_BIN" line list --json > "$RUN/lines.json"
"$LAYR_BIN" change list --json > "$RUN/changes.json"
"$LAYR_BIN" remote add store "file://$RUN/archive" --paper-key-out "$RUN/paper" >/dev/null
"$LAYR_BIN" push store -q
printf '%s\n' "$RUN"
