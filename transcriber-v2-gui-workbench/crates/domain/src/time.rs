//! Tiempo explícito con enteros.
//!
//! Unidad: *flicks* (1/705 600 000 s). Es divisible exactamente por las
//! frecuencias de fotograma habituales (24, 25, 30, 48, 50, 60, 90, 100, 120,
//! 23.976, 29.97, 59.94) y por las frecuencias de muestreo (44 100, 48 000,
//! 96 000), y por 1 000 (milisegundos de los JSON V1). Un `i64` cubre ±414 años.
//!
//! Los tiempos V1 son segundos `float` redondeados a milisegundos; la conversión
//! ms ↔ flicks es exacta en ambos sentidos.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Flicks por segundo.
pub const FLICKS_PER_SECOND: i64 = 705_600_000;

/// Instante o duración en flicks. Semántica (fuente, secuencia, presentación)
/// la define el campo que lo contiene; el tipo evita mezclar floats de UI.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ticks(pub i64);

impl Ticks {
    pub const ZERO: Ticks = Ticks(0);
    pub const MAX: Ticks = Ticks(i64::MAX);

    pub const fn from_flicks(v: i64) -> Self {
        Ticks(v)
    }

    pub const fn from_seconds(s: i64) -> Self {
        Ticks(s * FLICKS_PER_SECOND)
    }

    pub const fn from_millis(ms: i64) -> Self {
        Ticks(ms * (FLICKS_PER_SECOND / 1000))
    }

    /// Conversión desde segundos en coma flotante (entrada externa: ffprobe,
    /// JSON V1). Redondea al flick más cercano.
    pub fn from_seconds_f64(s: f64) -> Self {
        Ticks((s * FLICKS_PER_SECOND as f64).round() as i64)
    }

    /// Segundos como `f64` (para presentación y contratos externos que usan
    /// segundos). No usar para acumular.
    pub fn as_seconds_f64(self) -> f64 {
        self.0 as f64 / FLICKS_PER_SECOND as f64
    }

    /// Segundos redondeados a milisegundos, como los JSON V1 (`round(x, 3)`).
    pub fn as_seconds_ms(self) -> f64 {
        let ms = self.as_millis_round();
        ms as f64 / 1000.0
    }

    pub fn as_millis_round(self) -> i64 {
        let per_ms = FLICKS_PER_SECOND / 1000;
        // división con redondeo al más cercano, válida para negativos
        let (q, r) = (self.0.div_euclid(per_ms), self.0.rem_euclid(per_ms));
        if r * 2 >= per_ms { q + 1 } else { q }
    }

    /// Duración exacta de `n` fotogramas a `rate`.
    pub fn from_frames(n: i64, rate: Rational) -> Self {
        // n * (den/num) segundos = n * den * FLICKS / num
        let flicks = (n as i128) * (rate.den as i128) * (FLICKS_PER_SECOND as i128) / (rate.num as i128);
        Ticks(flicks as i64)
    }

    /// Índice de fotograma que contiene este instante (floor).
    pub fn frame_index(self, rate: Rational) -> i64 {
        let num = (self.0 as i128) * (rate.num as i128);
        let den = (rate.den as i128) * (FLICKS_PER_SECOND as i128);
        num.div_euclid(den) as i64
    }

    /// Redondea hacia abajo al inicio del fotograma que lo contiene.
    pub fn floor_to_frame(self, rate: Rational) -> Self {
        Ticks::from_frames(self.frame_index(rate), rate)
    }

    /// Redondea al inicio de fotograma más cercano.
    pub fn round_to_frame(self, rate: Rational) -> Self {
        let half = Ticks::from_frames(1, rate).0 / 2;
        Ticks(self.0 + half).floor_to_frame(rate)
    }

    pub fn from_samples(n: i64, sample_rate: u32) -> Self {
        Ticks(((n as i128) * (FLICKS_PER_SECOND as i128) / (sample_rate as i128)) as i64)
    }

    pub fn sample_index(self, sample_rate: u32) -> i64 {
        ((self.0 as i128) * (sample_rate as i128)).div_euclid(FLICKS_PER_SECOND as i128) as i64
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn abs(self) -> Self {
        Ticks(self.0.abs())
    }

    pub fn min(self, other: Self) -> Self {
        if self <= other { self } else { other }
    }

    pub fn max(self, other: Self) -> Self {
        if self >= other { self } else { other }
    }

    pub fn clamp(self, lo: Self, hi: Self) -> Self {
        self.max(lo).min(hi)
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Ticks(self.0.saturating_add(other.0))
    }

    /// Multiplica por un factor (velocidad de revisión, zoom). Redondea.
    pub fn scale_f64(self, factor: f64) -> Self {
        Ticks((self.0 as f64 * factor).round() as i64)
    }

    /// Timecode `hh:mm:ss.mmm` (como `editorial_io.format_time`).
    pub fn timecode_ms(self) -> String {
        let ms = self.max(Ticks::ZERO).as_millis_round();
        let (h, rem) = (ms / 3_600_000, ms % 3_600_000);
        let (m, rem) = (rem / 60_000, rem % 60_000);
        let (s, milli) = (rem / 1000, rem % 1000);
        format!("{h:02}:{m:02}:{s:02}.{milli:03}")
    }

    /// Reloj compacto `m:ss.d` / `h:mm:ss.d` (como `editorial_layers.clock`).
    pub fn clock(self) -> String {
        let secs = self.max(Ticks::ZERO).as_seconds_f64();
        let hours = (secs / 3600.0).floor();
        let rest = secs - hours * 3600.0;
        let minutes = (rest / 60.0).floor();
        let s = rest - minutes * 60.0;
        if hours >= 1.0 { format!("{}:{:02}:{:04.1}", hours as i64, minutes as i64, s) } else { format!("{}:{:04.1}", minutes as i64, s) }
    }

    /// Timecode con fotogramas `hh:mm:ss:ff` a `rate` (no drop-frame).
    pub fn timecode_frames(self, rate: Rational) -> String {
        let frame = self.max(Ticks::ZERO).frame_index(rate);
        let fps = ((rate.num as f64) / (rate.den as f64)).round() as i64;
        let fps = fps.max(1);
        let ff = frame % fps;
        let total_s = frame / fps;
        let (h, rem) = (total_s / 3600, total_s % 3600);
        let (m, s) = (rem / 60, rem % 60);
        format!("{h:02}:{m:02}:{s:02}:{ff:02}")
    }
}

impl fmt::Debug for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ticks({} = {:.6}s)", self.0, self.as_seconds_f64())
    }
}

impl fmt::Display for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.timecode_ms())
    }
}

impl Add for Ticks {
    type Output = Ticks;
    fn add(self, rhs: Ticks) -> Ticks {
        Ticks(self.0 + rhs.0)
    }
}
impl Sub for Ticks {
    type Output = Ticks;
    fn sub(self, rhs: Ticks) -> Ticks {
        Ticks(self.0 - rhs.0)
    }
}
impl AddAssign for Ticks {
    fn add_assign(&mut self, rhs: Ticks) {
        self.0 += rhs.0;
    }
}
impl SubAssign for Ticks {
    fn sub_assign(&mut self, rhs: Ticks) {
        self.0 -= rhs.0;
    }
}
impl std::ops::Mul<i64> for Ticks {
    type Output = Ticks;
    fn mul(self, rhs: i64) -> Ticks {
        Ticks(self.0 * rhs)
    }
}
impl std::ops::Div<i64> for Ticks {
    type Output = Ticks;
    fn div(self, rhs: i64) -> Ticks {
        Ticks(self.0 / rhs)
    }
}
impl Neg for Ticks {
    type Output = Ticks;
    fn neg(self) -> Ticks {
        Ticks(-self.0)
    }
}

/// Fracción racional positiva (frecuencia de fotograma, timebase).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}

impl Rational {
    pub const fn new(num: i64, den: i64) -> Self {
        Rational { num, den }
    }

    pub fn is_valid(&self) -> bool {
        self.num > 0 && self.den > 0
    }

    pub fn as_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Parsea `"30000/1001"` o `"30"` (formato ffprobe).
    pub fn parse(text: &str) -> Option<Rational> {
        let text = text.trim();
        if let Some((a, b)) = text.split_once('/') {
            let num: i64 = a.trim().parse().ok()?;
            let den: i64 = b.trim().parse().ok()?;
            let r = Rational { num, den };
            r.is_valid().then_some(r)
        } else {
            let num: i64 = text.parse().ok()?;
            let r = Rational { num, den: 1 };
            r.is_valid().then_some(r)
        }
    }

    pub fn reduced(&self) -> Rational {
        fn gcd(a: i64, b: i64) -> i64 {
            if b == 0 { a.abs() } else { gcd(b, a % b) }
        }
        let g = gcd(self.num, self.den).max(1);
        Rational { num: self.num / g, den: self.den / g }
    }

    /// Duración de un fotograma.
    pub fn frame_duration(&self) -> Ticks {
        Ticks::from_frames(1, *self)
    }
}

impl Default for Rational {
    fn default() -> Self {
        Rational { num: 30, den: 1 }
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 { write!(f, "{}", self.num) } else { write!(f, "{}/{}", self.num, self.den) }
    }
}

/// Intervalo semiabierto `[start, end)`. `start == end` es un punto (permitido
/// solo donde el contrato lo admite, p. ej. marcas puntuales).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: Ticks,
    pub end: Ticks,
}

impl TimeRange {
    pub fn new(start: Ticks, end: Ticks) -> Self {
        TimeRange { start, end }
    }

    pub fn from_start_duration(start: Ticks, duration: Ticks) -> Self {
        TimeRange { start, end: start + duration }
    }

    pub fn duration(&self) -> Ticks {
        self.end - self.start
    }

    pub fn is_point(&self) -> bool {
        self.start == self.end
    }

    pub fn is_ordered(&self) -> bool {
        self.start <= self.end
    }

    pub fn contains(&self, t: Ticks) -> bool {
        t >= self.start && t < self.end
    }

    /// Intersección no vacía (los puntos no se solapan con nada).
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn intersection(&self, other: &TimeRange) -> Option<TimeRange> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then_some(TimeRange { start, end })
    }

    /// `self` contiene por completo a `other` (bordes inclusivos).
    pub fn encloses(&self, other: &TimeRange) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    pub fn shifted(&self, delta: Ticks) -> TimeRange {
        TimeRange { start: self.start + delta, end: self.end + delta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flicks_are_exact_for_common_rates_and_samples() {
        for (n, d) in [(24, 1), (25, 1), (30, 1), (60, 1), (30000, 1001), (24000, 1001), (60000, 1001), (120, 1)] {
            let r = Rational::new(n, d);
            let one = Ticks::from_frames(1, r);
            assert_eq!(Ticks::from_frames(n, r), Ticks::from_seconds(d), "{r}");
            assert_eq!(one.frame_index(r), 1);
            assert_eq!((one - Ticks(1)).frame_index(r), 0);
        }
        assert_eq!(Ticks::from_samples(48_000, 48_000), Ticks::from_seconds(1));
        assert_eq!(Ticks::from_samples(44_100, 44_100), Ticks::from_seconds(1));
        assert_eq!(Ticks::from_millis(1500), Ticks::from_seconds_f64(1.5));
    }

    #[test]
    fn no_accumulated_drift_over_hours() {
        let r = Rational::new(30000, 1001);
        let mut t = Ticks::ZERO;
        let frames = 3 * 3600 * 30; // ~3 h
        for _ in 0..frames {
            t += r.frame_duration();
        }
        assert_eq!(t, Ticks::from_frames(frames, r));
        assert_eq!(t.frame_index(r), frames);
    }

    #[test]
    fn ms_round_trip_matches_v1_rounding() {
        for ms in [0i64, 1, 999, 1000, 12_345, 3_599_999] {
            let t = Ticks::from_millis(ms);
            assert_eq!(t.as_millis_round(), ms);
            assert_eq!(t.as_seconds_ms(), ms as f64 / 1000.0);
        }
        assert_eq!(Ticks::from_seconds_f64(1.0004).as_millis_round(), 1000);
        assert_eq!(Ticks::from_seconds_f64(1.0006).as_millis_round(), 1001);
    }

    #[test]
    fn timecodes() {
        assert_eq!(Ticks::from_millis(3_723_456).timecode_ms(), "01:02:03.456");
        assert_eq!(Ticks::from_millis(65_500).clock(), "1:05.5");
        assert_eq!(Ticks::from_millis(3_665_000).clock(), "1:01:05.0");
        assert_eq!(Ticks::from_frames(61, Rational::new(30, 1)).timecode_frames(Rational::new(30, 1)), "00:00:02:01");
    }

    #[test]
    fn ranges() {
        let a = TimeRange::new(Ticks(10), Ticks(20));
        let b = TimeRange::new(Ticks(20), Ticks(30));
        let c = TimeRange::new(Ticks(15), Ticks(25));
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&c));
        assert_eq!(a.intersection(&c), Some(TimeRange::new(Ticks(15), Ticks(20))));
        assert!(a.contains(Ticks(10)) && !a.contains(Ticks(20)));
        assert!(TimeRange::new(Ticks(0), Ticks(100)).encloses(&a));
        assert!(Rational::parse("30000/1001").is_some());
        assert!(Rational::parse("0/0").is_none());
    }
}
