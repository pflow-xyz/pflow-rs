//! A search plan: the pieces of a game search, picked by name in JSON.
//!
//! The search is fixed (alpha-beta over [`Game`], see [`crate::game`]); a
//! plan chooses what swaps in:
//!
//! ```json
//! {"mode": "exact",
//!  "order": ["tt_best", "immediate_win", "centrality", "history"],
//!  "filters": ["must_win", "non_losing"],
//!  "root": {"order": ["ode", "non_losing"]},
//!  "drivers": ["null_window", "aspiration", {"budget": {"nodes": 1000000}}],
//!  "memo": {"entries": 500000}}
//! ```
//!
//! - `mode`: `"exact"` (the default) holds every piece to returning the
//!   value, and [`Solver::choose`] to a move proven best (or none, past the
//!   budget); `"play"` lets `choose` guess when the budget runs out
//!   ([`Solver::with_play`]), and lets in pieces that trade the value for
//!   speed (none is built yet).
//! - `order`: [`Piece`] names, highest priority first. Order never changes
//!   a value, so any chain is legal in either mode. Default: the solver's.
//! - `root`: `{"order": [...]}`, pieces run once at the position searched
//!   ([`crate::root`]): an estimate's name (`"ode"`, at most one), which
//!   tries the root's moves best guess first, and `"non_losing"`, which
//!   drops the root's moves that leave the other side a win at once. Both
//!   keep the value, so are legal in either mode. No estimate is built into
//!   this crate: [`SearchPlan::solver_with`] takes one, and
//!   `pflow_solver::game::solver` supplies `ode`.
//! - `drivers`: `"null_window"` finds a value by yes/no searches (on by
//!   default, and off when `drivers` is given without it); `"mtd_f"` finds
//!   it by the same searches, choosing each next one from the bound the last
//!   proved ([`Solver::with_mtd_f`]; not together with `null_window`);
//!   `"aspiration"`
//!   starts from the root's estimate, which `root.order` must name
//!   ([`Solver::with_aspiration`]); `{"budget": {"nodes": N, "ms": T}}`
//!   gives a value up past N nodes or T milliseconds (either or both; see
//!   [`Solver::with_time`] for the clock). Each at most once: the search
//!   runs them in one order, budget(null_window(core)) (or mtd_f for null_window), whatever the list's.
//! - `memo`: `{"entries": N}` keeps the transposition table to N entries
//!   (by default [`Solver::MEMO`]).
//! - `filters`: [`Filter`] names, which drop moves only where the value is
//!   already known, so they are legal in either mode. Default:
//!   [`Filter::DEFAULT`]; `[]` runs none.
//! - Pieces that can change the value, a filter or a `heuristics` name:
//!   none is built yet. The registry knows the two it reserves (`beam`, a
//!   filter, and `static_eval`) only to refuse them, by name, in exact
//!   mode.
//!
//! [`SearchPlan::check`] is the legality check, run before a solver is
//! built; [`SearchPlan::solver`] builds it, and refuses what the net cannot
//! support (`centrality`, `threats` on a net declaring no goals).
//! [`SearchPlan::normalized`] states a plan's defaults and lists its sets in
//! the order the search runs them, and [`SearchPlan::net`] draws it as a
//! Petri net whose string diagram is the plan as one composite.

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::filter::{Filter, Filters};
use crate::game::{Game, Solver};
use crate::order::{Chain, Piece};
use crate::root::Estimate;

/// How much a plan may give up for speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The value, exactly: only pieces that cannot change it.
    #[default]
    Exact,
    /// Pieces that may change it are allowed.
    Play,
}

/// What a name in a plan can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Order,
    Driver,
    Filter,
    Heuristic,
    Estimate,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Order => "order piece",
            Kind::Driver => "driver",
            Kind::Filter => "filter",
            Kind::Heuristic => "heuristic",
            Kind::Estimate => "estimate",
        }
    }
}

/// The drivers there are.
const DRIVERS: [&str; 4] = ["null_window", "mtd_f", "aspiration", "budget"];

/// The estimates there are, none of them built here: a solver is given one
/// ([`SearchPlan::solver_with`]).
const ESTIMATES: [&str; 1] = ["ode"];

/// Reserved names of pieces that can change a value, none of them built.
const UNSOUND: [(&str, Kind); 2] = [("beam", Kind::Filter), ("static_eval", Kind::Heuristic)];

/// What `name` is, if the registry knows it.
fn kind_of(name: &str) -> Option<Kind> {
    if Piece::parse(name).is_ok() {
        Some(Kind::Order)
    } else if Filter::parse(name).is_ok() {
        Some(Kind::Filter)
    } else if DRIVERS.contains(&name) {
        Some(Kind::Driver)
    } else if ESTIMATES.contains(&name) {
        Some(Kind::Estimate)
    } else {
        UNSOUND.iter().find(|x| x.0 == name).map(|x| x.1)
    }
}

fn plan_error(msg: String) -> Error {
    Error::Game(format!("search plan: {msg}"))
}

/// A name in a plan, refused unless it is a `want`.
fn expect(name: &str, want: Kind) -> Result<()> {
    match kind_of(name) {
        None => Err(plan_error(format!(
            "unknown {} {name:?} (known: {:?}, {:?}, {:?}, {:?}, {:?})",
            want.noun(),
            Piece::NAMES,
            Filter::NAMES,
            DRIVERS,
            ESTIMATES,
            UNSOUND.map(|x| x.0)
        ))),
        Some(k) if k != want => Err(plan_error(format!(
            "{name:?} is a {}, not a {}",
            k.noun(),
            want.noun()
        ))),
        Some(_) => Ok(()),
    }
}

/// `{"entries": N}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memo {
    pub entries: usize,
}

/// `{"order": [...]}`: the root's pieces.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootPlan {
    pub order: Vec<String>,
}

/// `{"nodes": N, "ms": T}`, either or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    nodes: Option<usize>,
    ms: Option<u64>,
}

/// A search plan; see the module docs.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SearchPlan {
    pub mode: Mode,
    pub order: Option<Vec<String>>,
    /// Names, and `{"budget": {...}}` objects.
    pub drivers: Option<Vec<Value>>,
    pub filters: Option<Vec<String>>,
    pub root: Option<RootPlan>,
    pub heuristics: Vec<String>,
    pub memo: Option<Memo>,
}

/// What a plan's drivers come to.
struct Drivers {
    null_window: bool,
    mtd_f: bool,
    aspiration: bool,
    budget: Option<usize>,
    ms: Option<u64>,
}

/// What a plan's root comes to.
struct Roots<'p> {
    estimate: Option<&'p str>,
    non_losing: bool,
}

impl SearchPlan {
    /// A plan from its JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| plan_error(e.to_string()))
    }

    fn drivers(&self) -> Result<Drivers> {
        let Some(list) = &self.drivers else {
            return Ok(Drivers {
                null_window: true,
                mtd_f: false,
                aspiration: false,
                budget: None,
                ms: None,
            });
        };
        let mut d = Drivers {
            null_window: false,
            mtd_f: false,
            aspiration: false,
            budget: None,
            ms: None,
        };
        let twice = |name: &str| plan_error(format!("driver {name:?} is listed twice"));
        let mut budgeted = false;
        for v in list {
            match v {
                Value::String(name) => {
                    expect(name, Kind::Driver)?;
                    match name.as_str() {
                        "null_window" if d.null_window => return Err(twice(name)),
                        "mtd_f" if d.mtd_f => return Err(twice(name)),
                        "aspiration" if d.aspiration => return Err(twice(name)),
                        "null_window" => d.null_window = true,
                        "mtd_f" => d.mtd_f = true,
                        "aspiration" => d.aspiration = true,
                        _ => {
                            return Err(plan_error(format!(
                                "driver {name:?} takes arguments: {{{name:?}: {{\"nodes\": N}}}}"
                            )))
                        }
                    }
                }
                Value::Object(o) if o.len() == 1 => {
                    let (name, args) = o.iter().next().unwrap();
                    expect(name, Kind::Driver)?;
                    if name != "budget" {
                        return Err(plan_error(format!("driver {name:?} takes no arguments")));
                    }
                    if budgeted {
                        return Err(twice(name));
                    }
                    budgeted = true;
                    let b: Budget = serde_json::from_value(args.clone())
                        .map_err(|e| plan_error(format!("driver \"budget\": {e}")))?;
                    if b.nodes.is_none() && b.ms.is_none() {
                        return Err(plan_error(
                            "driver \"budget\" takes \"nodes\", \"ms\" or both".into(),
                        ));
                    }
                    if b.nodes == Some(0) || b.ms == Some(0) {
                        return Err(plan_error(
                            "driver \"budget\": nodes and ms must be at least 1".into(),
                        ));
                    }
                    d.budget = b.nodes;
                    d.ms = b.ms;
                }
                _ => {
                    return Err(plan_error(format!(
                        "a driver is a name or an object with one key, not {v}"
                    )))
                }
            }
        }
        if d.null_window && d.mtd_f {
            return Err(plan_error(
                "drivers \"null_window\" and \"mtd_f\" both find the value by yes/no searches: name one"
                    .into(),
            ));
        }
        Ok(d)
    }

    /// The order chain the plan names.
    fn chain(&self) -> Result<Chain> {
        let Some(names) = &self.order else {
            return Ok(Chain::default());
        };
        let mut pieces: Vec<Piece> = vec![];
        for n in names {
            expect(n, Kind::Order)?;
            let p = Piece::parse(n)?;
            if pieces.contains(&p) {
                return Err(plan_error(format!("order piece {n:?} is listed twice")));
            }
            pieces.push(p);
        }
        Chain::new(&pieces)
    }

    /// The root's pieces the plan names.
    fn roots(&self) -> Result<Roots<'_>> {
        let mut r = Roots {
            estimate: None,
            non_losing: false,
        };
        let names = self.root.as_ref().map_or(&[][..], |x| &x.order[..]);
        for (i, n) in names.iter().enumerate() {
            if names[..i].contains(n) {
                return Err(plan_error(format!("root piece {n:?} is listed twice")));
            }
            match kind_of(n) {
                Some(Kind::Estimate) if r.estimate.is_some() => {
                    return Err(plan_error(format!(
                        "root: {n:?} is a second estimate; the root takes one"
                    )))
                }
                Some(Kind::Estimate) => r.estimate = Some(n),
                _ if n == "non_losing" => r.non_losing = true,
                None => {
                    return Err(plan_error(format!(
                        "unknown root piece {n:?} (known: {ESTIMATES:?}, \"non_losing\")"
                    )))
                }
                Some(k) => {
                    return Err(plan_error(format!(
                        "{n:?} is a {}, not a root piece (an estimate, or \"non_losing\")",
                        k.noun()
                    )))
                }
            }
        }
        Ok(r)
    }

    /// The estimate the plan's root names, if any: what
    /// [`SearchPlan::solver_with`] must be given.
    pub fn estimate(&self) -> Result<Option<&str>> {
        Ok(self.roots()?.estimate)
    }

    /// The filters the plan names.
    fn filters(&self) -> Result<Vec<Filter>> {
        let Some(names) = &self.filters else {
            return Ok(Filter::DEFAULT.to_vec());
        };
        let mut out = vec![];
        for n in names {
            expect(n, Kind::Filter)?;
            let Ok(f) = Filter::parse(n) else {
                continue;
            };
            if out.contains(&f) {
                return Err(plan_error(format!("filter {n:?} is listed twice")));
            }
            out.push(f);
        }
        Ok(out)
    }

    /// The legality check: every name is known and in its place, and in
    /// exact mode no filter or heuristic is there to change the value.
    /// Needs no net; what a net cannot support is refused by
    /// [`SearchPlan::solver`].
    pub fn check(&self) -> Result<()> {
        self.chain()?;
        let d = self.drivers()?;
        let r = self.roots()?;
        if d.aspiration && r.estimate.is_none() {
            return Err(plan_error(
                "driver \"aspiration\" starts from the root's estimate: name one in root.order"
                    .into(),
            ));
        }
        if self.memo.is_some_and(|m| m.entries == 0) {
            return Err(plan_error("memo: entries must be at least 1".into()));
        }
        self.filters()?;
        for (names, kind) in [
            (self.filters.as_ref().unwrap_or(&vec![]), Kind::Filter),
            (&self.heuristics, Kind::Heuristic),
        ] {
            for n in names {
                expect(n, kind)?;
            }
            let unsound = names.iter().find(|n| UNSOUND.iter().any(|x| x.0 == *n));
            if let Some(n) = unsound {
                let why = match self.mode {
                    Mode::Exact => "can change the value, which exact mode does not allow",
                    Mode::Play => "is not built in this release",
                };
                return Err(plan_error(format!("{} {n:?} {why}", kind.noun())));
            }
        }
        Ok(())
    }

    /// A solver for `g` that runs the plan, refused when the plan is not
    /// legal or the net cannot support it, or names an estimate (see
    /// [`SearchPlan::solver_with`]).
    pub fn solver<'a>(&self, g: &'a Game) -> Result<Solver<'a>> {
        self.solver_with(g, None)
    }

    /// [`SearchPlan::solver`], given `estimate` for the one the plan's root
    /// names (refused when it is another, or missing). One given when the
    /// plan names none is not used.
    pub fn solver_with<'a>(
        &self,
        g: &'a Game,
        estimate: Option<Box<dyn Estimate + 'a>>,
    ) -> Result<Solver<'a>> {
        self.check()?;
        let d = self.drivers()?;
        let r = self.roots()?;
        let mut s = Solver::with_chain(g, d.budget.unwrap_or(usize::MAX), self.chain()?)?
            .with_null_window(d.null_window)
            .with_mtd_f(d.mtd_f)
            .with_filters(&self.filters()?)
            .with_root_non_losing(r.non_losing)
            .with_aspiration(d.aspiration)
            .with_play(self.mode == Mode::Play);
        if let Some(ms) = d.ms {
            s = s.with_time(std::time::Duration::from_millis(ms));
        }
        if let Some(m) = self.memo {
            s = s.with_memo(m.entries);
        }
        match (r.estimate, estimate) {
            (None, _) => Ok(s),
            (Some(want), Some(e)) if e.name() == want => Ok(s.with_estimate(e)),
            (Some(want), Some(e)) => Err(plan_error(format!(
                "the root names estimate {want:?}, but the solver was given {:?}",
                e.name()
            ))),
            (Some(want), None) => Err(plan_error(format!(
                "the root names estimate {want:?}, which is not built into pflow-metamodel \
                 (pflow_solver::game::solver supplies \"ode\")"
            ))),
        }
    }

    /// The plan with its defaults stated (the default chain, filters,
    /// drivers and memo, an empty root) and its sets in the order the
    /// search runs them, not the order listed: filters `must_win` then
    /// `non_losing`; the root's `non_losing` then its estimate (moves are
    /// dropped before the rest are ranked); drivers `null_window`,
    /// `aspiration`, then the budget. The order chain is kept as listed,
    /// since its order is its meaning. Two plans with the same normal form
    /// build the same solver. Refused when the plan is not legal.
    pub fn normalized(&self) -> Result<SearchPlan> {
        self.check()?;
        let d = self.drivers()?;
        let r = self.roots()?;
        let f = Filters::of(&self.filters()?);
        let mut drivers: Vec<Value> = vec![];
        if d.null_window {
            drivers.push("null_window".into());
        }
        if d.mtd_f {
            drivers.push("mtd_f".into());
        }
        if d.aspiration {
            drivers.push("aspiration".into());
        }
        if d.budget.is_some() || d.ms.is_some() {
            let mut b = serde_json::Map::new();
            if let Some(n) = d.budget {
                b.insert("nodes".into(), n.into());
            }
            if let Some(ms) = d.ms {
                b.insert("ms".into(), ms.into());
            }
            drivers.push(serde_json::json!({ "budget": b }));
        }
        let mut filters: Vec<String> = vec![];
        if f.must_win {
            filters.push(Filter::MustWin.name().into());
        }
        if f.non_losing {
            filters.push(Filter::NonLosing.name().into());
        }
        let mut root: Vec<String> = vec![];
        if r.non_losing {
            root.push("non_losing".into());
        }
        root.extend(r.estimate.map(String::from));
        Ok(SearchPlan {
            mode: self.mode,
            order: Some(
                self.chain()?
                    .pieces()
                    .iter()
                    .map(|p| p.name().into())
                    .collect(),
            ),
            drivers: Some(drivers),
            filters: Some(filters),
            root: Some(RootPlan { order: root }),
            heuristics: self.heuristics.clone(),
            memo: Some(self.memo.unwrap_or(Memo {
                entries: Solver::MEMO,
            })),
        })
    }

    /// The plan as a Petri net in pflow.xyz's editor shape (Shape A JSON),
    /// for a string diagram ([pflow-native]'s `view.string_diagram`, a port
    /// of pflow.xyz's): each transition is a box labeled by its id, each
    /// place a typed wire (`Position`, `MoveList`, `Key`, `Value`, `Table`,
    /// `Value⊥`, numbered where a type repeats). It draws the
    /// [`SearchPlan::normalized`] plan as one composite, top to bottom in
    /// the order the search composes it:
    ///
    /// - `moves`: a position's moves, then the filters (`filter.*`) and the
    ///   root's `root.non_losing`, each `MoveList → MoveList`;
    /// - the order chain: a box per piece (`order.*`, `*` marking a
    ///   decisive one), each giving its `Key`, and a `chain` box taking the
    ///   list and the keys in priority order, the root's estimate
    ///   (`root.ode`) last; with aspiration, the estimate's `Value` goes to
    ///   the aspiration driver too;
    /// - `core`, given the ordered list and the transposition table
    ///   (`memo`, a side input), with the mode in its id;
    /// - the drivers, innermost first: `null_window`, `aspiration`, and the
    ///   budget, outermost, with its arguments in its id.
    ///
    /// The JSON is written as text in a fixed order, so it is the same
    /// bytes whichever way serde_json orders a map. Equal normal forms give
    /// equal nets; refused when the plan is not legal.
    ///
    /// [pflow-native]: https://github.com/pflow-xyz/pflow-native
    pub fn net(&self) -> Result<String> {
        let p = self.normalized()?;
        let d = p.drivers()?;
        let r = p.roots()?;
        let f = Filters::of(&p.filters()?);
        let mut n = Net::default();
        let position = n.place("Position".into());
        let mut list = n.place("MoveList".into());
        let mut stage = 0;
        n.add("moves".into(), &[&position], &[&list]);
        for (on, id) in [
            (f.must_win, "filter.must_win"),
            (f.non_losing, "filter.non_losing"),
            (r.non_losing, "root.non_losing"),
        ] {
            if on {
                stage += 1;
                let next = n.place(format!("MoveList·{stage}"));
                n.add(id.into(), &[&list], &[&next]);
                list = next;
            }
        }
        let mut keys = vec![];
        for piece in p.chain()?.pieces() {
            let key = n.place(format!("Key·{}", keys.len() + 1));
            let mark = if piece.decisive() { "*" } else { "" };
            n.add(format!("order.{}{mark}", piece.name()), &[&list], &[&key]);
            keys.push(key);
        }
        let mut guess = None;
        if let Some(e) = r.estimate {
            let key = n.place(format!("Key·{}", keys.len() + 1));
            let mut out = vec![key.clone()];
            if d.aspiration {
                let v = n.place("Value".into());
                out.push(v.clone());
                guess = Some(v);
            }
            n.add(
                format!("root.{e}"),
                &[&list],
                &out.iter().collect::<Vec<_>>(),
            );
            keys.push(key);
        }
        stage += 1;
        let ordered = n.place(format!("MoveList·{stage}"));
        let chain_in: Vec<&String> = std::iter::once(&list).chain(&keys).collect();
        n.add("chain".into(), &chain_in, &[&ordered]);
        let table = n.place("Table".into());
        let entries = p.memo.map_or(Solver::MEMO, |m| m.entries);
        n.add(format!("memo(entries={entries})"), &[], &[&table]);
        let mut drivers: Vec<(String, Option<&String>)> = vec![];
        if d.null_window {
            drivers.push(("driver.null_window".into(), None));
        }
        if d.mtd_f {
            drivers.push(("driver.mtd_f".into(), None));
        }
        if d.aspiration {
            drivers.push(("driver.aspiration".into(), guess.as_ref()));
        }
        if d.budget.is_some() || d.ms.is_some() {
            let args: Vec<String> = [("nodes", d.budget.map(|x| x as u64)), ("ms", d.ms)]
                .iter()
                .filter_map(|(k, v)| v.map(|v| format!("{k}={v}")))
                .collect();
            drivers.push((format!("driver.budget({})", args.join(",")), None));
        }
        let last = drivers.len();
        // The outermost result is the plan's; the ones inside are numbered.
        let value = |i: usize| {
            if i == last {
                "Value⊥".to_string()
            } else {
                format!("Value⊥·{}", i + 1)
            }
        };
        let mode = match p.mode {
            Mode::Exact => "exact",
            Mode::Play => "play",
        };
        let mut v = n.place(value(0));
        n.add(format!("core(mode={mode})"), &[&ordered, &table], &[&v]);
        for (i, (id, side)) in drivers.into_iter().enumerate() {
            let next = n.place(value(i + 1));
            let inputs: Vec<&String> = std::iter::once(&v).chain(side).collect();
            n.add(id, &inputs, &[&next]);
            v = next;
        }
        Ok(n.json())
    }
}

/// A Petri net built for [`SearchPlan::net`]: places and transitions by id,
/// in the order added, and arcs box by box (inputs, then outputs).
#[derive(Default)]
struct Net {
    places: Vec<String>,
    transitions: Vec<String>,
    arcs: Vec<(String, String)>,
}

impl Net {
    fn place(&mut self, id: String) -> String {
        self.places.push(id.clone());
        id
    }

    /// Box `id`, its ports in the order given.
    fn add(&mut self, id: String, inputs: &[&String], outputs: &[&String]) {
        for p in inputs {
            self.arcs.push(((*p).clone(), id.clone()));
        }
        for p in outputs {
            self.arcs.push((id.clone(), (*p).clone()));
        }
        self.transitions.push(id);
    }

    /// The editor-shape JSON, one element per line.
    fn json(&self) -> String {
        let q = |s: &str| serde_json::to_string(s).expect("a string");
        let nodes = |ids: &[String], ty: &str| {
            ids.iter()
                .map(|id| format!("    {}: {{\"@type\": \"{ty}\"}}", q(id)))
                .collect::<Vec<_>>()
                .join(",\n")
        };
        let arcs = self
            .arcs
            .iter()
            .map(|(a, b)| {
                format!(
                    "    {{\"@type\": \"Arrow\", \"source\": {}, \"target\": {}, \"weight\": [1]}}",
                    q(a),
                    q(b)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        format!(
            "{{\n  \"@context\": \"https://pflow.xyz/schema\",\n  \"@type\": \"PetriNet\",\n  \
             \"@version\": \"1.1\",\n  \"places\": {{\n{}\n  }},\n  \"transitions\": {{\n{}\n  }},\n  \
             \"arcs\": [\n{arcs}\n  ]\n}}\n",
            nodes(&self.places, "Place"),
            nodes(&self.transitions, "Transition"),
        )
    }
}
