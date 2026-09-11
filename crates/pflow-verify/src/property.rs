//! Property language: linear expressions over places, and the verdict
//! shapes verification produces. Ported from go-pflow's `verify/property.go`.

use std::collections::HashMap;

/// What a [`Property`] asserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// No reachable marking is a deadlock (a non-final state with no
    /// enabled transition).
    DeadlockFree,
    /// No place can accumulate tokens without limit.
    Bounded,
    /// Every transition can fire from some reachable marking (L1
    /// quasi-liveness — NOT L4: a proved `Live` does not imply
    /// `DeadlockFree`, since a net can fire every transition along one path
    /// and still wedge on another).
    Live,
    /// Every execution eventually stops (the reachability graph is
    /// acyclic).
    Terminating,
    /// Some reachable marking matches `target` (a partial marking: only the
    /// places it names are constrained).
    Reachable,
    /// No reachable marking matches `target` — the safety form.
    Unreachable,
    /// A linear relation over places holds at every reachable marking, e.g.
    /// `"minted == circulating + burned"`. Written in `expr`.
    Invariant,
    /// At most `bound` of `places` hold a token simultaneously. Sugar for
    /// an invariant expression.
    MutualExclusion,
    /// The total token count never changes.
    Conserves,
}

/// A single assertion about a net.
#[derive(Debug, Clone, Default)]
pub struct Property {
    pub kind: Option<Kind>,
    pub name: String,
    /// The linear relation for `Kind::Invariant`.
    pub expr: String,
    /// The marking for `Kind::Reachable`/`Kind::Unreachable`.
    pub target: HashMap<String, i64>,
    /// `Kind::MutualExclusion` configuration.
    pub places: Vec<String>,
    pub bound: i64,
}

impl Property {
    pub fn new(kind: Kind) -> Self {
        Property { kind: Some(kind), ..Default::default() }
    }

    pub fn invariant(expr: impl Into<String>) -> Self {
        Property { kind: Some(Kind::Invariant), expr: expr.into(), ..Default::default() }
    }

    pub fn reachable(target: HashMap<String, i64>) -> Self {
        Property { kind: Some(Kind::Reachable), target, ..Default::default() }
    }

    pub fn unreachable(target: HashMap<String, i64>) -> Self {
        Property { kind: Some(Kind::Unreachable), target, ..Default::default() }
    }

    pub fn mutual_exclusion(places: Vec<String>, bound: i64) -> Self {
        Property { kind: Some(Kind::MutualExclusion), places, bound, ..Default::default() }
    }
}

/// The outcome of checking a [`Property`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The property holds and the check was complete.
    Proved,
    /// The property does not hold; `Verdict::counterexample` shows why.
    Refuted,
    /// The check could not be completed, typically because the state space
    /// exceeded the exploration limit. Not a pass.
    Unknown,
}

/// How a verdict was reached, which determines how far it generalizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Holds for every initial marking, via linear algebra on the incidence
    /// matrix. The strongest result available.
    Structural,
    /// Holds for the analyzed initial marking, via complete enumeration of
    /// its reachable state space.
    Exhaustive,
    /// Rests on a finite constructive witness found by targeted search.
    /// Decisive without requiring the whole state space to be enumerated.
    Witness,
    /// Exploration was truncated; only `Unknown` or `Refuted` verdicts are
    /// sound (a counterexample found in a partial search is still real).
    Partial,
    /// No exploration or structural argument was needed or run.
    None,
}

/// A replayable witness that a property fails.
#[derive(Debug, Clone, Default)]
pub struct Counterexample {
    /// The firing sequence from the initial marking that reaches `marking`.
    /// Empty means the initial marking itself is the witness.
    pub trace: Vec<String>,
    pub marking: HashMap<String, i64>,
    /// What is wrong with this marking, in plain terms.
    pub explanation: String,
}

/// The result of checking one [`Property`].
#[derive(Debug, Clone)]
pub struct Verdict {
    pub property: Property,
    pub status: Status,
    pub method: Method,
    /// A one-line human-readable summary.
    pub detail: String,
    /// The reason a property was proved — for a structural proof, the
    /// P-invariant that implies it.
    pub evidence: String,
    /// Populated when `status` is `Refuted`.
    pub counterexample: Option<Counterexample>,
}

impl Verdict {
    pub(crate) fn new(property: Property, status: Status, method: Method) -> Self {
        Verdict { property, status, method, detail: String::new(), evidence: String::new(), counterexample: None }
    }
}

/// The outcome of checking a set of properties against a net.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub verdicts: Vec<Verdict>,
    pub proved: usize,
    pub refuted: usize,
    pub unknown: usize,
    /// True only when every property was proved. A property that could not
    /// be decided keeps this `false` — unknown is not a pass.
    pub ok: bool,
    pub state_count: usize,
    pub truncated: bool,
}

impl Report {
    /// A short human-readable digest.
    pub fn summary(&self) -> String {
        let mut s = format!("{} proved, {} refuted, {} unknown", self.proved, self.refuted, self.unknown);
        if self.truncated {
            s.push_str(&format!(" (state space truncated at {} states)", self.state_count));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Linear expression parsing
// ---------------------------------------------------------------------------

/// A comparison operator in a property expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    Eq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
}

impl Relation {
    fn as_str(&self) -> &'static str {
        match self {
            Relation::Eq => "==",
            Relation::Ne => "!=",
            Relation::Le => "<=",
            Relation::Ge => ">=",
            Relation::Lt => "<",
            Relation::Gt => ">",
        }
    }
}

/// A parsed assertion of the form `sum(coeffs[p] * marking[p]) <rel>
/// constant`. Both sides of the source text are normalized into this shape.
#[derive(Debug, Clone)]
pub struct LinearExpr {
    pub coeffs: HashMap<String, i64>,
    pub rel: Relation,
    pub constant: i64,
}

impl LinearExpr {
    /// The place names referenced, sorted.
    pub fn places(&self) -> Vec<String> {
        let mut out: Vec<String> = self.coeffs.keys().cloned().collect();
        out.sort();
        out
    }

    /// The left-hand side, evaluated at `marking`.
    pub fn eval(&self, marking: &HashMap<String, i64>) -> i64 {
        self.coeffs.iter().map(|(p, c)| c * marking.get(p).copied().unwrap_or(0)).sum()
    }

    /// Whether the relation is satisfied at `marking`.
    pub fn holds(&self, marking: &HashMap<String, i64>) -> bool {
        let lhs = self.eval(marking);
        match self.rel {
            Relation::Eq => lhs == self.constant,
            Relation::Ne => lhs != self.constant,
            Relation::Le => lhs <= self.constant,
            Relation::Ge => lhs >= self.constant,
            Relation::Lt => lhs < self.constant,
            Relation::Gt => lhs > self.constant,
        }
    }

    /// The normalized expression.
    pub fn render(&self) -> String {
        format!("{} {} {}", self.render_lhs(), self.rel.as_str(), self.constant)
    }

    /// Just the left-hand side.
    pub fn render_lhs(&self) -> String {
        let mut b = String::new();
        for p in self.places() {
            let c = self.coeffs[&p];
            if c == 0 {
                continue;
            }
            if b.is_empty() && c < 0 {
                b.push('-');
            } else if !b.is_empty() {
                b.push_str(if c < 0 { " - " } else { " + " });
            }
            let mag = c.abs();
            if mag != 1 {
                b.push_str(&mag.to_string());
                b.push('*');
            }
            b.push_str(&quote_name(&p));
        }
        if b.is_empty() {
            b.push('0');
        }
        b
    }
}

/// A parse error for [`parse_expr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ParseError {}

fn err(msg: impl Into<String>) -> ParseError {
    ParseError(msg.into())
}

/// Parses a linear relation over place names.
///
/// Grammar: `expr := side relation side`, `side := [sign] term { sign term
/// }`, `term := integer | [integer '*'] identifier | identifier ['*'
/// integer]`, `relation := '==' | '=' | '!=' | '<=' | '>=' | '<' | '>'`.
///
/// Identifiers are place names: a letter or `_` followed by letters,
/// digits, `_`, `.` or `:`. Names containing other characters (or a leading
/// digit) can be quoted: `"my place" >= 1`.
///
/// Examples: `"a + b == 10"`, `"minted == circulating + burned"`,
/// `"busy1 + busy2 <= 1"`, `"2*boxes + loose == 12"`.
pub fn parse_expr(src: &str) -> Result<LinearExpr, ParseError> {
    let toks = tokenize(src)?;

    let mut rel_idx: Option<usize> = None;
    let mut rel = Relation::Eq;
    for (i, t) in toks.iter().enumerate() {
        if let TokKind::Rel(r) = t.kind {
            if rel_idx.is_some() {
                return Err(err(format!("expression has more than one comparison operator: {src:?}")));
            }
            rel_idx = Some(i);
            rel = r;
        }
    }
    let Some(rel_idx) = rel_idx else {
        return Err(err(format!(
            "expression has no comparison operator (expected one of ==, !=, <=, >=, <, >): {src:?}"
        )));
    };

    let (left, left_const) = parse_side(&toks[..rel_idx])
        .map_err(|e| err(format!("left of {}: {e}", rel.as_str())))?;
    let (right, right_const) = parse_side(&toks[rel_idx + 1..])
        .map_err(|e| err(format!("right of {}: {e}", rel.as_str())))?;

    let mut coeffs: HashMap<String, i64> = HashMap::new();
    for (p, c) in left {
        *coeffs.entry(p).or_insert(0) += c;
    }
    for (p, c) in right {
        *coeffs.entry(p).or_insert(0) -= c;
    }
    coeffs.retain(|_, c| *c != 0);

    if coeffs.is_empty() {
        return Err(err(format!("expression references no places: {src:?}")));
    }

    Ok(LinearExpr { coeffs, rel, constant: right_const - left_const })
}

fn parse_side(toks: &[Token]) -> Result<(HashMap<String, i64>, i64), ParseError> {
    if toks.is_empty() {
        return Err(err("empty expression"));
    }

    let mut coeffs: HashMap<String, i64> = HashMap::new();
    let mut constant = 0i64;
    let mut sign = 1i64;
    let mut expect_term = true;

    let mut i = 0;
    while i < toks.len() {
        match &toks[i].kind {
            TokKind::Plus | TokKind::Minus => {
                let is_minus = matches!(toks[i].kind, TokKind::Minus);
                if expect_term {
                    if is_minus {
                        sign = -sign;
                    }
                } else {
                    sign = if is_minus { -1 } else { 1 };
                    expect_term = true;
                }
            }
            TokKind::Num => {
                let n: i64 = toks[i]
                    .text
                    .parse()
                    .map_err(|_| err(format!("bad number {:?}", toks[i].text)))?;
                if i + 2 < toks.len() && matches!(toks[i + 1].kind, TokKind::Star) && matches!(toks[i + 2].kind, TokKind::Ident)
                {
                    *coeffs.entry(toks[i + 2].text.clone()).or_insert(0) += sign * n;
                    i += 2;
                } else {
                    constant += sign * n;
                }
                sign = 1;
                expect_term = false;
            }
            TokKind::Ident => {
                if i + 2 < toks.len() && matches!(toks[i + 1].kind, TokKind::Star) && matches!(toks[i + 2].kind, TokKind::Num)
                {
                    let n: i64 = toks[i + 2]
                        .text
                        .parse()
                        .map_err(|_| err(format!("bad number {:?}", toks[i + 2].text)))?;
                    *coeffs.entry(toks[i].text.clone()).or_insert(0) += sign * n;
                    i += 2;
                } else {
                    *coeffs.entry(toks[i].text.clone()).or_insert(0) += sign;
                }
                sign = 1;
                expect_term = false;
            }
            TokKind::Star => return Err(err("unexpected '*'")),
            TokKind::Rel(_) => return Err(err(format!("unexpected token {:?}", toks[i].text))),
        }
        i += 1;
    }

    if expect_term {
        return Err(err("expression ends with a dangling operator"));
    }

    Ok((coeffs, constant))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokKind {
    Ident,
    Num,
    Plus,
    Minus,
    Star,
    Rel(Relation),
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokKind,
    text: String,
}

fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut toks = Vec::new();
    let runes: Vec<char> = src.chars().collect();
    let mut i = 0;

    while i < runes.len() {
        let c = runes[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '+' => {
                toks.push(Token { kind: TokKind::Plus, text: "+".into() });
                i += 1;
            }
            '-' => {
                toks.push(Token { kind: TokKind::Minus, text: "-".into() });
                i += 1;
            }
            '*' => {
                toks.push(Token { kind: TokKind::Star, text: "*".into() });
                i += 1;
            }
            '=' | '!' | '<' | '>' => {
                if i + 1 < runes.len() && runes[i + 1] == '=' {
                    let text: String = runes[i..i + 2].iter().collect();
                    let rel = match text.as_str() {
                        "==" => Relation::Eq,
                        "!=" => Relation::Ne,
                        "<=" => Relation::Le,
                        ">=" => Relation::Ge,
                        _ => unreachable!(),
                    };
                    toks.push(Token { kind: TokKind::Rel(rel), text });
                    i += 2;
                    continue;
                }
                if c == '!' {
                    return Err(err(format!("'!' must be followed by '=' at offset {i}")));
                }
                if c == '=' {
                    toks.push(Token { kind: TokKind::Rel(Relation::Eq), text: "=".into() });
                } else {
                    let rel = if c == '<' { Relation::Lt } else { Relation::Gt };
                    toks.push(Token { kind: TokKind::Rel(rel), text: c.to_string() });
                }
                i += 1;
            }
            '"' => {
                let mut j = i + 1;
                while j < runes.len() && runes[j] != '"' {
                    j += 1;
                }
                if j >= runes.len() {
                    return Err(err(format!("unterminated quoted name at offset {i}")));
                }
                let text: String = runes[i + 1..j].iter().collect();
                toks.push(Token { kind: TokKind::Ident, text });
                i = j + 1;
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while j < runes.len() && runes[j].is_ascii_digit() {
                    j += 1;
                }
                let text: String = runes[i..j].iter().collect();
                toks.push(Token { kind: TokKind::Num, text });
                i = j;
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut j = i;
                while j < runes.len() && is_ident_rune(runes[j]) {
                    j += 1;
                }
                let text: String = runes[i..j].iter().collect();
                toks.push(Token { kind: TokKind::Ident, text });
                i = j;
            }
            _ => return Err(err(format!("unexpected character {c:?} at offset {i}"))),
        }
    }

    if toks.is_empty() {
        return Err(err("empty expression"));
    }
    Ok(toks)
}

/// Deliberately excludes `-`: it would make `"a-b"` ambiguous between
/// subtraction and a place literally named `"a-b"`. Hyphenated place names
/// must be quoted (`"ERC-020" >= 1`).
fn is_ident_rune(r: char) -> bool {
    r.is_alphanumeric() || r == '_' || r == '.' || r == ':'
}

/// Renders a place name so the printed expression re-parses to the same
/// expression. A name that is not a bare identifier is quoted.
fn quote_name(name: &str) -> String {
    let mut bare = !name.is_empty();
    for (i, r) in name.chars().enumerate() {
        if i == 0 && !(r.is_alphabetic() || r == '_') {
            bare = false;
            break;
        }
        if !is_ident_rune(r) {
            bare = false;
            break;
        }
    }
    if bare {
        name.to_string()
    } else {
        format!("\"{name}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, i64)]) -> HashMap<String, i64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn parses_a_simple_sum() {
        let e = parse_expr("a + b == 10").unwrap();
        assert_eq!(e.rel, Relation::Eq);
        assert_eq!(e.constant, 10);
        assert_eq!(e.eval(&m(&[("a", 4), ("b", 6)])), 10);
        assert!(e.holds(&m(&[("a", 4), ("b", 6)])));
    }

    #[test]
    fn parses_weighted_terms_both_orders() {
        let e = parse_expr("2*boxes + loose == 12").unwrap();
        assert_eq!(e.coeffs["boxes"], 2);
        assert_eq!(e.coeffs["loose"], 1);

        let e2 = parse_expr("loose + boxes*2 == 12").unwrap();
        assert_eq!(e2.coeffs["boxes"], 2);
    }

    #[test]
    fn collects_places_on_both_sides_of_the_relation() {
        let e = parse_expr("minted == circulating + burned").unwrap();
        assert_eq!(e.coeffs["minted"], 1);
        assert_eq!(e.coeffs["circulating"], -1);
        assert_eq!(e.coeffs["burned"], -1);
        assert_eq!(e.constant, 0);
    }

    #[test]
    fn quoted_names_round_trip() {
        let e = parse_expr("\"my place\" >= 1").unwrap();
        assert_eq!(e.rel, Relation::Ge);
        assert_eq!(e.render_lhs(), "\"my place\"");
    }

    #[test]
    fn rejects_expression_with_no_relation() {
        assert!(parse_expr("a + b").is_err());
    }

    #[test]
    fn rejects_expression_with_no_places() {
        assert!(parse_expr("3 == 3").is_err());
    }

    #[test]
    fn double_negative_folds_to_positive() {
        let e = parse_expr("a - -b == 0").unwrap();
        assert_eq!(e.coeffs["b"], 1);
    }

    #[test]
    fn render_round_trips_through_parse() {
        let e = parse_expr("busy1 + busy2 <= 1").unwrap();
        let rendered = e.render();
        let e2 = parse_expr(&rendered).unwrap();
        assert_eq!(e.coeffs, e2.coeffs);
        assert_eq!(e.constant, e2.constant);
    }
}
