//! SVG rendering for Petri nets and ODE solutions, ported from go-pflow's
//! `visualization` and `plotter` packages. The state-chart and workflow
//! renderers (`statemachine_svg.go`, `workflow_svg.go`) were ported once and
//! removed 2026-10-21 along with the crates they drew, for want of a
//! consumer (see ROADMAP.md, *Not ported, on purpose*).
//!
//! Each renderer is held to structural unit tests rather than a
//! byte-identical golden: go-pflow has no checked-in reference SVGs under
//! `visualization/testdata` or `plotter/testdata` (confirmed by reading the
//! packages directly, 2026-09-10; its own tests assert structure the same
//! way, e.g. `strings.Contains(svg, ...)`), and node draw order
//! in both languages follows unordered map iteration, which is not
//! reproducible even between two runs on the Go side. See each module's own
//! doc comment for what it ports field-for-field versus deliberately
//! narrows, and why.

pub mod colors;
pub mod net;
pub mod plotter;

pub use net::{render_petri_net_svg, save_petri_net_svg};
pub use plotter::{plot_solution, PlotData, Series, SvgPlotter};
