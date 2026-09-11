//! Color and XML-escaping helpers shared by the net and plotter renderers,
//! ported from go-pflow's `visualization/render.go` and `petri.Escape`.
//!
//! Two escaping functions exist because go-pflow has two: `render.go`'s
//! `escapeXML` (net/statemachine/workflow labels) escapes five characters
//! including quotes, while `petri.Escape` (used by `plotter/svg.go`) only
//! escapes `&`, `<`, `>`. Collapsing them into one would either under-escape
//! attribute-adjacent net labels or over-escape plot text relative to Go's
//! own output.

/// Escapes `&`, `<`, `>`, `"` and `'` — matches `visualization.escapeXML`.
pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Escapes `&`, `<`, `>` only — matches `petri.Escape`, used by the plotter.
pub fn escape_minimal(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Named token colors recognized from a `https://pflow.xyz/tokens/<name>`
/// URL, matching `getColorDictionary`.
fn color_dictionary(name: &str) -> Option<&'static str> {
    Some(match name {
        "black" => "#000000",
        "red" => "#dc3545",
        "blue" => "#007bff",
        "green" => "#28a745",
        "yellow" => "#ffc107",
        "orange" => "#fd7e14",
        "purple" => "#6f42c1",
        "pink" => "#e83e8c",
        "brown" => "#8b4513",
        "cyan" => "#17a2b8",
        "gray" => "#6c757d",
        "grey" => "#6c757d",
        "white" => "#ffffff",
        _ => return None,
    })
}

/// Extracts a color from a token URL or hex color string, matching
/// `extractColor`.
pub fn extract_color(token_url: &str) -> String {
    if token_url.is_empty() {
        return String::new();
    }
    if let Some(stripped) = token_url.strip_prefix('#') {
        let _ = stripped;
        return token_url.to_string();
    }
    if let Some(last) = token_url.rsplit('/').next() {
        let name = last.to_lowercase();
        if let Some(hex) = color_dictionary(&name) {
            return hex.to_string();
        }
    }
    String::new()
}

/// Lightens a `#rrggbb` hex color by moving it toward white by `factor`
/// (0-1), matching `lightenColor`. Non-hex or malformed input passes
/// through unchanged.
pub fn lighten_color(hex_color: &str, factor: f64) -> String {
    if !hex_color.starts_with('#') || hex_color.len() != 7 {
        return hex_color.to_string();
    }
    let Ok(r) = u8::from_str_radix(&hex_color[1..3], 16) else {
        return hex_color.to_string();
    };
    let Ok(g) = u8::from_str_radix(&hex_color[3..5], 16) else {
        return hex_color.to_string();
    };
    let Ok(b) = u8::from_str_radix(&hex_color[5..7], 16) else {
        return hex_color.to_string();
    };

    let lighten = |c: u8| -> u8 {
        let c = c as f64;
        (c + (255.0 - c) * factor).round() as u8
    };

    format!("#{:02x}{:02x}{:02x}", lighten(r), lighten(g), lighten(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_five_for_xml() {
        assert_eq!(
            escape_xml(r##"<a & "b" 'c'>"##),
            "&lt;a &amp; &quot;b&quot; &apos;c&apos;&gt;"
        );
    }

    #[test]
    fn escapes_only_three_for_minimal() {
        assert_eq!(escape_minimal(r##"<a & "b">"##), "&lt;a &amp; \"b\"&gt;");
    }

    #[test]
    fn extracts_named_and_hex_colors() {
        assert_eq!(extract_color("https://pflow.xyz/tokens/red"), "#dc3545");
        assert_eq!(extract_color("#123abc"), "#123abc");
        assert_eq!(extract_color(""), "");
        assert_eq!(extract_color("https://pflow.xyz/tokens/nope"), "");
    }

    #[test]
    fn lightens_toward_white() {
        assert_eq!(lighten_color("#000000", 1.0), "#ffffff");
        assert_eq!(lighten_color("#000000", 0.0), "#000000");
        assert_eq!(lighten_color("not-a-color", 0.5), "not-a-color");
    }
}
