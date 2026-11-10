//! `verify`: hosted `pflow_verify` (`mcpVerify` over go-pflow's `verify`),
//! answering in `verify.Report`'s JSON shape key for key: each verdict echoes
//! its property as go-pflow's `Property` struct marshals (`kind`, then `name`,
//! `expr`, `target`, `places`, `bound` when set), `method` is `""` when no
//! search ran, and an absent trace is `null`.
//!
//! The checks are `pflow_verify::Verifier`'s, over the color-expanded net.
//! go-pflow lets a property name a colored place by its base name (the sum
//! over its colors); for `invariant`, `mutual-exclusion` and `conserves` the
//! expression is expanded here the way go-pflow's `expandExprColors` does,
//! and a `reachable`/`unreachable` target naming a base place, which needs a
//! summed match the Rust verifier does not make, is refused, naming the
//! hosted server.

use std::collections::BTreeMap;

use pflow_core::ColorMap;
use pflow_reachability::Analyzer;
use pflow_verify::{parse_expr, Kind, LinearExpr, Method, Property, Status, Verdict, Verifier};
use serde_json::{json, Map, Value};

use crate::net::{expand, int_map, model_of, net_arg, num_arg, petri_net, MAX_STATES};

const PROPS_ERR: &str = "properties: a non-empty list of {kind, …} (see the tool description)";

/// One property as go-pflow's `verify.Property` decodes it.
#[derive(Clone, Default)]
struct GoProp {
    kind: String,
    name: String,
    expr: String,
    target: BTreeMap<String, i64>,
    places: Vec<String>,
    bound: i64,
}

impl GoProp {
    /// `verify.Property` as `json.Marshal` writes it.
    fn json(&self) -> Value {
        let mut m = Map::new();
        m.insert("kind".into(), json!(self.kind));
        if !self.name.is_empty() {
            m.insert("name".into(), json!(self.name));
        }
        if !self.expr.is_empty() {
            m.insert("expr".into(), json!(self.expr));
        }
        if !self.target.is_empty() {
            m.insert("target".into(), int_map(&self.target));
        }
        if !self.places.is_empty() {
            m.insert("places".into(), json!(self.places));
        }
        if self.bound != 0 {
            m.insert("bound".into(), json!(self.bound));
        }
        Value::Object(m)
    }

    fn kind(&self) -> Option<Kind> {
        Some(match self.kind.as_str() {
            "deadlock-free" => Kind::DeadlockFree,
            "bounded" => Kind::Bounded,
            "live" => Kind::Live,
            "l4-live" => Kind::L4Live,
            "terminating" => Kind::Terminating,
            "reachable" => Kind::Reachable,
            "unreachable" => Kind::Unreachable,
            "invariant" => Kind::Invariant,
            "mutual-exclusion" => Kind::MutualExclusion,
            "conserves" => Kind::Conserves,
            _ => return None,
        })
    }
}

/// `json.Unmarshal` into `[]verify.Property`: objects (or `null`), each
/// field of its Go type or absent/null; anything else refuses the whole list.
fn parse_props(v: Option<&Value>) -> Result<Vec<GoProp>, String> {
    let Some(Value::Array(items)) = v else {
        return Err(PROPS_ERR.into());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let m = match item {
            Value::Null => {
                out.push(GoProp::default());
                continue;
            }
            Value::Object(m) => m,
            _ => return Err(PROPS_ERR.into()),
        };
        let mut p = GoProp::default();
        for (k, v) in m {
            if v.is_null() {
                continue;
            }
            match k.to_ascii_lowercase().as_str() {
                "kind" => p.kind = v.as_str().ok_or(PROPS_ERR)?.to_string(),
                "name" => p.name = v.as_str().ok_or(PROPS_ERR)?.to_string(),
                "expr" => p.expr = v.as_str().ok_or(PROPS_ERR)?.to_string(),
                "target" => {
                    for (place, n) in v.as_object().ok_or(PROPS_ERR)? {
                        p.target.insert(place.clone(), go_int(n).ok_or(PROPS_ERR)?);
                    }
                }
                "places" => {
                    p.places = v
                        .as_array()
                        .ok_or(PROPS_ERR)?
                        .iter()
                        .map(|s| s.as_str().map(String::from).ok_or(PROPS_ERR))
                        .collect::<Result<_, _>>()?
                }
                "bound" => p.bound = go_int(v).ok_or(PROPS_ERR)?,
                _ => {}
            }
        }
        out.push(p);
    }
    if out.is_empty() {
        return Err(PROPS_ERR.into());
    }
    Ok(out)
}

/// A JSON number Go decodes into an `int`.
fn go_int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| {
        v.as_f64()
            .filter(|f| f.fract() == 0.0 && f.abs() < 9.2e18)
            .map(|f| f as i64)
    })
}

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    crate::net::whole_weights(&doc)?;
    let max = num_arg(args, "max_states", 2000.0, 1.0, MAX_STATES)? as usize;
    let props = parse_props(args.get("properties"))?;

    // Local addition (pflow.xyz clients never send it, so the hosted answers stay byte-equal): with
    // `explain_truncation: true` a truncated answer also carries the analyzer's own reason it stopped.
    let explain = args.get("explain_truncation").and_then(Value::as_bool) == Some(true);
    let (expanded, cm) = expand(&petri_net(&doc));
    let model = model_of(&expanded);
    let verifier = Verifier::new(&model).with_max_states(max);

    let mut verdicts = Vec::with_capacity(props.len());
    let mut analyzed = false;
    let (mut proved, mut refuted, mut unknown) = (0, 0, 0);
    for gp in &props {
        let Some(kind) = gp.kind() else {
            unknown += 1;
            verdicts.push(json!({
                "property": gp.json(),
                "status": "unknown",
                "method": "",
                "detail": format!("unknown property kind {}", go_quote(&gp.kind)),
            }));
            continue;
        };
        let prop = to_rust(gp, kind, cm.as_ref())?;
        let verdict = verifier.check_one(prop);
        match verdict.status {
            Status::Proved => proved += 1,
            Status::Refuted => refuted += 1,
            Status::Unknown => unknown += 1,
        }
        analyzed |= matches!(
            kind,
            Kind::DeadlockFree | Kind::Live | Kind::L4Live | Kind::Terminating
        ) || matches!(verdict.method, Method::Exhaustive | Method::Partial);
        verdicts.push(verdict_json(gp, kind, &verdict, &model, max));
    }

    let (state_count, truncated, truncated_reason) = if analyzed {
        let r = Analyzer::new(&model).with_max_states(max).analyze();
        (r.state_count, r.truncated, r.truncate_msg)
    } else {
        (0, false, String::new())
    };
    let mut out = json!({
        "verdicts": verdicts,
        "proved": proved,
        "refuted": refuted,
        "unknown": unknown,
        "ok": refuted == 0 && unknown == 0,
        "state_count": state_count,
        "truncated": truncated,
    });
    if explain && truncated && !truncated_reason.is_empty() {
        out["truncated_reason"] = json!(truncated_reason);
    }
    Ok(out)
}

/// The property the Rust verifier checks: go-pflow's, with base color-place
/// names expanded as `expandExprColors` expands them.
fn to_rust(gp: &GoProp, kind: Kind, cm: Option<&ColorMap>) -> Result<Property, String> {
    let mut p = Property {
        kind: Some(kind),
        name: gp.name.clone(),
        expr: gp.expr.clone(),
        target: gp.target.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        places: gp.places.clone(),
        bound: gp.bound,
    };
    let Some(cm) = cm else {
        return Ok(p);
    };
    match kind {
        Kind::Reachable | Kind::Unreachable => {
            if let Some(base) = gp.target.keys().find(|k| cm.expanded.contains_key(*k)) {
                return Err(format!(
                    "target place {base:?} is a colored place's base name (the sum over its colors), which only the \
                     hosted server answers; call pflow_verify on https://pflow.xyz/mcp, or name its colors \
                     ({})",
                    cm.expanded[base].join(", ")
                ));
            }
        }
        Kind::MutualExclusion if !gp.places.is_empty() => {
            let bound = if gp.bound == 0 { 1 } else { gp.bound };
            let quoted: Vec<String> = gp.places.iter().map(|p| format!("\"{p}\"")).collect();
            p.kind = Some(Kind::Invariant);
            p.expr = expand_expr(&format!("{} <= {bound}", quoted.join(" + ")), cm);
        }
        Kind::Invariant => p.expr = expand_expr(&gp.expr, cm),
        _ => {}
    }
    Ok(p)
}

/// `expandExprColors` on expression text: a base place's coefficient spread
/// over its colors. Text that does not parse is left for the verifier to
/// refuse with its own message.
fn expand_expr(src: &str, cm: &ColorMap) -> String {
    let Ok(expr) = parse_expr(src) else {
        return src.to_string();
    };
    let mut coeffs = std::collections::HashMap::new();
    for (place, c) in &expr.coeffs {
        match cm.expanded.get(place) {
            Some(names) => {
                for n in names {
                    *coeffs.entry(n.clone()).or_insert(0) += c;
                }
            }
            None => *coeffs.entry(place.clone()).or_insert(0) += c,
        }
    }
    LinearExpr {
        coeffs,
        rel: expr.rel,
        constant: expr.constant,
    }
    .render()
}

fn method(m: Method) -> &'static str {
    match m {
        Method::Structural => "structural",
        Method::Exhaustive => "exhaustive",
        Method::Witness => "witness",
        Method::Partial => "partial",
        Method::None => "",
    }
}

fn verdict_json(
    gp: &GoProp,
    kind: Kind,
    v: &Verdict,
    model: &pflow_metamodel::Model,
    max: usize,
) -> Value {
    let mut m = Map::new();
    m.insert("property".into(), gp.json());
    m.insert(
        "status".into(),
        json!(match v.status {
            Status::Proved => "proved",
            Status::Refuted => "refuted",
            Status::Unknown => "unknown",
        }),
    );
    m.insert("method".into(), json!(method(v.method)));
    m.insert("detail".into(), json!(v.detail));
    if !v.evidence.is_empty() {
        m.insert("evidence".into(), json!(v.evidence));
    }
    if let Some(c) = &v.counterexample {
        let mut c = c.clone();
        if kind == Kind::Bounded && v.method == Method::Witness {
            c.explanation = witness_explanation(model, max).unwrap_or(c.explanation);
        }
        if kind == Kind::L4Live && v.status == Status::Refuted {
            // go-pflow reports the analysis' liveness witness: every
            // transition shut out of the region it reaches, where
            // pflow-verify names the first not-live transition's own.
            let r = Analyzer::new(model).with_max_states(max).analyze();
            if let Some(w) = r.liveness.witness {
                let place = if w.region_size > 1 {
                    format!(
                        "the net enters this closed region of {} markings",
                        w.region_size
                    )
                } else {
                    "this marking is reached".to_string()
                };
                c.explanation = format!(
                    "{} can never fire again once {place}",
                    w.never_fires.join(", ")
                );
                c.trace = w.trace;
                c.marking = w.marking.into_iter().collect();
            }
        }
        let trace = if c.trace.is_empty() {
            Value::Null
        } else {
            json!(c.trace)
        };
        let marking: BTreeMap<&String, &i64> = c.marking.iter().collect();
        m.insert(
            "counterexample".into(),
            json!({"trace": trace, "marking": marking, "explanation": c.explanation}),
        );
    }
    Value::Object(m)
}

/// go-pflow's unboundedness explanation, with the covered marking printed as
/// Go prints a `map[string]int` (`map[a:1 b:0]`).
fn witness_explanation(model: &pflow_metamodel::Model, max: usize) -> Option<String> {
    let w = Analyzer::new(model)
        .with_max_states(max)
        .find_unbounded_witness()?;
    let from: BTreeMap<&String, &i64> = w.from.iter().collect();
    let from = from
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!(
        "after the prefix [{}], repeating [{}] returns to a marking that strictly covers map[{from}], adding tokens to {} each time — so the count is unbounded",
        format_trace(&w.prefix),
        format_trace(&w.pump),
        w.places.join(", ")
    ))
}

fn format_trace(trace: &[String]) -> String {
    if trace.is_empty() {
        "(already satisfied at the initial marking)".to_string()
    } else {
        trace.join(" → ")
    }
}

/// Go's `%q`.
fn go_quote(s: &str) -> String {
    format!("{s:?}")
}
