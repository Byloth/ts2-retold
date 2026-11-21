#!/bin/sh
# Makes a game directory as a full installation lays it out, from the files
# of an installation disc (ADR 0003). The disc keeps the part meant for the
# hard disk and the part left on the CD apart; a full installation puts them
# side by side. Nothing is copied: the entries are links, and both
# directories are git-ignored.
#
#     tools/game-dir-from-disc.sh && export TS2_GAME_DIR="$PWD/ref/game"
#
# With two arguments, another disc and its directory (the Italian disc,
# ADR 0016):
#
#     tools/game-dir-from-disc.sh ref/disc-it ref/game-it
#     export TS2_ITALIAN_DIR="$PWD/ref/game-it"
set -eu
cd "$(dirname "$0")/.."
disc_root=${1:-ref/disc}
game=${2:-ref/game}
# Discs differ in the case of their directory names.
find_in() { find "$1" -maxdepth 1 -iname "$2" | head -n 1; }
setup=$(find_in "$disc_root" setup)
disc=$(find_in "${setup:-/nonexistent}" toy2)
hd=$(find_in "${disc:-/nonexistent}" hd)
cd_part=$(find_in "${disc:-/nonexistent}" cd)
exe=$(find_in "${hd:-/nonexistent}" toy2.exe)
[ -n "$exe" ] || { echo "no disc files under $disc_root" >&2; exit 1; }
mkdir -p "$game"
link() { [ -n "$1" ] && ln -sfn "$(realpath "$1")" "$game/$2"; }
link "$exe" toy2.exe
link "$(find_in "$hd" data)" data
link "$(find_in "$cd_part" rtlibs)" rtlibs
link "$(find_in "$cd_part" audio)" audio
link "$(find_in "$cd_part" validate.tta)" validate.tta
echo "$game is ready"
