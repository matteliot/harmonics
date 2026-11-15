//! The `cargo sim` demo: its score (the rendering itself is `pedalkit::host`).
//!
//! Plays the same original phrase in A minor at 90 bpm in each mode on a
//! modelled electric guitar (see `guitar`): an eighth-note line, a held A3,
//! then an Am chord. It drives the patch's entry points the way the firmware
//! does (128-sample blocks, footswitch events at set times), stepping
//! through the modes with the footswitch (press, press, press, then hold
//! back to mode 1), so each announcement is heard, and writes a stereo float
//! WAV. Prints per-section level stats, the tracked pitch and the harmonic
//! levels of the held A3 in each mode (which harmonics each mode keeps),
//! and fails on NaN or clipping.

use crate::pedalkit::guitar::Note;
use crate::pedalkit::host::{Ev, Score};

const LINE: [f32; 8] = [110.0, 130.81, 164.81, 220.0, 196.0, 164.81, 146.83, 164.81];
pub const HELD: f32 = 220.0;
const AM: [f32; 5] = [110.0, 164.81, 220.0, 261.63, 329.63];

enum Local {
    /// Play these notes, strummed this many seconds apart; `ring` lets what's already sounding carry on.
    Play(Vec<Note>, f32, bool),
    Press,
    Hold,
}

/// Where each mode's held A3 starts (s).
pub type Marks = Vec<(f64, &'static str)>;

fn local_score() -> (Vec<(f64, Local, &'static str)>, f64, Marks) {
    let mut ev = vec![];
    let mut marks = vec![];
    let q = 60.0 / 90.0;
    let e = q / 2.0;
    let mut t = 0.1;
    let mut seed: u32 = 0x1357_9bdf;
    let mut slop = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) * 0.006
    };
    let mut phrase = |ev: &mut Vec<(f64, Local, &'static str)>, t: &mut f64, marks: &mut Marks, name: &'static str| {
        for f in LINE {
            ev.push((*t + slop(), Local::Play(vec![Note::new(f).vel(0.8)], 0.0, false), ""));
            *t += e;
        }
        marks.push((*t, name));
        ev.push((*t, Local::Play(vec![Note::new(HELD).vel(0.85).t60(4.0)], 0.0, false), ""));
        *t += 4.0 * q;
        ev.push((*t, Local::Play(AM.iter().map(|f| Note::new(*f).vel(0.75)).collect(), 0.015, false), ""));
        *t += 4.0 * q;
        ev.push((*t, Local::Play(vec![], 0.0, false), ""));
        *t += 0.4;
    };

    phrase(&mut ev, &mut t, &mut marks, "1 regular");
    for (name, label) in [
        ("2 harmonics only", "2 harmonics only"),
        ("3 low harmonics", "3 low harmonics"),
        ("4 high harmonics", "4 high harmonics"),
    ] {
        ev.push((t, Local::Press, label));
        t += 2.2; // let the announcement finish
        phrase(&mut ev, &mut t, &mut marks, name);
    }
    // Hold: straight back to mode 1, then a last chord rung out.
    ev.push((t, Local::Hold, "hold: back to 1"));
    t += 2.2;
    ev.push((t, Local::Play(AM.iter().map(|f| Note::new(*f).vel(0.75)).collect(), 0.015, false), ""));
    t += 4.0 * q;
    (ev, t + 2.5, marks)
}

/// The score as pedalkit events, and where each mode's phrase starts.
pub fn score() -> (Score, f64, Marks) {
    let (ev, total, marks) = local_score();
    (ev.into_iter().map(|(t, e, l)| (t, e.into(), l)).collect(), total, marks)
}

impl From<Local> for Ev {
    fn from(e: Local) -> Ev {
        match e {
            Local::Play(notes, strum, ring) => Ev::Play(notes, strum, ring),
            Local::Press => Ev::Press,
            Local::Hold => Ev::Hold,
        }
    }
}
