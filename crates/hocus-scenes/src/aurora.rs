//! The aurora following the music (#801): the board "Aurore en
//! musique"'s drive.

/// The board's tempo.
pub const BPM: f64 = 96.0;

/// The board's drive at `t` (seconds of music): its swell, and its kick
/// on the beat.
pub fn drive(t: f64) -> (f64, f64) {
    let beat = (t * BPM / 60.0).fract();
    let kick = (-beat * 5.5).exp();
    let energy = 0.35 + 0.65 * ((t * 0.11).sin() * 0.5 + 0.5).powf(1.5);
    (energy, kick)
}
