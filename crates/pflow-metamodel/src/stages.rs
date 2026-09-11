//! `ExpandStages`: materializes a `Transition.stages` declaration into a
//! structural Erlang chain, ported from go-pflow's `metamodel/stages.go`.

use std::collections::{HashMap, HashSet};

use crate::error::Error;
use crate::schema::{Arc, ArcType, Model, Place, Transition};

/// Records how [`Model::expand_stages`] rewrote a model, so an engine can
/// run the expanded net and report in the original vocabulary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StageExpansion {
    /// Maps each stage place to the staged transition's input place — the
    /// place a job "still counts as" while mid-service.
    pub carrier_of: HashMap<String, String>,
    /// Maps the last stage transition of each staged transition back to
    /// the original id.
    pub final_stage: HashMap<String, String>,
    /// Every stage transition (all k of them) per original id, in order.
    pub stage_ids: HashMap<String, Vec<String>>,
    /// The declared k per staged transition.
    pub stages: HashMap<String, i64>,
}

impl StageExpansion {
    /// Whether `id` is an internal stage place.
    pub fn is_stage_place(&self, id: &str) -> bool {
        self.carrier_of.contains_key(id)
    }

    /// Rewrites a rate-override map addressed to original transition ids
    /// into one addressed to the expanded net: an override for a staged
    /// transition lands on every stage, scaled by k so the mean duration
    /// still matches the override. Unstaged entries pass through.
    pub fn translate_rates(&self, rates: &HashMap<String, f64>) -> HashMap<String, f64> {
        let mut out = HashMap::with_capacity(rates.len());
        for (id, v) in rates {
            if let Some(stages) = self.stage_ids.get(id) {
                let k = *self.stages.get(id).unwrap_or(&1) as f64;
                for sid in stages {
                    out.insert(sid.clone(), v * k);
                }
            } else {
                out.insert(id.clone(), *v);
            }
        }
        out
    }
}

impl Model {
    /// Materializes every `stages` declaration as a structural Erlang
    /// chain: the staged transition `T` with input place `p` becomes
    /// `p -> T@1 -> T@stage1 -> T@2 -> ... -> T@stageK-1 -> T@K -> outputs`,
    /// with each stage transition at `stages * rate`, so the mean is
    /// unchanged and the variance falls by `stages`.
    ///
    /// A model with no `stages` declarations is returned unchanged (clone,
    /// no expansion record). The declaration is narrow, and every
    /// violation is an error naming the transition: stages goes on the
    /// service transition draining a dedicated in-progress place, and
    /// requires exactly one consuming input arc, weight 1, kinetic; no
    /// guard; no read or inhibitor arcs on the transition; and an input
    /// place with no capacity that no other arc reads, tests or consumes.
    pub fn expand_stages(&self) -> Result<(Model, Option<StageExpansion>), Error> {
        let mut staged: Vec<String> = Vec::new();
        for t in &self.transitions {
            if t.stages > 1 {
                staged.push(t.id.clone());
            }
            if t.stages < 0 {
                return Err(Error::NegativeStages(t.id.clone(), t.stages));
            }
        }
        if staged.is_empty() {
            return Ok((self.clone(), None));
        }
        staged.sort();

        // Consumers and referencers of every place, for the exclusivity
        // checks, computed from the original (unexpanded) model.
        let place_set: HashSet<&str> = self.places.iter().map(|p| p.id.as_str()).collect();
        let mut consumers_of: HashMap<String, Vec<String>> = HashMap::new();
        let mut touches_place: HashMap<String, Vec<String>> = HashMap::new();
        for a in &self.arcs {
            if place_set.contains(a.from.as_str()) {
                touches_place
                    .entry(a.from.clone())
                    .or_default()
                    .push(a.to.clone());
                if matches!(a.typ, ArcType::Normal) {
                    consumers_of
                        .entry(a.from.clone())
                        .or_default()
                        .push(a.to.clone());
                }
            }
        }

        let mut out = self.clone();
        let mut exp = StageExpansion::default();

        for id in &staged {
            let t = out
                .transition_by_id(id)
                .expect("id came from out's own transitions");
            let k = t.stages;
            if !t.guard.is_empty() {
                return Err(Error::Stage(
                    id.clone(),
                    "stages cannot be declared with a guard — the expansion cannot decide which stage the guard gates".to_string(),
                ));
            }

            // Classify the original's arcs.
            let mut input_idx: Option<usize> = None;
            for (i, a) in out.arcs.iter().enumerate() {
                if a.to == *id && !matches!(a.typ, ArcType::Normal) {
                    return Err(Error::Stage(
                        id.clone(),
                        format!(
                            "stages cannot be declared with a {} arc — whether the gate holds for the whole service or only its start is a modelling decision the expansion must not make",
                            a.typ
                        ),
                    ));
                }
                if a.to == *id {
                    if input_idx.is_some() {
                        return Err(Error::Stage(
                            id.clone(),
                            "stages need exactly one consuming input (the in-progress place); found several".to_string(),
                        ));
                    }
                    input_idx = Some(i);
                }
            }
            let input_idx = input_idx.ok_or_else(|| {
                Error::Stage(
                    id.clone(),
                    "stages need a consuming input place; a source transition has no duration to shape".to_string(),
                )
            })?;

            let (input_weight, input_kinetic, p) = {
                let a = &out.arcs[input_idx];
                (a.weight, a.is_kinetic(), a.from.clone())
            };
            if input_weight > 1 {
                return Err(Error::Stage(
                    id.clone(),
                    format!(
                        "stages need an input weight of 1; a batch of {input_weight} entering one Erlang chain is a modelling decision the expansion must not make"
                    ),
                ));
            }
            if !input_kinetic {
                return Err(Error::Stage(
                    id.clone(),
                    "stages need a kinetic input — each in-service job progresses independently, which is what kinetic means".to_string(),
                ));
            }
            let capped = out
                .place_by_id(&p)
                .map(|pl| pl.capacity > 0)
                .unwrap_or(true);
            if capped {
                return Err(Error::Stage(
                    id.clone(),
                    format!("input place {p:?} has a capacity; stage places would hold tokens the bound cannot see"),
                ));
            }
            let consumers = consumers_of.get(&p).map(|v| v.len()).unwrap_or(0);
            if consumers != 1 {
                return Err(Error::Stage(
                    id.clone(),
                    format!(
                        "input place {p:?} feeds other transitions too; whether a mid-service job can still be taken is a modelling decision the expansion must not make"
                    ),
                ));
            }
            let touches = touches_place.get(&p).map(|v| v.len()).unwrap_or(0);
            if touches != consumers {
                return Err(Error::Stage(
                    id.clone(),
                    format!("input place {p:?} is read or tested by other arcs, which would not see mid-service jobs after expansion"),
                ));
            }

            let rate = t.rate * k as f64;
            let desc = t.description.clone();

            let first_id = format!("{id}@1");
            {
                let t_mut = out.transition_by_id_mut(id).expect("checked above");
                t_mut.id = first_id.clone();
                t_mut.rate = rate;
                t_mut.stages = 0;
                t_mut.description = format!("{desc} (stage 1 of {k})");
            }
            out.arcs[input_idx].to = first_id.clone();

            let mut ids = vec![first_id.clone()];
            for s in 1..k {
                let stage_place = format!("{id}@stage{s}");
                let next_id = format!("{id}@{}", s + 1);
                out.places.push(Place {
                    id: stage_place.clone(),
                    description: format!("mid-service: {id}, {s} of {k} stages done"),
                    ..Default::default()
                });
                out.transitions.push(Transition {
                    id: next_id.clone(),
                    rate,
                    description: format!("{desc} (stage {} of {k})", s + 1),
                    ..Default::default()
                });
                let prev = ids.last().expect("ids always non-empty").clone();
                out.arcs.push(Arc {
                    from: prev,
                    to: stage_place.clone(),
                    ..Default::default()
                });
                out.arcs.push(Arc {
                    from: stage_place.clone(),
                    to: next_id.clone(),
                    ..Default::default()
                });
                exp.carrier_of.insert(stage_place, p.clone());
                ids.push(next_id);
            }
            let final_id = ids.last().expect("ids always non-empty").clone();
            // The original's outputs move to the final stage.
            for a in out.arcs.iter_mut() {
                if a.from == *id {
                    a.from = final_id.clone();
                }
            }
            exp.final_stage.insert(final_id, id.clone());
            exp.stage_ids.insert(id.clone(), ids);
            exp.stages.insert(id.clone(), k);
        }

        Ok((out, Some(exp)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firing::Marking;

    // The brewing idiom: start moves a job into a dedicated in-progress
    // place, and the staged finish drains it.
    fn staged_model(k: i64) -> Model {
        Model {
            name: "wash".into(),
            places: vec![
                Place {
                    id: "queue".into(),
                    initial: 3,
                    ..Default::default()
                },
                Place {
                    id: "washing".into(),
                    ..Default::default()
                },
                Place {
                    id: "bay_free".into(),
                    initial: 1,
                    ..Default::default()
                },
                Place {
                    id: "done".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "start".into(),
                    rate: 720.0,
                    ..Default::default()
                },
                Transition {
                    id: "finish".into(),
                    rate: 4.0,
                    stages: k,
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "queue".into(),
                    to: "start".into(),
                    ..Default::default()
                },
                Arc {
                    from: "bay_free".into(),
                    to: "start".into(),
                    ..Default::default()
                },
                Arc {
                    from: "start".into(),
                    to: "washing".into(),
                    ..Default::default()
                },
                Arc {
                    from: "washing".into(),
                    to: "finish".into(),
                    ..Default::default()
                },
                Arc {
                    from: "finish".into(),
                    to: "done".into(),
                    ..Default::default()
                },
                Arc {
                    from: "finish".into(),
                    to: "bay_free".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn identity_when_no_stages() {
        let m = staged_model(0);
        let (out, exp) = m.expand_stages().unwrap();
        assert_eq!(out, m);
        assert!(exp.is_none());

        let m1 = staged_model(1);
        let (out1, exp1) = m1.expand_stages().unwrap();
        assert_eq!(
            out1, m1,
            "stages: 1 means plain exponential, not an expansion"
        );
        assert!(exp1.is_none());
    }

    #[test]
    fn expand_stages_chain() {
        let m = staged_model(3);
        let (out, exp) = m.expand_stages().unwrap();
        let exp = exp.unwrap();
        assert!(
            m.transition_by_id("finish").is_some(),
            "expansion mutated its input"
        );

        for id in ["finish@1", "finish@2", "finish@3"] {
            let tr = out
                .transition_by_id(id)
                .unwrap_or_else(|| panic!("missing stage transition {id}"));
            assert_eq!(tr.rate, 12.0, "{id} rate should be 3 x 4 so the mean holds");
        }
        assert!(
            out.transition_by_id("finish").is_none(),
            "original transition id survived expansion"
        );

        assert_eq!(exp.carrier_of.len(), 2, "3 stages need 2 stage places");
        for (sp, carrier) in &exp.carrier_of {
            assert_eq!(carrier, "washing");
            assert!(
                out.place_by_id(sp).is_some(),
                "stage place {sp} not in expanded model"
            );
        }
        assert_eq!(
            exp.final_stage.get("finish@3").map(|s| s.as_str()),
            Some("finish")
        );

        // The final stage owns the outputs; the first owns the input.
        let last_out = out.arcs.iter().filter(|a| a.from == "finish@3").count();
        let first_in = out
            .arcs
            .iter()
            .filter(|a| a.to == "finish@1" && a.from == "washing")
            .count();
        assert_eq!(last_out, 2);
        assert_eq!(first_in, 1);

        // Rate translation: an override for finish lands on every stage, x3.
        let rates: HashMap<String, f64> =
            [("finish".to_string(), 6.0), ("start".to_string(), 100.0)].into();
        let tr = exp.translate_rates(&rates);
        assert_eq!(tr["finish@1"], 18.0);
        assert_eq!(tr["finish@2"], 18.0);
        assert_eq!(tr["finish@3"], 18.0);
        assert_eq!(tr["start"], 100.0);
    }

    #[test]
    fn expand_stages_refusals() {
        type Case = (&'static str, fn(&mut Model), &'static str);
        let cases: Vec<Case> = vec![
            (
                "guard",
                |m| m.transitions[1].guard = "queue > 0".into(),
                "guard",
            ),
            ("negative", |m| m.transitions[1].stages = -2, "negative"),
            (
                "read arc",
                |m| {
                    m.arcs.push(Arc {
                        from: "queue".into(),
                        to: "finish".into(),
                        typ: ArcType::Read,
                        ..Default::default()
                    })
                },
                "read arc",
            ),
            (
                "second input",
                |m| {
                    m.arcs.push(Arc {
                        from: "queue".into(),
                        to: "finish".into(),
                        ..Default::default()
                    })
                },
                "exactly one",
            ),
            ("batch weight", |m| m.arcs[3].weight = 2, "weight of 1"),
            (
                "non-kinetic input",
                |m| m.arcs[3].kinetic = Some(false),
                "kinetic",
            ),
            (
                "capacitated carrier",
                |m| m.places[1].capacity = 5,
                "capacity",
            ),
            (
                "competing consumer",
                |m| {
                    m.transitions.push(Transition {
                        id: "abandon".into(),
                        rate: 1.0,
                        ..Default::default()
                    });
                    m.arcs.push(Arc {
                        from: "washing".into(),
                        to: "abandon".into(),
                        ..Default::default()
                    });
                },
                "other transitions",
            ),
            (
                "tested carrier",
                |m| {
                    m.transitions.push(Transition {
                        id: "watch".into(),
                        rate: 1.0,
                        ..Default::default()
                    });
                    m.arcs.push(Arc {
                        from: "washing".into(),
                        to: "watch".into(),
                        typ: ArcType::Read,
                        ..Default::default()
                    });
                },
                "read or tested",
            ),
            (
                "source transition",
                |m| {
                    m.arcs = vec![Arc {
                        from: "finish".into(),
                        to: "done".into(),
                        ..Default::default()
                    }]
                },
                "no duration to shape",
            ),
        ];
        for (name, edit, want) in cases {
            let mut m = staged_model(3);
            edit(&mut m);
            let err = m.expand_stages().unwrap_err();
            assert!(
                err.to_string().contains(want),
                "{name}: want error containing {want:?}, got {err}"
            );
        }
    }

    #[test]
    fn expanded_chain_executes_one_stage_at_a_time() {
        let m = staged_model(2);
        let (out, exp) = m.expand_stages().unwrap();
        let exp = exp.unwrap();

        let mut mk: Marking = out.initial_marking();
        // Move a job into service.
        assert!(out.enabled("start", &mk));
        mk = out.fire("start", &mk);
        assert_eq!(mk["washing"], 1);

        // First stage fires: moves washing's job into the stage place, not
        // yet into done.
        assert!(out.enabled("finish@1", &mk));
        mk = out.fire("finish@1", &mk);
        assert_eq!(mk["done"], 0);
        let stage_place = exp
            .carrier_of
            .keys()
            .find(|_| true)
            .cloned()
            .expect("one stage place for 2 stages");
        assert_eq!(mk[&stage_place], 1);

        // Final stage fires: output appears only now.
        assert!(out.enabled("finish@2", &mk));
        mk = out.fire("finish@2", &mk);
        assert_eq!(mk["done"], 1);
        assert_eq!(mk["bay_free"], 1);
    }
}
