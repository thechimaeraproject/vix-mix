//! A name as one plain word: what a channel's PipeWire node name ends in, and how the control
//! socket compares a channel or a plugin written by name. It lives here, beside the names it
//! reads, because the backend that names nodes builds on Linux only and the control socket's
//! addresses are resolved on every platform.

/// Lower-case ASCII letters and digits, every other run of characters one `-`, none at the
/// ends; `channel` when nothing is left.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "channel".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_becomes_lower_case_words_joined_by_one_dash() {
        assert_eq!(slug("Voice chat"), "voice-chat");
        assert_eq!(slug("  Mic #2 (Wave:3) "), "mic-2-wave-3");
        assert_eq!(slug("Pro-Q 3"), "pro-q-3");
        // Two names that differ only in case and punctuation are the same word.
        assert_eq!(slug("Music!"), slug("music"));
    }

    #[test]
    fn a_name_with_no_letter_or_digit_is_the_word_channel() {
        assert_eq!(slug("\u{1f3b5}"), "channel");
        assert_eq!(slug(""), "channel");
        assert_eq!(slug(" - "), "channel");
    }
}
