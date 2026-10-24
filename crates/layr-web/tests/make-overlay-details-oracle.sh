#!/bin/bash
# Copy the CLI contracts exported by t37 from the same frozen store under test.
set -euo pipefail
: "${LAYR_MODEL_FIXTURE_OUT:?Set LAYR_MODEL_FIXTURE_OUT to the t37 JSON export}"
: "${LAYR_ORACLE_OUT:?Set LAYR_ORACLE_OUT to the output directory}"
for label in visible hidden-reader; do
    mkdir -p "$LAYR_ORACLE_OUT/$label"
    cp "$LAYR_MODEL_FIXTURE_OUT/overlay-show-$label.json" "$LAYR_ORACLE_OUT/$label/show.json"
    cp "$LAYR_MODEL_FIXTURE_OUT/overlay-ops-$label.json" "$LAYR_ORACLE_OUT/$label/ops.json"
done

# File paths retain their independent CLI oracle; show JSON contains metadata only.
: "${LAYR_BIN:?Set LAYR_BIN to the pinned CLI binary}"
: "${LAYR_ORACLE_ARCHIVE:?Set LAYR_ORACLE_ARCHIVE to the t37 store}"
: "${LAYR_ORACLE_PAPER:?Set LAYR_ORACLE_PAPER to its disposable recovery file}"
RUN=$(mktemp -d /root/work/layr-web-overlay-oracle.XXXXXX)
truncate -s 2G "$RUN/store.img"
mkfs.btrfs -q -f "$RUN/store.img"
mkdir "$RUN/store"
mount -o loop,user_subvol_rm_allowed "$RUN/store.img" "$RUN/store"
trap 'cd /; umount "$RUN/store"' EXIT
export LAYR_ROOT="$RUN/store" LAYR_PAPER_KEY_FILE="$LAYR_ORACLE_PAPER"
unset LAYR_PROJECT LAYR_LINE LAYR_SOCKET LAYR_PERSON_KEY_FILE
"$LAYR_BIN" clone "file://$LAYR_ORACLE_ARCHIVE" model-golden -q
cd "$LAYR_ROOT/projects/model-golden/lines/main"
"$LAYR_BIN" overlay ls-files secrets > "$LAYR_ORACLE_OUT/visible/paths.txt"
"$LAYR_BIN" overlay ls-files hidden-golden > "$LAYR_ORACLE_OUT/hidden-reader/paths.txt"
