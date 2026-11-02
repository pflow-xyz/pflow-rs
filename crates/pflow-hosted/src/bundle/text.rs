//! How the hosted tools (Go) print values in their messages: `%q` for a
//! string, `json.Marshal` for a value, a Rust `{:?}` list for a list of
//! names. The arguments reached the hosted server as JSON-RPC numbers
//! (doubles), so numbers print as the double they became.

use serde_json::Value;

/// Go's `strconv.Quote` (`%q`): printable characters as they are, the usual
/// backslash escapes, and `\x`, `\u` or `\U` for the rest.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            c if c == ' ' || !(c.is_control() || c.is_whitespace()) => out.push(c),
            c if (c as u32) < 0x80 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) <= 0xffff => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push('"');
    out
}

/// `["a", "b"]`: a list of names as the hosted tools (and Rust's `{:?}` of
/// a `Vec<&str>`) print it.
pub fn quote_list<S: AsRef<str>>(xs: &[S]) -> String {
    let q: Vec<String> = xs.iter().map(|x| quote(x.as_ref())).collect();
    format!("[{}]", q.join(", "))
}

/// A number as Go's `encoding/json` prints the double it arrived as.
pub fn number(n: &serde_json::Number) -> String {
    let f = n.as_f64().unwrap_or(0.0);
    let a = f.abs();
    if a != 0.0 && !(1e-6..1e21).contains(&a) {
        // shortest digits, exponent written as Go writes it: e+21, e-07 → e-7
        let s = format!("{f:e}");
        match s.split_once('e') {
            Some((m, e)) if e.starts_with('-') => format!("{m}e{e}"),
            Some((m, e)) => format!("{m}e+{e}"),
            None => s,
        }
    } else {
        format!("{f}")
    }
}

/// Go's `strconv.FormatFloat(x, 'f', 0, 64)` below 1e19, else `'g', -1`: an
/// integer-valued double printed as the integer it is.
pub fn integer(f: f64) -> String {
    if f.abs() < 1e19 {
        format!("{f:.0}")
    } else {
        let s = format!("{f:e}");
        match s.split_once('e') {
            Some((m, e)) if e.starts_with('-') => format!("{m}e{e}"),
            Some((m, e)) => format!("{m}e+{e}"),
            None => s,
        }
    }
}

/// `json.Marshal` of a value: compact, keys sorted, `<`, `>` and `&`
/// escaped as Go escapes them.
pub fn json(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => write_str(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write(&m[k], out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_text_is_gos() {
        assert_eq!(
            json(&json!({"to": "b:<x>", "from": 1.0, "n": 1.5e21, "s": 1e-7})),
            r#"{"from":1,"n":1.5e+21,"s":1e-7,"to":"b:\u003cx\u003e"}"#
        );
    }

    #[test]
    fn integers_print_as_the_double() {
        assert_eq!(integer(9007199254740993.0), "9007199254740992");
        assert_eq!(integer(1.8446744073709552e19), "1.8446744073709552e+19");
    }

    #[test]
    fn quote_is_gos() {
        assert_eq!(quote("a\"b\n"), r#""a\"b\n""#);
        assert_eq!(quote_list(&["a", "b"]), r#"["a", "b"]"#);
    }
}
