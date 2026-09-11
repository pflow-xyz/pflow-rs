//! Trajectory and summary types. Ported field-for-field from go-pflow's
//! `stochastic.Result`/`Metrics`/`Depletion`/`Contention`
//! (`stochastic/stochastic.go`).

use std::collections::HashMap;

use super::supply::SupplyKind;

/// One place's values over time.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub place: String,
    pub values: Vec<f64>,
    /// The ensemble spread, populated when `realizations > 1`.
    pub std_dev: Option<Vec<f64>>,
}

/// A trajectory: sample times plus one series per token place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimResult {
    pub times: Vec<f64>,
    pub series: Vec<Series>,
    pub final_: HashMap<String, f64>,
    /// Places that reach zero within the horizon, earliest first.
    pub depleted: Vec<Depletion>,
    /// What the run spent its time waiting for, capacity constraints first
    /// and the longest wait first within each kind.
    pub contended: Vec<Contention>,
    pub method: String,

    /// Constraints the model expresses that this run could not enforce. An
    /// empty list is a claim: everything the net says was applied. Distinct
    /// from `assumptions` — see the field doc there.
    pub caveats: Vec<String>,
    /// What the engine had to assume in order to answer at all: properties
    /// of the method, true of every model it runs, and not repairable by
    /// editing the net.
    pub assumptions: Vec<String>,

    pub metrics: Option<Metrics>,
}

/// Summarises a stochastic run in the terms of the thing being modelled.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metrics {
    /// Mean number of firings per transition over the horizon.
    pub throughput: HashMap<String, f64>,
    /// Time-weighted mean and 95th percentile per place — a dwell-time
    /// estimator over the whole trajectory, not an average of the reported
    /// sample points (which would over-weight the t=0 transient at a coarse
    /// grid).
    pub mean: HashMap<String, f64>,
    pub p95: HashMap<String, f64>,
    /// The busy fraction of every `<pool>/available` + `<pool>/busy` (or
    /// bare `available`/`busy`) pair the model exposes.
    pub utilization: HashMap<String, f64>,
    /// Mean number of delayed firings still in progress at the horizon, per
    /// timed transition.
    pub in_flight: HashMap<String, f64>,
}

/// Records when a place first runs out.
#[derive(Debug, Clone, PartialEq)]
pub struct Depletion {
    pub place: String,
    pub at: f64,
    /// True when the place was back above the floor by the end of the
    /// horizon (a barista pool momentarily at zero, not a pantry that
    /// stayed empty).
    pub recovered: bool,
}

/// Records how long a place was the sole unmet input of a transition.
#[derive(Debug, Clone, PartialEq)]
pub struct Contention {
    pub place: String,
    pub fraction: f64,
    /// Whether waiting on this place is a capacity finding
    /// ([`SupplyKind::is_capacity`]) or a statement about demand.
    pub kind: SupplyKind,
    /// The transitions this place held up, sorted.
    pub blocking: Vec<String>,
}

/// What the SSA engine cannot avoid assuming: every step's duration is
/// drawn from an exponential distribution, the most erratic a step can be
/// for a given average.
pub const EXPONENTIAL_SERVICE_ASSUMPTION: &str = "this engine assumes every step takes a random, exponentially distributed amount of \
    time, which is the most erratic a shop can be for a given average. Work with a predictable duration \u{2014} a shot \
    pulls in about the same time every time \u{2014} queues roughly half as much, so treat the waiting and the walkouts \
    here as the bad case, not the typical one.";
