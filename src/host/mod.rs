//! Host-only demo: `cargo sim [OUT.wav]` plays the same phrase in each mode
//! (see `score`) through the patch (see `pedalkit::host`), then reports, for
//! each mode's held A3, the tracked pitch, which harmonics it keeps and the
//! phrase's level.

mod score;

use crate::entry::{ENTRY, pedal};
use crate::pedalkit::host::{self, BLOCK, Options, SRU};
use score::HELD;

pub fn main() {
    let out = host::out_path("target/render/harmonics_demo.wav");
    let (events, total, marks) = score::score();
    let o = Options { first: "1 regular", input_trim: 0.6, working: 16 };
    let mut pitch = vec![];
    let r = host::render_with(&ENTRY, events, total, o, &mut || pitch.push(pedal().pitch()));
    let (lo, inp) = (&r.l, &r.input);
    let ok = host::stats::report(&r.l, &r.r, r.sections.clone());

    // Which harmonics each mode keeps: for the held A3's first 8 harmonics,
    // 0.4 to 1.4 s into the note, the output over the input (so each note's
    // own spectrum, which varies with pick position and attack, cancels
    // out), in dB relative to the best-passed harmonic.
    println!("\nHeld A3 (220 Hz): tracked pitch and the gain at each harmonic, output over input (dB re the best-passed)");
    println!("{:<18} {:>13} {}", "mode", "pitch Hz", (1..=8).map(|k| format!("{:>6}", format!("h{k}"))).collect::<String>() + "  phrase dB");
    for (start, name) in &marks {
        let a = ((start + 0.4) * SRU as f64) as usize;
        let mag = |w: &[f32], k: usize| {
            let f = HELD as f64 * k as f64;
            let (mut c, mut s) = (0.0f64, 0.0f64);
            for (i, v) in w.iter().enumerate() {
                let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / w.len() as f64).cos();
                let ph = std::f64::consts::TAU * f * i as f64 / SRU as f64;
                c += *v as f64 * win * ph.cos();
                s += *v as f64 * win * ph.sin();
            }
            (c * c + s * s).sqrt()
        };
        let mags: Vec<f64> = (1..=8).map(|k| mag(&lo[a..a + SRU], k) / mag(&inp[a..a + SRU], k).max(1e-12)).collect();
        let top = mags.iter().cloned().fold(1e-12, f64::max);
        let (p0, p1) = (((start + 0.4) * SRU as f64) as usize / BLOCK, ((start + 1.4) * SRU as f64) as usize / BLOCK);
        let lo_p = pitch[p0..p1].iter().cloned().fold(f32::MAX, f32::min);
        let hi_p = pitch[p0..p1].iter().cloned().fold(0.0, f32::max);
        let tracked = format!("{lo_p:.1}-{hi_p:.1}");
        let row: String = mags.iter().map(|m| format!("{:>6.0}", 20.0 * (m / top).log10())).collect();
        // Level of the whole phrase (line, held note, chord), without the announcement.
        let w = &lo[((start - 8.0 * 60.0 / 180.0) * SRU as f64) as usize..((start + 8.0 * 60.0 / 90.0) * SRU as f64) as usize];
        let level = 10.0 * (w.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / w.len() as f64).log10();
        println!("{name:<18} {tracked:>13} {row} {level:>8.1}");
    }
    println!("final LED colour index: {}", unsafe { (ENTRY.get_state_idx)(&r.env) });
    host::wav::write_wav(&out, &r.l, &r.r);
    println!("wrote {out} ({:.1} s)", r.l.len() as f64 / SRU as f64);
    if !ok {
        eprintln!("error: NaN or full-scale output");
        std::process::exit(1);
    }
}
