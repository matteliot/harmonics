# ai.md — Project Requirements Log

This file records every requirement given for the **harmonics** project, so the whole project can be recreated from scratch by handing this file to an AI assistant as the prompt.

It is kept up to date as new requirements are given. Requirements are listed in the order they were given; later entries override earlier ones where they conflict (and the earlier entry is marked as superseded).

---

## Meta / Process Requirements

1. Maintain this `ai.md` file at the repo root. Its purpose is to keep a list of all requirements so the entire project can be recreated from this file alone. Update it as work progresses, whenever a new requirement is given. Keep lines unwrapped (wrap only around 200 characters at a natural break).

## Project Requirements

2. **In the existing `harmonics` directory, build a Polyend Endless patch the same way as the sibling projects (`../mirrorball` etc.) in which the guitar plays in four modes: 1 regular guitar, 2 only the harmonics, 3 only the low harmonics, 4 only the higher harmonics. A computer-generated voice says which mode has just been switched into. The modes are switched on the footswitch.** Pushed to `master` with the commit message `ai`.

### Implementation decisions (needed to recreate requirement 2)

- **Toolchain and packaging (copied from `../mirrorball`).** `no_std` Rust, edition 2024, nightly, `thumbv7em-none-eabihf`. Copied: `src/abi.rs`, `src/dsp.rs`, `src/guitar.rs` (host-only guitar model), `patch.ld`, `build.rs`, `.cargo/config.toml` (`-fp64`, `cargo sim` alias), `rust-toolchain.toml`, `.gitignore`. The reverb (`Comb`, `Allpass` with runtime lengths starting at 0, `Reverb`), generic `Global<T>`, `Bufs` in `.bss` zeroed in place in `patch_init` (`core::ptr::write_bytes`), entry points and header come from mirrorball's `main.rs`. The working buffer is unused.
- **`build-endl.sh`** makes `target/endless/harmonics.endl` and checks the 512 KB limit and f64 use. The f64 check now only looks at instructions inside function symbols (ranges from `llvm-objdump -t`, `F` entries): `.patch_image` also holds `.rodata`, and the embedded prompts were disassembled as code, giving a false hit (`vcvtm.s32.f64`, an ARMv8 instruction the M7 doesn't have, inside prompt bytes attributed to `memcpy`). Verified that the filter still reports a hit inside a function range. Note: with `-fp64`, f64 arithmetic in Rust becomes soft-float calls, not FPU instructions.
- **Interpretation:** "harmonics" means each played note's overtones, found by pitch tracking and filtering around multiples of the tracked fundamental. This works for single notes; with chords the tracker locks onto one pitch.
- **Input:** left jack, HP 70 Hz.
- **Pitch tracker (YIN):** 4:1 decimation (anti-alias: two biquad LPs at 2 kHz, Q 0.541 and 1.307) into a 1024-sample ring in `Bufs`; every 64 decimated samples (5.3 ms), once 490 are in, copy the newest 490 (window 320 + max lag 170) to `Bufs.win`, skip if RMS < 0.002, difference function for lags 1..=170 and cumulative-mean normalisation (`Bufs.d`), take the first lag ≥ 8 under 0.15 (walked down to its local minimum), else the global minimum if under 0.35, else keep the old pitch; parabolic interpolation; f0 = 12000 / lag. The pitch used by the filters glides to it (one-pole, 8 ms) and the filters are retuned every 32 samples (all frequencies capped at 16 kHz).
- **Modes** (all run all the time; 15 ms crossfade on a switch; per-mode makeup 0 / +1 / +7.3 / +2.3 dB):
  1. The input.
  2. `x − BP(f0, Q1)` (a notch), then 4th-order Butterworth HP at 1.5·f0 (biquads Q 0.541, 1.307).
  3. Sum of band-passes (0 dB peak) at 2·f0, 3·f0, 4·f0, Q 10 (Q 6 left h5 at only −9 dB).
  4. `x − BP(4·f0, Q3)`, then `− BP(3·f0, Q3)` of that, then 4th-order Butterworth HP at 4.5·f0.
- **Blend knob:** adds `Blend × (1 − mode-1 crossfade gain) × x`, the dry guitar under modes 2–4.
- **Clean amp:** HP 80 Hz → peak compressor (`target = 0.5/|1.4x|` above 0.5; attack one-pole 5 ms, release 50 ms) → `tanh(1.4x·comp)·0.7` → peak 250 Hz Q0.8 −2 dB → peak 3 kHz Q0.8 +2.5 dB → +16 dB. Cab: HP 70 Q0.7, peak 150 Hz +1.5 dB, peak 2.5 kHz Q1.2 +2 dB, LP 9 kHz Q0.6. Room: the shared Freeverb with feedback 0.82, damping 0.45, 20 ms pre-delay (960 samples), wet 0.3.
- **Announcements:** `make-prompts.sh` (Mac only; results committed in `prompts/`) renders "Mode one. Regular guitar.", "Mode two. Harmonics only.", "Mode three. Low harmonics.", "Mode four. High harmonics." with `say -v Samantha` (override with `VOICE=`) at 16 kHz 16-bit, trims silence (threshold 300, keeping 20 ms before and 50 ms after), normalises to −1 dBFS peak and writes raw G.711 µ-law (`prompts/modeN.ulaw`, ~1.9 s, ~30 KB each). The patch embeds them with `include_bytes!` and decodes µ-law per sample, linearly interpolated from 16 kHz to 48 kHz (`prompt_pos += 1/3`), then a 7 kHz one-pole LP. A mode switch (re)starts that mode's prompt. Level `Voice² × 1.0`; the guitar ducks by `0.5 × duck × min(1, voice gain)` (duck one-pole 30 ms). The prompt is added after the guitar's output gain, before the output limiter: `limit((dry + 0.3·wet) × out × duck + voice)`. Verified: the patch's decoder output correlates 0.9999 with a fresh `say` render.
- **Output:** `Level²` (unity at 0.75) × 0.36, `tanh × 0.98`. A played phrase sits around −22.6 dB RMS in every mode (measured within 0.4 dB of each other).
- **Controls:** Blend (default 0), Voice (default 0.6), Level (default 0.75). Press: next mode (1→2→3→4→1), announced. Hold: back to mode 1, announced. Starts in mode 1 (silently). LED: dim white / light blue / dark cobalt / light yellow for modes 1–4; red if the working buffer is missing.
- **Host audition:** `cargo sim` plays the same original phrase at 90 bpm in each mode, input ×0.6, ±6 ms slop: eighths A2 C3 E3 A3 G3 E3 D3 E3 (vel 0.8), a held A3 (vel 0.85, T60 4 s, 4 beats), an Am chord (A2 E3 A3 C4 E4, 15 ms strum, 4 beats), a stop and 0.4 s. Mode 1, then press + 2.2 s (for the announcement) + phrase for modes 2, 3, 4, then hold (back to 1) + 2.2 s + the Am chord + 2.5 s tail. Output `target/render/harmonics_demo.wav`. Prints per-section peak/RMS/side/correlation, then for each mode's held A3: the tracked pitch range over 0.4–1.4 s into the note, the gain at harmonics 1–8 (output over input, Hann-windowed DFT, so the note's own spectrum cancels out; normalising to the strongest output harmonic had wrongly suggested a 3rd-harmonic leak in mode 4) relative to the best-passed one, and the phrase's RMS without the announcement. Fails on NaN or full-scale output. Measured: mode 2 h1 −66 dB, rest within 5 dB; mode 3 h1 −23, h2–h4 kept, h5 −13, h6+ ≤ −15; mode 4 h2 −31, h3 −69, h4 −64, h5 −7, h6+ kept.

3. **Ship the built patch in a `release/` directory at the repo root** (`release/harmonics.endl`, a copy of `target/endless/harmonics.endl` from `./build-endl.sh`), committed to git, so a user can download it from GitHub and copy it straight to the pedal without building. README has an Install section saying so. Rebuild and recopy it whenever the patch changes.
