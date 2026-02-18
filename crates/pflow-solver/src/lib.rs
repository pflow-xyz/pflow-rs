//! ODE solvers for Petri net simulation using mass-action kinetics.

pub mod equilibrium;
pub mod implicit;
pub mod methods;
pub mod ode;

pub use equilibrium::{
    find_equilibrium, find_equilibrium_accurate, find_equilibrium_fast, is_equilibrium,
    solve_until_equilibrium, EquilibriumOptions, EquilibriumResult, OptionPair,
};
pub use methods::Solver;
pub use ode::{copy_state, solve, ODEFunc, Options, Problem, Solution};
