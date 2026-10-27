//! A search plan: the pieces of a game search, picked by name in JSON.
//!
//! The search is fixed (alpha-beta over [`Game`], see [`crate::game`]); a
//! plan chooses what swaps in:
//!
//! ```json
//! {"mode": "exact",
//!  "order": ["tt_best", "immediate_win", "centrality", "history"],
//!  "drivers": ["null_window", {"budget": {"nodes": 1000000}}],
//!  "memo": {"entries": 500000}}
//! ```
//!
//! - `mode`: `"exact"` (the default) holds every piece to returning the
//!   value; `"play"` would allow pieces that trade it for speed.
//! - `order`: [`Piece`] names, highest priority first. Order never changes
//!   a value, so any chain is legal in either mode. Default: the solver's.
//! - `drivers`: `"null_window"` finds a value by yes/no searches (on by
//!   default, and off when `drivers` is given without it); `{"budget":
//!   {"nodes": N}}` gives a value up on a position past N nodes.
//! - `memo`: `{"entries": N}` keeps the transposition table to N entries.
//! - `filters`, `heuristics`: names of pieces that can change the value.
//!   None is built yet: the registry knows the two it reserves (`beam`,
//!   `static_eval`) only to refuse them, by name, in exact mode.
//!
//! [`SearchPlan::check`] is the legality check, run before a solver is
//! built; [`SearchPlan::solver`] builds it, and refuses what the net cannot
//! support (`centrality`, `threats` on a net declaring no goals).

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::game::{Game, Solver};
use crate::order::{Chain, Piece};

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
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Order => "order piece",
            Kind::Driver => "driver",
            Kind::Filter => "filter",
            Kind::Heuristic => "heuristic",
        }
    }
}

/// The drivers there are.
const DRIVERS: [&str; 2] = ["null_window", "budget"];

/// Reserved names of pieces that can change a value, none of them built.
const UNSOUND: [(&str, Kind); 2] = [("beam", Kind::Filter), ("static_eval", Kind::Heuristic)];

/// What `name` is, if the registry knows it.
fn kind_of(name: &str) -> Option<Kind> {
    if Piece::parse(name).is_ok() {
        Some(Kind::Order)
    } else if DRIVERS.contains(&name) {
        Some(Kind::Driver)
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
            "unknown {} {name:?} (known: {:?}, {:?}, {:?})",
            want.noun(),
            Piece::NAMES,
            DRIVERS,
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

/// `{"nodes": N}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    nodes: usize,
}

/// A search plan; see the module docs.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SearchPlan {
    pub mode: Mode,
    pub order: Option<Vec<String>>,
    /// Names, and `{"budget": {...}}` objects.
    pub drivers: Option<Vec<Value>>,
    pub filters: Vec<String>,
    pub heuristics: Vec<String>,
    pub memo: Option<Memo>,
}

/// What a plan's drivers come to.
struct Drivers {
    null_window: bool,
    budget: Option<usize>,
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
                budget: None,
            });
        };
        let mut d = Drivers {
            null_window: false,
            budget: None,
        };
        for v in list {
            match v {
                Value::String(name) => {
                    expect(name, Kind::Driver)?;
                    match name.as_str() {
                        "null_window" => d.null_window = true,
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
                    let b: Budget = serde_json::from_value(args.clone())
                        .map_err(|e| plan_error(format!("driver \"budget\": {e}")))?;
                    if b.nodes == 0 {
                        return Err(plan_error(
                            "driver \"budget\": nodes must be at least 1".into(),
                        ));
                    }
                    d.budget = Some(b.nodes);
                }
                _ => {
                    return Err(plan_error(format!(
                        "a driver is a name or an object with one key, not {v}"
                    )))
                }
            }
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

    /// The legality check: every name is known and in its place, and in
    /// exact mode no filter or heuristic is there to change the value.
    /// Needs no net; what a net cannot support is refused by
    /// [`SearchPlan::solver`].
    pub fn check(&self) -> Result<()> {
        self.chain()?;
        self.drivers()?;
        if self.memo.is_some_and(|m| m.entries == 0) {
            return Err(plan_error("memo: entries must be at least 1".into()));
        }
        for (names, kind) in [
            (&self.filters, Kind::Filter),
            (&self.heuristics, Kind::Heuristic),
        ] {
            for n in names {
                expect(n, kind)?;
            }
            if let Some(n) = names.first() {
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
    /// legal or the net cannot support it.
    pub fn solver<'a>(&self, g: &'a Game) -> Result<Solver<'a>> {
        self.check()?;
        let d = self.drivers()?;
        let mut s = Solver::with_chain(g, d.budget.unwrap_or(usize::MAX), self.chain()?)?
            .with_null_window(d.null_window);
        if let Some(m) = self.memo {
            s = s.with_memo(m.entries);
        }
        Ok(s)
    }
}
