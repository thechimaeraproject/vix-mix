//! A value as a request and a reply write it: a switch, a number, or text.

use std::fmt;

/// One value of the console, as the control socket carries it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A mute, a bypass, a link: `true` or `false`.
    Switch(bool),
    /// A fader's position, its decibels, a slot. A silent fader's decibels are negative
    /// infinity, written `-inf`.
    Number(f64),
    /// A name, in double quotes as JSON writes text.
    Text(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Switch(on) => write!(f, "{on}"),
            Value::Number(n) if *n == f64::NEG_INFINITY => f.write_str(SILENT),
            Value::Number(n) => write!(f, "{n}"),
            // JSON's text escapes a quote, a backslash and every control character, so a
            // name never ends the line it is written on. Text cannot fail to serialize.
            Value::Text(text) => {
                f.write_str(&serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string()))
            }
        }
    }
}

/// How a silent fader's decibels are written, and read.
pub const SILENT: &str = "-inf";

/// A switch as a request writes it: `true` or `false`, and `on`, `off`, `1` and `0`, which
/// is what a controller that knows only numbers sends. Case does not matter.
pub fn switch(token: &str) -> Option<bool> {
    match token.to_ascii_lowercase().as_str() {
        "true" | "on" | "1" => Some(true),
        "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// A number as a request writes it: a finite number, or `-inf` (or `-infinity`) for silence.
/// Not a number, and never infinity upwards: a fader has a top.
pub fn number(token: &str) -> Option<f64> {
    let lower = token.to_ascii_lowercase();
    if lower == SILENT || lower == "-infinity" {
        return Some(f64::NEG_INFINITY);
    }
    // Rust reads `inf`, `infinity` and `nan` as numbers; a request may not send them.
    token.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Text as a request writes it: in double quotes as JSON writes text, or a bare word (or
/// several) as it stands. `None` for text that opens a quote and is not JSON's.
pub fn word(token: &str) -> Option<String> {
    if token.starts_with('"') {
        serde_json::from_str::<String>(token).ok()
    } else {
        Some(token.to_string())
    }
}

/// `n` to `places` decimals, and never negative zero: what a reply prints for a position or
/// for decibels, so the same fader always reads the same.
pub fn rounded(n: f64, places: i32) -> f64 {
    if !n.is_finite() {
        return n;
    }
    let scale = 10f64.powi(places);
    let r = (n * scale).round() / scale;
    if r == 0.0 { 0.0 } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_written_as_one_word_that_reads_back() {
        assert_eq!(Value::Switch(true).to_string(), "true");
        assert_eq!(Value::Number(0.84).to_string(), "0.84");
        assert_eq!(Value::Number(-12.0).to_string(), "-12");
        assert_eq!(Value::Number(f64::NEG_INFINITY).to_string(), "-inf");
        assert_eq!(Value::Text("Mic".to_string()).to_string(), "\"Mic\"");
        for said in ["true", "0.84", "-12", "-inf"] {
            let read = switch(said)
                .map(Value::Switch)
                .or_else(|| number(said).map(Value::Number))
                .expect("a value");
            assert_eq!(read.to_string(), said);
        }
    }

    #[test]
    fn a_name_with_a_quote_or_a_line_end_stays_on_one_line() {
        let name = "Mic \"A\"\nline two\\";
        let written = Value::Text(name.to_string()).to_string();
        assert!(!written.contains('\n'), "{written}");
        // Any JSON reader reads the name back.
        assert_eq!(serde_json::from_str::<String>(&written).unwrap(), name);
    }

    #[test]
    fn text_is_a_bare_word_or_in_double_quotes() {
        assert_eq!(word("headphones").as_deref(), Some("headphones"));
        assert_eq!(
            word("Built-in speakers").as_deref(),
            Some("Built-in speakers")
        );
        assert_eq!(
            word("\"Built-in speakers\"").as_deref(),
            Some("Built-in speakers")
        );
        assert_eq!(word("\"\"").as_deref(), Some(""));
        // A quote that is opened is JSON's, or it is not text.
        assert_eq!(word("\"Built-in"), None);
    }

    #[test]
    fn a_switch_is_true_or_false_and_what_a_number_only_controller_sends() {
        for on in ["true", "TRUE", "on", "1"] {
            assert_eq!(switch(on), Some(true), "{on}");
        }
        for off in ["false", "Off", "0"] {
            assert_eq!(switch(off), Some(false), "{off}");
        }
        for neither in ["yes", "2", "", "-1", "0.5"] {
            assert_eq!(switch(neither), None, "{neither}");
        }
    }

    #[test]
    fn a_number_is_finite_or_silence_and_nothing_else() {
        assert_eq!(number("-12.5"), Some(-12.5));
        assert_eq!(number("1e-2"), Some(0.01));
        assert_eq!(number("-inf"), Some(f64::NEG_INFINITY));
        assert_eq!(number("-Infinity"), Some(f64::NEG_INFINITY));
        for not in ["inf", "+inf", "infinity", "nan", "NaN", "loud", "", "1,5"] {
            assert_eq!(number(not), None, "{not}");
        }
    }

    #[test]
    fn rounding_keeps_a_reading_steady_and_zero_unsigned() {
        // 0.84 as the model's f32 holds it.
        assert_eq!(rounded(f64::from(0.84_f32), 4), 0.84);
        assert_eq!(rounded(-0.000_01, 2).to_string(), "0");
        assert_eq!(rounded(-11.999_999, 2), -12.0);
        assert_eq!(rounded(f64::NEG_INFINITY, 2), f64::NEG_INFINITY);
    }
}
