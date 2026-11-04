//! The fader taper: a position on the fader (0..=1) to decibels and back, and the readout
//! text. Ported from prototype v5's `TAPER`, `posToDb`, `dbToPos` and `dbTxt`
//! (`docs/design/prototype/index.html`), table and ramp unchanged:
//!
//! | position | 0 | 0.02 | 0.14 | 0.36 | 0.56 | 0.70 | 0.84 | 1 |
//! |---|---|---|---|---|---|---|---|---|
//! | dB | −∞ | −70 | −48 | −24 | −12 | −6 | 0 | +6 |
//!
//! Linear between the points; below 0.02 a ramp from −90 dB (at 0) to −70 dB, with anything at
//! or below 0.002 silent. Two refinements, both because positions here are `f32` where the
//! prototype's were JavaScript doubles: the table is compared and interpolated at the `f32`
//! values of its own positions, so every table point gives its decibels exactly — 0.84 is
//! exactly 0 dB, which keeps a send at unity on the engine's bit-exact ×1 path — and the
//! arithmetic runs in `f64`, as the prototype's did.

/// The position of 0 dB: where a new send starts and where double-click returns a fader.
pub const UNITY: f32 = 0.84;

/// The prototype's `TAPER`: position, decibels.
const TAPER: [(f32, f32); 8] = [
    (0.0, f32::NEG_INFINITY),
    (0.02, -70.0),
    (0.14, -48.0),
    (0.36, -24.0),
    (0.56, -12.0),
    (0.70, -6.0),
    (0.84, 0.0),
    (1.0, 6.0),
];

/// At or below this position the fader is silent.
const SILENT_BELOW: f32 = 0.002;
/// Where the table's first finite point sits, and the ramp below it ends.
const RAMP_TOP: f32 = 0.02;
/// The ramp's decibels at position 0 (the prototype's `-90`).
const RAMP_FLOOR_DB: f64 = -90.0;

/// Decibels for a fader position: −∞ at or below 0.002, the −90..−70 ramp below 0.02, the
/// table above, +6 at and past the top.
pub fn position_to_db(position: f32) -> f32 {
    position_to_db_f64(position) as f32
}

/// The fader position for a gain in decibels: 0 for −∞, anything at or below −90 dB, NaN and
/// (as the prototype's `!isFinite` has it) +∞; 1 at and past +6 dB.
pub fn db_to_position(db: f32) -> f32 {
    if !db.is_finite() || f64::from(db) <= RAMP_FLOOR_DB {
        return 0.0;
    }
    let d = f64::from(db);
    let ramp_top_db = f64::from(TAPER[1].1);
    if d < ramp_top_db {
        return ((d - RAMP_FLOOR_DB) / (ramp_top_db - RAMP_FLOOR_DB) * f64::from(RAMP_TOP)) as f32;
    }
    for pair in TAPER[1..].windows(2) {
        let (p1, d1) = (f64::from(pair[0].0), f64::from(pair[0].1));
        let (p2, d2) = (f64::from(pair[1].0), f64::from(pair[1].1));
        if d <= d2 {
            return (p1 + (p2 - p1) * (d - d1) / (d2 - d1)) as f32;
        }
    }
    1.0
}

/// The readout under a fader: `−∞` (with U+2212, as the prototype prints it), `0.0` within
/// 0.05 dB of unity, otherwise one decimal with a sign on boosts (`+3.1`, `-8.6`).
pub fn format_db(position: f32) -> String {
    let db = position_to_db_f64(position);
    if !db.is_finite() {
        return "\u{2212}\u{221e}".to_string();
    }
    if db.abs() < 0.05 {
        return "0.0".to_string();
    }
    // JavaScript's toFixed rounds a tie away from zero; Rust's formatter rounds it to even.
    let tenths = (db.abs() * 10.0).round();
    let sign = if db > 0.0 { "+" } else { "-" };
    format!("{sign}{}.{}", (tenths / 10.0).trunc(), tenths % 10.0)
}

/// Linear amplitude for a fader position: 0 when silent, exactly 1 at [`UNITY`].
pub fn position_to_gain(position: f32) -> f32 {
    let db = position_to_db(position);
    if db == f32::NEG_INFINITY {
        0.0
    } else if db == 0.0 {
        1.0
    } else {
        10f32.powf(db / 20.0)
    }
}

fn position_to_db_f64(position: f32) -> f64 {
    if position.is_nan() || position <= SILENT_BELOW {
        return f64::NEG_INFINITY;
    }
    if position < RAMP_TOP {
        let ramp_top_db = f64::from(TAPER[1].1);
        return RAMP_FLOOR_DB
            + f64::from(position) / f64::from(RAMP_TOP) * (ramp_top_db - RAMP_FLOOR_DB);
    }
    for pair in TAPER[1..].windows(2) {
        let (p1, d1) = pair[0];
        let (p2, d2) = pair[1];
        if position <= p2 {
            let (p, p1, d1, p2, d2) = (
                f64::from(position),
                f64::from(p1),
                f64::from(d1),
                f64::from(p2),
                f64::from(d2),
            );
            return d1 + (d2 - d1) * (p - p1) / (p2 - p1);
        }
    }
    f64::from(TAPER[TAPER.len() - 1].1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn every_table_point_is_its_decibels_exactly() {
        for (position, db) in [
            (0.84, 0.0),
            (0.70, -6.0),
            (0.56, -12.0),
            (0.36, -24.0),
            (0.14, -48.0),
            (0.02, -70.0),
            (1.0, 6.0),
        ] {
            assert_eq!(position_to_db(position), db, "position {position}");
            assert_eq!(db_to_position(db), position, "{db} dB");
        }
        assert_eq!(position_to_db(UNITY), 0.0);
        assert_eq!(position_to_gain(UNITY), 1.0);
    }

    #[test]
    fn at_or_below_two_thousandths_is_silence() {
        for position in [0.002, 0.001, 0.0, -0.5, f32::NAN] {
            assert_eq!(position_to_db(position), f32::NEG_INFINITY, "{position}");
            assert_eq!(position_to_gain(position), 0.0);
        }
        assert!(position_to_db(0.0021).is_finite());
    }

    #[test]
    fn below_the_table_is_the_prototypes_ramp_from_minus_ninety() {
        // The prototype's values at these positions (Node, from index.html).
        for (position, db) in [(0.003, -87.0), (0.01, -80.0), (0.019, -71.0)] {
            assert!((position_to_db(position) - db).abs() < 1e-4, "{position}");
        }
        assert!((db_to_position(-80.0) - 0.01).abs() < 1e-7);
        assert_eq!(db_to_position(-90.0), 0.0);
        assert_eq!(db_to_position(-120.0), 0.0);
    }

    #[test]
    fn between_points_it_matches_the_prototype() {
        // posToDb at the prototype's own example positions, printed by Node.
        for (position, db) in [
            (0.2, -41.454_545),
            (0.5, -15.6),
            (0.58, -11.142_857),
            (0.64, -8.571_428),
            (0.76, -3.428_571),
            (0.9, 2.25),
            (0.95, 4.125),
        ] {
            assert!((position_to_db(position) - db).abs() < 1e-4, "{position}");
        }
    }

    #[test]
    fn the_ends_clamp() {
        assert_eq!(position_to_db(1.2), 6.0);
        assert_eq!(db_to_position(10.0), 1.0);
        assert_eq!(db_to_position(f32::NEG_INFINITY), 0.0);
        // The prototype's `!isFinite(d)` reads +∞ and NaN as the bottom too.
        assert_eq!(db_to_position(f32::INFINITY), 0.0);
        assert_eq!(db_to_position(f32::NAN), 0.0);
    }

    #[test]
    fn the_readout_is_the_prototypes_text() {
        assert_eq!(format_db(0.0), "\u{2212}\u{221e}");
        assert_eq!(format_db(0.002), "\u{2212}\u{221e}");
        assert_eq!(format_db(UNITY), "0.0");
        assert_eq!(format_db(0.923), "+3.1");
        assert_eq!(format_db(0.64), "-8.6");
        // 0.9 is +2.2500000000000018 dB as a double and +2.249999 as the f32 a position is,
        // so it reads +2.3 in the prototype and +2.2 here: a tie the two cannot agree on.
        assert_eq!(format_db(0.9), "+2.2");
        assert_eq!(format_db(0.92), "+3.0");
        assert_eq!(format_db(0.003), "-87.0");
        assert_eq!(format_db(0.2), "-41.5");
        assert_eq!(format_db(1.0), "+6.0");
    }

    proptest! {
        #[test]
        fn position_survives_a_round_trip_through_decibels(position in 0.0021f32..=1.0) {
            let back = db_to_position(position_to_db(position));
            prop_assert!((back - position).abs() < 1e-5, "{position} came back as {back}");
        }

        #[test]
        fn louder_never_reads_quieter(a in 0.0f32..=1.0, b in 0.0f32..=1.0) {
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(position_to_db(low) <= position_to_db(high));
        }
    }
}
