# harmonics

A [Polyend Endless](https://polyend.com/endless/) patch that plays your guitar in four modes, stepped through on the
footswitch, with a computer voice announcing each mode as you switch. It's written in `no_std` Rust against the patch
ABI of Polyend's [FxPatchSDK](https://github.com/polyend/FxPatchSDK) (ABI 0x000B), so no ARM GCC is needed.

| Mode | What you hear | Announcement |
|---|---|---|
| 1 | **Regular guitar**: the guitar as it is | "Mode one. Regular guitar." |
| 2 | **Harmonics only**: everything except each note's fundamental | "Mode two. Harmonics only." |
| 3 | **Low harmonics**: only the 2nd, 3rd and 4th harmonics (octave, twelfth, double octave): hollow, organ-like | "Mode three. Low harmonics." |
| 4 | **High harmonics**: only the 5th harmonic and up: glassy and whistling | "Mode four. High harmonics." |

Everything then goes through a clean amp with gentle compression, a small cab sim and a small room. Input comes from
the left jack. The modes are level-matched, so switching doesn't jump in volume.

**How the harmonics are found:** the patch tracks the pitch of what you play (YIN on a 4:1 decimated copy of the input,
70 Hz to 1.5 kHz, updated every 5.3 ms) and filters around multiples of it:

- Mode 2: a notch on the fundamental plus a 4th-order high-pass at 1.5× the fundamental.
- Mode 3: resonators (Q 10) on harmonics 2, 3 and 4.
- Mode 4: notches on harmonics 3 and 4 plus a 4th-order high-pass at 4.5× the fundamental.

All four modes run all the time, so a switch is a click-free crossfade. **Modes 2 to 4 work on single notes.** With
chords, the tracker locks onto one pitch and the filters follow that note's harmonics, so the result is less exact.

**The voice:** the pedal can't synthesize speech, so the announcements were rendered with macOS `say` (Samantha) by
`make-prompts.sh` and are embedded in the patch as 16 kHz µ-law (about 120 KB). The guitar ducks 6 dB under them, and
the Voice knob sets their level (fully CCW silences them). To change the wording or the voice, edit
`make-prompts.sh` and run it on a Mac (`VOICE=Fred ./make-prompts.sh`), then rebuild.

| Control | Function |
|---|---|
| Left knob | **Blend**: dry guitar mixed back under modes 2–4. CCW is harmonics only |
| Mid knob | **Voice**: announcement volume. CCW is silent |
| Right knob | **Level**: output, unity at 3 o'clock |
| Footswitch press | **Next mode**: 1 → 2 → 3 → 4 → 1, announced |
| Footswitch hold | **Back to mode 1**, announced |

| LED | Mode |
|---|---|
| dim white | 1, regular |
| light blue | 2, harmonics only |
| dark cobalt | 3, low harmonics |
| light yellow | 4, high harmonics |

Red means the firmware didn't provide the working buffer.

## Build

```sh
rustup target add thumbv7em-none-eabihf --toolchain nightly
rustup component add llvm-tools --toolchain nightly
cargo build --release        # ELF for the pedal (default target is thumbv7em-none-eabihf)
./build-endl.sh              # build + make target/endless/harmonics.endl, check f64 use and size
./build-endl.sh --deploy     # ...and copy to /Volumes/ENDLESS
./make-prompts.sh            # (Mac only, optional) re-render the announcements into prompts/
```

## Audition on your computer

```sh
cargo sim                    # -> target/render/harmonics_demo.wav
```

This renders the same original A-minor phrase at 90 bpm in each mode on a modelled electric guitar (`src/guitar.rs`):
an eighth-note line, a held A3, then an Am chord. It steps through the modes with the footswitch (press, press, press,
then hold back to mode 1), so you hear each announcement. Then it prints each section's level, plus a table for the
held A3: the tracked pitch, the gain at each of its first 8 harmonics (output over input, so the note's own spectrum
cancels out) and each mode's phrase level.
