//! Declared structural decision variables: an arc weight bound across a
//! group of arcs, or a place capacity. Ported from go-pflow's
//! `metamodel/parameters.go`.

use crate::error::Error;
use crate::schema::{Model, Parameter, ParameterArc};

fn parameter_arc_weight(m: &Model, r: &ParameterArc) -> Result<i64, Error> {
    for a in &m.arcs {
        if a.from == r.from && a.to == r.to {
            return Ok(a.effective_weight());
        }
    }
    Err(Error::NoArc {
        from: r.from.clone(),
        to: r.to.clone(),
    })
}

impl Parameter {
    /// Reads the parameter's current value out of `m` — the bound arcs'
    /// shared weight or the bound place's capacity. Bound arcs that
    /// disagree are an error: the declaration claims they are one
    /// decision, and the model contradicts it.
    pub fn base_value(&self, m: &Model) -> Result<i64, Error> {
        if !self.arcs.is_empty() {
            let mut base = 0i64;
            for r in &self.arcs {
                let w = parameter_arc_weight(m, r)
                    .map_err(|e| Error::Parameter(self.id.clone(), e.to_string()))?;
                if base == 0 {
                    base = w;
                    continue;
                }
                if w != base {
                    return Err(Error::Parameter(
                        self.id.clone(),
                        format!("bound arcs disagree ({base} vs {w}); one decision cannot have two values"),
                    ));
                }
            }
            return Ok(base);
        }
        if !self.capacity.is_empty() {
            return match m.place_by_id(&self.capacity) {
                Some(p) => Ok(p.capacity),
                None => Err(Error::Parameter(
                    self.id.clone(),
                    format!("no place {:?}", self.capacity),
                )),
            };
        }
        Err(Error::Parameter(
            self.id.clone(),
            "binds nothing: set arcs or capacity".to_string(),
        ))
    }
}

impl Model {
    /// Every defect in the parameter declarations at once: a parameter
    /// binding nothing or two things, naming an arc or place the model has
    /// not got, bound arcs whose current weights disagree, a duplicate id,
    /// an id colliding with a place or transition, or bounds that exclude
    /// the bound element's current value.
    pub fn validate_parameters(&self) -> Vec<Error> {
        let mut errs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for p in &self.places {
            names.insert(p.id.as_str());
        }
        for t in &self.transitions {
            names.insert(t.id.as_str());
        }
        for p in &self.parameters {
            if p.id.is_empty() {
                errs.push(Error::Parameter(String::new(), "has no id".to_string()));
                continue;
            }
            if seen.contains(p.id.as_str()) {
                errs.push(Error::Parameter(p.id.clone(), "declared twice".to_string()));
            }
            seen.insert(p.id.as_str());
            if names.contains(p.id.as_str()) {
                errs.push(Error::Parameter(
                    p.id.clone(),
                    "collides with a place or transition of the same name".to_string(),
                ));
            }
            if !p.arcs.is_empty() && !p.capacity.is_empty() {
                errs.push(Error::Parameter(
                    p.id.clone(),
                    "binds both arcs and a capacity; pick one".to_string(),
                ));
                continue;
            }
            let base = match p.base_value(self) {
                Ok(b) => b,
                Err(e) => {
                    errs.push(e);
                    continue;
                }
            };
            if p.min > 0 && p.max > 0 && p.min > p.max {
                errs.push(Error::Parameter(
                    p.id.clone(),
                    format!("min {} exceeds max {}", p.min, p.max),
                ));
                continue;
            }
            if (p.min > 0 && base < p.min) || (p.max > 0 && base > p.max) {
                errs.push(Error::Parameter(
                    p.id.clone(),
                    format!(
                        "the bound element's current value {base} is outside [min {}, max {}]",
                        p.min, p.max
                    ),
                ));
            }
        }
        errs
    }

    /// Returns a copy of `self` with `values` applied to the bound weights
    /// and capacities. An unknown parameter name is an error, never a
    /// silent no-op. Values outside a parameter's declared bounds, or below
    /// 1 for an arc weight, are errors for the same reason. An empty
    /// assignment returns an unchanged clone.
    pub fn apply_parameters(
        &self,
        values: &std::collections::HashMap<String, i64>,
    ) -> Result<Model, Error> {
        let mut out = self.clone();
        if values.is_empty() {
            return Ok(out);
        }
        for (id, &v) in values {
            let p = out
                .parameter_by_id(id)
                .cloned()
                .ok_or_else(|| Error::UnknownParameter(id.clone()))?;
            if p.min > 0 && v < p.min {
                return Err(Error::Parameter(
                    id.clone(),
                    format!("{v} is below min {}", p.min),
                ));
            }
            if p.max > 0 && v > p.max {
                return Err(Error::Parameter(
                    id.clone(),
                    format!("{v} is above max {}", p.max),
                ));
            }
            if !p.arcs.is_empty() {
                if v < 1 {
                    return Err(Error::Parameter(
                        id.clone(),
                        format!("an arc weight must be at least 1, got {v}"),
                    ));
                }
                for r in &p.arcs {
                    let mut found = false;
                    for a in out.arcs.iter_mut() {
                        if a.from == r.from && a.to == r.to {
                            a.weight = v;
                            found = true;
                        }
                    }
                    if !found {
                        return Err(Error::Parameter(
                            id.clone(),
                            format!("no arc {} -> {}", r.from, r.to),
                        ));
                    }
                }
            } else if !p.capacity.is_empty() {
                if v < 0 {
                    return Err(Error::Parameter(
                        id.clone(),
                        format!("a capacity cannot be negative, got {v}"),
                    ));
                }
                let pl = out.place_by_id_mut(&p.capacity).ok_or_else(|| {
                    Error::Parameter(id.clone(), format!("no place {:?}", p.capacity))
                })?;
                pl.capacity = v;
            } else {
                return Err(Error::Parameter(
                    id.clone(),
                    "binds nothing: set arcs or capacity".to_string(),
                ));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Arc, Place, Transition};
    use std::collections::HashMap;

    fn batch_model() -> Model {
        Model {
            name: "batch".into(),
            places: vec![
                Place {
                    id: "dough".into(),
                    initial: 10,
                    ..Default::default()
                },
                Place {
                    id: "baked".into(),
                    capacity: 20,
                    ..Default::default()
                },
            ],
            transitions: vec![Transition {
                id: "bake".into(),
                ..Default::default()
            }],
            arcs: vec![
                Arc {
                    from: "dough".into(),
                    to: "bake".into(),
                    weight: 4,
                    ..Default::default()
                },
                Arc {
                    from: "bake".into(),
                    to: "baked".into(),
                    weight: 4,
                    ..Default::default()
                },
            ],
            parameters: vec![Parameter {
                id: "batch_size".into(),
                arcs: vec![
                    ParameterArc {
                        from: "dough".into(),
                        to: "bake".into(),
                    },
                    ParameterArc {
                        from: "bake".into(),
                        to: "baked".into(),
                    },
                ],
                min: 1,
                max: 10,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn base_value_reads_the_shared_weight() {
        let m = batch_model();
        let p = m.parameter_by_id("batch_size").unwrap();
        assert_eq!(p.base_value(&m).unwrap(), 4);
    }

    #[test]
    fn base_value_errors_when_bound_arcs_disagree() {
        let mut m = batch_model();
        m.arcs[1].weight = 5;
        let p = m.parameter_by_id("batch_size").unwrap();
        let err = p.base_value(&m).unwrap_err();
        assert!(err.to_string().contains("disagree"), "{err}");
    }

    #[test]
    fn apply_parameters_moves_every_bound_arc_together() {
        let m = batch_model();
        let values: HashMap<String, i64> = [("batch_size".to_string(), 6)].into();
        let out = m.apply_parameters(&values).unwrap();
        assert_eq!(out.arcs[0].weight, 6);
        assert_eq!(out.arcs[1].weight, 6);
        // Original is untouched.
        assert_eq!(m.arcs[0].weight, 4);
    }

    #[test]
    fn apply_parameters_rejects_unknown_name() {
        let m = batch_model();
        let values: HashMap<String, i64> = [("nope".to_string(), 1)].into();
        let err = m.apply_parameters(&values).unwrap_err();
        assert!(matches!(err, Error::UnknownParameter(_)));
    }

    #[test]
    fn apply_parameters_rejects_out_of_bounds() {
        let m = batch_model();
        let values: HashMap<String, i64> = [("batch_size".to_string(), 99)].into();
        let err = m.apply_parameters(&values).unwrap_err();
        assert!(err.to_string().contains("above max"), "{err}");
    }

    #[test]
    fn apply_parameters_empty_assignment_is_a_no_op() {
        let m = batch_model();
        let out = m.apply_parameters(&HashMap::new()).unwrap();
        assert_eq!(out, m);
    }

    #[test]
    fn validate_parameters_flags_collision_with_element_name() {
        let mut m = batch_model();
        m.parameters[0].id = "dough".into();
        let errs = m.validate_parameters();
        assert!(
            errs.iter().any(|e| e.to_string().contains("collides")),
            "{errs:?}"
        );
    }

    #[test]
    fn capacity_parameter_round_trips() {
        let m = Model {
            name: "shelf".into(),
            places: vec![Place {
                id: "queue".into(),
                capacity: 5,
                ..Default::default()
            }],
            transitions: vec![Transition {
                id: "arrive".into(),
                ..Default::default()
            }],
            arcs: vec![Arc {
                from: "arrive".into(),
                to: "queue".into(),
                ..Default::default()
            }],
            parameters: vec![Parameter {
                id: "shelf_size".into(),
                capacity: "queue".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let p = m.parameter_by_id("shelf_size").unwrap();
        assert_eq!(p.base_value(&m).unwrap(), 5);

        let values: HashMap<String, i64> = [("shelf_size".to_string(), 8)].into();
        let out = m.apply_parameters(&values).unwrap();
        assert_eq!(out.place_by_id("queue").unwrap().capacity, 8);
    }
}
