#!/bin/sh
# Render the spoken mode announcements into src/harmonics/prompts/modeN.ulaw (16 kHz G.711 mu-law; the patch
# embeds these files) with the shared pedalkit tool. The results are committed, so building doesn't need a Mac;
# rerun this only to change the wording or the voice (VOICE=..., RATE=...).
set -eu
cd "$(dirname "$0")"
exec src/pedalkit/tools/make-prompts.sh src/harmonics/prompts \
    "mode1=Mode one. Regular guitar." \
    "mode2=Mode two. Harmonics only." \
    "mode3=Mode three. Low harmonics." \
    "mode4=Mode four. High harmonics."
