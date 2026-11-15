//! harmonics: a Polyend Endless patch that plays the guitar in four modes,
//! stepped through on the footswitch, with a spoken announcement of each:
//!   1  Regular guitar   the guitar as it is
//!   2  Harmonics only   everything but the fundamental: each note's overtones
//!   3  Low harmonics    only the 2nd, 3rd and 4th harmonics (the octave, the
//!                       twelfth and the double octave: organ-like, hollow)
//!   4  High harmonics   only the 5th harmonic and up (glassy, whistling)
//! then a clean amp with gentle compression, a cab sim and a small room.
//!
//! The harmonics are found by tracking the pitch of what's played (YIN on a
//! 4:1 decimated copy of the input, 70 Hz to 1.5 kHz, every 5.3 ms) and
//! filtering around multiples of it, so modes 2 to 4 follow single notes;
//! with chords they follow whichever pitch the tracker locks onto. All four
//! modes run all the time, so switching is a click-free crossfade.
//!
//! The announcements ("Mode two. Harmonics only.") were rendered with macOS
//! `say` by `make-prompts.sh` and are embedded as 16 kHz mu-law (the pedal
//! can't synthesise speech itself). The guitar ducks a little under them.
//!
//! Controls
//!   Left knob   Blend   (dry guitar mixed back into modes 2-4; CCW = harmonics only)
//!   Mid knob    Voice   (announcement volume; CCW = silent)
//!   Right knob  Level   (output; unity at 3 o'clock)
//!   Footswitch press  next mode (1 -> 2 -> 3 -> 4 -> 1), announced
//!   Footswitch hold   straight back to mode 1, announced
//!   LED  mode 1 dim white | 2 light blue | 3 dark cobalt | 4 light yellow
//!
//! Host builds (`cargo sim`) render a demo through the same code; see `host`.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

/// Reusable blocks (effects, DSP, the Endless ABI); see its docs.
mod pedalkit;

/// The pedal itself.
mod harmonics;

endless_patch!(kit = crate::pedalkit, pedal = crate::harmonics::Patch);

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
fn main() {
    host::main();
}
