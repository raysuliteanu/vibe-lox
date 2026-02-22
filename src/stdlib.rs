/// Read one line from a [`std::io::BufRead`] source, stripping the trailing
/// newline (`\n` or `\r\n`).
///
/// Returns `None` at EOF or on an I/O error.
pub fn read_line_from<R: std::io::BufRead>(reader: &mut R) -> Option<String> {
    let mut buf = String::new();
    match reader.read_line(&mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if buf.ends_with('\n') {
                buf.pop();
                if buf.ends_with('\r') {
                    buf.pop();
                }
            }
            Some(buf)
        }
    }
}

/// Parse a string as a Lox `NUMBER` literal, trimming surrounding whitespace.
///
/// Accepts: `DIGIT+ ("." DIGIT+)?` — no sign, no scientific notation.
/// Returns `None` if the string is not a valid Lox number.
pub fn parse_lox_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let mut i = 0;

    // Consume leading digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None; // no leading digit
    }

    // Optional decimal part
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let decimal_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == decimal_start {
            return None; // "3." — no digits after dot
        }
    }

    // Must have consumed the entire string
    if i != bytes.len() {
        return None;
    }

    s.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::io::Cursor;

    #[test]
    fn read_line_returns_string_without_newline() {
        let mut r = Cursor::new(b"hello\nworld\n");
        assert_eq!(read_line_from(&mut r), Some("hello".into()));
        assert_eq!(read_line_from(&mut r), Some("world".into()));
        assert_eq!(read_line_from(&mut r), None);
    }

    #[test]
    fn read_line_strips_crlf() {
        let mut r = Cursor::new(b"hello\r\nworld\r\n");
        assert_eq!(read_line_from(&mut r), Some("hello".into()));
        assert_eq!(read_line_from(&mut r), Some("world".into()));
    }

    #[test]
    fn read_line_empty_line_returns_empty_string() {
        let mut r = Cursor::new(b"\nhello\n");
        assert_eq!(read_line_from(&mut r), Some("".into()));
        assert_eq!(read_line_from(&mut r), Some("hello".into()));
    }

    #[test]
    fn read_line_no_trailing_newline() {
        let mut r = Cursor::new(b"last");
        assert_eq!(read_line_from(&mut r), Some("last".into()));
        assert_eq!(read_line_from(&mut r), None);
    }

    #[test]
    fn read_line_empty_input_returns_none() {
        let mut r = Cursor::new(b"");
        assert_eq!(read_line_from(&mut r), None);
    }

    #[rstest]
    #[case("42", Some(42.0))]
    #[case("3.14", Some(3.14))]
    #[case("0", Some(0.0))]
    #[case("007", Some(7.0))]
    #[case("0.5", Some(0.5))]
    #[case("  7  ", Some(7.0))]
    fn parse_lox_number_valid(#[case] input: &str, #[case] expected: Option<f64>) {
        assert_eq!(parse_lox_number(input), expected);
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    #[case("-1")]
    #[case("1e5")]
    #[case("3.14.15")]
    #[case("3.")]
    #[case(".5")]
    #[case("inf")]
    #[case("nan")]
    #[case("abc")]
    #[case("1 2")]
    fn parse_lox_number_invalid(#[case] input: &str) {
        assert_eq!(parse_lox_number(input), None);
    }
}
