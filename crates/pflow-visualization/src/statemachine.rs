//! State chart SVG rendering, ported from go-pflow's
//! `visualization/statemachine_svg.go`.
//!
//! **Layout adapts to a structural difference between the two `Chart`
//! models, not a missing feature.** go-pflow's `Region.States` is a *flat*
//! map where a substate's `State.Parent` points back at its parent (so
//! parent and child are siblings in the same map, and hierarchy comes from
//! walking `Parent`, to arbitrary depth in principle). `pflow-statemachine`'s
//! [`pflow_statemachine::types::State`] instead nests substates in
//! `children`, and [`pflow_statemachine::builder::ChartBuilder::sub`] never
//! nests a child under a child — see that module's doc and tests — so the
//! hierarchy is exactly one level deep by construction on this side. This
//! module's `layout_state_machine` walks that nesting directly (level 0 =
//! top-level states, level 1 = their `children`) rather than porting Go's
//! parent-pointer level assignment, which has no input it could ever
//! disagree with on a chart built through this crate's own builder.
//!
//! **Not ported: per-transition label-collision offsetting.**
//! `calculateTransitionOffsets`/`assignOffsets` in go-pflow spread
//! overlapping vertical-transition labels apart by trial-and-error Y deltas.
//! This port draws every non-horizontal transition at one fixed curve
//! offset (side chosen by direction, same as Go's base case) — a labeled
//! diagram with many transitions between the same pair of levels can overlap
//! text, a cosmetic gap tracked here rather than silently claimed as parity.

use std::collections::HashMap;
use std::fmt::Write as _;

use pflow_statemachine::types::{Chart, Region, State, StatePath, Transition};

use crate::colors::escape_xml;

/// Sorted keys of a `HashMap<String, _>`, for deterministic draw order —
/// mirrors `pflow_statemachine::types::sorted_keys`, which is
/// crate-private there.
fn sorted_keys<V>(m: &HashMap<String, V>) -> Vec<String> {
    let mut out: Vec<String> = m.keys().cloned().collect();
    out.sort();
    out
}

/// Controls state machine rendering, matching go-pflow's
/// `StateMachineSVGOptions`.
#[derive(Clone, Debug)]
pub struct StateMachineSvgOptions {
    pub state_width: f64,
    pub state_height: f64,
    pub state_spacing_x: f64,
    pub state_spacing_y: f64,
    pub region_spacing: f64,
    pub padding: f64,
    pub show_labels: bool,
    pub show_events: bool,
    pub show_initial: bool,
    pub color_by_region: bool,
}

impl Default for StateMachineSvgOptions {
    fn default() -> Self {
        StateMachineSvgOptions {
            state_width: 100.0,
            state_height: 40.0,
            state_spacing_x: 150.0,
            state_spacing_y: 70.0,
            region_spacing: 100.0,
            padding: 60.0,
            show_labels: true,
            show_events: true,
            show_initial: true,
            color_by_region: true,
        }
    }
}

const REGION_COLORS: [&str; 6] = ["#e3f2fd", "#f3e5f5", "#e8f5e9", "#fff3e0", "#fce4ec", "#e0f7fa"];
const REGION_STROKES: [&str; 6] = ["#1976d2", "#7b1fa2", "#388e3c", "#f57c00", "#c2185b", "#0097a7"];

#[derive(Clone, Copy, Default)]
struct StatePos {
    x: f64,
    y: f64,
}

struct RegionLayout {
    states: HashMap<String, StatePos>,
}

/// Renders a [`Chart`] as an SVG document, matching go-pflow's
/// `RenderStateMachineSVG`.
pub fn render_state_machine_svg(chart: &Chart, opts: &StateMachineSvgOptions) -> String {
    let layout = layout_state_machine(chart, opts);

    let (mut min_x, mut min_y, mut max_x, mut max_y) = calculate_bounds(&layout, opts);
    min_x -= opts.padding;
    min_y -= opts.padding;
    max_x += opts.padding;
    max_y += opts.padding;

    let mut width = max_x - min_x;
    let mut height = max_y - min_y;
    if width < 200.0 {
        width = 200.0;
    }
    if height < 100.0 {
        height = 100.0;
    }

    let mut buf = String::new();
    let _ = writeln!(
        buf,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{:.1} {:.1} {:.1} {:.1}" width="{:.0}" height="{:.0}">"##,
        min_x, min_y, width, height, width, height
    );
    let _ = writeln!(
        buf,
        r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#f8f9fa" rx="8"/>"##,
        min_x, min_y, width, height
    );

    buf.push_str("<defs>");
    buf.push_str("<style>");
    buf.push_str(".state { stroke-width: 2; rx: 8; }");
    buf.push_str(".state-initial { stroke-width: 3; }");
    buf.push_str(".state-composite { stroke-dasharray: 5,3; }");
    buf.push_str(".transition { stroke: #666; stroke-width: 1.5; fill: none; }");
    buf.push_str(".transition-self { stroke: #999; }");
    buf.push_str(".arrowhead { fill: #666; }");
    buf.push_str(".initial-marker { fill: #333; }");
    buf.push_str(".state-label { font-family: system-ui, Arial; font-size: 11px; fill: #333; text-anchor: middle; dominant-baseline: middle; }");
    buf.push_str(".event-label { font-family: system-ui, Arial; font-size: 9px; fill: #666; text-anchor: middle; }");
    buf.push_str(".region-label { font-family: system-ui, Arial; font-size: 10px; fill: #999; font-style: italic; }");
    buf.push_str(".region-box { fill: none; stroke: #ddd; stroke-width: 1; stroke-dasharray: 3,3; }");
    buf.push_str(".chart-title { font-family: system-ui, Arial; font-size: 14px; font-weight: bold; fill: #333; }");
    buf.push_str("</style>");

    buf.push_str(r##"<marker id="sm-arrowhead" markerWidth="10" markerHeight="7" refX="9" refY="3.5" orient="auto">"##);
    buf.push_str(r##"<polygon points="0 0, 10 3.5, 0 7" class="arrowhead"/>"##);
    buf.push_str("</marker>");
    buf.push_str("</defs>\n");

    if !chart.name.is_empty() {
        let _ = writeln!(
            buf,
            r##"<text x="{:.1}" y="{:.1}" class="chart-title">{}</text>"##,
            min_x + 10.0,
            min_y + 20.0,
            escape_xml(&chart.name)
        );
    }

    for region_name in sorted_keys(&layout) {
        draw_region_box(&mut buf, &region_name, &layout[&region_name]);
    }

    for trans in &chart.transitions {
        draw_transition(&mut buf, chart, trans, &layout, opts);
    }

    for (color_idx, region_name) in sorted_keys(&chart.regions).into_iter().enumerate() {
        let region = &chart.regions[&region_name];
        let region_layout = &layout[&region_name];
        let (fill, stroke) = if opts.color_by_region {
            (REGION_COLORS[color_idx % REGION_COLORS.len()], REGION_STROKES[color_idx % REGION_STROKES.len()])
        } else {
            ("#fafafa", "#666")
        };

        for state_name in sorted_keys(&region.states) {
            let state = &region.states[&state_name];
            if let Some(&pos) = region_layout.states.get(&state_name) {
                let is_initial = state_name == region.initial;
                draw_state(&mut buf, state, &state_name, pos, is_initial, fill, stroke, opts);
            }
            for sub_name in sorted_keys(&state.children) {
                let sub = &state.children[&sub_name];
                let key = format!("{state_name}:{sub_name}");
                if let Some(&pos) = region_layout.states.get(&key) {
                    draw_state(&mut buf, sub, &sub_name, pos, sub.initial, fill, stroke, opts);
                }
            }
        }
    }

    if opts.show_initial {
        for region_name in sorted_keys(&chart.regions) {
            let region = &chart.regions[&region_name];
            if region.initial.is_empty() {
                continue;
            }
            if let Some(&pos) = layout[&region_name].states.get(&region.initial) {
                draw_initial_marker(&mut buf, pos, opts);
            }
        }
    }

    buf.push_str("</svg>\n");
    buf
}

/// Renders a [`Chart`] and writes it to `path`.
pub fn save_state_machine_svg(chart: &Chart, path: &std::path::Path, opts: &StateMachineSvgOptions) -> std::io::Result<()> {
    std::fs::write(path, render_state_machine_svg(chart, opts))
}

fn layout_state_machine(chart: &Chart, opts: &StateMachineSvgOptions) -> HashMap<String, RegionLayout> {
    let mut layout = HashMap::new();
    let mut region_y = 0.0;
    for region_name in sorted_keys(&chart.regions) {
        let region = &chart.regions[&region_name];
        let (reg_layout, height) = layout_region(region, region_y, opts);
        layout.insert(region_name, reg_layout);
        region_y += height + opts.region_spacing;
    }
    layout
}

/// Assigns positions within one region: top-level states at x-level 0
/// (ordered alphabetically with the region's initial state first), each
/// composite state's `children` immediately at x-level 1. Returns the
/// layout and its total height for stacking the next region below it.
fn layout_region(region: &Region, start_y: f64, opts: &StateMachineSvgOptions) -> (RegionLayout, f64) {
    let mut state_names = sorted_keys(&region.states);
    if !region.initial.is_empty() {
        if let Some(idx) = state_names.iter().position(|n| n == &region.initial) {
            state_names.swap(0, idx);
        }
    }

    let mut states = HashMap::new();
    let mut level1_i = 0usize;
    let mut max_width = 0.0f64;
    let mut max_height = 0.0f64;

    for (level0_i, name) in state_names.iter().enumerate() {
        let x = 0.0;
        let y = start_y + level0_i as f64 * opts.state_spacing_y;
        states.insert(name.clone(), StatePos { x, y });
        if x + opts.state_width > max_width {
            max_width = x + opts.state_width;
        }
        if y + opts.state_height - start_y > max_height {
            max_height = y + opts.state_height - start_y;
        }

        let state = &region.states[name];
        let mut sub_names = sorted_keys(&state.children);
        sub_names.sort_by_key(|n| !state.children[n].initial);
        for sub_name in sub_names {
            let sx = opts.state_spacing_x;
            let sy = start_y + level1_i as f64 * opts.state_spacing_y;
            states.insert(format!("{name}:{sub_name}"), StatePos { x: sx, y: sy });
            level1_i += 1;
            if sx + opts.state_width > max_width {
                max_width = sx + opts.state_width;
            }
            if sy + opts.state_height - start_y > max_height {
                max_height = sy + opts.state_height - start_y;
            }
        }
    }

    (RegionLayout { states }, max_height + opts.padding)
}

fn calculate_bounds(layout: &HashMap<String, RegionLayout>, opts: &StateMachineSvgOptions) -> (f64, f64, f64, f64) {
    let mut first = true;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (0.0, 0.0, 0.0, 0.0);
    for reg in layout.values() {
        for pos in reg.states.values() {
            let node_min_x = pos.x - opts.state_width / 2.0;
            let node_max_x = pos.x + opts.state_width / 2.0;
            let node_min_y = pos.y - opts.state_height / 2.0;
            let node_max_y = pos.y + opts.state_height / 2.0;
            if first {
                min_x = node_min_x;
                max_x = node_max_x;
                min_y = node_min_y;
                max_y = node_max_y;
                first = false;
            } else {
                min_x = min_x.min(node_min_x);
                max_x = max_x.max(node_max_x);
                min_y = min_y.min(node_min_y);
                max_y = max_y.max(node_max_y);
            }
        }
    }

    let curve_space = opts.state_width / 2.0 + 30.0 + 50.0;
    max_x += curve_space;
    min_x -= curve_space;

    (min_x, min_y, max_x, max_y)
}

fn draw_region_box(buf: &mut String, name: &str, layout: &RegionLayout) {
    let mut first = true;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (0.0, 0.0, 0.0, 0.0);
    for pos in layout.states.values() {
        let node_min_x = pos.x - 10.0;
        let node_max_x = pos.x + 10.0;
        let node_min_y = pos.y - 10.0;
        let node_max_y = pos.y + 10.0;
        if first {
            min_x = node_min_x;
            max_x = node_max_x;
            min_y = node_min_y;
            max_y = node_max_y;
            first = false;
        } else {
            min_x = min_x.min(node_min_x);
            max_x = max_x.max(node_max_x);
            min_y = min_y.min(node_min_y);
            max_y = max_y.max(node_max_y);
        }
    }
    if first {
        return;
    }

    let _ = writeln!(
        buf,
        r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" class="region-box"/>"##,
        min_x,
        min_y,
        max_x - min_x,
        max_y - min_y
    );
    let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="region-label">{}</text>"##, min_x + 5.0, min_y - 5.0, escape_xml(name));
}

#[allow(clippy::too_many_arguments)]
fn draw_state(buf: &mut String, state: &State, name: &str, pos: StatePos, is_initial: bool, fill: &str, stroke: &str, opts: &StateMachineSvgOptions) {
    let x = pos.x - opts.state_width / 2.0;
    let y = pos.y - opts.state_height / 2.0;

    let mut class = String::from("state");
    if is_initial {
        class.push_str(" state-initial");
    }
    if !state.is_leaf {
        class.push_str(" state-composite");
    }

    let _ = writeln!(
        buf,
        r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="8" fill="{}" stroke="{}" class="{}"/>"##,
        x, y, opts.state_width, opts.state_height, fill, stroke, class
    );

    if opts.show_labels {
        let mut label = if state.name.is_empty() { name.to_string() } else { state.name.clone() };
        if label.chars().count() > 12 {
            label = label.chars().take(9).collect::<String>() + "...";
        }
        let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="state-label">{}</text>"##, pos.x, pos.y, escape_xml(&label));
    }
}

fn draw_initial_marker(buf: &mut String, pos: StatePos, opts: &StateMachineSvgOptions) {
    let marker_x = pos.x - opts.state_width / 2.0 - 20.0;
    let marker_y = pos.y;

    let _ = writeln!(buf, r##"<circle cx="{:.1}" cy="{:.1}" r="6" class="initial-marker"/>"##, marker_x, marker_y);
    let _ = writeln!(
        buf,
        r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#333" stroke-width="1.5" marker-end="url(#sm-arrowhead)"/>"##,
        marker_x + 8.0,
        marker_y,
        pos.x - opts.state_width / 2.0 - 2.0,
        marker_y
    );
}

/// Resolves a transition endpoint's `(region, layout key)`, matching
/// go-pflow's flat-path handling: a two-segment path (`"region:state"`)
/// names a top-level state directly; a three-segment path
/// (`"region:state:sub"`) names a nested substate.
fn resolve_endpoint(chart: &Chart, path: &str) -> Option<(String, String)> {
    let sp = StatePath(path.to_string());
    let region = sp.region();
    let state = sp.state();
    if state.is_empty() {
        // Flat path with no region prefix: find which region has it.
        let name = region;
        for (region_name, r) in &chart.regions {
            if r.states.contains_key(&name) {
                return Some((region_name.clone(), name));
            }
        }
        return None;
    }
    if !chart.regions.get(&region)?.states.contains_key(&state) {
        return None;
    }
    let sub = sp.substate();
    if sub.is_empty() {
        Some((region, state))
    } else {
        Some((region, format!("{state}:{sub}")))
    }
}

fn draw_transition(buf: &mut String, chart: &Chart, trans: &Transition, layout: &HashMap<String, RegionLayout>, opts: &StateMachineSvgOptions) {
    let Some((src_region, src_key)) = resolve_endpoint(chart, &trans.source) else { return };
    let Some((trg_region, trg_key)) = resolve_endpoint(chart, &trans.target) else { return };

    let Some(&src_pos) = layout.get(&src_region).and_then(|l| l.states.get(&src_key)) else { return };
    let Some(&trg_pos) = layout.get(&trg_region).and_then(|l| l.states.get(&trg_key)) else { return };

    if src_region == trg_region && src_key == trg_key {
        draw_self_transition(buf, src_pos, &trans.event, opts);
        return;
    }

    let dx = trg_pos.x - src_pos.x;
    let dy = trg_pos.y - src_pos.y;

    let (mut x1, mut y1, mut x2, mut y2);
    if dx.abs() > dy.abs() {
        if dx > 0.0 {
            x1 = src_pos.x + opts.state_width / 2.0;
            x2 = trg_pos.x - opts.state_width / 2.0;
        } else {
            x1 = src_pos.x - opts.state_width / 2.0;
            x2 = trg_pos.x + opts.state_width / 2.0;
        }
        y1 = src_pos.y;
        y2 = trg_pos.y;
    } else {
        if dy > 0.0 {
            y1 = src_pos.y + opts.state_height / 2.0;
            y2 = trg_pos.y - opts.state_height / 2.0;
        } else {
            y1 = src_pos.y - opts.state_height / 2.0;
            y2 = trg_pos.y + opts.state_height / 2.0;
        }
        x1 = src_pos.x;
        x2 = trg_pos.x;
    }

    let arrow_offset = 2.0;
    let final_dx = x2 - x1;
    let final_dy = y2 - y1;
    let dist = (final_dx * final_dx + final_dy * final_dy).sqrt().max(1.0);
    x2 -= (final_dx / dist) * arrow_offset;
    y2 -= (final_dy / dist) * arrow_offset;
    let _ = (&mut x1, &mut y1);

    let class = "transition";

    if dx.abs() > dy.abs() {
        if (y1 - y2).abs() < 5.0 {
            let _ = writeln!(
                buf,
                r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" class="{}" marker-end="url(#sm-arrowhead)"/>"##,
                x1, y1, x2, y2, class
            );
        } else {
            let mid_x = (x1 + x2) / 2.0;
            let _ = writeln!(
                buf,
                r##"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" class="{}" marker-end="url(#sm-arrowhead)"/>"##,
                x1, y1, mid_x, y1, mid_x, y2, x2, y2, class
            );
        }
        if opts.show_events && !trans.event.is_empty() {
            let label_x = (x1 + x2) / 2.0;
            let label_y = (y1 + y2) / 2.0 - 10.0;
            let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="event-label">{}</text>"##, label_x, label_y, escape_xml(&trans.event));
        }
    } else {
        let curve_offset = opts.state_width / 2.0 + 30.0;
        let mid_y = (y1 + y2) / 2.0;

        if dy > 0.0 {
            let _ = writeln!(
                buf,
                r##"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" class="{}" marker-end="url(#sm-arrowhead)"/>"##,
                x1,
                y1,
                x1 + curve_offset,
                mid_y,
                x2 + curve_offset,
                mid_y,
                x2,
                y2,
                class
            );
            if opts.show_events && !trans.event.is_empty() {
                let label_x = src_pos.x + opts.state_width / 2.0 + 35.0;
                let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="event-label">{}</text>"##, label_x, mid_y, escape_xml(&trans.event));
            }
        } else {
            let _ = writeln!(
                buf,
                r##"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" class="{}" marker-end="url(#sm-arrowhead)"/>"##,
                x1,
                y1,
                x1 - curve_offset,
                mid_y,
                x2 - curve_offset,
                mid_y,
                x2,
                y2,
                class
            );
            if opts.show_events && !trans.event.is_empty() {
                let label_x = src_pos.x - opts.state_width / 2.0 - 35.0;
                let _ = writeln!(
                    buf,
                    r##"<text x="{:.1}" y="{:.1}" class="event-label" text-anchor="end">{}</text>"##,
                    label_x,
                    mid_y,
                    escape_xml(&trans.event)
                );
            }
        }
    }
}

fn draw_self_transition(buf: &mut String, pos: StatePos, event: &str, opts: &StateMachineSvgOptions) {
    let x = pos.x;
    let y = pos.y - opts.state_height / 2.0;
    let loop_height = 25.0;
    let loop_width = 20.0;

    let _ = writeln!(
        buf,
        r##"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" class="transition transition-self" marker-end="url(#sm-arrowhead)"/>"##,
        x - loop_width,
        y,
        x - loop_width,
        y - loop_height,
        x + loop_width,
        y - loop_height,
        x + loop_width - 5.0,
        y
    );

    if !event.is_empty() {
        let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="event-label">{}</text>"##, x, y - loop_height - 5.0, escape_xml(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_statemachine::builder::ChartBuilder;

    fn traffic_light() -> Chart {
        let mut b = ChartBuilder::new("light");
        b.region("state")
            .state("red")
            .initial()
            .state("green")
            .state("yellow")
            .end_region()
            .when("timer")
            .in_("state:red")
            .go_to("state:green")
            .when("timer")
            .in_("state:green")
            .go_to("state:yellow")
            .when("timer")
            .in_("state:yellow")
            .go_to("state:red");
        b.build()
    }

    #[test]
    fn renders_well_formed_svg() {
        let svg = render_state_machine_svg(&traffic_light(), &StateMachineSvgOptions::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("viewBox="));
    }

    #[test]
    fn every_state_and_the_title_appear() {
        let svg = render_state_machine_svg(&traffic_light(), &StateMachineSvgOptions::default());
        assert!(svg.contains(">light<"));
        assert!(svg.contains(">red<"));
        assert!(svg.contains(">green<"));
        assert!(svg.contains(">yellow<"));
    }

    #[test]
    fn initial_state_gets_the_initial_class_and_marker() {
        let svg = render_state_machine_svg(&traffic_light(), &StateMachineSvgOptions::default());
        assert!(svg.contains("state state-initial"));
        assert!(svg.contains("initial-marker"));
    }

    #[test]
    fn wraparound_transition_is_a_self_loop_when_endpoints_match() {
        let mut b = ChartBuilder::new("spin");
        b.region("state").state("only").initial().end_region().when("noop").in_("state:only").go_to("state:only");
        let chart = b.build();
        let svg = render_state_machine_svg(&chart, &StateMachineSvgOptions::default());
        assert!(svg.contains("transition-self"));
    }

    #[test]
    fn nested_substates_render_as_their_own_boxes() {
        let mut b = ChartBuilder::new("clock");
        b.region("mode").state("dateTime").sub("default").initial().end().sub("holding").end();
        let chart = b.build();
        let svg = render_state_machine_svg(&chart, &StateMachineSvgOptions::default());
        assert!(svg.contains(">dateTime<"));
        assert!(svg.contains(">default<"));
        assert!(svg.contains(">holding<"));
        // The composite parent carries the dashed-border class.
        assert!(svg.contains("state-composite"));
    }

    #[test]
    fn event_label_is_drawn_on_non_self_transitions() {
        let svg = render_state_machine_svg(&traffic_light(), &StateMachineSvgOptions::default());
        assert!(svg.contains(">timer<"));
    }

    #[test]
    fn empty_chart_still_produces_a_well_formed_svg() {
        // No states means the bounds default to (0,0,0,0); the fixed
        // curve-space margin (`state_width/2 + 30 + 50`) added on top of
        // that alone already exceeds both size floors, so — unlike an
        // empty net or an empty workflow — neither floor triggers here.
        // What matters is that an empty chart still renders cleanly.
        let svg = render_state_machine_svg(&Chart::default(), &StateMachineSvgOptions::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("viewBox="));
    }

    #[test]
    fn chart_name_is_xml_escaped() {
        let mut b = ChartBuilder::new("A & B");
        b.region("state").state("s").initial();
        let chart = b.build();
        let svg = render_state_machine_svg(&chart, &StateMachineSvgOptions::default());
        assert!(svg.contains("A &amp; B"));
    }
}
