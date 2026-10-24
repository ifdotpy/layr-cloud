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
"$LAYR_BIN" line new feature -q
cd ../feature
printf 'feature bytes\n' > test.txt
"$LAYR_BIN" save -qam feature
"$LAYR_BIN" line list --json > "$RUN/lines.json"
"$LAYR_BIN" remote add store "file://$RUN/archive" --paper-key-out "$RUN/paper" >/dev/null
"$LAYR_BIN" push store -q
printf 'Fixture: %s\n' "$RUN"
