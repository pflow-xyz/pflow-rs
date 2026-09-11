//! Shape A -> Shape B: the one converter from the pflow.xyz editor document
//! to a [`pflow_metamodel::Model`], ported from go-pflow's `parser` package
//! (`json.go`, `colors.go`'s consumer half in `json.go`'s `ModelFromJSON`).
//!
//! The ecosystem has two JSON shapes for a net: the pflow.xyz editor shape
//! (places and transitions as objects keyed by id, arcs with
//! `source`/`target`, per-color vectors, `inhibitTransition`, a CID as
//! `@id` — [`pflow_core::json`]'s territory) and the metamodel shape
//! (arrays with ids, `from`/`to`, rate, schedule, parameters, ...) that
//! every engine and tool reads ([`pflow_metamodel`]). [`model_from_json`]
//! is the one bridge between them, so the unfolding rules — how colors
//! become places, what an output-side inhibitor means, which copies are
//! dropped — exist once, here, instead of once per consumer.
//!
//! Held to the editor-shape goldens: `go-pflow/parser/testdata/editor-shape/*.json`
//! (copied byte-identical into `tests/fixtures/editor-shape/`), which pin
//! `parsed`, `expanded` and `unfolded` for every input. `tests/parity.rs`
//! replays every golden and asserts `==` after canonical JSON serialisation.

pub mod editor_shape;

use pflow_core::{unfold, ArcKind, ColorMap, PetriNet};
use pflow_metamodel::{Arc as MmArc, ArcType, Model, Place as MmPlace, Transition as MmTransition};
use serde::Deserialize;
use serde_json::Value;

pub use editor_shape::{
    normalize, NormalizedArc, NormalizedNet, NormalizedPlace, NormalizedTransition,
};

/// Errors converting a Shape A document.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Core(#[from] pflow_core::json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// How far below its base place, in editor units, each color copy of a
/// place is offset when a colored net is unfolded — go-pflow's
/// `parser.ColorCopySpacing`. Every copy inherits the base coordinates
/// otherwise, and a rendered unfolded net would stack them all on one
/// pixel.
pub const COLOR_COPY_SPACING: i64 = 60;

/// Reports whether `data` is an editor-shape document: a JSON object whose
/// `"places"` is itself an object keyed by place id (the metamodel shape
/// carries an array). Port of go-pflow's `parser.IsPflowJSON`.
pub fn is_pflow_json(data: &[u8]) -> bool {
    #[derive(Deserialize, Default)]
    struct Probe {
        #[serde(default)]
        places: Option<Value>,
    }
    matches!(
        serde_json::from_slice::<Probe>(data),
        Ok(Probe {
            places: Some(Value::Object(_)),
        })
    )
}

#[derive(Deserialize, Default)]
struct Head {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

/// Converts an editor-shape document into a [`pflow_metamodel::Model`] —
/// port of go-pflow's `parser.ModelFromJSON`.
///
///   - Token colors are shortened to their last URI path segment
///     (`"https://pflow.xyz/tokens/red"` -> `"red"`) when that stays
///     unique, so an unfolded place is `"queue.red"`, not an invalid
///     identifier carrying a URL.
///   - A multi-color net is unfolded to one place per color per base
///     place. Color copies no arc touches are dropped.
///   - Per-color capacity is summed into the scalar capacity of each copy.
///   - An input-side `inhibitTransition` (place -> transition) is an
///     inhibitor arc. An output-side one (transition -> place) is the
///     editor's read arc — the transition requires the tokens and moves
///     none — and is emitted as the metamodel's explicit read arc from the
///     place to the transition.
///   - Labels become descriptions; roles have no metamodel field and are
///     dropped.
///
/// The returned [`ColorMap`] is `None` for a single-color net and
/// otherwise maps unfolded ids back to base place and color, so a caller
/// can report in the document's own vocabulary.
pub fn model_from_json(data: &[u8]) -> Result<(Model, Option<ColorMap>)> {
    let head: Head = serde_json::from_slice(data).unwrap_or_default();

    let net: PetriNet = pflow_core::from_json(data)?;
    let u = unfold(&net);

    let mut model = Model {
        name: head.name,
        description: head.description,
        ..Default::default()
    };

    let mut place_ids: Vec<&String> = u.net.places.keys().collect();
    place_ids.sort();
    for id in place_ids {
        let pl = &u.net.places[id];
        let mut y = pl.y as i64;
        if let Some(cm) = &u.color_map {
            if let Some(cref) = cm.base.get(id) {
                y += cref.color as i64 * COLOR_COPY_SPACING;
            }
        }
        let capacity: i64 = pl.capacity.iter().map(|&c| c as i64).sum();
        model.places.push(MmPlace {
            id: id.clone(),
            initial: pl.token_count() as i64,
            capacity,
            x: pl.x as i64,
            y,
            description: pl.label_text.clone().unwrap_or_default(),
            ..Default::default()
        });
    }

    let mut trans_ids: Vec<&String> = u.net.transitions.keys().collect();
    trans_ids.sort();
    for id in trans_ids {
        let tr = &u.net.transitions[id];
        model.transitions.push(MmTransition {
            id: id.clone(),
            x: tr.x as i64,
            y: tr.y as i64,
            description: tr.label_text.clone().unwrap_or_default(),
            ..Default::default()
        });
    }

    for a in &u.arcs {
        let typ = match a.kind {
            ArcKind::Normal => ArcType::Normal,
            ArcKind::Inhibitor => ArcType::Inhibitor,
            ArcKind::Read => ArcType::Read,
        };
        model.arcs.push(MmArc {
            from: a.source.clone(),
            to: a.target.clone(),
            weight: a.weight as i64,
            typ,
            ..Default::default()
        });
    }

    Ok((model, u.color_map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pflow_json_true_for_places_object() {
        assert!(is_pflow_json(br#"{"places": {"p": {}}}"#));
    }

    #[test]
    fn is_pflow_json_false_for_places_array() {
        assert!(!is_pflow_json(br#"{"places": [{"id": "p"}]}"#));
    }

    #[test]
    fn is_pflow_json_false_for_missing_places() {
        assert!(!is_pflow_json(br#"{"transitions": {}}"#));
    }

    #[test]
    fn is_pflow_json_false_for_null_places() {
        assert!(!is_pflow_json(br#"{"places": null}"#));
    }

    #[test]
    fn is_pflow_json_false_for_invalid_json() {
        assert!(!is_pflow_json(b"not json"));
    }

    #[test]
    fn model_from_json_single_color_basic() {
        let doc = br#"{
            "name": "cafe",
            "description": "a coffee shop",
            "places": {"cups": {"initial": [3], "x": 10, "y": 20, "label": "Cups"}},
            "transitions": {"brew": {"x": 30, "y": 40, "label": "Brew"}},
            "arcs": [{"source": "cups", "target": "brew", "weight": [1]}]
        }"#;
        let (model, cm) = model_from_json(doc).unwrap();
        assert!(cm.is_none());
        assert_eq!(model.name, "cafe");
        assert_eq!(model.description, "a coffee shop");
        assert_eq!(model.places.len(), 1);
        let p = &model.places[0];
        assert_eq!(p.id, "cups");
        assert_eq!(p.initial, 3);
        assert_eq!(p.x, 10);
        assert_eq!(p.y, 20);
        assert_eq!(p.description, "Cups");
        assert_eq!(model.transitions.len(), 1);
        assert_eq!(model.transitions[0].id, "brew");
        assert_eq!(model.arcs.len(), 1);
        assert_eq!(model.arcs[0].from, "cups");
        assert_eq!(model.arcs[0].to, "brew");
        assert_eq!(model.arcs[0].weight, 1);
        assert_eq!(model.arcs[0].typ, ArcType::Normal);
    }

    #[test]
    fn model_from_json_input_side_inhibitor_stays_inhibitor() {
        let doc = br#"{
            "places": {"p": {"initial": [1]}, "t_out": {"initial": [0]}},
            "transitions": {"t": {}},
            "arcs": [
                {"source": "p", "target": "t", "inhibitTransition": true},
                {"source": "t", "target": "t_out"}
            ]
        }"#;
        let (model, _) = model_from_json(doc).unwrap();
        let inhibitor = model.arcs.iter().find(|a| a.from == "p").unwrap();
        assert_eq!(inhibitor.typ, ArcType::Inhibitor);
        assert_eq!(inhibitor.to, "t");
    }

    #[test]
    fn model_from_json_output_side_inhibitor_becomes_read_arc() {
        let doc = br#"{
            "places": {"p": {"initial": [1]}, "q": {"initial": [0]}},
            "transitions": {"t": {}},
            "arcs": [
                {"source": "p", "target": "t"},
                {"source": "t", "target": "q", "inhibitTransition": true}
            ]
        }"#;
        let (model, _) = model_from_json(doc).unwrap();
        let read = model.arcs.iter().find(|a| a.typ == ArcType::Read).unwrap();
        assert_eq!(read.from, "q");
        assert_eq!(read.to, "t");
    }

    #[test]
    fn model_from_json_unfolds_colors_and_offsets_copies() {
        let doc = br#"{
            "token": ["red", "blue"],
            "places": {"pool": {"initial": [3, 1], "x": 0, "y": 100}},
            "transitions": {"t": {}},
            "arcs": [{"source": "pool", "target": "t", "weight": [1, 1]}]
        }"#;
        let (model, cm) = model_from_json(doc).unwrap();
        let cm = cm.expect("2-color net must produce a ColorMap");
        assert_eq!(model.places.len(), 2, "{:?}", model.places);
        let red = model.places.iter().find(|p| p.id == "pool.red").unwrap();
        let blue = model.places.iter().find(|p| p.id == "pool.blue").unwrap();
        assert_eq!(red.y, 100);
        assert_eq!(blue.y, 100 + COLOR_COPY_SPACING);
        assert_eq!(cm.colors, vec!["red".to_string(), "blue".to_string()]);
    }

    #[test]
    fn model_from_json_drops_untouched_color_copies() {
        let doc = br#"{
            "token": ["red", "blue"],
            "places": {
                "src": {"initial": [1, 0]},
                "sink": {"initial": [0, 0]}
            },
            "transitions": {"t": {}},
            "arcs": [
                {"source": "src", "target": "t", "weight": [1, 0]},
                {"source": "t", "target": "sink", "weight": [1, 0]}
            ]
        }"#;
        let (model, _) = model_from_json(doc).unwrap();
        let ids: Vec<&str> = model.places.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"src.red"));
        assert!(ids.contains(&"sink.red"));
        assert!(!ids.contains(&"src.blue"));
        assert!(!ids.contains(&"sink.blue"));
    }

    #[test]
    fn model_from_json_short_color_names_from_urls() {
        let doc = br#"{
            "token": ["https://pflow.xyz/tokens/red", "https://pflow.xyz/tokens/blue"],
            "places": {"pool": {"initial": [1, 1]}},
            "transitions": {"t": {}},
            "arcs": [{"source": "pool", "target": "t", "weight": [1, 1]}]
        }"#;
        let (model, cm) = model_from_json(doc).unwrap();
        let cm = cm.unwrap();
        assert_eq!(cm.colors, vec!["red".to_string(), "blue".to_string()]);
        let ids: Vec<&str> = model.places.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"pool.red"));
        assert!(ids.contains(&"pool.blue"));
    }

    #[test]
    fn model_from_json_capacity_summed_per_color() {
        let doc = br#"{
            "places": {"p": {"initial": [1], "capacity": [10]}},
            "transitions": {},
            "arcs": []
        }"#;
        let (model, _) = model_from_json(doc).unwrap();
        assert_eq!(model.places[0].capacity, 10);
    }
}
