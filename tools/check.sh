#!/bin/sh
# Runs the recordings of out/input and compares what each run gives (its
# count of instructions, its frames, its sound) with what the same run
# gave when it was last recorded as good.
#
#   tools/check.sh [PROGRAM] [--record]
#
# PROGRAM is `target/release/andy` if not given; pass
# `out/recomp/target/release/andy-game` to check the compiled game, which
# must give the same as the interpreter. With --record the outcomes are
# written down as the good ones (out/check/NAME.good) instead of compared.
# TS2_GAME_DIR must be set; everything written is under out/, which git
# ignores. The graphics adapter is the software one unless the
# environment says otherwise: pictures are compared byte for byte, and
# two adapters need not draw the same bytes.
set -eu

program=target/release/andy
record=no
for argument in "$@"; do
    case "$argument" in
        --record) record=yes ;;
        *) program=$argument ;;
    esac
done
: "${TS2_GAME_DIR:?set TS2_GAME_DIR to the reference copy of the game}"
# The outcomes are those of the reference copy alone: another language or
# other discs' signs in the atlas change what is run.
unset TS2_LANGUAGE_DIR TS2_OTHER_DISCS

# A recording and the seconds of the machine's clock it is run for.
# A third field says more: `new` starts the run from a game never played
# (an empty state directory) and not from what out/run holds, as the
# recordings made by `play --record --state DIR` on an empty directory
# want; `new-orbit` moreover leaves the camera turned by the mouse on (the
# other recordings are of the keyboard and the original's camera).
runs="level:260 pause:200 options:170 options-list:135 en-menu:150 mouse-page:205 robot:94:new stairs:90:new wall:100:new orbit:64:new-orbit visor:70:new-orbit pad:64:new-orbit wasd:59:new-orbit cameras:74:new-orbit"

mkdir -p out/check
one() {
    name=${1%%:*}
    rest=${1#*:}
    seconds=${rest%%:*}
    work=out/check/$name
    rm -rf "$work"
    mkdir -p "$work"
    # A run never writes to out/run itself: it starts from a copy of it.
    state=$work/state
    cp -r out/run "$state"
    rm -f "$state/retold.ini"
    camera="--without orbit-camera --without walking-keys"
    # The outcomes are of the game's own picture, pixel for pixel.
    grain="--without render-scale"
    case $rest in *:new*) rm -rf "$state"; mkdir -p "$state" ;; esac
    case $rest in *-orbit) camera= ;; esac
    # shellcheck disable=SC2086
    "$program" boot --limit 12000000000 --seconds "$seconds" --gpu --state "$state" $camera $grain \
        --input "out/input/$name.txt" --frames "$work/frames" --frame-step 100 \
        --sound "$work/sound.wav" > "$work/log.txt" 2>&1 || true
    {
        grep -E "^instructions executed|^frames shown|^stopped at" "$work/log.txt" || true
        echo "frames $(cat "$work"/frames/*.png 2>/dev/null | sha256sum | cut -d' ' -f1)"
        echo "sound  $(sha256sum < "$work/sound.wav" | cut -d' ' -f1)"
    } > "$work.outcome"
    rm -rf "$work"
}
for run in $runs; do
    if [ -f "out/input/${run%%:*}.txt" ]; then one "$run" & fi
done
wait

failed=0
for run in $runs; do
    name=${run%%:*}
    [ -f "out/check/$name.outcome" ] || { echo "$name: no recording out/input/$name.txt"; continue; }
    if [ "$record" = yes ]; then
        mv "out/check/$name.outcome" "out/check/$name.good"
        echo "$name: recorded"
    elif [ ! -f "out/check/$name.good" ]; then
        echo "$name: nothing recorded as good yet (run with --record)"
        failed=1
    elif cmp -s "out/check/$name.outcome" "out/check/$name.good"; then
        echo "$name: the same"
    else
        echo "$name: DIFFERENT"
        diff "out/check/$name.good" "out/check/$name.outcome" | sed 's/^/    /' || true
        failed=1
    fi
done
exit $failed
