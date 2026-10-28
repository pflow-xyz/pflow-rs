//! The mass-action ODE of a game's net as a root estimate (`ode`, see
//! [`pflow_metamodel::root`]): from a position, every transition fires
//! continuously at rate 1, both sides' moves and the automatic ones alike,
//! for `horizon` model time, and the guess is the reward read off where the
//! tokens end up. It is pflow-native's ODE play scorer, with the continuous
//! net compiled once per game rather than once per solve.
//!
//! The continuous net, read off [`Game::ix`]: a transition's flux is the
//! product of the tokens on its input and read places (mass action, weights
//! left out of the product as in [`crate::ode`]); it takes weight × flux
//! from each input place and puts weight × flux on each output place, and a
//! read arc only gates it. Inhibitors and capacities have no continuous
//! form and are left out. A guess only orders moves and picks where a run
//! of yes/no searches starts, so a poor one costs nodes, never a value.

use pflow_core::{PetriNet, State};
use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::{Estimate, Result, SearchPlan};

use crate::methods;
use crate::ode::{integrate_vec, Options};

/// One transition of the continuous net.
#[derive(Debug, Clone)]
struct Flow {
    /// Places whose tokens multiply into the flux: inputs, then reads.
    gates: Vec<usize>,
    /// (place, weight): what a unit of flux takes, and puts.
    takes: Vec<(usize, f64)>,
    puts: Vec<(usize, f64)>,
}

/// The `ode` estimate of a game. See the module docs.
#[derive(Debug, Clone)]
pub struct OdeEstimate {
    flows: Vec<Flow>,
    places: usize,
    reward: Vec<(usize, f64)>,
    horizon: f64,
    method: methods::Solver,
    opts: Options,
}

impl OdeEstimate {
    /// The model time solved ahead by default, as pflow-native's ODE player.
    pub const HORIZON: f64 = 3.0;

    /// The estimate of `g`, solving [`OdeEstimate::HORIZON`] ahead.
    pub fn new(g: &Game) -> Self {
        let ix = &g.ix;
        let weighed = |xs: &[(usize, i64)]| xs.iter().map(|&(p, w)| (p, w as f64)).collect();
        let flows = (0..ix.transitions.len())
            .map(|t| Flow {
                gates: ix
                    .inputs(t)
                    .iter()
                    .chain(ix.reads(t))
                    .map(|x| x.0)
                    .collect(),
                takes: weighed(ix.inputs(t)),
                puts: weighed(ix.outputs(t)),
            })
            .collect();
        Self {
            flows,
            places: ix.places.len(),
            reward: g.reward.clone(),
            horizon: Self::HORIZON,
            method: methods::tsit5(),
            opts: Self::options(Self::HORIZON),
        }
    }

    /// Solves `horizon` ahead instead (a positive number).
    pub fn with_horizon(mut self, horizon: f64) -> Self {
        self.horizon = horizon;
        self.opts = Self::options(horizon);
        self
    }

    /// pflow-native's: the default options, with at least 50 steps over
    /// the horizon.
    fn options(horizon: f64) -> Options {
        let mut o = Options::default_opts();
        o.dtmax = o.dtmax.min(horizon / 50.0).max(o.dtmin);
        o.dt = o.dt.min(o.dtmax);
        o
    }

    /// du/dt at `u`.
    fn rate(&self, u: &[f64]) -> Vec<f64> {
        let mut du = vec![0.0; self.places];
        for f in &self.flows {
            let mut flux = 1.0;
            for &p in &f.gates {
                if u[p] <= 0.0 {
                    flux = 0.0;
                    break;
                }
                flux *= u[p];
            }
            if flux > 0.0 {
                for &(p, w) in &f.takes {
                    du[p] -= flux * w;
                }
                for &(p, w) in &f.puts {
                    du[p] += flux * w;
                }
            }
        }
        du
    }

    /// The state `horizon` ahead of marking `mk`, and the steps taken.
    pub fn final_state(&self, mk: &[i64]) -> (Vec<f64>, usize) {
        let u0: Vec<f64> = mk.iter().map(|&v| v as f64).collect();
        let f = |_: f64, u: &[f64]| self.rate(u);
        let (t, mut u) = integrate_vec(&f, &u0, [0.0, self.horizon], &self.method, &self.opts);
        (u.pop().unwrap_or(u0), t.len() - 1)
    }

    /// The same continuous net as a pflow-core net (a read arc as an arc
    /// each way) with marking `mk` as its state: what an [`crate::Problem`]
    /// is built from, when the net is compiled for every solve.
    pub fn petri_net(g: &Game, mk: &[i64]) -> (PetriNet, State) {
        let ix = &g.ix;
        let mut b = PetriNet::build();
        for (p, id) in ix.places.iter().enumerate() {
            b = b.place(id, mk[p] as f64);
        }
        for (t, id) in ix.transitions.iter().enumerate() {
            b = b.transition(id);
            for &(p, w) in ix.inputs(t) {
                b = b.arc(&ix.places[p], id, w as f64);
            }
            for &(p, w) in ix.reads(t) {
                b = b
                    .arc(&ix.places[p], id, w as f64)
                    .arc(id, &ix.places[p], w as f64);
            }
            for &(p, w) in ix.outputs(t) {
                b = b.arc(id, &ix.places[p], w as f64);
            }
        }
        let net = b.done();
        let state = net.set_state(None);
        (net, state)
    }

    /// The options a solve takes, for running the same one elsewhere.
    pub fn solve_options(&self) -> &Options {
        &self.opts
    }
}

impl Estimate for OdeEstimate {
    fn name(&self) -> &'static str {
        "ode"
    }

    fn estimate(&mut self, _: &Game, mk: &[i64]) -> Result<f64> {
        let (u, _) = self.final_state(mk);
        Ok(self.reward.iter().map(|&(p, w)| w * u[p]).sum())
    }
}

/// [`SearchPlan::solver_with`], given the [`OdeEstimate`] of `g` when the
/// plan's root names `ode`.
pub fn solver<'a>(plan: &SearchPlan, g: &'a Game) -> Result<Solver<'a>> {
    match plan.estimate()? {
        Some("ode") => plan.solver_with(g, Some(Box::new(OdeEstimate::new(g)))),
        _ => plan.solver(g),
    }
}
