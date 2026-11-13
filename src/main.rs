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

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod abi;
#[allow(dead_code)]
mod dsp;

use core::cell::UnsafeCell;

use abi::*;
use dsp::*;

// ---------------------------------------------------------------- voicing

/// Pitch tracker: decimation factor, YIN window and lag range (decimated
/// samples: 1.5 kHz down to ~70 Hz), how often it runs (decimated samples),
/// the YIN threshold, the worst dip still accepted when none is under it,
/// the input level it needs, and how fast the tracked pitch glides (s).
const DEC: u32 = 4;
const DSR: f32 = SR / DEC as f32;
const YIN_W: usize = 320;
const TAU_MIN: usize = 8;
const TAU_MAX: usize = 170;
const YIN_HOP: u32 = 64;
const YIN_THRESH: f32 = 0.15;
const YIN_ACCEPT: f32 = 0.35;
const PITCH_MIN_RMS: f32 = 0.002;
const GLIDE_S: f32 = 0.008;

/// Mode 2: notch on the fundamental (Q) and a 4th-order high-pass at this
/// multiple of it.
const FUND_Q: f32 = 1.0;
const M2_HP: f32 = 1.5;
/// Mode 3: resonators (Q) on harmonics 2..=4.
const LOW_Q: f32 = 10.0;
/// Mode 4: notches on the 3rd and 4th harmonics (Q) and a 4th-order
/// high-pass at this multiple of the fundamental.
const NOTCH4_Q: f32 = 3.0;
const M4_HP: f32 = 4.5;
/// Filters stay below this (Hz).
const F_MAX: f32 = 16_000.0;
/// Per-mode makeup (dB), so the modes play at about the same level, and the
/// crossfade time on a switch (s).
const MAKEUP_DB: [f32; 4] = [0.0, 1.0, 7.3, 2.3];
const XFADE_S: f32 = 0.015;

/// Clean amp: drive and makeup.
const CLEAN_DRIVE: f32 = 1.4;
const CLEAN_MAKEUP_DB: f32 = 16.0;

/// Small room: feedback, damping, pre-delay (samples, 20 ms), wet level.
const ROOM: f32 = 0.82;
const DAMP: f32 = 0.45;
const PREDELAY: f32 = 960.0;
const WET: f32 = 0.3;

/// Announcements: sample rate of the embedded prompts, level at full Voice
/// (peaks are at -1 dBFS in the files), how far the guitar ducks under them
/// and how fast (s).
const PROMPT_SR: f32 = 16_000.0;
const VOICE_MAX: f32 = 1.0;
const VOICE_DEFAULT: f32 = 0.6;
const DUCK: f32 = 0.5;
const DUCK_S: f32 = 0.03;

/// Output trim: unity Level puts a played phrase around -22 dB RMS (single
/// notes and clean chords are spiky, so this leaves their peaks headroom).
const OUT_TRIM: f32 = 0.36;

/// The spoken mode announcements, 16 kHz G.711 mu-law (see make-prompts.sh).
static PROMPTS: [&[u8]; 4] = [
    include_bytes!("../prompts/mode1.ulaw"),
    include_bytes!("../prompts/mode2.ulaw"),
    include_bytes!("../prompts/mode3.ulaw"),
    include_bytes!("../prompts/mode4.ulaw"),
];

/// G.711 mu-law byte to a sample in -1..1.
#[inline]
fn ulaw(b: u8) -> f32 {
    let u = !b;
    let exp = (u >> 4) & 7;
    let mag = ((((u & 0x0F) as i32) << 3) + 0x84) << exp;
    let s = (mag - 0x84) as f32 * (1.0 / 32768.0);
    if u & 0x80 != 0 { -s } else { s }
}

// ---------------------------------------------------------------- reverb

/// Freeverb-style lowpass-feedback comb with a runtime length.
struct Comb<const N: usize> {
    buf: [f32; N],
    len: usize,
    i: usize,
    filt: f32,
}

impl<const N: usize> Comb<N> {
    const NEW: Self = Self { buf: [0.0; N], len: 0, i: 0, filt: 0.0 };

    #[inline]
    fn process(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let o = self.buf[self.i];
        self.filt = o + (self.filt - o) * damp;
        self.buf[self.i] = x + self.filt * fb;
        self.i += 1;
        if self.i >= self.len {
            self.i = 0;
        }
        o
    }
}

/// Schroeder allpass (feedback 0.5) with a runtime length.
struct Allpass<const N: usize> {
    buf: [f32; N],
    len: usize,
    i: usize,
}

impl<const N: usize> Allpass<N> {
    const NEW: Self = Self { buf: [0.0; N], len: 0, i: 0 };

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let o = self.buf[self.i];
        self.buf[self.i] = x + o * 0.5;
        self.i += 1;
        if self.i >= self.len {
            self.i = 0;
        }
        o - x
    }
}

/// Freeverb tunings (44.1 kHz) scaled to 48 kHz; the right channel is spread.
const COMB_LEN: [usize; 8] = [1214, 1293, 1389, 1475, 1547, 1622, 1694, 1759];
const AP_LEN: [usize; 4] = [605, 480, 371, 245];
const SPREAD: usize = 25;

struct Reverb {
    pre: Delay<2048>,
    cl: [Comb<2048>; 8],
    cr: [Comb<2048>; 8],
    al: [Allpass<1024>; 4],
    ar: [Allpass<1024>; 4],
}

impl Reverb {
    const NEW: Self = Self {
        pre: Delay::NEW,
        cl: [Comb::NEW; 8],
        cr: [Comb::NEW; 8],
        al: [Allpass::NEW; 4],
        ar: [Allpass::NEW; 4],
    };

    fn init(&mut self) {
        for k in 0..8 {
            self.cl[k].len = COMB_LEN[k];
            self.cr[k].len = COMB_LEN[k] + SPREAD;
        }
        for k in 0..4 {
            self.al[k].len = AP_LEN[k];
            self.ar[k].len = AP_LEN[k] + SPREAD;
        }
    }

    /// Wet only, stereo, at roughly the input's level.
    #[inline]
    fn process(&mut self, x: f32, room: f32) -> (f32, f32) {
        self.pre.push(x);
        let x = self.pre.read(PREDELAY) * 0.015;
        let mut l = 0.0;
        let mut r = 0.0;
        for c in self.cl.iter_mut() {
            l += c.process(x, room, DAMP);
        }
        for c in self.cr.iter_mut() {
            r += c.process(x, room, DAMP);
        }
        for a in self.al.iter_mut() {
            l = a.process(l);
        }
        for a in self.ar.iter_mut() {
            r = a.process(r);
        }
        (l * 3.0, r * 3.0)
    }
}

// ---------------------------------------------------------------- state

/// The delay lines and the pitch tracker's working arrays. They are all
/// zero at rest, so they live in .bss and add nothing to the patch file;
/// `patch_init` zeroes them before use.
struct Bufs {
    rev: Reverb,
    /// Decimated input ring for the pitch tracker.
    dec: [f32; 1024],
    /// The newest YIN_W + TAU_MAX decimated samples, oldest first.
    win: [f32; 512],
    /// YIN difference function, then its cumulative-mean-normalised form.
    d: [f32; TAU_MAX + 2],
}

impl Bufs {
    const NEW: Self = Self { rev: Reverb::NEW, dec: [0.0; 1024], win: [0.0; 512], d: [0.0; TAU_MAX + 2] };
}

struct Hm {
    ready: bool,
    error: bool,
    // knobs, 0..1
    blend: f32,
    voice: f32,
    level: f32,
    mode: usize,
    // smoothed values and targets
    out_g: f32,
    out_t: f32,
    voice_g: f32,
    voice_t: f32,
    blend_s: f32,
    gains: [f32; 4],
    makeup: [f32; 4],
    // pitch tracker
    dec_lp: [Biquad; 2],
    dec_phase: u32,
    dec_w: usize,
    dec_n: u32,
    hop: u32,
    f0: f32,
    f_s: f32,
    retune: u32,
    // mode filters
    hp_in: Biquad,
    m2_bp: Biquad,
    m2_hp: [Biquad; 2],
    m3_bp: [Biquad; 3],
    m4_bp: Biquad,
    m4_bp3: Biquad,
    m4_hp: [Biquad; 2],
    // clean amp and cab
    clean_hp: Biquad,
    clean_low: Biquad,
    clean_sparkle: Biquad,
    comp: f32,
    cab: [Biquad; 4],
    // announcements
    prompt: usize,
    prompt_pos: f32,
    speaking: bool,
    voice_lp: OnePole,
    duck: f32,
    // coefficients
    c_param: f32,
    c_glide: f32,
    c_xfade: f32,
    c_comp: f32,
    c_comp_att: f32,
    c_duck: f32,
}

impl Hm {
    const NEW: Self = Self {
        ready: false,
        error: false,
        blend: 0.0,
        voice: VOICE_DEFAULT,
        level: 0.75,
        mode: 0,
        out_g: 0.0,
        out_t: 0.0,
        voice_g: 0.0,
        voice_t: 0.0,
        blend_s: 0.0,
        gains: [1.0, 0.0, 0.0, 0.0],
        makeup: [1.0; 4],
        dec_lp: [Biquad::NEW; 2],
        dec_phase: 0,
        dec_w: 0,
        dec_n: 0,
        hop: 0,
        f0: 220.0,
        f_s: 220.0,
        retune: 0,
        hp_in: Biquad::NEW,
        m2_bp: Biquad::NEW,
        m2_hp: [Biquad::NEW; 2],
        m3_bp: [Biquad::NEW; 3],
        m4_bp: Biquad::NEW,
        m4_bp3: Biquad::NEW,
        m4_hp: [Biquad::NEW; 2],
        clean_hp: Biquad::NEW,
        clean_low: Biquad::NEW,
        clean_sparkle: Biquad::NEW,
        comp: 1.0,
        cab: [Biquad::NEW; 4],
        prompt: 0,
        prompt_pos: 0.0,
        speaking: false,
        voice_lp: OnePole::NEW,
        duck: 0.0,
        c_param: 1.0,
        c_glide: 1.0,
        c_xfade: 1.0,
        c_comp: 1.0,
        c_comp_att: 1.0,
        c_duck: 1.0,
    };

    fn init(&mut self, b: &mut Bufs) {
        self.hp_in.highpass(70.0, 0.707);
        // Anti-alias for the 4:1 decimation (4th-order Butterworth, 2 kHz).
        self.dec_lp[0].lowpass(2000.0, 0.541);
        self.dec_lp[1].lowpass(2000.0, 1.307);
        self.tune();
        // A clean amp with a little sparkle, into a small open-back cab.
        self.clean_hp.highpass(80.0, 0.707);
        self.clean_low.peak(250.0, 0.8, -2.0);
        self.clean_sparkle.peak(3000.0, 0.8, 2.5);
        self.cab[0].highpass(70.0, 0.7);
        self.cab[1].peak(150.0, 1.0, 1.5);
        self.cab[2].peak(2500.0, 1.2, 2.0);
        self.cab[3].lowpass(9000.0, 0.6);
        self.voice_lp.set(7000.0);
        b.rev.init();
        for (m, d) in self.makeup.iter_mut().zip(MAKEUP_DB) {
            *m = db(d);
        }
        self.c_param = coef(0.02);
        self.c_glide = coef(GLIDE_S);
        self.c_xfade = coef(XFADE_S);
        self.c_comp = coef(0.05);
        // A 5 ms attack: a per-sample attack would ride the gain within each
        // cycle, and that modulation puts intermodulation products right on
        // other harmonics (h8 - h5 = h3), undoing the mode filters.
        self.c_comp_att = coef(0.005);
        self.c_duck = coef(DUCK_S);
        self.retarget();
        self.out_g = self.out_t;
        self.voice_g = self.voice_t;
        self.blend_s = self.blend;
        self.gains = [0.0; 4];
        self.gains[self.mode] = 1.0;
        self.ready = true;
    }

    fn retarget(&mut self) {
        // Square law: 0 at minimum, unity at 3 o'clock, +5 dB at maximum.
        let v = self.level / 0.75;
        self.out_t = v * v * OUT_TRIM;
        self.voice_t = self.voice * self.voice * VOICE_MAX;
    }

    /// Switch mode and announce it.
    fn set_mode(&mut self, mode: usize) {
        self.mode = mode % 4;
        self.prompt = self.mode;
        self.prompt_pos = 0.0;
        self.speaking = true;
    }

    /// Point the mode filters at the tracked pitch.
    fn tune(&mut self) {
        let f = self.f_s;
        let lim = |x: f32| x.min(F_MAX);
        self.m2_bp.bandpass(lim(f), FUND_Q);
        self.m2_hp[0].highpass(lim(M2_HP * f), 0.541);
        self.m2_hp[1].highpass(lim(M2_HP * f), 1.307);
        for (k, bp) in self.m3_bp.iter_mut().enumerate() {
            bp.bandpass(lim((k as f32 + 2.0) * f), LOW_Q);
        }
        self.m4_bp.bandpass(lim(4.0 * f), NOTCH4_Q);
        self.m4_bp3.bandpass(lim(3.0 * f), NOTCH4_Q);
        self.m4_hp[0].highpass(lim(M4_HP * f), 0.541);
        self.m4_hp[1].highpass(lim(M4_HP * f), 1.307);
    }

    /// YIN pitch estimate over the newest decimated samples; updates `f0`
    /// when the input is loud enough and clearly pitched.
    fn yin(&mut self, b: &mut Bufs) {
        let n = YIN_W + TAU_MAX;
        let start = self.dec_w.wrapping_sub(n - 1);
        let mut e = 0.0;
        for i in 0..n {
            let v = b.dec[(start + i) & 1023];
            b.win[i] = v;
            e += v * v;
        }
        if e < PITCH_MIN_RMS * PITCH_MIN_RMS * n as f32 {
            return;
        }
        let w = &b.win;
        let d = &mut b.d;
        // Difference function, then cumulative-mean normalisation.
        let mut sum = 0.0;
        d[0] = 1.0;
        for tau in 1..=TAU_MAX {
            let mut s = 0.0;
            for j in 0..YIN_W {
                let x = w[j] - w[j + tau];
                s += x * x;
            }
            sum += s;
            d[tau] = if sum > 0.0 { s * tau as f32 / sum } else { 1.0 };
        }
        // First dip under the threshold (then down to its bottom), else the
        // deepest dip if it's still convincing.
        let mut best = 0;
        let mut tau = TAU_MIN;
        while tau <= TAU_MAX {
            if d[tau] < YIN_THRESH {
                while tau < TAU_MAX && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                best = tau;
                break;
            }
            tau += 1;
        }
        if best == 0 {
            let mut m = TAU_MIN;
            for t in TAU_MIN..=TAU_MAX {
                if d[t] < d[m] {
                    m = t;
                }
            }
            if d[m] > YIN_ACCEPT {
                return;
            }
            best = m;
        }
        // Parabolic interpolation around the dip.
        let mut t = best as f32;
        if best > TAU_MIN && best < TAU_MAX {
            let (a, c0, c) = (d[best - 1], d[best], d[best + 1]);
            let den = a - 2.0 * c0 + c;
            if den > 0.0 {
                t += 0.5 * (a - c) / den;
            }
        }
        self.f0 = DSR / t;
    }

    /// Clean amp with gentle compression (mono).
    #[inline]
    fn clean(&mut self, x: f32) -> f32 {
        let x = self.clean_hp.process(x);
        let a = x.abs() * CLEAN_DRIVE;
        let target = if a > 0.5 { 0.5 / a } else { 1.0 };
        self.comp += (target - self.comp) * if target < self.comp { self.c_comp_att } else { self.c_comp };
        let y = tanh(x * CLEAN_DRIVE * self.comp) * 0.7;
        self.clean_sparkle.process(self.clean_low.process(y))
    }

    /// The current announcement's next sample (16 kHz, linearly
    /// interpolated up to 48 kHz), or 0 when it's done.
    #[inline]
    fn speak(&mut self) -> f32 {
        let mut v = 0.0;
        if self.speaking {
            let p = PROMPTS[self.prompt];
            let i = self.prompt_pos as usize;
            if i + 1 < p.len() {
                let fr = self.prompt_pos - i as f32;
                let (a, c) = (ulaw(p[i]), ulaw(p[i + 1]));
                v = a + (c - a) * fr;
                self.prompt_pos += PROMPT_SR / SR;
            } else {
                self.speaking = false;
            }
        }
        self.voice_lp.process(v)
    }

    #[inline]
    fn tick(&mut self, b: &mut Bufs, x: f32) -> (f32, f32) {
        self.out_g += (self.out_t - self.out_g) * self.c_param;
        self.voice_g += (self.voice_t - self.voice_g) * self.c_param;
        self.blend_s += (self.blend - self.blend_s) * self.c_param;

        let x = self.hp_in.process(x);

        // Pitch: decimate 4:1 into the ring, run YIN every YIN_HOP decimated
        // samples once the ring holds a full window.
        let dx = self.dec_lp[0].process(x);
        let dx = self.dec_lp[1].process(dx);
        self.dec_phase += 1;
        if self.dec_phase == DEC {
            self.dec_phase = 0;
            self.dec_w = (self.dec_w + 1) & 1023;
            b.dec[self.dec_w] = dx;
            self.dec_n = self.dec_n.saturating_add(1);
            self.hop += 1;
            if self.hop >= YIN_HOP && self.dec_n as usize > YIN_W + TAU_MAX {
                self.hop = 0;
                self.yin(b);
            }
        }
        self.f_s += (self.f0 - self.f_s) * self.c_glide;
        self.retune = self.retune.wrapping_add(1);
        if self.retune & 31 == 0 {
            self.tune();
        }

        // The four modes, all running, crossfaded.
        let m2 = x - self.m2_bp.process(x);
        let m2 = self.m2_hp[0].process(m2);
        let m2 = self.m2_hp[1].process(m2);
        let m3 = self.m3_bp.iter_mut().map(|bp| bp.process(x)).sum::<f32>();
        let m4 = x - self.m4_bp.process(x);
        let m4 = m4 - self.m4_bp3.process(m4);
        let m4 = self.m4_hp[0].process(m4);
        let m4 = self.m4_hp[1].process(m4);
        let modes = [x, m2, m3, m4];
        let mut y = 0.0;
        for k in 0..4 {
            let t = if k == self.mode { 1.0 } else { 0.0 };
            self.gains[k] += (t - self.gains[k]) * self.c_xfade;
            y += self.gains[k] * self.makeup[k] * modes[k];
        }
        // Blend the dry guitar back under the harmonics (not in mode 1).
        y += self.blend_s * (1.0 - self.gains[0]) * x;

        let mut y = self.clean(y) * db(CLEAN_MAKEUP_DB);
        for c in self.cab.iter_mut() {
            y = c.process(y);
        }
        let (wl, wr) = b.rev.process(y, ROOM);

        // The announcement goes on top, with the guitar ducked under it.
        let v = self.speak() * self.voice_g;
        let st = if self.speaking { 1.0 } else { 0.0 };
        self.duck += (st - self.duck) * self.c_duck;
        let g = self.out_g * (1.0 - DUCK * self.duck * (self.voice_g / VOICE_MAX).min(1.0));
        (limit((y + WET * wl) * g + v), limit((y + WET * wr) * g + v))
    }

    fn led(&self) -> Color {
        if self.error {
            return Color::Red;
        }
        match self.mode {
            0 => Color::DimWhite,
            1 => Color::LightBlue,
            2 => Color::DarkCobalt,
            _ => Color::LightYellow,
        }
    }
}

/// Output soft limiter, never reaching ±1.
#[inline]
fn limit(x: f32) -> f32 {
    tanh(x) * 0.98
}

/// All patch state. The firmware calls every entry point from the audio
/// thread (SDK: "no need to synchronize").
struct Global<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Global<T> {}
static STATE: Global<Hm> = Global(UnsafeCell::new(Hm::NEW));
static BUFS: Global<Bufs> = Global(UnsafeCell::new(Bufs::NEW));

#[allow(clippy::mut_from_ref)]
fn state() -> &'static mut Hm {
    // SAFETY: single-threaded access from the firmware's audio thread.
    unsafe { &mut *STATE.0.get() }
}

#[allow(clippy::mut_from_ref)]
fn bufs() -> &'static mut Bufs {
    // SAFETY: as `state`.
    unsafe { &mut *BUFS.0.get() }
}

/// Zeroes the delay lines and tracker arrays in place. The loader should already have cleared
/// .bss, but nothing promises it, and a NaN left in a feedback loop never
/// decays. (In place: a zeroed copy would be a large stack temporary.)
fn clear_bufs() {
    // SAFETY: `Bufs` is only f32 and usize fields, so all-zero bits are a
    // valid value; single-threaded as `state`.
    unsafe { core::ptr::write_bytes(BUFS.0.get(), 0, 1) }
}

// ---------------------------------------------------------------- entry points

const PARAM_BLEND: i32 = 0;
const PARAM_VOICE: i32 = 1;
const PARAM_LEVEL: i32 = 2;

unsafe extern "C" fn patch_init(env: *const PatchEnv) {
    if env.is_null() || unsafe { (*env).abi_version } != PATCH_ABI_VERSION as u32 {
        return;
    }
    clear_bufs();
    state().init(bufs());
}

unsafe extern "C" fn update_buffers(_: *const PatchEnv, left: *mut f32, right: *mut f32, samples: i32) -> f32 {
    let Ok(n) = usize::try_from(samples) else { return 0.0 };
    if left.is_null() || right.is_null() || n == 0 {
        return 0.0;
    }
    // SAFETY: the firmware passes two buffers of `samples` floats.
    let (l, r) = unsafe { (core::slice::from_raw_parts_mut(left, n), core::slice::from_raw_parts_mut(right, n)) };
    let (s, bf) = (state(), bufs());
    if !s.ready {
        return 0.0;
    }
    for (a, b) in l.iter_mut().zip(r.iter_mut()) {
        let (ol, or) = s.tick(bf, *a);
        *a = ol;
        *b = or;
    }
    0.0
}

unsafe extern "C" fn set_buffer(_: *const PatchEnv, buffer: *mut f32) {
    // All state is static; the working buffer isn't needed.
    if buffer.is_null() {
        state().error = true;
    }
}

unsafe extern "C" fn get_buffer_size(_: *const PatchEnv) -> i32 {
    WORKING_BUFFER_SIZE
}

unsafe extern "C" fn get_param_min(_: *const PatchEnv, _: i32) -> f32 {
    0.0
}

unsafe extern "C" fn get_param_max(_: *const PatchEnv, _: i32) -> f32 {
    1.0
}

unsafe extern "C" fn get_param_default(_: *const PatchEnv, idx: i32) -> f32 {
    match idx {
        PARAM_BLEND => 0.0,
        PARAM_VOICE => VOICE_DEFAULT,
        _ => 0.75,
    }
}

unsafe extern "C" fn is_param_enabled(_: *const PatchEnv, idx: i32, source_id: i32) -> u8 {
    // Knobs only (source 0), like the SDK wrapper.
    (source_id == 0 && (0..PARAM_COUNT).contains(&idx)) as u8
}

unsafe fn write_c_str(s: &str, out: *mut u8, len: usize) {
    if out.is_null() || len == 0 {
        return;
    }
    let n = s.len().min(len - 1);
    // SAFETY: the firmware provides `len` writable bytes at `out`.
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), out, n);
        *out.add(n) = 0;
    }
}

unsafe extern "C" fn get_param_name(_: *const PatchEnv, idx: i32, out: *mut u8, len: usize) {
    let name = match idx {
        PARAM_BLEND => "Blend",
        PARAM_VOICE => "Voice",
        PARAM_LEVEL => "Level",
        _ => "",
    };
    unsafe { write_c_str(name, out, len) }
}

unsafe extern "C" fn get_param_unit(_: *const PatchEnv, _: i32, out: *mut u8, len: usize) {
    unsafe { write_c_str("", out, len) }
}

unsafe extern "C" fn set_param(_: *const PatchEnv, idx: i32, value: f32) {
    let s = state();
    let v = value.clamp(0.0, 1.0);
    match idx {
        PARAM_BLEND => s.blend = v,
        PARAM_VOICE => s.voice = v,
        PARAM_LEVEL => s.level = v,
        _ => return,
    }
    if s.ready {
        s.retarget();
    }
}

unsafe extern "C" fn special_action(_: *const PatchEnv, idx: i32) {
    let s = state();
    match idx {
        ACTION_FOOTSWITCH_PRESS => s.set_mode(s.mode + 1),
        ACTION_FOOTSWITCH_HOLD => s.set_mode(0),
        _ => {}
    }
}

unsafe extern "C" fn get_state_idx(_: *const PatchEnv) -> i32 {
    state().led() as i32
}

// ---------------------------------------------------------------- header

#[cfg(target_os = "none")]
unsafe extern "C" {
    static __patch_bss_start: u8;
    static patch_image_size: u8;
    static patch_bss_size: u8;
}

#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".patch_header")]
#[used]
pub static patch_header: PatchHeader = PatchHeader {
    magic: PATCH_MAGIC,
    abi_version: PATCH_ABI_VERSION,
    flags: PATCH_FLAG_NONE,
    init: patch_init,
    agent_update_buffers: update_buffers,
    agent_set_buffer: set_buffer,
    agent_get_buffer_size: get_buffer_size,
    agent_get_param_min: get_param_min,
    agent_get_param_max: get_param_max,
    agent_get_param_default: get_param_default,
    agent_is_param_enabled: is_param_enabled,
    agent_get_param_name: get_param_name,
    agent_get_param_unit: get_param_unit,
    agent_set_param: set_param,
    agent_special_action: special_action,
    agent_get_state_idx: get_state_idx,
    image_size: &raw const patch_image_size,
    bss_begin: &raw const __patch_bss_start,
    bss_size: &raw const patch_bss_size,
    reserved: [0; 16],
};

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(target_os = "none"))]
#[allow(dead_code)] // each demo uses only some of the playing techniques
mod guitar;
#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
fn main() {
    host::main();
}
