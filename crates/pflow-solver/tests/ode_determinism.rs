//! ODE results must not depend on `HashMap` iteration order.
//!
//! `PetriNet::places`/`transitions` and `State` are std `HashMap`s, and every
//! fresh map gets its own random hash keys, so two builds of the *same* net
//! iterate their transitions in different orders. The solver used to walk
//! those maps directly to lay out the state vector and to accumulate each
//! place's flux terms, so the same net could integrate to answers differing in
//! the last bits (and, through the step controller, in step count). These
//! tests build one net many times over fresh maps and demand bit-identical
//! results (`to_bits` equality, no tolerance) from every solver entry point.

use std::collections::HashMap;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_solver::equilibrium::{solve_until_equilibrium, EquilibriumOptions};
use pflow_solver::implicit::{implicit_euler, trbdf2};
use pflow_solver::{methods, ode};

const BUILDS: usize = 16;
const SOURCES: usize = 12;

/// A hub place fed by many transitions (so its `du` sums many flux terms whose
/// float sum depends on order), drained by a bimolecular and a unimolecular
/// sink. `rotate` varies insertion order too; the net's content is identical
/// for every `rotate`.
fn build(rotate: usize) -> (PetriNet, HashMap<String, f64>) {
    let mut net = PetriNet::new();
    let mut rates = HashMap::new();
    net.add_place("hub", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("partner", vec![3.7], vec![], 0.0, 0.0, None);
    net.add_place("sink", vec![0.0], vec![], 0.0, 0.0, None);
    for k in 0..SOURCES {
        let i = (k + rotate) % SOURCES;
        let src = format!("s{i:02}");
        let t = format!("feed{i:02}");
        net.add_place(
            src.clone(),
            vec![1.0 + i as f64 / 3.0],
            vec![],
            0.0,
            0.0,
            None,
        );
        net.add_transition(t.clone(), "default", 0.0, 0.0, None);
        net.add_arc(src, t.clone(), vec![1.0], false);
        net.add_arc(t.clone(), "hub", vec![1.0 + (i % 3) as f64], false);
        rates.insert(t, 0.1 + 0.7 * i as f64 / 11.0);
    }
    net.add_transition("bind", "default", 0.0, 0.0, None);
    net.add_arc("hub", "bind", vec![1.0], false);
    net.add_arc("partner", "bind", vec![1.0], false);
    net.add_arc("bind", "sink", vec![1.0], false);
    rates.insert("bind".into(), 0.03);
    net.add_transition("decay", "default", 0.0, 0.0, None);
    net.add_arc("hub", "decay", vec![1.0], false);
    net.add_arc("decay", "sink", vec![1.0], false);
    rates.insert("decay".into(), 0.21);
    (net, rates)
}

fn problem(rotate: usize) -> ode::Problem {
    let (net, rates) = build(rotate);
    // Rebuild the initial state and rates into fresh maps as well.
    let state = net.set_state(None);
    let rates = net.set_rates(Some(&rates));
    ode::Problem::new(net, state, [0.0, 30.0], rates)
}

fn assert_state_bits_eq(ctx: &str, a: &State, b: &State) {
    assert_eq!(a.len(), b.len(), "{ctx}: state size");
    for (k, va) in a {
        let vb = b.get(k).unwrap_or_else(|| panic!("{ctx}: {k} missing"));
        assert!(
            va.to_bits() == vb.to_bits(),
            "{ctx}: {k}: {va:?} ({:#018x}) vs {vb:?} ({:#018x})",
            va.to_bits(),
            vb.to_bits()
        );
    }
}

fn assert_solution_bits_eq(ctx: &str, a: &ode::Solution, b: &ode::Solution) {
    assert_eq!(a.state_labels, b.state_labels, "{ctx}: state_labels order");
    assert_eq!(a.t.len(), b.t.len(), "{ctx}: step count");
    for (i, (ta, tb)) in a.t.iter().zip(&b.t).enumerate() {
        assert!(
            ta.to_bits() == tb.to_bits(),
            "{ctx}: t[{i}]: {ta:?} vs {tb:?}"
        );
    }
    for (i, (ua, ub)) in a.u.iter().zip(&b.u).enumerate() {
        assert_state_bits_eq(&format!("{ctx}: u[{i}]"), ua, ub);
    }
}

/// The premise of the other tests: fresh maps really do iterate differently.
/// If this ever fails, the determinism tests below have become vacuous.
#[test]
fn fresh_builds_iterate_transitions_in_different_orders() {
    let orders: Vec<Vec<String>> = (0..BUILDS)
        .map(|r| build(r).0.transitions.keys().cloned().collect())
        .collect();
    assert!(
        orders.iter().any(|o| o != &orders[0]),
        "all {BUILDS} builds iterated transitions identically"
    );
}

#[test]
fn state_labels_are_sorted() {
    let prob = problem(0);
    let mut sorted = prob.state_labels.clone();
    sorted.sort();
    assert_eq!(prob.state_labels, sorted);
}

#[test]
fn tsit5_solve_is_bit_identical_across_builds() {
    let opts = ode::Options::default_opts();
    let reference = ode::solve(&problem(0), &methods::tsit5(), &opts);
    assert!(reference.t.len() > 10, "integration should take real steps");
    for r in 1..BUILDS {
        let sol = ode::solve(&problem(r), &methods::tsit5(), &opts);
        assert_solution_bits_eq(&format!("tsit5 build {r}"), &reference, &sol);
    }
}

#[test]
fn scalar_derivative_is_bit_identical_across_builds() {
    let p0 = problem(0);
    let du0 = (p0.f)(0.0, &p0.u0);
    for r in 1..BUILDS {
        let p = problem(r);
        assert_state_bits_eq(&format!("f(u0) build {r}"), &du0, &(p.f)(0.0, &p.u0));
    }
}

#[test]
fn equilibrium_and_implicit_are_bit_identical_across_builds() {
    let opts = ode::Options::default_opts();
    let eq_opts = EquilibriumOptions::default_opts();
    let fixed = ode::Options {
        dt: 0.05,
        adaptive: false,
        ..ode::Options::default_opts()
    };

    let p0 = problem(0);
    let (eq_sol0, eq0) = solve_until_equilibrium(&p0, &methods::tsit5(), &opts, &eq_opts);
    let ie0 = implicit_euler(&p0, &fixed);
    let tr0 = trbdf2(&p0, &fixed);

    for r in 1..BUILDS {
        let p = problem(r);
        let (eq_sol, eq) = solve_until_equilibrium(&p, &methods::tsit5(), &opts, &eq_opts);
        assert_solution_bits_eq(&format!("equilibrium build {r}"), &eq_sol0, &eq_sol);
        assert_eq!(eq0.steps, eq.steps, "equilibrium build {r}: steps");
        assert!(
            eq0.time.to_bits() == eq.time.to_bits(),
            "equilibrium build {r}: time"
        );
        assert!(
            eq0.max_change.to_bits() == eq.max_change.to_bits(),
            "equilibrium build {r}: max_change"
        );
        assert_state_bits_eq(
            &format!("equilibrium build {r}: state"),
            &eq0.state,
            &eq.state,
        );

        assert_solution_bits_eq(
            &format!("implicit_euler build {r}"),
            &ie0,
            &implicit_euler(&p, &fixed),
        );
        assert_solution_bits_eq(&format!("trbdf2 build {r}"), &tr0, &trbdf2(&p, &fixed));
    }
}
