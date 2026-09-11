//! SVG rendering for Petri nets, state charts, workflows and ODE solutions,
//! ported from go-pflow's `visualization` and `plotter` packages.
//!
//! Each renderer is held to structural unit tests rather than a
//! byte-identical golden: go-pflow has no checked-in reference SVGs under
//! `visualization/testdata` or `plotter/testdata` (confirmed by reading the
//! packages directly, 2026-09-10; its own tests assert structure the same
//! way, e.g. `strings.Contains(svg, ...)`), and node/task/state draw order
//! in both languages follows unordered map iteration, which is not
//! reproducible even between two runs on the Go side. See each module's own
//! doc comment for what it ports field-for-field versus deliberately
//! narrows, and why.

pub mod colors;
pub mod net;
pub mod plotter;
pub mod statemachine;
pub mod workflow;

pub use net::{render_petri_net_svg, save_petri_net_svg};
pub use plotter::{plot_solution, PlotData, Series, SvgPlotter};
pub use statemachine::{render_state_machine_svg, save_state_machine_svg, StateMachineSvgOptions};
pub use workflow::{render_workflow_svg, save_workflow_svg, WorkflowSvgOptions};
