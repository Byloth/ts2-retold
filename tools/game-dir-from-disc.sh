#!/bin/sh
# Makes ref/game, a game directory as a full installation lays it out, from
# the files of the installation disc in ref/disc (ADR 0003). The disc keeps
# the part meant for the hard disk and the part left on the CD apart; a full
# installation puts them side by side. Nothing is copied: the entries are
# links, and both directories are git-ignored.
#
#     tools/game-dir-from-disc.sh && export TS2_GAME_DIR="$PWD/ref/game"
set -eu
cd "$(dirname "$0")/.."
disc=ref/disc/Setup/Toy2
[ -f "$disc/HD/toy2.exe" ] || { echo "no disc files under $disc" >&2; exit 1; }
mkdir -p ref/game
ln -sfn "../disc/Setup/Toy2/HD/toy2.exe" ref/game/toy2.exe
ln -sfn "../disc/Setup/Toy2/HD/data" ref/game/data
ln -sfn "../disc/Setup/Toy2/CD/rtlibs" ref/game/rtlibs
ln -sfn "../disc/Setup/Toy2/CD/audio" ref/game/audio
ln -sfn "../disc/Setup/Toy2/CD/validate.tta" ref/game/validate.tta
echo "ref/game is ready"
