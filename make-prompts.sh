#!/bin/sh
# Render the spoken mode announcements with macOS `say` and store them as raw 8-bit G.711 mu-law at 16 kHz in
# prompts/modeN.ulaw (the patch embeds these files). The results are committed, so building doesn't need a Mac;
# rerun this only to change the wording or the voice.
set -eu
cd "$(dirname "$0")"
VOICE="${VOICE:-Samantha}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
i=1
for text in "Mode one. Regular guitar." "Mode two. Harmonics only." "Mode three. Low harmonics." "Mode four. High harmonics."; do
    say -v "$VOICE" -o "$TMP/p.wav" --file-format=WAVE --data-format=LEI16@16000 "$text"
    python3 -I - "$TMP/p.wav" "prompts/mode$i.ulaw" <<'PY'
import struct, sys
data = open(sys.argv[1], 'rb').read()
# Find the 'data' chunk of the WAVE file.
p = 12
while data[p:p+4] != b'data':
    p += 8 + struct.unpack('<I', data[p+4:p+8])[0]
n = struct.unpack('<I', data[p+4:p+8])[0] // 2
s = list(struct.unpack('<%dh' % n, data[p+8:p+8+2*n]))
# Trim leading/trailing silence (keep 20 ms of lead-in), normalise to -1 dBFS peak.
th = 300
a = next(i for i, v in enumerate(s) if abs(v) > th)
b = len(s) - next(i for i, v in enumerate(reversed(s)) if abs(v) > th)
s = s[max(0, a - 320):min(len(s), b + 800)]
g = 0.89 * 32767 / max(abs(v) for v in s)
def ulaw(v):
    v = int(max(-32635, min(32635, round(v * g))))
    sign = 0x80 if v < 0 else 0
    v = abs(v) + 0x84
    exp = 7
    while exp > 0 and not (v & (0x4000 >> (7 - exp))):
        exp -= 1
    mant = (v >> (exp + 3)) & 0x0F
    return ~(sign | (exp << 4) | mant) & 0xFF
open(sys.argv[2], 'wb').write(bytes(ulaw(v) for v in s))
print('%s: %.2f s, %d bytes' % (sys.argv[2], len(s) / 16000, len(s)))
PY
    i=$((i + 1))
done
