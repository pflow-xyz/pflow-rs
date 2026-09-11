//! Petri net SVG rendering, ported from go-pflow's
//! `visualization/render.go` and `visualization/svg.go`.
//!
//! go-pflow renders from its own `parser`-shaped `PetriNet` JSON-LD struct
//! (`visualization.PetriNet`), reached by first converting a `petri.PetriNet`
//! into that shape (`convertToJSONLD`) and marshalling it to JSON only to
//! unmarshal it again. [`pflow_core::PetriNet`] already *is* that JSON-LD
//! shape (places/transitions keyed by id with `x`/`y`/`label_text`, arcs
//! with `weight`/`inhibit_transition`), so this port renders directly from
//! it and skips the JSON round trip entirely.
//!
//! Node draw order here follows `HashMap` iteration (unspecified, as it is
//! in Go's own `map` iteration `render.go` draws from) — geometry is not
//! affected, since every position is an explicit `x`/`y` on the node, not a
//! layout computed from draw order. That is also why this module is held to
//! structural unit tests rather than a byte-identical golden: go-pflow's own
//! checked-in example SVGs are one particular random map-iteration ordering
//! from whenever they were generated, not a reproducible output even on the
//! Go side.

use std::collections::HashMap;
use std::fmt::Write as _;

use pflow_core::{Arc as CoreArc, PetriNet, Place as CorePlace};

use crate::colors::{escape_xml, extract_color, lighten_color};

const PLACE_RADIUS: f64 = 16.0;
const TRANSITION_WIDTH: f64 = 30.0;
const TRANSITION_HEIGHT: f64 = 30.0;
const PLACE_PADDING: f64 = 18.0;
const TRANSITION_PADDING: f64 = 17.0;
const ARROWHEAD_SIZE: f64 = 8.0;
const INHIBITOR_RADIUS: f64 = 6.0;
const TIP_OFFSET_MULTIPLIER: f64 = 0.9;
const MIN_DISTANCE: f64 = 1.0;
const TRANSITION_RADIUS: f64 = 4.0;

#[derive(Clone, Copy)]
struct NodePosition {
    x: f64,
    y: f64,
    is_place: bool,
}

/// Renders a [`pflow_core::PetriNet`] as an SVG document, matching
/// go-pflow's `GenerateSVG`.
/// Renders a [`PetriNet`] and writes it to `path`, matching go-pflow's
/// `SaveSVG`.
pub fn save_petri_net_svg(net: &PetriNet, path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, render_petri_net_svg(net))
}

pub fn render_petri_net_svg(net: &PetriNet) -> String {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = calculate_bounds(net);

    let padding = 50.0;
    min_x -= padding;
    min_y -= padding;
    max_x += padding;
    max_y += padding;

    let mut width = max_x - min_x;
    let mut height = max_y - min_y;
    if width < 100.0 {
        width = 100.0;
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
    buf.push_str(".place { fill: #fff; stroke: #333; stroke-width: 2; }");
    buf.push_str(".place-cap-full { fill: #ffebee; }");
    buf.push_str(".transition { fill: #ffffff; stroke: #000; stroke-width: 1; }");
    buf.push_str(".transition-active { fill: #62fa75; stroke: #000; }");
    buf.push_str(".arc { stroke: #cfcfcf; stroke-width: 1; fill: none; }");
    buf.push_str(".arc-active { stroke: #2a6fb8; }");
    buf.push_str(".arrowhead { fill: #cfcfcf; }");
    buf.push_str(".arrowhead-active { fill: #2a6fb8; }");
    buf.push_str(".inhibitor { fill: #fff; stroke: #cfcfcf; stroke-width: 1.3; }");
    buf.push_str(".inhibitor-active { stroke: #2a6fb8; }");
    buf.push_str(".token-dot { fill: #333; }");
    buf.push_str(".token-text { font-family: system-ui, Arial; font-size: 12px; fill: #333; text-anchor: middle; dominant-baseline: middle; }");
    buf.push_str(".weight-badge { font-family: system-ui, Arial; font-size: 10px; fill: #666; text-anchor: middle; dominant-baseline: middle; }");
    buf.push_str(".weight-bg { fill: #fafafa; stroke: #ddd; stroke-width: 1; }");
    buf.push_str(".weight-bg-active { fill: #e8f0fb; stroke: #2a6fb8; }");
    buf.push_str(".label-text { font-family: system-ui, Arial; font-size: 11px; fill: #333; text-anchor: middle; dominant-baseline: hanging; }");
    buf.push_str("</style>");
    buf.push_str("</defs>\n");

    let mut nodes: HashMap<&str, NodePosition> = HashMap::new();
    for (id, place) in &net.places {
        nodes.insert(
            id.as_str(),
            NodePosition { x: place.x, y: place.y, is_place: true },
        );
    }
    for (id, transition) in &net.transitions {
        nodes.insert(
            id.as_str(),
            NodePosition { x: transition.x, y: transition.y, is_place: false },
        );
    }

    let marks = calculate_marking(net);
    let arc_groups = group_arcs_by_node_pair(&net.arcs);

    for (i, arc) in net.arcs.iter().enumerate() {
        let (Some(&src), Some(&trg)) = (nodes.get(arc.source.as_str()), nodes.get(arc.target.as_str()))
        else {
            continue;
        };

        let related_transition_id = if src.is_place { &arc.target } else { &arc.source };
        let active = is_enabled(related_transition_id, net, &marks, &nodes);

        let curve_offset = get_arc_curve_offset(arc, i, &arc_groups);

        draw_arc(&mut buf, src, trg, arc, active, &net.token, curve_offset);
    }

    for (id, place) in &net.places {
        let token_count: i64 = place.initial.iter().sum::<f64>().round() as i64;
        let capacity = get_capacity(place);
        let is_full = capacity.is_finite() && token_count as f64 >= capacity;
        let label = place_label(place, id);
        draw_place(&mut buf, place.x, place.y, token_count, is_full, &label);
    }

    for (id, transition) in &net.transitions {
        let active = is_enabled(id, net, &marks, &nodes);
        let label = transition_label(transition, id);
        draw_transition(&mut buf, transition.x, transition.y, active, &label);
    }

    buf.push_str("</svg>\n");
    buf
}

fn place_label(place: &CorePlace, id: &str) -> String {
    place
        .label_text
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.to_string())
}

fn transition_label(transition: &pflow_core::Transition, id: &str) -> String {
    transition
        .label_text
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.to_string())
}

fn calculate_bounds(net: &PetriNet) -> (f64, f64, f64, f64) {
    let mut first = true;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (0.0, 0.0, 0.0, 0.0);

    let mut update = |x: f64, y: f64, first: &mut bool| {
        if *first {
            min_x = x;
            max_x = x;
            min_y = y;
            max_y = y;
            *first = false;
        } else {
            if x < min_x {
                min_x = x;
            }
            if x > max_x {
                max_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if y > max_y {
                max_y = y;
            }
        }
    };

    for place in net.places.values() {
        update(place.x, place.y, &mut first);
    }
    for transition in net.transitions.values() {
        update(transition.x, transition.y, &mut first);
    }

    (min_x, min_y, max_x, max_y)
}

fn draw_place(buf: &mut String, x: f64, y: f64, token_count: i64, is_full: bool, label: &str) {
    let class = if is_full { "place place-cap-full" } else { "place" };
    let _ = writeln!(
        buf,
        r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" class="{}"/>"##,
        x, y, PLACE_RADIUS, class
    );

    if token_count > 1 {
        let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="token-text">{}</text>"##, x, y, token_count);
    } else if token_count == 1 {
        let _ = writeln!(buf, r##"<circle cx="{:.1}" cy="{:.1}" r="3" class="token-dot"/>"##, x, y);
    }

    if !label.is_empty() {
        let label_y = y + PLACE_RADIUS + 6.0;
        let _ = writeln!(
            buf,
            r##"<text x="{:.1}" y="{:.1}" class="label-text">{}</text>"##,
            x, label_y, escape_xml(label)
        );
    }
}

fn draw_transition(buf: &mut String, x: f64, y: f64, active: bool, label: &str) {
    let class = if active { "transition transition-active" } else { "transition" };
    let _ = writeln!(
        buf,
        r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" ry="{:.1}" class="{}"/>"##,
        x - TRANSITION_WIDTH / 2.0,
        y - TRANSITION_HEIGHT / 2.0,
        TRANSITION_WIDTH,
        TRANSITION_HEIGHT,
        TRANSITION_RADIUS,
        TRANSITION_RADIUS,
        class
    );

    if !label.is_empty() {
        let label_y = y + TRANSITION_HEIGHT / 2.0 + 6.0;
        let _ = writeln!(
            buf,
            r##"<text x="{:.1}" y="{:.1}" class="label-text">{}</text>"##,
            x, label_y, escape_xml(label)
        );
    }
}

fn group_arcs_by_node_pair(arcs: &[CoreArc]) -> HashMap<String, Vec<usize>> {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, arc) in arcs.iter().enumerate() {
        let key = format!("{}->{}", arc.source, arc.target);
        groups.entry(key).or_default().push(idx);
    }
    groups
}

fn get_arc_curve_offset(arc: &CoreArc, arc_idx: usize, arc_groups: &HashMap<String, Vec<usize>>) -> f64 {
    let key = format!("{}->{}", arc.source, arc.target);
    let reverse_key = format!("{}->{}", arc.target, arc.source);

    let empty = Vec::new();
    let group = arc_groups.get(&key).unwrap_or(&empty);
    let reverse_group = arc_groups.get(&reverse_key).unwrap_or(&empty);

    if group.len() == 1 && reverse_group.is_empty() {
        return 0.0;
    }

    let Some(pos_in_group) = group.iter().position(|&idx| idx == arc_idx) else {
        return 0.0;
    };

    let total_arcs = group.len();
    let base_offset = 30.0;

    if !reverse_group.is_empty() {
        if total_arcs == 1 {
            base_offset
        } else {
            base_offset * (1 + pos_in_group) as f64
        }
    } else if total_arcs == 2 {
        if pos_in_group == 0 {
            base_offset
        } else {
            -base_offset
        }
    } else if pos_in_group == 0 {
        0.0
    } else {
        let layer = (pos_in_group as f64 / 2.0).ceil();
        let direction = if pos_in_group % 2 == 0 { -1.0 } else { 1.0 };
        direction * base_offset * layer
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_arc(
    buf: &mut String,
    src: NodePosition,
    trg: NodePosition,
    arc: &CoreArc,
    active: bool,
    tokens: &[String],
    curve_offset: f64,
) {
    let pad_src = if src.is_place { PLACE_PADDING } else { TRANSITION_PADDING };
    let pad_trg = if trg.is_place { PLACE_PADDING } else { TRANSITION_PADDING };

    let dx = trg.x - src.x;
    let dy = trg.y - src.y;
    let mut dist = (dx * dx + dy * dy).sqrt();
    if dist == 0.0 {
        dist = MIN_DISTANCE;
    }
    let ux = dx / dist;
    let uy = dy / dist;

    let tip_offset = if arc.inhibit_transition {
        INHIBITOR_RADIUS + 2.0
    } else {
        ARROWHEAD_SIZE * TIP_OFFSET_MULTIPLIER
    };

    let ex = src.x + ux * pad_src;
    let ey = src.y + uy * pad_src;
    let fx = trg.x - ux * (pad_trg + tip_offset);
    let fy = trg.y - uy * (pad_trg + tip_offset);

    let arc_color = get_arc_color(arc, tokens, active);

    let (end_dir_x, end_dir_y);

    if curve_offset != 0.0 {
        let mid_x = (ex + fx) / 2.0;
        let mid_y = (ey + fy) / 2.0;
        let perp_x = -uy;
        let perp_y = ux;
        let control_x = mid_x + perp_x * curve_offset;
        let control_y = mid_y + perp_y * curve_offset;

        let _ = writeln!(
            buf,
            r##"<path d="M {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1}" stroke="{}" stroke-width="1" fill="none"/>"##,
            ex, ey, control_x, control_y, fx, fy, arc_color
        );

        let tdx = fx - control_x;
        let tdy = fy - control_y;
        let mut t_dist = (tdx * tdx + tdy * tdy).sqrt();
        if t_dist == 0.0 {
            t_dist = MIN_DISTANCE;
        }
        end_dir_x = tdx / t_dist;
        end_dir_y = tdy / t_dist;
    } else {
        let _ = writeln!(
            buf,
            r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="1" fill="none"/>"##,
            ex, ey, fx, fy, arc_color
        );
        end_dir_x = ux;
        end_dir_y = uy;
    }

    if arc.inhibit_transition {
        let _ = writeln!(
            buf,
            r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="#fff" stroke="{}" stroke-width="1.3"/>"##,
            fx, fy, INHIBITOR_RADIUS, arc_color
        );
    } else {
        let ahx = fx + (-end_dir_x * ARROWHEAD_SIZE - end_dir_y * ARROWHEAD_SIZE * 0.45);
        let ahy = fy + (-end_dir_y * ARROWHEAD_SIZE + end_dir_x * ARROWHEAD_SIZE * 0.45);
        let bhx = fx + (-end_dir_x * ARROWHEAD_SIZE + end_dir_y * ARROWHEAD_SIZE * 0.45);
        let bhy = fy + (-end_dir_y * ARROWHEAD_SIZE - end_dir_x * ARROWHEAD_SIZE * 0.45);

        let _ = writeln!(
            buf,
            r##"<path d="M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Z" fill="{}"/>"##,
            fx, fy, ahx, ahy, bhx, bhy, arc_color
        );
    }

    let weight = get_arc_weight(arc);

    let (bx, by) = if curve_offset != 0.0 {
        let mid_x = (ex + fx) / 2.0;
        let mid_y = (ey + fy) / 2.0;
        let perp_x = -uy;
        let perp_y = ux;
        let control_x = mid_x + perp_x * curve_offset;
        let control_y = mid_y + perp_y * curve_offset;

        let t = 0.5;
        (
            (1.0 - t) * (1.0 - t) * ex + 2.0 * (1.0 - t) * t * control_x + t * t * fx,
            (1.0 - t) * (1.0 - t) * ey + 2.0 * (1.0 - t) * t * control_y + t * t * fy,
        )
    } else {
        ((ex + fx) / 2.0, (ey + fy) / 2.0)
    };

    let mut badge_bg_color = "#fafafa".to_string();
    let badge_border_color = arc_color.clone();
    let mut badge_text_color = "#666".to_string();
    if active {
        badge_bg_color = lighten_color(&arc_color, 0.85);
        badge_text_color = arc_color.clone();
    }

    let _ = writeln!(
        buf,
        r##"<circle cx="{:.1}" cy="{:.1}" r="10" fill="{}" stroke="{}" stroke-width="1"/>"##,
        bx, by, badge_bg_color, badge_border_color
    );

    let _ = writeln!(
        buf,
        r##"<text x="{:.1}" y="{:.1}" font-family="system-ui, Arial" font-size="10px" fill="{}" text-anchor="middle" dominant-baseline="middle">{}</text>"##,
        bx, by, badge_text_color, weight
    );
}

fn calculate_marking(net: &PetriNet) -> HashMap<String, i64> {
    net.places
        .iter()
        .map(|(id, place)| (id.clone(), place.initial.iter().sum::<f64>().round() as i64))
        .collect()
}

fn is_enabled(
    transition_id: &str,
    net: &PetriNet,
    marks: &HashMap<String, i64>,
    nodes: &HashMap<&str, NodePosition>,
) -> bool {
    let _ = nodes;
    for arc in &net.arcs {
        let weight = get_arc_weight(arc);

        if arc.inhibit_transition {
            if arc.target == transition_id {
                if let Some(&tokens) = marks.get(arc.source.as_str()) {
                    if tokens >= weight {
                        return false;
                    }
                }
            } else if arc.source == transition_id {
                match marks.get(arc.target.as_str()) {
                    Some(&tokens) => {
                        if tokens < weight {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        } else if arc.target == transition_id {
            match marks.get(arc.source.as_str()) {
                Some(&tokens) => {
                    if tokens < weight {
                        return false;
                    }
                }
                None => return false,
            }
        } else if arc.source == transition_id {
            if let Some(place) = net.places.get(arc.target.as_str()) {
                let capacity = get_capacity(place);
                if let Some(&tokens) = marks.get(arc.target.as_str()) {
                    if capacity.is_finite() && (tokens + weight) as f64 > capacity {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Returns the place's capacity against the token TOTAL the renderer draws.
/// See `visualization.getCapacity`'s doc comment: a zero component means
/// that color is unbounded, and one unbounded color makes the total
/// unbounded, so the bound only exists when every component is non-zero, in
/// which case it is their sum.
fn get_capacity(place: &CorePlace) -> f64 {
    if place.capacity.is_empty() {
        return f64::INFINITY;
    }
    let mut total = 0.0;
    for &c in &place.capacity {
        if c == 0.0 {
            return f64::INFINITY;
        }
        total += c;
    }
    total
}

fn get_arc_color(arc: &CoreArc, tokens: &[String], active: bool) -> String {
    let default_weight = [1.0];
    let weight: &[f64] = if arc.weight.is_empty() { &default_weight } else { &arc.weight };

    let mut used_colors = Vec::new();
    for (i, &w) in weight.iter().enumerate() {
        if w > 0.0 {
            if let Some(token) = tokens.get(i) {
                let color = extract_color(token);
                if !color.is_empty() {
                    used_colors.push(color);
                }
            }
        }
    }

    if used_colors.is_empty() {
        return if active { "#2a6fb8".to_string() } else { "#cfcfcf".to_string() };
    }

    let color = &used_colors[0];
    if active {
        color.clone()
    } else {
        lighten_color(color, 0.6)
    }
}

fn get_arc_weight(arc: &CoreArc) -> i64 {
    if arc.weight.is_empty() {
        return 1;
    }
    for &w in &arc.weight {
        if w > 0.0 {
            return w.round() as i64;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::{Arc as CoreArc, Place, PetriNet, Transition};

    fn simple_net() -> PetriNet {
        let mut net = PetriNet::new();
        net.places.insert(
            "p0".to_string(),
            Place::new("p0", vec![1.0], vec![], 0.0, 0.0, None),
        );
        net.places.insert(
            "p1".to_string(),
            Place::new("p1", vec![0.0], vec![], 200.0, 0.0, None),
        );
        net.transitions.insert(
            "t0".to_string(),
            Transition::new("t0", "", 100.0, 0.0, None),
        );
        net.arcs.push(CoreArc::new("p0", "t0", vec![1.0], false));
        net.arcs.push(CoreArc::new("t0", "p1", vec![1.0], false));
        net
    }

    #[test]
    fn renders_well_formed_svg() {
        let svg = render_petri_net_svg(&simple_net());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("viewBox="));
    }

    #[test]
    fn labels_default_to_the_node_id() {
        let svg = render_petri_net_svg(&simple_net());
        assert!(svg.contains(">p0<"));
        assert!(svg.contains(">p1<"));
        assert!(svg.contains(">t0<"));
    }

    #[test]
    fn explicit_label_overrides_id() {
        let mut net = simple_net();
        net.places.get_mut("p0").unwrap().label_text = Some("Widgets".to_string());
        let svg = render_petri_net_svg(&net);
        assert!(svg.contains(">Widgets<"));
        assert!(!svg.contains(">p0<"));
    }

    #[test]
    fn draws_a_single_token_as_a_dot_and_multiple_as_text() {
        let svg = render_petri_net_svg(&simple_net());
        assert!(svg.contains(r##"class="token-dot""##));

        let mut net = simple_net();
        net.places.get_mut("p0").unwrap().initial = vec![3.0];
        let svg = render_petri_net_svg(&net);
        assert!(svg.contains(">3<"));
    }

    #[test]
    fn enabled_transition_gets_the_active_class() {
        // p0 has a token and feeds t0: t0 is enabled.
        let svg = render_petri_net_svg(&simple_net());
        assert!(svg.contains("transition transition-active"));
    }

    #[test]
    fn disabled_transition_has_no_active_class() {
        let mut net = simple_net();
        net.places.get_mut("p0").unwrap().initial = vec![0.0];
        let svg = render_petri_net_svg(&net);
        // Match the element's class attribute, not the bare class name: the
        // stylesheet defines `.transition-active` in every document whether
        // or not anything uses it.
        assert!(!svg.contains(r##"class="transition transition-active""##));
        assert!(svg.contains(r##"class="transition""##));
    }

    #[test]
    fn inhibitor_arc_draws_a_circle_not_an_arrowhead() {
        let mut net = simple_net();
        net.arcs[0].inhibit_transition = true;
        let svg = render_petri_net_svg(&net);
        // The inhibitor terminator is a stroked circle at the transition end.
        assert!(svg.contains(r##"stroke-width="1.3""##));
    }

    #[test]
    fn full_capacity_place_gets_the_cap_full_class() {
        let mut net = simple_net();
        net.places.get_mut("p0").unwrap().capacity = vec![1.0];
        // p0 already holds 1 of its capacity 1.
        let svg = render_petri_net_svg(&net);
        assert!(svg.contains(r##"class="place place-cap-full""##));
    }

    #[test]
    fn zero_capacity_component_is_unbounded() {
        let mut net = simple_net();
        net.places.get_mut("p0").unwrap().initial = vec![5.0, 0.0];
        net.places.get_mut("p0").unwrap().capacity = vec![2.0, 0.0];
        let svg = render_petri_net_svg(&net);
        assert!(!svg.contains(r##"class="place place-cap-full""##));
    }

    #[test]
    fn label_text_is_xml_escaped() {
        let mut net = simple_net();
        net.places.get_mut("p0").unwrap().label_text = Some("A & B".to_string());
        let svg = render_petri_net_svg(&net);
        assert!(svg.contains("A &amp; B"));
    }

    #[test]
    fn empty_net_still_produces_minimum_sized_svg() {
        let svg = render_petri_net_svg(&PetriNet::new());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains(r##"width="100""##));
        assert!(svg.contains(r##"height="100""##));
    }

    #[test]
    fn parallel_arcs_between_the_same_pair_curve_apart() {
        // Two arcs t0->p1 duplicated: forces the multi-arc curve branch.
        let mut net = simple_net();
        net.arcs.push(CoreArc::new("t0", "p1", vec![1.0], false));
        let svg = render_petri_net_svg(&net);
        // A curved arc renders as a quadratic path (`Q`), not just a line.
        assert!(svg.contains(" Q "));
    }
}
