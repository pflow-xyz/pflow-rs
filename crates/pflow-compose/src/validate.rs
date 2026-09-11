//! `Bundle::validate`, ported from go-pflow's `metamodel/compose.go`
//! (`Validate`, `validateNetType`, `validateLink`, `validatePlacePair`,
//! `validateDataLinkObserver`).

use std::collections::{HashMap, HashSet};

use pflow_metamodel::{ArcType, Model};

use crate::bundle::*;
use crate::guard::{parse_condition, resolve_lowering};
use crate::matrix::link_legal;
use crate::queue::is_unbounded_queue;

/// A resolved endpoint after ports have been resolved to elements.
pub(crate) struct ResolvedEndpoint<'a> {
    pub subnet: &'a Subnet,
    pub place: String,
    pub transition: String,
}

impl<'a> ResolvedEndpoint<'a> {
    fn element(&self) -> &str {
        if !self.transition.is_empty() {
            &self.transition
        } else {
            &self.place
        }
    }
}

impl std::fmt::Display for ResolvedEndpoint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.subnet.id, self.element())
    }
}

/// Turns an [`Endpoint`] into the concrete subnet + element it names.
pub(crate) fn resolve<'a>(b: &'a Bundle, e: &Endpoint) -> Result<ResolvedEndpoint<'a>, String> {
    let s = b
        .subnet_by_id(&e.subnet)
        .ok_or_else(|| format!("unknown subnet {:?}", e.subnet))?;

    let (place, transition) = if !e.port.is_empty() {
        // A declared port takes priority; otherwise fall back to the
        // derived boundary (one inout port per exported place).
        let port = match s.port_by_id(&e.port) {
            Some(p) => p.clone(),
            None => s
                .derived_ports()
                .into_iter()
                .find(|p| p.id == e.port)
                .ok_or_else(|| format!("subnet {:?} has no port {:?}", e.subnet, e.port))?,
        };
        if port.is_transition() {
            (String::new(), port.transition)
        } else {
            (port.place, String::new())
        }
    } else if !e.place.is_empty() {
        (e.place.clone(), String::new())
    } else if !e.transition.is_empty() {
        (String::new(), e.transition.clone())
    } else {
        return Err(format!(
            "endpoint on subnet {:?} names neither a port, a place, nor a transition",
            e.subnet
        ));
    };

    if !place.is_empty() && s.model.place_by_id(&place).is_none() {
        return Err(format!("subnet {:?} has no place {:?}", e.subnet, place));
    }
    if !transition.is_empty() && s.model.transition_by_id(&transition).is_none() {
        return Err(format!("subnet {:?} has no transition {:?}", e.subnet, transition));
    }
    Ok(ResolvedEndpoint {
        subnet: s,
        place,
        transition,
    })
}

impl Bundle {
    /// Checks the bundle's structure and typing.
    pub fn validate(&self) -> ValidationResult {
        let mut res = ValidationResult {
            valid: true,
            ..Default::default()
        };

        let mut seen = HashSet::new();
        for s in &self.subnets {
            if s.id.is_empty() {
                res.valid = false;
                res.errors
                    .push(ValidationError::new(ERR_DUPLICATE_SUBNET, "subnet has an empty ID", ""));
                continue;
            }
            if !seen.insert(s.id.clone()) {
                res.valid = false;
                res.errors.push(ValidationError::new(
                    ERR_DUPLICATE_SUBNET,
                    format!("duplicate subnet ID {:?}", s.id),
                    &s.id,
                ));
                continue;
            }

            if s.net_type == NetType::Untyped {
                res.warnings.push(ValidationError::new(
                    WARN_UNTYPED_SUBNET,
                    format!("subnet {:?} has no net_type, so no link is rejected by typing", s.id),
                    &s.id,
                ));
            }

            for ve in validate_arcs(&s.model) {
                res.valid = false;
                res.errors.push(ValidationError::new(
                    &ve.code,
                    format!("subnet {:?}: {}", s.id, ve.message),
                    format!("{}:{}", s.id, ve.element),
                ));
            }

            let mut port_ids = HashSet::new();
            for p in &s.ports {
                if !port_ids.insert(p.id.clone()) {
                    res.valid = false;
                    res.errors.push(ValidationError::new(
                        ERR_DUPLICATE_PORT,
                        format!("subnet {:?} has duplicate port {:?}", s.id, p.id),
                        format!("{}:{}", s.id, p.id),
                    ));
                    continue;
                }

                if p.is_transition() {
                    if s.model.transition_by_id(&p.transition).is_none() {
                        res.valid = false;
                        res.errors.push(ValidationError::new(
                            ERR_PORT_UNKNOWN_ELEMENT,
                            format!("port {:?} of subnet {:?} names unknown transition {:?}", p.id, s.id, p.transition),
                            format!("{}:{}", s.id, p.id),
                        ));
                    }
                    continue;
                }

                match s.model.place_by_id(&p.place) {
                    None => {
                        res.valid = false;
                        res.errors.push(ValidationError::new(
                            ERR_PORT_UNKNOWN_ELEMENT,
                            format!("port {:?} of subnet {:?} names unknown place {:?}", p.id, s.id, p.place),
                            format!("{}:{}", s.id, p.id),
                        ));
                    }
                    Some(place) if !place.exported => {
                        res.valid = false;
                        res.errors.push(ValidationError::new(
                            ERR_PORT_NOT_EXPORTED,
                            format!("port {:?} of subnet {:?} exposes place {:?}, which is not exported", p.id, s.id, p.place),
                            format!("{}:{}", s.id, p.id),
                        ));
                    }
                    _ => {}
                }
            }

            self.validate_net_type(s, &mut res);
        }

        if !res.valid {
            return res; // endpoint checks below would cascade
        }

        // Global ID uniqueness when namespacing is off.
        if !self.namespaced() {
            let mut flat: HashMap<String, String> = HashMap::new();
            for s in &self.subnets {
                for p in &s.model.places {
                    if let Some(prev) = flat.get(&p.id) {
                        res.valid = false;
                        res.errors.push(ValidationError::new(
                            ERR_DUPLICATE_ID,
                            format!("place {:?} appears in both {:?} and {:?} but namespacing is off", p.id, prev, s.id),
                            &p.id,
                        ));
                    }
                    flat.insert(p.id.clone(), s.id.clone());
                }
                for t in &s.model.transitions {
                    if let Some(prev) = flat.get(&t.id) {
                        res.valid = false;
                        res.errors.push(ValidationError::new(
                            ERR_DUPLICATE_ID,
                            format!("transition {:?} appears in both {:?} and {:?} but namespacing is off", t.id, prev, s.id),
                            &t.id,
                        ));
                    }
                    flat.insert(t.id.clone(), s.id.clone());
                }
            }
        }

        for (idx, l) in self.links.iter().enumerate() {
            self.validate_link(l, idx, &mut res);
        }

        // At most one simulation objective survives composition.
        let mut objectives = Vec::new();
        for s in &self.subnets {
            if let Some(sim) = &s.model.simulation {
                if !sim.objective.is_empty() {
                    objectives.push(s.id.clone());
                }
            }
        }
        if objectives.len() > 1 {
            res.valid = false;
            res.errors.push(ValidationError::new(
                ERR_MULTIPLE_OBJECTIVES,
                format!(
                    "subnets {} each declare a simulation objective; there is no meaningful composite objective",
                    objectives.join(", ")
                ),
                "",
            ));
        }

        res
    }

    /// Reports the first error, or `Ok(())` when the bundle is valid.
    pub fn must_validate(&self) -> Result<(), String> {
        let res = self.validate();
        if res.valid {
            return Ok(());
        }
        let msgs: Vec<String> = res
            .errors
            .iter()
            .map(|e| format!("[{}] {}", e.code, e.message))
            .collect();
        Err(format!("invalid bundle: {}", msgs.join("; ")))
    }

    fn validate_net_type(&self, s: &Subnet, res: &mut ValidationResult) {
        if is_unbounded_queue(s) {
            res.warnings.push(ValidationError::new(
                WARN_UNBOUNDED_QUEUE,
                format!(
                    "queue {:?} has no capacity, so its enqueue is a source transition and the net is not structurally bounded; bound it with a capacity, or fuse enqueue with a producer whose own net is bounded",
                    s.id
                ),
                &s.id,
            ));
        }

        if s.net_type == NetType::Workflow {
            let marked: i64 = s
                .model
                .places
                .iter()
                .filter(|p| p.is_token() && p.initial > 0)
                .map(|p| p.initial)
                .sum();
            if marked != 1 {
                res.warnings.push(ValidationError::new(
                    WARN_WORKFLOW_CURSOR,
                    format!("WorkflowNet {:?} starts with {marked} tokens; a workflow cursor should be exactly one", s.id),
                    &s.id,
                ));
            }
        }
    }

    fn validate_link(&self, l: &Link, idx: usize, res: &mut ValidationResult) {
        let label = if l.id.is_empty() {
            format!("links[{idx}]")
        } else {
            l.id.clone()
        };

        let from = match resolve(self, &l.from) {
            Ok(r) => r,
            Err(e) => {
                res.valid = false;
                res.errors
                    .push(ValidationError::new(ERR_BAD_ENDPOINT, format!("{label}: from: {e}"), &label));
                return;
            }
        };
        let to = match resolve(self, &l.to) {
            Ok(r) => r,
            Err(e) => {
                res.valid = false;
                res.errors
                    .push(ValidationError::new(ERR_BAD_ENDPOINT, format!("{label}: to: {e}"), &label));
                return;
            }
        };

        match l.kind {
            LinkKind::Token | LinkKind::Data => {
                if !from.transition.is_empty() || !to.transition.is_empty() {
                    res.valid = false;
                    res.errors.push(ValidationError::new(
                        ERR_ENDPOINT_KIND,
                        format!("{label}: a {:?} link connects places, but an endpoint names a transition", l.kind),
                        &label,
                    ));
                    return;
                }
            }
            LinkKind::Event => {
                if !from.place.is_empty() || !to.place.is_empty() {
                    res.valid = false;
                    res.errors.push(ValidationError::new(
                        ERR_ENDPOINT_KIND,
                        format!("{label}: an event link connects transitions, but an endpoint names a place"),
                        &label,
                    ));
                    return;
                }
            }
            LinkKind::Guard => {
                if from.transition.is_empty() {
                    res.valid = false;
                    res.errors.push(ValidationError::new(
                        ERR_ENDPOINT_KIND,
                        format!("{label}: a guard link's from must be the gated transition"),
                        &label,
                    ));
                    return;
                }
                if to.place.is_empty() {
                    res.valid = false;
                    res.errors.push(ValidationError::new(
                        ERR_ENDPOINT_KIND,
                        format!("{label}: a guard link's to must be the observed place"),
                        &label,
                    ));
                    return;
                }
            }
        }

        let (ok, why) = link_legal(l.kind, from.subnet.net_type, to.subnet.net_type);
        if !ok {
            res.valid = false;
            res.errors
                .push(ValidationError::new(ERR_ILLEGAL_LINK, format!("{label}: {why}"), &label));
            return;
        }

        match l.kind {
            LinkKind::Token => {
                self.validate_place_pair(&label, &from, &to, res);
                // Port direction checks are skipped here: this
                // implementation resolves ports to bare place/transition
                // names rather than keeping the `Port` object on the
                // resolved endpoint (see `resolve`), so there is nothing to
                // re-check port `kind` against. The place-pair and
                // structural checks above are the ones the flattened model
                // actually depends on.
            }
            LinkKind::Data => {
                self.validate_place_pair(&label, &from, &to, res);
                self.validate_data_link_observer(&label, &to, res);
            }
            LinkKind::Guard => {
                if let Err(e) = parse_condition(&l.condition) {
                    res.valid = false;
                    res.errors
                        .push(ValidationError::new(ERR_BAD_CONDITION, format!("{label}: {e}"), &label));
                    return;
                }
                let lowered = match resolve_lowering(l) {
                    Ok(l) => l,
                    Err(e) => {
                        res.valid = false;
                        res.errors
                            .push(ValidationError::new(ERR_BAD_CONDITION, format!("{label}: {e}"), &label));
                        return;
                    }
                };
                res.warnings.push(ValidationError::new(
                    WARN_RESTRICTIVE_LINK,
                    format!(
                        "{label}: a guard link restricts {}; properties proved of it alone may no longer hold",
                        from.subnet.id
                    ),
                    &label,
                ));
                if lowered.strategy == crate::bundle::LOWERING_EXPR {
                    res.warnings.push(ValidationError::new(
                        WARN_GUARD_OPAQUE,
                        format!("{label}: lowered to a guard expression, which reachability and verify do not evaluate; static claims about this net are weakened"),
                        &label,
                    ));
                }
            }
            LinkKind::Event => {}
        }
    }

    fn validate_place_pair(&self, label: &str, from: &ResolvedEndpoint, to: &ResolvedEndpoint, res: &mut ValidationResult) {
        let fp = from.subnet.model.place_by_id(&from.place);
        let tp = to.subnet.model.place_by_id(&to.place);
        let (fp, tp) = match (fp, tp) {
            (Some(a), Some(b)) => (a, b),
            _ => return,
        };
        if fp.is_token() != tp.is_token() {
            res.valid = false;
            res.errors.push(ValidationError::new(
                ERR_KIND_MISMATCH,
                format!(
                    "{label}: cannot fuse {from} ({}) with {to} ({}) — a counter and a data cell have no common semantics",
                    kind_name(fp),
                    kind_name(tp)
                ),
                label,
            ));
        }
        if !fp.typ.is_empty() && !tp.typ.is_empty() && fp.typ != tp.typ {
            res.valid = false;
            res.errors.push(ValidationError::new(
                ERR_TYPE_MISMATCH,
                format!("{label}: cannot fuse {from} (type {:?}) with {to} (type {:?})", fp.typ, tp.typ),
                label,
            ));
        }
    }

    fn validate_data_link_observer(&self, label: &str, to: &ResolvedEndpoint, res: &mut ValidationResult) {
        for a in &to.subnet.model.arcs {
            if a.is_read_only() {
                continue;
            }
            if a.from == to.place || a.to == to.place {
                res.valid = false;
                res.errors.push(ValidationError::new(
                    ERR_DATALINK_CONSUMES,
                    format!(
                        "{label}: observer {to} has arc {} -> {} on the observed place; a data link is read-only. Express the dependency as a read arc (tokens >= n), an inhibitor arc, or a guard (tokens({:?}) > 0).",
                        a.from, a.to, to.place
                    ),
                    label,
                ));
                return;
            }
        }
    }
}

/// Checks each arc's type and, for read arcs, its direction. Ported from
/// go-pflow's `metamodel/validation.go` `ValidateArcs`.
///
/// An unknown arc type is an error: every reader that does not recognise a
/// type falls back to treating the arc as a normal consuming one, which
/// turns a constraint into token theft.
pub(crate) fn validate_arcs(m: &Model) -> Vec<ValidationError> {
    let mut out = Vec::new();
    for a in &m.arcs {
        let label = format!("{} -> {}", a.from, a.to);

        if !matches!(a.typ, ArcType::Normal | ArcType::Inhibitor | ArcType::Read) {
            out.push(ValidationError::new(
                ERR_UNKNOWN_ARC_TYPE,
                format!("arc {label} has unknown type {:?}; this build understands \"\" (normal), \"inhibitor\", \"read\"", a.typ),
                &label,
            ));
            continue;
        }

        // A read arc tests a place's marking, so only place -> transition
        // carries meaning.
        if a.is_read() && (m.place_by_id(&a.from).is_none() || m.transition_by_id(&a.to).is_none()) {
            out.push(ValidationError::new(
                ERR_READ_ARC_DIRECTION,
                format!("read arc {label} must run place -> transition"),
                &label,
            ));
        }

        // Kinetics only has meaning on a consuming place -> transition arc.
        if !a.is_kinetic() {
            let from_is_token_place = m.place_by_id(&a.from).is_some_and(|p| p.is_token());
            let consuming = from_is_token_place && m.transition_by_id(&a.to).is_some();
            if a.is_read_only() || !consuming {
                out.push(ValidationError::new(
                    ERR_KINETIC_MISPLACED,
                    format!(r#"arc {label} declares "kinetic": false, but only a consuming place -> transition arc appears in a rate law"#),
                    &label,
                ));
            }
        }
    }
    out
}
