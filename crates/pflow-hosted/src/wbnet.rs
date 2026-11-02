//! What the workbench tools (`sde`, `what_if`) share: the net as the
//! stochastic engines see it, the arguments read with pflow-native's rules,
//! and the answer fitted to the hosted result budget.
//!
//! Ported from pflow-xyz `cmd/webserver/mcp_workbench_engine.go` (itself a
//! line-by-line port of pflow-native-core's `stochastic.rs`,
//! `commands/stochastic.rs` and `sim.rs`), with pflow-rs's own engines
//! underneath: the parser is the one native uses, the SDE and the scenario
//! SSA are the ones native calls.
//!
//! Like the hosted server, places are listed in id order (the document place,
//! then the colour), not in the document's order: the order of a JSON
//! object's members is not something a caller can rely on surviving. That
//! changes the order of series, table columns and the place list in two
//! messages, never a number.

use std::collections::{BTreeMap, HashMap};

use pflow_core::colors::{short_color_names, ColorMap};
use pflow_metamodel::Model;
use serde_json::{Map, Value};

// ---- caps: pflow-native's, then the hosted server's tighter ones ----

/// Realizations per call (native's `MAX_RUNS`).
pub(crate) const MAX_RUNS: usize = 200;
/// Model time (native's `MAX_HORIZON`).
pub(crate) const MAX_HORIZON: f64 = 10_000.0;
/// Tokens in one place / across the net a run may start from.
pub(crate) const MAX_PLACE_TOKENS: i64 = 1_000_000;
pub(crate) const MAX_TOTAL_TOKENS: i64 = 4_000_000;
/// Token places (colours unfolded) and transitions the hosted tools take.
pub(crate) const MAX_PLACES: usize = 300;
pub(crate) const MAX_TRANSITIONS: usize = 300;
/// The hosted result guard (`mcpMaxResultBytes`, 60 KB): every answer fits
/// it whole.
pub(crate) const MAX_RESULT_BYTES: i64 = 60 << 10;

// ---- formatting, as pflow-native's messages print values ----

/// A float as Rust's Display prints it (shortest round-trip, no exponent):
/// what native's messages show and the hosted server imitates.
pub(crate) fn rust_float(v: f64) -> String {
    format!("{v}")
}

/// Strings as Rust's `{:?}` of a `Vec<String>`: `["a", "b"]`.
pub(crate) fn rust_list(xs: &[String]) -> String {
    format!("{xs:?}")
}

/// Rust's `f as i64` (saturating, NaN as 0).
pub(crate) fn rust_i64(f: f64) -> i64 {
    f as i64
}

// ---- the answer's size, as the hosted server measures it ----

/// The JSON text Go's `encoding/json` writes for `v`. The hosted server
/// measures its answers in that text, so fitting the answer here measures it
/// the same way (a float as `3`, not `3.0`; `<` as `<`).
pub(crate) fn go_json(v: &Value) -> String {
    let mut out = String::new();
    write_go_json(v, &mut out);
    out
}

/// The bytes `v` takes as Go writes it.
pub(crate) fn go_json_len(v: &Value) -> i64 {
    go_json(v).len() as i64
}

/// The bytes a string takes as a JSON string, as Go writes it.
pub(crate) fn go_str_len(s: &str) -> i64 {
    let mut out = String::new();
    write_go_str(s, &mut out);
    out.len() as i64
}

fn write_go_json(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else {
                out.push_str(&go_float(n.as_f64().unwrap_or(0.0)));
            }
        }
        Value::String(s) => write_go_str(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, e) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_go_json(e, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, e)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_go_str(k, out);
                out.push(':');
                write_go_json(e, out);
            }
            out.push('}');
        }
    }
}

/// A float64 as `encoding/json` writes it: shortest round-trip digits, in
/// exponent form below 1e-6 or from 1e21 (`1e-7`, `1e+21`).
fn go_float(f: f64) -> String {
    let a = f.abs();
    if a != 0.0 && !(1e-6..1e21).contains(&a) {
        // Rust's {:e} is `1.5e-7` / `1e21`; Go's `1.5e-7` / `1e+21`.
        let s = format!("{f:e}");
        match s.find('e') {
            Some(i) if !s[i + 1..].starts_with('-') => format!("{}e+{}", &s[..i], &s[i + 1..]),
            _ => s,
        }
    } else {
        format!("{f}")
    }
}

fn write_go_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A number for the answer: a non-finite value is null, as native writes it.
pub(crate) fn num(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub(crate) fn nums(v: &[f64]) -> Value {
    Value::Array(v.iter().map(|&x| num(x)).collect())
}

// ---- arguments, read with pflow-native's rules ----

/// What a tool argument must be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgKind {
    Any,
    Num,
    Int,
    Str,
    StrList,
}

impl ArgKind {
    fn name(self) -> &'static str {
        match self {
            ArgKind::Any => "anything",
            ArgKind::Num => "a number",
            ArgKind::Int => "an integer",
            ArgKind::Str => "a string",
            ArgKind::StrList => "a list of strings",
        }
    }

    fn admits(self, v: &Value) -> bool {
        match self {
            ArgKind::Any => true,
            ArgKind::Num => v.is_number(),
            ArgKind::Int => v.as_f64().is_some_and(|f| f.is_finite() && f == f.trunc()),
            ArgKind::Str => v.is_string(),
            ArgKind::StrList => v.as_array().is_some_and(|l| l.iter().all(Value::is_string)),
        }
    }
}

/// A tool call's arguments. An absent key and null are the same.
pub(crate) struct Args<'a>(pub &'a Map<String, Value>);

impl<'a> Args<'a> {
    /// Refuses an argument the tool does not take, or one of the wrong type
    /// (native's `CommandSpec::check_args`), before anything runs.
    pub fn check(tool: &str, args: &'a Value, kinds: &[(&str, ArgKind)]) -> Result<Self, String> {
        let a = args
            .as_object()
            .ok_or_else(|| format!("{tool}: the arguments must be a JSON object"))?;
        let mut keys: Vec<&String> = a.keys().collect();
        keys.sort();
        for k in keys {
            let Some(&(_, kind)) = kinds.iter().find(|(n, _)| n == k) else {
                let mut takes: Vec<String> = kinds.iter().map(|(n, _)| n.to_string()).collect();
                takes.sort();
                return Err(format!(
                    "{tool}: unknown argument {k:?}; takes {}",
                    rust_list(&takes)
                ));
            };
            let v = &a[k];
            if v.is_null() || kind.admits(v) {
                continue;
            }
            return Err(format!(
                "{tool}: argument {k:?} should be {}, got {}",
                kind.name(),
                go_json(v)
            ));
        }
        Ok(Args(a))
    }

    pub fn get(&self, k: &str) -> Option<&'a Value> {
        self.0.get(k).filter(|v| !v.is_null())
    }

    pub fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }

    pub fn num(&self, k: &str) -> Option<f64> {
        self.get(k).and_then(Value::as_f64)
    }

    pub fn str(&self, k: &str) -> Option<&'a str> {
        self.get(k).and_then(Value::as_str)
    }

    /// Native's `uint`: absent is the default, negative an error.
    pub fn uint(&self, k: &str, def: usize) -> Result<usize, String> {
        match self.num(k) {
            None => Ok(def),
            Some(x) if x < 0.0 => Err(format!("{k} must be ≥ 0, got {}", rust_float(x))),
            // Over every cap; the caller's range check says so.
            Some(x) if x > i32::MAX as f64 => Ok(i32::MAX as usize),
            Some(x) => Ok(x as usize),
        }
    }

    /// Native's `parse_seed`: a non-negative integer, at most 2^53.
    pub fn seed(&self, k: &str, def: u64) -> Result<u64, String> {
        let Some(v) = self.get(k) else {
            return Ok(def);
        };
        let x = match v.as_f64() {
            Some(x) if x.is_finite() => x,
            _ => return Err(format!("{k} must be a finite number, got {}", go_json(v))),
        };
        if x != x.trunc() {
            return Err(format!("{k} must be an integer, got {}", go_json(v)));
        }
        if x < 0.0 {
            return Err(format!("{k} must be ≥ 0, got {}", rust_i64(x)));
        }
        if x > (1u64 << 53) as f64 {
            return Err(format!("{k} must be at most {}", 1u64 << 53));
        }
        Ok(x as u64)
    }

    /// The `places` filter: `None` without one.
    pub fn strings(&self, k: &str) -> Option<Vec<String>> {
        self.get(k).and_then(Value::as_array).map(|l| {
            l.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
    }
}

pub(crate) fn check_runs(what: &str, n: usize) -> Result<(), String> {
    if !(1..=MAX_RUNS).contains(&n) {
        return Err(format!("{what} {n} is outside 1..{MAX_RUNS}"));
    }
    Ok(())
}

pub(crate) fn check_samples(what: &str, n: usize, max: usize) -> Result<(), String> {
    if !(2..=max).contains(&n) {
        return Err(format!("{what} {n} is outside 2..{max}"));
    }
    Ok(())
}

pub(crate) fn check_horizon(what: &str, h: f64, max: f64) -> Result<(), String> {
    if !(h > 0.0 && h <= max) {
        return Err(format!(
            "{what} {} is outside (0, {}]",
            rust_float(h),
            rust_float(max)
        ));
    }
    Ok(())
}

/// Native's `check_tokens`: a starting marking over the caps is refused. The
/// per-place cap guards the scenario engine's ledger.
pub(crate) fn check_tokens(start: &HashMap<String, i64>, per_place: bool) -> Result<(), String> {
    let mut keys: Vec<&String> = start.keys().collect();
    keys.sort();
    let mut total = 0i64;
    for k in keys {
        let v = start[k];
        if v < 0 {
            return Err(format!("marking for {k:?} is negative ({v})"));
        }
        if per_place && v > MAX_PLACE_TOKENS {
            return Err(format!(
                "marking for {k:?} is {v}, over the {MAX_PLACE_TOKENS} cap"
            ));
        }
        total = total.saturating_add(v);
    }
    if total > MAX_TOTAL_TOKENS {
        return Err(format!(
            "marking totals {total} tokens, over the {MAX_TOTAL_TOKENS} cap"
        ));
    }
    Ok(())
}

// ---- the net, as native's stochastic engines see it ----

/// Native's `stochastic::Net`: the editor document compiled by pflow-parser
/// (colours unfolded, output-side inhibitors as read arcs), its token places
/// in id order with display names (`place`, or `place.colour`), and the
/// document's `rate` fields.
pub(crate) struct WbNet {
    pub doc: Map<String, Value>,
    pub model: Model,
    colors: Option<ColorMap>,
    short: Vec<String>,
    /// Token places (model ids), id order.
    pub places: Vec<String>,
    /// Display names, aligned with `places`.
    pub display: Vec<String>,
    /// The document's `rate` fields; absent means 1.
    rates: HashMap<String, f64>,
}

impl WbNet {
    /// Reads the `net` argument and caps its size.
    pub fn from_args(args: &Args) -> Result<Self, String> {
        let doc = args
            .get("net")
            .and_then(Value::as_object)
            .ok_or("net: a JSON object (the editor's format) is required")?;
        let n = Self::new(doc.clone())?;
        n.check_size()?;
        Ok(n)
    }

    pub fn new(doc: Map<String, Value>) -> Result<Self, String> {
        let raw = serde_json::to_vec(&Value::Object(executable(&doc)))
            .map_err(|e| format!("cannot read the net: {e}"))?;
        let (model, colors) = pflow_parser::model_from_json(&raw)
            .map_err(|e| format!("pflow-rs cannot read the net: {e}"))?;
        let short = colors
            .as_ref()
            .map(|cm| short_color_names(&cm.colors))
            .unwrap_or_default();
        let mut n = WbNet {
            doc,
            model,
            colors,
            short,
            places: vec![],
            display: vec![],
            rates: HashMap::new(),
        };
        let mut keys: Vec<(String, usize, String)> = n
            .model
            .places
            .iter()
            .filter(|p| p.is_token())
            .map(|p| {
                let (b, c) = n.base(&p.id);
                (b, c, p.id.clone())
            })
            .collect();
        keys.sort();
        n.places = keys.into_iter().map(|(_, _, id)| id).collect();
        n.display = n.places.iter().map(|id| n.display_of(id)).collect();
        n.rates = rates_of(&n.doc);
        Ok(n)
    }

    /// The hosted cap on the net, whatever the arguments.
    fn check_size(&self) -> Result<(), String> {
        if self.places.len() > MAX_PLACES {
            return Err(format!(
                "the net has {} token places (colours unfolded), over this server's cap of {MAX_PLACES}",
                self.places.len()
            ));
        }
        if self.model.transitions.len() > MAX_TRANSITIONS {
            return Err(format!(
                "the net has {} transitions, over this server's cap of {MAX_TRANSITIONS}",
                self.model.transitions.len()
            ));
        }
        Ok(())
    }

    /// The document place and colour index of a model place.
    pub fn base(&self, id: &str) -> (String, usize) {
        match self.colors.as_ref().and_then(|cm| cm.base.get(id)) {
            Some(r) => (r.place.clone(), r.color),
            None => (id.to_string(), 0),
        }
    }

    pub fn display_of(&self, id: &str) -> String {
        match self.colors.as_ref().and_then(|cm| cm.base.get(id)) {
            Some(r) => {
                let c = self
                    .short
                    .get(r.color)
                    .cloned()
                    .unwrap_or_else(|| r.color.to_string());
                format!("{}.{c}", r.place)
            }
            None => id.to_string(),
        }
    }

    /// Native's `Net::resolve_place`: a model id, a display name, or a
    /// document place that unfolded to exactly one token place.
    pub fn resolve_place(&self, key: &str) -> Result<String, String> {
        if let Some(i) =
            (0..self.places.len()).find(|&i| self.places[i] == key || self.display[i] == key)
        {
            return Ok(self.places[i].clone());
        }
        let copies: Vec<&String> = self
            .places
            .iter()
            .filter(|p| self.base(p).0 == key)
            .collect();
        match copies.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(format!(
                "no token place {key:?}; places are {}",
                rust_list(&self.display)
            )),
            _ => Err(format!(
                "place {key:?} holds several colors; name one of {}",
                rust_list(
                    &copies
                        .iter()
                        .map(|p| self.display_of(p))
                        .collect::<Vec<_>>()
                )
            )),
        }
    }

    /// The net's marking over the token places, as native reads it: the
    /// document's count for the place's colour, rounded.
    pub fn start(&self) -> HashMap<String, i64> {
        let places = self.doc.get("places").and_then(Value::as_object);
        self.places
            .iter()
            .map(|id| {
                let (b, c) = self.base(id);
                let v = match places
                    .and_then(|p| p.get(&b))
                    .and_then(|p| p.get("initial"))
                {
                    Some(Value::Array(x)) => x.get(c).and_then(Value::as_f64).unwrap_or(0.0),
                    Some(Value::Number(x)) if c == 0 => x.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                (id.clone(), v.round() as i64)
            })
            .collect()
    }

    /// The document's rates (what the scenario engine is handed).
    pub fn rates(&self) -> &HashMap<String, f64> {
        &self.rates
    }

    pub fn rate(&self, t: &str) -> f64 {
        self.rates.get(t).copied().unwrap_or(1.0)
    }

    /// Which places a `places` filter keeps (a display name, or a document
    /// place for all its colours), aligned with `places`; all without one.
    pub fn series_filter(&self, filter: Option<&[String]>) -> Result<Vec<bool>, String> {
        let Some(filter) = filter else {
            return Ok(vec![true; self.places.len()]);
        };
        let mut keep = vec![false; self.places.len()];
        for f in filter {
            let mut found = false;
            for (i, p) in self.places.iter().enumerate() {
                if &self.display[i] == f || &self.base(p).0 == f {
                    keep[i] = true;
                    found = true;
                }
            }
            if !found {
                return Err(format!(
                    "places: no place {f:?}; places are {}",
                    rust_list(&self.display)
                ));
            }
        }
        Ok(keep)
    }

    /// The series (objects with a `name`) of the places `keep` names.
    pub fn select_series(&self, series: Vec<Value>, keep: &[bool]) -> Vec<Value> {
        let kept: Vec<&String> = self
            .display
            .iter()
            .zip(keep)
            .filter(|(_, k)| **k)
            .map(|(d, _)| d)
            .collect();
        series
            .into_iter()
            .filter(|s| {
                s.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| kept.iter().any(|d| d.as_str() == n))
            })
            .collect()
    }
}

/// Native's `sim::executable`: an arc whose weights are all zero moves and
/// tests nothing in pflow.xyz, but the engines read weight 0 as "unset" (1),
/// so it is dropped. Inhibitor arcs are kept as written.
fn executable(doc: &Map<String, Value>) -> Map<String, Value> {
    let inert = |a: &Value| -> bool {
        let Some(m) = a.as_object() else {
            return false;
        };
        if m.get("inhibitTransition") == Some(&Value::Bool(true)) {
            return false;
        }
        match m.get("weight") {
            Some(Value::Number(w)) => w.as_f64() == Some(0.0),
            Some(Value::Array(w)) => {
                !w.is_empty() && w.iter().all(|x| x.as_f64().unwrap_or(0.0) == 0.0)
            }
            _ => false,
        }
    };
    let mut out = doc.clone();
    if let Some(Value::Array(arcs)) = out.get_mut("arcs") {
        arcs.retain(|a| !inert(a));
    }
    out
}

/// Each transition's `rate` from the editor's format (a finite number ≥ 0).
fn rates_of(doc: &Map<String, Value>) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    if let Some(ts) = doc.get("transitions").and_then(Value::as_object) {
        for (id, t) in ts {
            if let Some(r) = t.get("rate").and_then(Value::as_f64) {
                if r >= 0.0 && r.is_finite() {
                    out.insert(id.clone(), r);
                }
            }
        }
    }
    out
}

/// A map keyed by name, its keys sorted, its values numbers.
pub(crate) fn sorted_nums<K: AsRef<str>>(m: impl IntoIterator<Item = (K, f64)>) -> Value {
    let b: BTreeMap<String, f64> = m.into_iter().map(|(k, v)| (k.as_ref().into(), v)).collect();
    Value::Object(b.into_iter().map(|(k, v)| (k, num(v))).collect())
}

// ---- downsampling (native's `sim::downsample`) ----

/// Indices thinning `n` samples to at most `max` points, keeping the last.
pub(crate) fn downsample(n: usize, max: usize) -> Vec<usize> {
    let stride = n.div_ceil(max.max(2)).max(1);
    let mut idx: Vec<usize> = (0..n).step_by(stride).collect();
    if n > 0 && idx.last() != Some(&(n - 1)) {
        idx.push(n - 1);
    }
    idx
}

/// `downsample(n, most).len()`, without the vector.
pub(crate) fn downsample_len(n: usize, most: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let stride = n.div_ceil(most.max(2)).max(1);
    let mut k = n.div_ceil(stride);
    if (k - 1) * stride != n - 1 {
        k += 1;
    }
    k
}

pub(crate) fn pick(v: &[f64], idx: &[usize]) -> Vec<f64> {
    idx.iter().map(|&i| v[i]).collect()
}

pub(crate) fn last(v: &[f64]) -> f64 {
    v.last().copied().unwrap_or(0.0)
}

// ---- fitting the answer to the result budget ----

/// The most bytes one number takes in an answer, with its separator: Go
/// writes a float64 in at most 25 characters (-0.0000012345678901234567).
/// Upper bounds on an answer's size count every number at that length, lower
/// bounds at two (`0,`).
pub(crate) const NUM_BYTES: i64 = 26;

/// How a tool fits its answer's count of points (or samples) to the result
/// budget.
pub(crate) struct Sizing<'a> {
    /// The tool, as refusals name it.
    pub tool: &'a str,
    /// "points" or "samples".
    pub what: &'a str,
    pub def: i64,
    pub min: i64,
    /// Whether the caller chose the count.
    pub explicit: bool,
    pub budget: i64,
    /// Bound the answer's bytes with count n: every number at its longest or
    /// its shortest.
    pub upper: Box<dyn Fn(i64) -> i64 + 'a>,
    pub lower: Box<dyn Fn(i64) -> i64 + 'a>,
    /// How many points the answer's arrays hold with count n.
    pub grid: Box<dyn Fn(i64) -> i64 + 'a>,
    /// What else makes the answer smaller.
    pub hint: &'a str,
}

/// The hosted `fitAnswer`: builds the answer with count `req`, or a smaller
/// one, so that it fits whole. When the upper bound says it fits it is built
/// once. A count the caller chose that the lower bound says cannot fit is
/// refused before anything runs, naming a count that fits for certain; in
/// between the answer is built and measured, and refused if it does not fit,
/// naming the count its measured size says would. The default count is
/// lowered instead, and the answer says so in `note`. It is never cut.
pub(crate) fn fit_answer(
    s: &Sizing,
    req: i64,
    mut build: impl FnMut(i64) -> Result<Map<String, Value>, String>,
) -> Result<Value, String> {
    let with_note = |out: &mut Map<String, Value>, k: i64| {
        if k != req {
            out.insert(
                "note".into(),
                Value::String(format!(
                    "{} is {k}, not the default {}, so that the answer fits this server's {}-byte limit ({})",
                    s.what, s.def, s.budget, s.hint
                )),
            );
        }
    };
    let finish = |mut out: Map<String, Value>, k: i64| -> Result<Value, String> {
        with_note(&mut out, k);
        let out = Value::Object(out);
        let b = go_json_len(&out);
        if b > s.budget {
            return Err(format!(
                "{}: the answer came to {b} bytes, over this server's {}-byte limit; {}",
                s.tool, s.budget, s.hint
            ));
        }
        Ok(out)
    };
    let too_big = |k: i64| {
        format!(
            "the answer would pass this server's {}-byte limit even at {k} {}; {}",
            s.budget, s.what, s.hint
        )
    };
    let mut sure = req; // the most the upper bound says fit
    while sure >= s.min && (s.upper)(sure) > s.budget {
        sure -= 1;
    }
    if sure == req {
        let out = build(req)?;
        return finish(out, req);
    }
    let mut k = req;
    if (s.lower)(k) > s.budget {
        if s.explicit {
            if sure >= s.min {
                return Err(format!(
                    "{} {req} would make an answer of at least {} bytes, over this server's {}-byte limit; {sure} fit for certain, or {}",
                    s.what, (s.lower)(req), s.budget, s.hint
                ));
            }
            return Err(format!(
                "{} {req} would make an answer of at least {} bytes, over this server's {}-byte limit; {}",
                s.what, (s.lower)(req), s.budget, s.hint
            ));
        }
        while k > s.min && (s.lower)(k) > s.budget {
            k -= 1;
        }
        if (s.lower)(k) > s.budget {
            return Err(too_big(k));
        }
    }
    let mut tries = 0;
    while tries < 3 && k > sure {
        tries += 1;
        let mut out = build(k)?;
        with_note(&mut out, k);
        let size = go_json_len(&Value::Object(out.clone()));
        if size <= s.budget {
            return finish(out, k);
        }
        // What the arrays that grow with the count take, per point, says how
        // many points would fit next to the rest.
        let fixed = go_json_len(&without_points(&out));
        let mut fit = k - 1;
        let points = (s.grid)(k);
        if points > 0 && size > fixed {
            let most =
                ((s.budget - fixed) as f64 / ((size - fixed) as f64 / points as f64) * 0.98) as i64;
            while fit >= s.min && (s.grid)(fit) > most {
                fit -= 1;
            }
        }
        if s.explicit {
            if fit >= s.min {
                return Err(format!(
                    "{} {req} makes an answer of {size} bytes, over this server's {}-byte limit; about {fit} fit, or {}",
                    s.what, s.budget, s.hint
                ));
            }
            return Err(format!(
                "{} {req} makes an answer of {size} bytes, over this server's {}-byte limit; {}",
                s.what, s.budget, s.hint
            ));
        }
        if fit < s.min {
            return Err(too_big(s.min));
        }
        k = fit;
    }
    if sure < s.min {
        return Err(too_big(s.min));
    }
    let out = build(sure)?;
    finish(out, sure)
}

/// An answer with its time series' arrays emptied: what the answer takes
/// whatever its count of points.
fn without_points(out: &Map<String, Value>) -> Value {
    let mut c = out.clone();
    if let Some(Value::Object(ts)) = c.get_mut("timeseries") {
        ts.insert("x".into(), Value::Array(vec![]));
        if let Some(Value::Array(series)) = ts.get_mut("series") {
            for s in series.iter_mut() {
                if let Value::Object(m) = s {
                    m.insert("values".into(), Value::Array(vec![]));
                    if m.contains_key("band") {
                        m.insert("band".into(), Value::Array(vec![]));
                    }
                }
            }
        }
    }
    Value::Object(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn go_float_text() {
        for (f, want) in [
            (3.0, "3"),
            (0.1, "0.1"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (1e-10, "1e-10"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (0.000001, "0.000001"),
            (-2.5e300, "-2.5e+300"),
            (0.30000000000000004, "0.30000000000000004"),
        ] {
            assert_eq!(go_float(f), want, "{f}");
        }
    }

    #[test]
    fn go_json_text() {
        let v = json!({"b": [1, 2.0, null, true], "a": "x<y&\"z\"\n"});
        // Go escapes <, > and & as a backslash, u and four hex digits.
        let esc = |hex: &str| ["\\", "u", hex].concat();
        let want = [
            r#"{"a":"x"#,
            &esc("003c"),
            "y",
            &esc("0026"),
            r#"\"z\"\n","b":[1,2,null,true]}"#,
        ]
        .concat();
        assert_eq!(go_json(&v), want);
    }

    #[test]
    fn downsample_len_matches() {
        for n in 0..300 {
            for m in 0..120 {
                assert_eq!(downsample_len(n, m), downsample(n, m).len(), "{n} {m}");
            }
        }
    }
}
