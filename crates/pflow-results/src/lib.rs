//! Structured simulation results — the `results` half of ROADMAP.md
//! Phase 4, ported from go-pflow's `results` package (types, analysis,
//! builder, io).
//!
//! `results` has no go-pflow-produced golden fixture (no `cmd/*-goldens`
//! analogue emits one, the same interim state Phase 3 documents for
//! `templates`/`derive`), so this port is held to Rust unit tests instead.
//! go-pflow's `hypothesis` and `sensitivity` packages and `results/sweep.go`
//! were ported here once and removed 2026-10-21 because nothing consumed
//! them (see ROADMAP.md, *Not ported, on purpose*).

pub mod results;
