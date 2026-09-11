//! Colored-token unfolding — a port of go-pflow's `petri/colors.go`.
//!
//! `Place::initial`, `Place::capacity` and `Arc::weight` are vectors, one
//! component per token color (`PetriNet::token` names them). A net is
//! multi-color as soon as any of those has length > 1. [`PetriNet::expand_colors`]
//! unfolds such a net into an equivalent single-color net — the standard
//! colored-net unfolding: each place becomes one place per color
//! (`"pool.red"`, `"pool.blue"`, ...), each arc becomes one arc per color
//! with a non-zero weight component, and transitions are shared so a firing
//! still moves all colors atomically. The semantics are exactly the
//! component-wise rules of pflow-xyz's `petri-sim.js` (the shared JS/Go
//! firing contract), reproduced here byte-for-byte from go-pflow.

use std::collections::{HashMap, HashSet};

use crate::net::PetriNet;

/// Identifies one color component of a base place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorRef {
    pub place: String,
    pub color: usize,
}

/// Records how a multi-color net was unfolded into a single-color net by
/// [`PetriNet::expand_colors`]. Maps each expanded place name back to its
/// base place and color index, and forward from each base place to its
/// expanded names in color order.
#[derive(Debug, Clone, Default)]
pub struct ColorMap {
    /// The color names used for expansion, index-aligned with the per-color
    /// vectors (`initial`, `capacity`, `weight`).
    pub colors: Vec<String>,
    /// Base place name -> expanded place names, one per color.
    pub expanded: HashMap<String, Vec<String>>,
    /// Expanded place name -> (base place, color index).
    pub base: HashMap<String, ColorRef>,
}

impl ColorMap {
    /// Returns the base place and color name for an expanded place, or the
    /// input unchanged when it is not an expanded name. `cm` is `None` for
    /// the single-color case — go-pflow's nil-receiver methods, made
    /// explicit.
    pub fn base_name(cm: Option<&ColorMap>, expanded: &str) -> (String, String, bool) {
        let cm = match cm {
            Some(cm) => cm,
            None => return (expanded.to_string(), String::new(), false),
        };
        match cm.base.get(expanded) {
            Some(r) => (r.place.clone(), cm.colors[r.color].clone(), true),
            None => (expanded.to_string(), String::new(), false),
        }
    }

    /// Folds a marking over expanded places back to per-base-place totals —
    /// the scalar projection, useful for reporting.
    pub fn sum_by_base(
        cm: Option<&ColorMap>,
        marking: &HashMap<String, i64>,
    ) -> HashMap<String, i64> {
        let cm = match cm {
            Some(cm) => cm,
            None => return marking.clone(),
        };
        let mut out = HashMap::new();
        for (name, &v) in marking {
            let key = match cm.base.get(name) {
                Some(r) => r.place.clone(),
                None => name.clone(),
            };
            *out.entry(key).or_insert(0) += v;
        }
        out
    }

    /// [`ColorMap::sum_by_base`] for continuous state vectors.
    pub fn sum_by_base_float(
        cm: Option<&ColorMap>,
        state: &HashMap<String, f64>,
    ) -> HashMap<String, f64> {
        let cm = match cm {
            Some(cm) => cm,
            None => return state.clone(),
        };
        let mut out = HashMap::new();
        for (name, &v) in state {
            let key = match cm.base.get(name) {
                Some(r) => r.place.clone(),
                None => name.clone(),
            };
            *out.entry(key).or_insert(0.0) += v;
        }
        out
    }

    /// Returns the expanded place names for a base place, in color order. A
    /// name that is already expanded (or unknown) returns itself as a
    /// single-element vector, so callers can treat every name uniformly.
    pub fn lookup(cm: Option<&ColorMap>, name: &str) -> Vec<String> {
        let cm = match cm {
            Some(cm) => cm,
            None => return vec![name.to_string()],
        };
        match cm.expanded.get(name) {
            Some(names) => names.clone(),
            None => vec![name.to_string()],
        }
    }
}

impl PetriNet {
    /// Reports whether the net uses more than one token color — i.e. any
    /// place initial/capacity vector or arc weight vector has more than one
    /// component, or more than one token color is declared.
    pub fn is_multi_color(&self) -> bool {
        if self.token.len() > 1 {
            return true;
        }
        for p in self.places.values() {
            if p.initial.len() > 1 || p.capacity.len() > 1 {
                return true;
            }
        }
        for a in &self.arcs {
            if a.weight.len() > 1 {
                return true;
            }
        }
        false
    }

    /// The number of token colors the net uses: the longest vector found
    /// across declared token names, place initials/capacities, and arc
    /// weights.
    pub fn color_count(&self) -> usize {
        let mut c = self.token.len();
        for p in self.places.values() {
            c = c.max(p.initial.len()).max(p.capacity.len());
        }
        for a in &self.arcs {
            c = c.max(a.weight.len());
        }
        c
    }

    /// Unfolds a multi-color net into an equivalent single-color net — see
    /// the module docs. Color names come from `token` where declared, else
    /// `"c0"`, `"c1"`, .... If an expanded name would collide with an
    /// existing place, the separator is doubled until unique
    /// (`"pool.red"` -> `"pool..red"`).
    ///
    /// Single-color nets are returned as-is (cloned) with `None`: callers
    /// can treat `cm.is_none()` as "nothing was expanded".
    pub fn expand_colors(&self) -> (PetriNet, Option<ColorMap>) {
        let colors = self.color_count();
        if colors <= 1 {
            return (self.clone(), None);
        }

        let mut names = Vec::with_capacity(colors);
        for i in 0..colors {
            if i < self.token.len() && !self.token[i].is_empty() {
                names.push(self.token[i].clone());
            } else {
                names.push(format!("c{i}"));
            }
        }

        // Choose a separator that cannot collide with existing place names.
        let mut sep = ".".to_string();
        loop {
            let mut collision = false;
            for base in self.places.keys() {
                for c in &names {
                    if self.places.contains_key(&format!("{base}{sep}{c}")) {
                        collision = true;
                    }
                }
            }
            if !collision {
                break;
            }
            sep.push('.');
        }

        let mut cm = ColorMap {
            colors: names.clone(),
            expanded: HashMap::with_capacity(self.places.len()),
            base: HashMap::with_capacity(self.places.len() * colors),
        };

        let mut out = PetriNet::new();
        // The unfolded net is single-color by construction.
        out.token = Vec::new();

        for (base, p) in &self.places {
            let mut expanded = Vec::with_capacity(colors);
            for (i, name_i) in names.iter().enumerate() {
                let name = format!("{base}{sep}{name_i}");
                expanded.push(name.clone());
                cm.base.insert(
                    name.clone(),
                    ColorRef {
                        place: base.clone(),
                        color: i,
                    },
                );

                let initial = p.initial.get(i).copied().unwrap_or(0.0);
                let capacity = match p.capacity.get(i) {
                    Some(&c) if c > 0.0 => vec![c],
                    _ => vec![],
                };
                out.add_place(
                    name,
                    vec![initial],
                    capacity,
                    p.x,
                    p.y,
                    p.label_text.clone(),
                );
            }
            cm.expanded.insert(base.clone(), expanded);
        }

        for (label, t) in &self.transitions {
            out.add_transition(label.clone(), t.role.clone(), t.x, t.y, t.label_text.clone());
        }

        for a in &self.arcs {
            // Weight defaults to [1] when empty, matching weight_sum() and
            // the JS getArcWeight helper.
            let w: Vec<f64> = if a.weight.is_empty() {
                vec![1.0]
            } else {
                a.weight.clone()
            };

            let source_is_place = self.places.contains_key(&a.source);
            let target_is_place = self.places.contains_key(&a.target);

            for (i, &wi) in w.iter().enumerate() {
                if wi == 0.0 || i >= colors {
                    continue;
                }
                let src = if source_is_place {
                    cm.expanded[&a.source][i].clone()
                } else {
                    a.source.clone()
                };
                let dst = if target_is_place {
                    cm.expanded[&a.target][i].clone()
                } else {
                    a.target.clone()
                };
                out.add_arc(src, dst, vec![wi], a.inhibit_transition);
            }
        }

        (out, Some(cm))
    }

    /// Maps a state vector keyed by this (multi-color) net's place names
    /// onto the place names of its `expand_colors` unfolding.
    ///
    /// Expanded keys (`"pool.red"`) pass through untouched and pin one
    /// color. A base key (`"pool"`) carries a TOTAL across colors — the
    /// shape `set_state` and `Place::token_count` produce — and is
    /// distributed across that place's colors in the proportions of its
    /// declared `initial` vector. When the declared vector is empty or sums
    /// to zero there are no proportions to follow, so the whole total goes
    /// to color 0.
    ///
    /// Returns `state` unchanged on a single-color net.
    pub fn expand_state(&self, state: &HashMap<String, f64>) -> HashMap<String, f64> {
        let (_, cm) = self.expand_colors();
        let cm = match cm {
            Some(cm) => cm,
            None => return state.clone(),
        };

        let mut out = HashMap::with_capacity(state.len() * cm.colors.len().max(1));
        for (name, &total) in state {
            let place = match self.places.get(name) {
                Some(p) => p,
                None => {
                    // Already expanded, or not a place at all — pass through.
                    out.insert(name.clone(), total);
                    continue;
                }
            };
            let expanded = &cm.expanded[name];

            let declared: f64 = place.initial.iter().sum();
            if declared == 0.0 {
                for (i, en) in expanded.iter().enumerate() {
                    out.insert(en.clone(), if i == 0 { total } else { 0.0 });
                }
                continue;
            }
            for (i, en) in expanded.iter().enumerate() {
                let share = place.initial.get(i).copied().unwrap_or(0.0);
                out.insert(en.clone(), total * share / declared);
            }
        }
        out
    }
}

/// Turns pflow.xyz token URIs into the names color unfolding uses: the last
/// path segment of a URL, the token itself otherwise. Names are shortened
/// only when the result stays unique, so two tokens that differ only in
/// host keep their full form. Port of go-pflow's `parser.ShortColorNames`.
pub fn short_color_names(tokens: &[String]) -> Vec<String> {
    if tokens.is_empty() {
        return tokens.to_vec();
    }
    let mut short = Vec::with_capacity(tokens.len());
    let mut seen = HashSet::with_capacity(tokens.len());
    for tok in tokens {
        let name = short_name(tok);
        if seen.contains(&name) {
            return tokens.to_vec();
        }
        seen.insert(name.clone());
        short.push(name);
    }
    short
}

/// The last non-empty path segment of `tok` if it parses as an absolute URL
/// (has a `scheme://`), else `tok` itself.
fn short_name(tok: &str) -> String {
    if let Some(idx) = tok.find("://") {
        let rest = &tok[idx + 3..];
        if let Some(slash) = rest.find('/') {
            let mut path = &rest[slash..];
            if let Some(q) = path.find(['?', '#']) {
                path = &path[..q];
            }
            let trimmed = path.trim_end_matches('/');
            let base = match trimmed.rfind('/') {
                Some(pos) => &trimmed[pos + 1..],
                None => trimmed.trim_start_matches('/'),
            };
            if !base.is_empty() && base != "." {
                return base.to_string();
            }
        }
    }
    tok.to_string()
}

/// Classification of an [`UnfoldedArc`]'s role, matching the arc-type
/// distinction go-pflow's `metamodel.Arc` makes once an editor document
/// reaches an engine. `pflow-core` stops here — it has no
/// `pflow-metamodel` dependency — but computes the same classification so a
/// downstream converter (`pflow-parser` / `pflow-metamodel`) does not have
/// to re-derive the direction rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcKind {
    /// A normal consuming arc.
    Normal,
    /// An input-side inhibitor (`place -> transition`): blocks firing at
    /// `>= weight`, consumes nothing.
    Inhibitor,
    /// An output-side inhibitor (`transition -> place` with
    /// `inhibit_transition` set), rewritten to its true meaning — the
    /// editor's test arc, made explicit. `source`/`target` on the
    /// [`UnfoldedArc`] are already reversed to `place -> transition`: the
    /// transition requires `weight` tokens in `place` without consuming
    /// them.
    Read,
}

/// One arc of a color-unfolded net, with its true source/target and
/// [`ArcKind`].
#[derive(Debug, Clone)]
pub struct UnfoldedArc {
    pub source: String,
    pub target: String,
    pub weight: f64,
    pub kind: ArcKind,
}

/// The result of [`unfold`]: the color-expanded, arc-less-copy-pruned net
/// plus its arcs classified by [`ArcKind`].
#[derive(Debug, Clone)]
pub struct UnfoldedNet {
    pub net: PetriNet,
    pub arcs: Vec<UnfoldedArc>,
    pub color_map: Option<ColorMap>,
}

/// Unfolds `net` the way go-pflow's `parser.ModelFromJSON` does before an
/// editor document reaches an engine — stopped at the `pflow-core`
/// boundary, before metamodel-specific concerns (integer capacities, ids,
/// coordinates) that belong to `pflow-parser`:
///
///   - token colors are shortened ([`short_color_names`]);
///   - a multi-color net is unfolded to one place per color
///     ([`PetriNet::expand_colors`]);
///   - color copies no arc touches are dropped: they carry no behaviour and
///     would otherwise fail connectivity validation and collapse
///     sensitivity analysis;
///   - an input-side `inhibit_transition` (place -> transition) stays an
///     inhibitor; an output-side one (transition -> place) is the editor's
///     read arc and is rewritten to `place -> transition` with
///     [`ArcKind::Read`].
pub fn unfold(net: &PetriNet) -> UnfoldedNet {
    let mut shortened = net.clone();
    shortened.token = short_color_names(&net.token);

    let (expanded, color_map) = shortened.expand_colors();

    let mut touched: HashSet<&str> = HashSet::with_capacity(expanded.arcs.len() * 2);
    for a in &expanded.arcs {
        touched.insert(a.source.as_str());
        touched.insert(a.target.as_str());
    }

    let mut pruned_places = HashMap::with_capacity(expanded.places.len());
    for (id, p) in &expanded.places {
        let is_untouched_copy = color_map
            .as_ref()
            .map(|cm| cm.base.contains_key(id) && !touched.contains(id.as_str()))
            .unwrap_or(false);
        if is_untouched_copy {
            continue;
        }
        pruned_places.insert(id.clone(), p.clone());
    }

    let is_place = |name: &str| expanded.places.contains_key(name);

    let arcs = expanded
        .arcs
        .iter()
        .map(|a| {
            let weight = a.weight_sum();
            if a.inhibit_transition {
                if is_place(&a.source) {
                    UnfoldedArc {
                        source: a.source.clone(),
                        target: a.target.clone(),
                        weight,
                        kind: ArcKind::Inhibitor,
                    }
                } else {
                    // Output-side inhibitor: the editor's test arc. The
                    // transition reads the place without consuming it.
                    UnfoldedArc {
                        source: a.target.clone(),
                        target: a.source.clone(),
                        weight,
                        kind: ArcKind::Read,
                    }
                }
            } else {
                UnfoldedArc {
                    source: a.source.clone(),
                    target: a.target.clone(),
                    weight,
                    kind: ArcKind::Normal,
                }
            }
        })
        .collect();

    let pruned = PetriNet {
        places: pruned_places,
        transitions: expanded.transitions.clone(),
        arcs: expanded.arcs.clone(),
        token: expanded.token.clone(),
    };

    UnfoldedNet {
        net: pruned,
        arcs,
        color_map,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(m: &HashMap<String, crate::net::Place>) -> Vec<String> {
        m.keys().cloned().collect()
    }

    #[test]
    fn expand_colors_single_color_is_identity() {
        let net = PetriNet::build()
            .place("a", 2.0)
            .transition("t")
            .arc("a", "t", 1.0)
            .done();
        let (out, cm) = net.expand_colors();
        assert!(cm.is_none());
        assert_eq!(out.places.len(), net.places.len());
        assert!(!net.is_multi_color());
    }

    #[test]
    fn expand_colors_basic() {
        let mut net = PetriNet::new();
        net.token = vec!["red".into(), "blue".into()];
        net.add_place("pool", vec![3.0, 1.0], vec![], 0.0, 0.0, None);
        net.add_place("out", vec![0.0, 0.0], vec![], 0.0, 0.0, None);
        net.add_transition("move", "default", 0.0, 0.0, None);
        net.add_arc("pool", "move", vec![1.0, 2.0], false);
        net.add_arc("move", "out", vec![1.0, 2.0], false);

        assert!(net.is_multi_color());

        let (out, cm) = net.expand_colors();
        let cm = cm.expect("2-color net must produce a ColorMap");

        assert_eq!(out.places.len(), 4, "{:?}", keys(&out.places));
        assert_eq!(out.places["pool.red"].token_count(), 3.0);
        assert_eq!(out.places["pool.blue"].token_count(), 1.0);

        assert_eq!(out.transitions.len(), 1);
        assert_eq!(out.arcs.len(), 4);

        let (base, color, ok) = ColorMap::base_name(Some(&cm), "pool.blue");
        assert!(ok);
        assert_eq!(base, "pool");
        assert_eq!(color, "blue");

        let mut marking = HashMap::new();
        marking.insert("pool.red".to_string(), 2i64);
        marking.insert("pool.blue".to_string(), 1i64);
        marking.insert("out.red".to_string(), 1i64);
        let sum = ColorMap::sum_by_base(Some(&cm), &marking);
        assert_eq!(sum["pool"], 3);
        assert_eq!(sum["out"], 1);
    }

    #[test]
    fn expand_colors_zero_weight_components_skipped() {
        let mut net = PetriNet::new();
        net.add_place("p", vec![1.0, 1.0], vec![], 0.0, 0.0, None);
        net.add_place("q", vec![0.0, 0.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("p", "t", vec![1.0, 0.0], false);
        net.add_arc("t", "q", vec![0.0, 2.0], false);

        let (out, _) = net.expand_colors();
        assert_eq!(out.arcs.len(), 2);
    }

    #[test]
    fn expand_colors_shorter_vectors() {
        let mut net = PetriNet::new();
        net.add_place("wide", vec![1.0, 2.0], vec![5.0, 0.0], 0.0, 0.0, None);
        net.add_place("narrow", vec![4.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("narrow", "t", vec![1.0], false);

        let (out, cm) = net.expand_colors();
        let cm = cm.unwrap();

        assert_eq!(out.places["narrow.c1"].token_count(), 0.0);
        assert_eq!(out.places["wide.c0"].capacity, vec![5.0]);
        assert!(out.places["wide.c1"].capacity.is_empty());
        assert_eq!(out.arcs.len(), 1);
        assert_eq!(cm.colors[0], "c0");
        assert_eq!(cm.colors[1], "c1");
    }

    #[test]
    fn expand_colors_name_collision() {
        let mut net = PetriNet::new();
        net.token = vec!["red".into(), "blue".into()];
        net.add_place("pool", vec![1.0, 1.0], vec![], 0.0, 0.0, None);
        net.add_place("pool.red", vec![7.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("pool", "t", vec![1.0, 1.0], false);

        let (out, cm) = net.expand_colors();
        let cm = cm.unwrap();

        let total: f64 = out.places.values().map(|p| p.token_count()).sum();
        assert_eq!(total, 9.0);

        let mut seen = HashSet::new();
        for names in cm.expanded.values() {
            for n in names {
                assert!(seen.insert(n.clone()), "duplicate expanded name {n}");
            }
        }
    }

    #[test]
    fn expand_colors_defaults_undeclared_arc_weight() {
        let mut net = PetriNet::new();
        net.token = vec!["red".into(), "blue".into()];
        net.add_place("a", vec![1.0, 1.0], vec![], 0.0, 0.0, None);
        net.add_place("b", vec![0.0, 0.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("a", "t", vec![], false);
        net.add_arc("t", "b", vec![], false);

        let (out, cm) = net.expand_colors();
        assert!(cm.is_some());
        assert_eq!(out.arcs.len(), 2);
        for arc in &out.arcs {
            assert_eq!(arc.weight_sum(), 1.0);
            assert!(arc.source == "a.red" || arc.target == "b.red");
        }
    }

    #[test]
    fn expand_state_round_trip() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![2.0, 6.0], vec![], 0.0, 0.0, None);
        n.add_place("sink", vec![0.0, 0.0], vec![], 0.0, 0.0, None);

        let got = n.expand_state(&n.set_state(None));

        assert_eq!(got["pool.red"], 2.0);
        assert_eq!(got["pool.blue"], 6.0);
        assert_eq!(got["sink.red"], 0.0);
        assert_eq!(got["sink.blue"], 0.0);
        assert_eq!(got.len(), 4);
    }

    #[test]
    fn expand_state_scales_proportionally() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![2.0, 6.0], vec![], 0.0, 0.0, None);

        let mut state = HashMap::new();
        state.insert("pool".to_string(), 4.0);
        let got = n.expand_state(&state);

        assert_eq!(got["pool.red"], 1.0);
        assert_eq!(got["pool.blue"], 3.0);
    }

    #[test]
    fn expand_state_empty_declaration_goes_to_first_color() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![2.0, 6.0], vec![], 0.0, 0.0, None);
        n.add_place("sink", vec![0.0, 0.0], vec![], 0.0, 0.0, None);

        let mut state = HashMap::new();
        state.insert("sink".to_string(), 5.0);
        let got = n.expand_state(&state);

        assert_eq!(got["sink.red"], 5.0);
        assert_eq!(got["sink.blue"], 0.0);
    }

    #[test]
    fn expand_state_is_idempotent() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![2.0, 6.0], vec![], 0.0, 0.0, None);

        let once = n.expand_state(&n.set_state(None));
        let twice = n.expand_state(&once);

        assert_eq!(once.len(), twice.len());
        for (k, v) in &once {
            assert_eq!(twice[k], *v);
        }
    }

    #[test]
    fn expand_state_single_color_is_noop() {
        let mut n = PetriNet::new();
        n.add_place("pool", vec![5.0], vec![], 0.0, 0.0, None);

        let input = n.set_state(None);
        let got = n.expand_state(&input);

        assert_eq!(got["pool"], 5.0);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn sum_by_base_float_and_lookup() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![2.0, 6.0], vec![], 0.0, 0.0, None);
        let (_, cm) = n.expand_colors();

        let mut state = HashMap::new();
        state.insert("pool.red".to_string(), 1.5);
        state.insert("pool.blue".to_string(), 2.5);
        let folded = ColorMap::sum_by_base_float(cm.as_ref(), &state);
        assert_eq!(folded["pool"], 4.0);

        assert_eq!(ColorMap::lookup(cm.as_ref(), "pool").len(), 2);
        assert_eq!(
            ColorMap::lookup(cm.as_ref(), "pool.red"),
            vec!["pool.red".to_string()]
        );
        assert_eq!(ColorMap::lookup(None, "pool"), vec!["pool".to_string()]);

        let mut nil_state = HashMap::new();
        nil_state.insert("pool".to_string(), 3.0);
        assert_eq!(
            ColorMap::sum_by_base_float(None, &nil_state)["pool"],
            3.0
        );
    }

    #[test]
    fn is_multi_color_agrees_with_color_count() {
        let cases: Vec<(&str, PetriNet, bool)> = vec![
            ("empty net", PetriNet::new(), false),
            ("single color place", {
                let mut n = PetriNet::new();
                n.add_place("p", vec![1.0], vec![], 0.0, 0.0, None);
                n
            }, false),
            ("two token names", {
                let mut n = PetriNet::new();
                n.token = vec!["red".into(), "blue".into()];
                n.add_place("p", vec![1.0], vec![], 0.0, 0.0, None);
                n
            }, true),
            ("multi-color initial", {
                let mut n = PetriNet::new();
                n.add_place("p", vec![1.0, 2.0], vec![], 0.0, 0.0, None);
                n
            }, true),
            ("multi-color capacity", {
                let mut n = PetriNet::new();
                n.add_place("p", vec![1.0], vec![3.0, 3.0], 0.0, 0.0, None);
                n
            }, true),
            ("multi-color arc weight", {
                let mut n = PetriNet::new();
                n.add_place("p", vec![1.0], vec![], 0.0, 0.0, None);
                n.add_transition("t", "default", 0.0, 0.0, None);
                n.add_arc("p", "t", vec![1.0, 1.0], false);
                n
            }, true),
        ];

        for (name, net, want) in cases {
            assert_eq!(net.is_multi_color(), want, "{name}: is_multi_color");
            assert_eq!(net.color_count() > 1, want, "{name}: color_count");
        }
    }

    #[test]
    fn base_name_handles_expanded_unknown_and_nil() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![1.0, 1.0], vec![], 0.0, 0.0, None);
        let (_, cm) = n.expand_colors();

        let (place, color, ok) = ColorMap::base_name(cm.as_ref(), "pool.blue");
        assert!(ok);
        assert_eq!(place, "pool");
        assert_eq!(color, "blue");

        let (place, color, ok) = ColorMap::base_name(cm.as_ref(), "somewhere-else");
        assert!(!ok);
        assert_eq!(place, "somewhere-else");
        assert_eq!(color, "");

        let (place, color, ok) = ColorMap::base_name(None, "pool");
        assert!(!ok);
        assert_eq!(place, "pool");
        assert_eq!(color, "");
    }

    #[test]
    fn sum_by_base_passthrough_and_nil() {
        let mut n = PetriNet::new();
        n.token = vec!["red".into(), "blue".into()];
        n.add_place("pool", vec![1.0, 1.0], vec![], 0.0, 0.0, None);
        let (_, cm) = n.expand_colors();

        let mut marking = HashMap::new();
        marking.insert("pool.red".to_string(), 2i64);
        marking.insert("pool.blue".to_string(), 3i64);
        marking.insert("unrelated".to_string(), 7i64);
        let got = ColorMap::sum_by_base(cm.as_ref(), &marking);
        assert_eq!(got["pool"], 5);
        assert_eq!(got["unrelated"], 7);

        let mut nil_in = HashMap::new();
        nil_in.insert("pool".to_string(), 5i64);
        assert_eq!(ColorMap::sum_by_base(None, &nil_in)["pool"], 5);
    }

    #[test]
    fn short_color_names_shortens_unique_urls() {
        let tokens = vec![
            "https://pflow.xyz/tokens/red".to_string(),
            "https://pflow.xyz/tokens/brown".to_string(),
            "https://pflow.xyz/tokens/blue".to_string(),
        ];
        let got = short_color_names(&tokens);
        assert_eq!(got, vec!["red", "brown", "blue"]);
    }

    #[test]
    fn short_color_names_keeps_non_urls_unchanged() {
        let tokens = vec!["red".to_string(), "blue".to_string()];
        assert_eq!(short_color_names(&tokens), tokens);
    }

    #[test]
    fn short_color_names_keeps_full_form_on_collision() {
        let tokens = vec![
            "https://a.example/tokens/x".to_string(),
            "https://b.example/tokens/x".to_string(),
        ];
        assert_eq!(short_color_names(&tokens), tokens);
    }

    #[test]
    fn short_color_names_empty_is_identity() {
        let tokens: Vec<String> = vec![];
        assert_eq!(short_color_names(&tokens), tokens);
    }

    #[test]
    fn unfold_prunes_untouched_color_copies() {
        // "sink" declares two colors but only color 0 is ever wired to an
        // arc; color 1's copy must be dropped.
        let mut net = PetriNet::new();
        net.token = vec!["red".into(), "blue".into()];
        net.add_place("src", vec![1.0, 0.0], vec![], 0.0, 0.0, None);
        net.add_place("sink", vec![0.0, 0.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("src", "t", vec![1.0, 0.0], false);
        net.add_arc("t", "sink", vec![1.0, 0.0], false);

        let u = unfold(&net);

        assert!(u.net.places.contains_key("src.red"));
        assert!(u.net.places.contains_key("sink.red"));
        assert!(!u.net.places.contains_key("src.blue"));
        assert!(!u.net.places.contains_key("sink.blue"));
    }

    #[test]
    fn unfold_rewrites_output_side_inhibitor_as_read() {
        let mut net = PetriNet::new();
        net.add_place("p", vec![1.0], vec![], 0.0, 0.0, None);
        net.add_place("q", vec![0.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        // Output-side inhibitor: transition -> place, the editor's test arc.
        net.add_arc("t", "q", vec![1.0], true);
        // Input-side inhibitor: place -> transition, a normal inhibitor.
        net.add_arc("p", "t", vec![1.0], true);

        let u = unfold(&net);
        assert_eq!(u.arcs.len(), 2);

        let read = u.arcs.iter().find(|a| a.kind == ArcKind::Read).unwrap();
        assert_eq!(read.source, "q");
        assert_eq!(read.target, "t");

        let inhibitor = u
            .arcs
            .iter()
            .find(|a| a.kind == ArcKind::Inhibitor)
            .unwrap();
        assert_eq!(inhibitor.source, "p");
        assert_eq!(inhibitor.target, "t");
    }

    #[test]
    fn unfold_single_color_net_is_shape_preserving() {
        let net = PetriNet::build()
            .place("a", 2.0)
            .transition("t")
            .arc("a", "t", 1.0)
            .done();

        let u = unfold(&net);
        assert!(u.color_map.is_none());
        assert_eq!(u.net.places.len(), net.places.len());
        assert_eq!(u.arcs.len(), 1);
        assert_eq!(u.arcs[0].kind, ArcKind::Normal);
    }
}
