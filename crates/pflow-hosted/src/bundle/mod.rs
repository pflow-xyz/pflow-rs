//! The composition tools: `bundle_example`, `bundle_validate`,
//! `bundle_add_link`, `bundle_candidates`, `bundle_flatten`, `bundle_run`
//! and `bundle_incidence` (hosted `pflow_bundle_*`, pflow-xyz
//! `cmd/webserver/mcp_model*.go`).
//!
//! A composition is a PetriNetBundle: independently authored subnets, the
//! ports on their boundaries and the typed links between them. The hosted
//! tools are the composition page's `bundle.*` commands (pflow-native) on
//! go-pflow; here they run on what pflow-native itself runs on,
//! pflow-compose's [`pflow_compose::Bundle`], its validation and its
//! flattening, with the hosted tools' arguments, limits, refusals and answer
//! shapes. They are held to the same pflow-native goldens the hosted tools
//! are (`tests/bundle_parity.rs`).
//!
//! Every tool reads its bundle the same way, through [`read::bundle_arg`].
//! Each is stateless: an edit (`bundle_add_link`) returns the changed links,
//! nothing is stored. A refusal is the hosted one: the hosted tool's name in
//! front (`pflow_bundle_add_link: …`), the words pflow-native would say, and,
//! for a port that is not there, a hint behind (`. subnet "x" has the ports
//! …`).

mod catalog;
mod read;
mod rules;
mod text;

use serde_json::{json, Map, Value};

use read::Kind;

pub const BUNDLE_TYPE: &str = "PetriNetBundle";

pub const MAX_SUBNETS: usize = 32;
/// Across the bundle.
pub const MAX_PLACES: usize = 1000;
pub const MAX_TRANSITIONS: usize = 1000;
pub const MAX_ARCS: usize = 4000;
pub const MAX_PORTS: usize = 512;
pub const MAX_LINKS: usize = 200;
pub const MAX_CONSTRAINTS: usize = 200;
pub const MAX_SCRIPT: usize = 1000;
pub const MAX_EXPECT: usize = 100;
/// Incidence: flat places x flat transitions, plus each subnet's own.
pub const MAX_MATRIX_CELLS: usize = 50000;
/// What a flat net may weigh in an answer.
pub const MAX_ANSWER_BYTES: usize = 2 << 20;
/// The hosted server's cut on any result, in bytes (named in a refusal).
const MAX_RESULT_BYTES: usize = 60 << 10;

const BUNDLE_ONLY: &[(&str, Kind)] = &[("bundle", Kind::Any)];
const EXAMPLE_KINDS: &[(&str, Kind)] = &[("instances", Kind::StrList), ("links", Kind::Any)];
const ADD_LINK_KINDS: &[(&str, Kind)] = &[
    ("bundle", Kind::Any),
    ("kind", Kind::Str),
    ("from", Kind::Any),
    ("to", Kind::Any),
];
const CANDIDATE_KINDS: &[(&str, Kind)] = &[
    ("bundle", Kind::Any),
    ("from", Kind::Any),
    ("prefer", Kind::Str),
    ("only_ok", Kind::Bool),
];
const RUN_KINDS: &[(&str, Kind)] = &[
    ("bundle", Kind::Any),
    ("scenario", Kind::Str),
    ("script", Kind::StrList),
    ("expect", Kind::Any),
];

/// Runs one of the composition tools (by its local name).
pub fn call(tool: &str, args: &Value) -> Result<Value, String> {
    let run: fn(&Map<String, Value>) -> Result<Value, String> = match tool {
        "bundle_example" => example,
        "bundle_validate" => validate,
        "bundle_add_link" => add_link,
        "bundle_candidates" => candidates,
        "bundle_flatten" => flatten,
        "bundle_run" => run,
        "bundle_incidence" => incidence,
        _ => return Err(format!("unknown tool {tool:?}")),
    };
    let hosted = format!("pflow_{tool}");
    read::object(&hosted, args).and_then(run)
}

/// A refusal in native's words, named for the tool.
fn refuse(tool: &str, native: impl std::fmt::Display) -> String {
    format!("{tool}: {native}")
}

/// … with the tool's hint behind it.
fn refuse_hinted(tool: &str, native: &str, hint: &str) -> String {
    if hint.is_empty() {
        refuse(tool, native)
    } else {
        format!("{tool}: {native}. {hint}")
    }
}

fn issues(v: &[pflow_compose::ValidationError]) -> Vec<Value> {
    v.iter()
        .map(|e| json!({"code": e.code, "message": e.message, "element": e.element}))
        .collect()
}

/// The links as the bundle document spells them (pflow-compose's serde).
fn links_json(ls: &[pflow_compose::Link]) -> Value {
    serde_json::to_value(ls).unwrap_or_else(|_| json!([]))
}

// ------------------------------------------------------------ the tools

fn validate(a: &Map<String, Value>) -> Result<Value, String> {
    let b = read::bundle_arg("pflow_bundle_validate", a, BUNDLE_ONLY)?;
    let res = b.validate();
    Ok(
        json!({"valid": res.valid, "errors": issues(&res.errors), "warnings": issues(&res.warnings)}),
    )
}

fn flatten(a: &Map<String, Value>) -> Result<Value, String> {
    const TOOL: &str = "pflow_bundle_flatten";
    let b = read::bundle_arg(TOOL, a, BUNDLE_ONLY)?;
    let f = match rules::flatten(&b) {
        Ok(f) => f,
        Err(e) => return Ok(json!({"ok": false, "error": e})),
    };
    f.check_answer(TOOL)?;
    Ok(f.view())
}

fn add_link(a: &Map<String, Value>) -> Result<Value, String> {
    const TOOL: &str = "pflow_bundle_add_link";
    let b = read::bundle_arg(TOOL, a, ADD_LINK_KINDS)?;
    for k in ["kind", "from", "to"] {
        if a.get(k).is_none_or(Value::is_null) {
            return Err(format!("{TOOL}: missing argument {}", text::quote(k)));
        }
    }
    let l = rules::parse_link(&json!({"kind": a["kind"], "from": a["from"], "to": a["to"]}))
        .map_err(|e| refuse(TOOL, e))?;
    let v = rules::verdict_in(&b, &l);
    if !v.ok {
        // a port that is not there: say which ones are
        let mut hint = String::new();
        for e in [&l.from, &l.to] {
            if let Some(s) = b.subnet_by_id(&e.subnet) {
                let ports = read::effective_ports(s);
                if !e.port.is_empty() && !ports.iter().any(|p| p.id == e.port) {
                    hint = read::ports_hint(s, &ports, true);
                    break;
                }
            }
        }
        return Err(refuse_hinted(TOOL, &v.refusal(), &hint));
    }
    if b.links.len() >= MAX_LINKS {
        return Err(format!(
            "{TOOL}: the composition has {} links; at most {MAX_LINKS}",
            b.links.len()
        ));
    }
    let mut links = b.links.clone();
    links.push(l.clone());
    Ok(json!({
        "index": b.links.len(),
        "link": rules::link_view(&l),
        "key": rules::link_key(&l),
        "verdict": v.view(),
        "links": links_json(&links),
    }))
}

fn candidates(a: &Map<String, Value>) -> Result<Value, String> {
    const TOOL: &str = "pflow_bundle_candidates";
    // only declared ports count here: no derived ones
    let b = read::bundle_arg(TOOL, a, CANDIDATE_KINDS)?;
    let Some(from) = a.get("from").filter(|v| !v.is_null()) else {
        return Err(format!("{TOOL}: missing argument \"from\""));
    };
    let from = rules::parse_endpoint(from).map_err(|e| refuse(TOOL, e))?;
    let prefer = a.get("prefer").and_then(Value::as_str).unwrap_or("auto");
    let only_ok = a.get("only_ok").and_then(Value::as_bool).unwrap_or(false);
    let mut list = rules::candidates(&b, &from, prefer)
        .map_err(|h| refuse_hinted(TOOL, &h.native, &h.hint))?;
    let is_ok = |c: &Value| c["ok"] == json!(true);
    let lit = list.iter().filter(|c| is_ok(c)).count();
    if only_ok {
        list.retain(is_ok);
    }
    Ok(json!({"from": rules::end_view(&from), "prefer": prefer, "lit": lit, "candidates": list}))
}

fn run(a: &Map<String, Value>) -> Result<Value, String> {
    const TOOL: &str = "pflow_bundle_run";
    let b = read::bundle_arg(TOOL, a, RUN_KINDS)?;
    let scenario = a.get("scenario").and_then(Value::as_str);
    let script_arg = a.get("script").and_then(Value::as_array);
    if scenario.is_some() == script_arg.is_some() {
        return Err(refuse(TOOL, "pass `scenario` or `script`"));
    }
    let mut script: Vec<String> = script_arg
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    if script.len() > MAX_SCRIPT {
        return Err(format!(
            "{TOOL}: script has {} steps; at most {MAX_SCRIPT}",
            script.len()
        ));
    }
    let has_expect = a.get("expect").is_some_and(|v| !v.is_null());
    let (mut id, mut title) = ("script", "script");
    let mut expect = vec![];
    if let Some(name) = scenario {
        let Some(sc) = catalog::scenario(name) else {
            return Err(refuse(
                TOOL,
                format!(
                    "no scenario {} is declared for this model; there are {}",
                    text::quote(name),
                    rules::names(&catalog::scenario_ids())
                ),
            ));
        };
        (id, title, script, expect) = (sc.id, sc.title, sc.script(), sc.expects());
        if has_expect {
            return Err(format!(
                "{TOOL}: `expect` goes with `script`; a scenario brings its own"
            ));
        }
    } else if has_expect {
        expect = expect_arg(&a["expect"])?;
    }
    let f = rules::flatten(&b).map_err(|e| refuse(TOOL, e))?;
    f.check_answer(TOOL)?;
    if scenario.is_none() {
        // A scenario's predicates are native's, and a place that is not there
        // reads as 0 (flat: null says so). Ours are the caller's: a typo with
        // n = 0 would pass, so a place that is not in the composition is refused.
        for (i, e) in expect.iter().enumerate() {
            if f.place_of(&e.place).is_none() {
                return Err(format!(
                    "{TOOL}: expect[{i}]: no place {} in the composition",
                    text::quote(&e.place)
                ));
            }
        }
    }
    rules::run(&f, id, title, &script, &expect)
}

/// `[{place, n, says?}]`.
fn expect_arg(v: &Value) -> Result<Vec<rules::Expect>, String> {
    const TOOL: &str = "pflow_bundle_run";
    let Some(list) = v.as_array() else {
        return Err(format!(
            "{TOOL}: expect should be a list of {{place, n, says}}, got {}",
            text::json(v)
        ));
    };
    if list.len() > MAX_EXPECT {
        return Err(format!(
            "{TOOL}: {} predicates; at most {MAX_EXPECT}",
            list.len()
        ));
    }
    let mut out = vec![];
    for (i, raw) in list.iter().enumerate() {
        let empty = Map::new();
        let m = raw.as_object().unwrap_or(&empty);
        let place = m.get("place").and_then(Value::as_str).unwrap_or("");
        let n = m.get("n").and_then(Value::as_f64);
        let n = match n {
            Some(n) if !place.is_empty() && n.fract() == 0.0 && n.abs() < read::EXACT_INT => n,
            _ => {
                return Err(format!(
                    "{TOOL}: expect[{i}] should be {{place: \"<subnet>/<place>\", n: an integer below 2^53, says?}}, got {}",
                    text::json(raw)
                ))
            }
        };
        let mut keys: Vec<&String> = m.keys().collect();
        keys.sort();
        if let Some(k) = keys
            .into_iter()
            .find(|k| !matches!(k.as_str(), "place" | "n" | "says"))
        {
            return Err(format!(
                "{TOOL}: expect[{i}]: unknown field {}; takes place, n, says",
                text::quote(k)
            ));
        }
        out.push(rules::Expect {
            place: place.to_string(),
            n: n as i64,
            says: m
                .get("says")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    Ok(out)
}

fn incidence(a: &Map<String, Value>) -> Result<Value, String> {
    const TOOL: &str = "pflow_bundle_incidence";
    let b = read::bundle_arg(TOOL, a, BUNDLE_ONLY)?;
    let f = match rules::flatten(&b) {
        Ok(f) => f,
        Err(e) => return Ok(json!({"ok": false, "error": e})),
    };
    f.check_answer(TOOL)?;
    let fm = &f.model;
    // the matrices are rows x columns of numbers, of the flat net and of each
    // subnet; each declared constraint carries its y·C, one per flat transition
    let mut cells = fm.places.len() * fm.transitions.len();
    for s in &b.subnets {
        cells += s.model.places.len() * s.model.transitions.len();
    }
    cells += fm.constraints.len() * fm.transitions.len();
    if cells > MAX_MATRIX_CELLS {
        return Err(format!(
            "{TOOL}: the matrices would have {cells} cells ({} flat places x {} flat transitions, and each subnet's own); at most {MAX_MATRIX_CELLS}, and the answer limit is {} KB. pflow_bundle_flatten lists the arcs, which are the matrix's non-zero cells",
            fm.places.len(),
            fm.transitions.len(),
            MAX_RESULT_BYTES >> 10
        ));
    }
    let c = rules::incidence(fm);
    let ids = |v: Vec<&String>| -> Value { json!(v) };
    let blocks: Vec<Value> = b
        .subnets
        .iter()
        .map(|s| {
            json!({
                "subnet": s.id,
                "rows": ids(s.model.places.iter().map(|p| &p.id).collect()),
                "cols": ids(s.model.transitions.iter().map(|t| &t.id).collect()),
                "matrix": rules::incidence(&s.model),
            })
        })
        .collect();
    let invariants: Vec<Value> = fm
        .constraints
        .iter()
        .map(|k| {
            let mut entry = Map::new();
            entry.insert("id".into(), json!(k.id));
            entry.insert("expr".into(), json!(k.expr));
            entry.insert("holds".into(), Value::Null);
            if let Some(check) = rules::invariant_check(fm, &c, &k.expr) {
                entry.extend(check);
            }
            Value::Object(entry)
        })
        .collect();
    let mut view = json!({
        "ok": true,
        "rows": ids(fm.places.iter().map(|p| &p.id).collect()),
        "cols": ids(fm.transitions.iter().map(|t| &t.id).collect()),
        "matrix": c,
        "blocks": blocks,
        "invariants": invariants,
    });
    // the flat net's arcs read as 1 once several subnets are flattened; the
    // blocks are each subnet's own arcs, as written
    let flat_n = rules::weightless(fm, true);
    let block_n: usize = b
        .subnets
        .iter()
        .map(|s| rules::weightless(&s.model, true))
        .sum();
    if flat_n + block_n > 0 {
        let mut place = vec![];
        if flat_n > 0 {
            place.push(format!("the flat matrix ({flat_n})"));
        }
        if block_n > 0 {
            place.push(format!("the subnets' own blocks ({block_n})"));
        }
        view["notes"] = json!([format!(
            "Arcs that move tokens have no weight and are counted as 0 in {}, but they fire as weight 1 (pflow_bundle_run). Give every arc an explicit weight.",
            place.join(" and ")
        )]);
    }
    Ok(view)
}

fn example(a: &Map<String, Value>) -> Result<Value, String> {
    const TOOL: &str = "pflow_bundle_example";
    read::check_args(TOOL, a, EXAMPLE_KINDS)?;
    let instances: Vec<String> = match a.get("instances").and_then(Value::as_array) {
        Some(l) => l
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect(),
        None => catalog::DEFAULT_INSTANCES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let links = match a.get("links") {
        None | Some(Value::Null) => catalog::default_links(),
        Some(Value::Array(list)) => {
            if list.len() > MAX_LINKS {
                return Err(format!("{TOOL}: {} links; at most {MAX_LINKS}", list.len()));
            }
            list.iter()
                .map(|x| rules::parse_link(x).map_err(|e| refuse(TOOL, e)))
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(other) => {
            return Err(format!(
                "{TOOL}: links must be an array, got {}",
                text::json(other)
            ))
        }
    };
    let o = catalog::open(&instances, &links).map_err(|e| refuse(TOOL, e))?;
    Ok(example_view(&o))
}

fn example_view(o: &catalog::Opened) -> Value {
    // the whole bundle, as the other tools read it
    let mut bundle = serde_json::to_value(&o.bundle).unwrap_or_else(|_| json!({}));
    if bundle.get("subnets").is_none() {
        bundle["subnets"] = json!([]); // a composition of no instances has no subnets, not a null
    }
    let have = |id: &str| o.instances.contains(&id);
    // the scenarios are the orders content's, which attaches to a composition
    // that includes its three anchors
    let scenarios: Vec<Value> = if have("orders") && have("workers") && have("payment") {
        catalog::SCENARIOS
            .iter()
            .map(catalog::Scenario::view)
            .collect()
    } else {
        vec![]
    };
    json!({
        "bundle": bundle,
        "instances": o.instances,
        "unavailable": o.unavailable,
        "dropped_links": o.dropped,
        "catalog": catalog::catalog().iter().map(catalog::InstanceSpec::view).collect::<Vec<_>>(),
        "default": catalog::DEFAULT_INSTANCES,
        "default_links": catalog::default_links().iter().map(rules::link_view).collect::<Vec<_>>(),
        "params_note": catalog::PARAMS_NOTE,
        "scenarios": scenarios,
    })
}
