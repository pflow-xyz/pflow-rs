//! `Bundle::flatten` / `flatten_with_map`, ported from go-pflow's
//! `metamodel/compose_flatten.go`.

use std::collections::HashMap;

use pflow_metamodel::{Arc as MArc, ArcType, Constraint, Event, Model, Place, Simulation, SolverConfig, StateKind};

use crate::bundle::*;
use crate::exprrewrite::rewrite_place_refs;
use crate::fuse::{fuse_transitions, subnet_place_alias};
use crate::guard::{guard_conjunct, resolve_lowering};
use crate::unionfind::UnionFind;
use crate::validate::resolve;

impl Bundle {
    /// Lowers the bundle to a single `Model`.
    pub fn flatten(&self) -> Result<Model, String> {
        Ok(self.flatten_with_map()?.0)
    }

    /// Lowers the bundle and also returns the rewrite map.
    ///
    /// Pipeline: identity short-circuit -> validate -> place equivalence
    /// classes (TokenLink+DataLink) -> transition equivalence classes
    /// (EventLink) -> emit merged places -> emit fused transitions -> emit
    /// retargeted, weight-merged arcs -> lower guard links -> rewrite
    /// guards, constraints and simulation through the alias maps.
    pub fn flatten_with_map(&self) -> Result<(Model, FlattenMap), String> {
        // 0. Identity: a lone subnet with no links flattens to itself, with
        // no namespacing.
        if self.subnets.len() == 1 && self.links.is_empty() {
            let mut out = self.subnets[0].model.clone();
            if !self.name.is_empty() {
                out.name = self.name.clone();
            }
            out.constraints.extend(self.constraints.iter().cloned());
            return Ok((out, identity_flatten_map(self)));
        }

        // 1. Validate.
        let res = self.validate();
        if !res.valid {
            return Err(self.must_validate().unwrap_err());
        }
        let mut fm = FlattenMap {
            warnings: res.warnings,
            ..Default::default()
        };

        let subnets = self.sorted_subnets();

        // 2. Place classes.
        let mut places = UnionFind::new();
        for s in &subnets {
            for p in &s.model.places {
                places.add(&format!("{}/{}", s.id, p.id));
            }
        }
        for l in &self.links {
            if l.kind != LinkKind::Token && l.kind != LinkKind::Data {
                continue;
            }
            if let (Ok(from), Ok(to)) = (resolve(self, &l.from), resolve(self, &l.to)) {
                places.union(&format!("{}/{}", from.subnet.id, from.place), &format!("{}/{}", to.subnet.id, to.place));
            }
        }

        // 3. Transition classes, merged by EventLink.
        let mut transitions = UnionFind::new();
        for s in &subnets {
            for t in &s.model.transitions {
                transitions.add(&format!("{}/{}", s.id, t.id));
            }
        }
        let mut event_link_ids: HashMap<String, String> = HashMap::new();
        for l in &self.links {
            if l.kind != LinkKind::Event {
                continue;
            }
            if let (Ok(from), Ok(to)) = (resolve(self, &l.from), resolve(self, &l.to)) {
                transitions.union(&format!("{}/{}", from.subnet.id, from.transition), &format!("{}/{}", to.subnet.id, to.transition));
            }
        }
        for l in &self.links {
            if l.kind != LinkKind::Event || l.id.is_empty() {
                continue;
            }
            if let Ok(from) = resolve(self, &l.from) {
                let root = transitions.find(&format!("{}/{}", from.subnet.id, from.transition));
                if let Some(prev) = event_link_ids.get(&root) {
                    if prev != &l.id {
                        return Err(format!(
                            "event links {prev:?} and {:?} fuse the same transitions but name the result differently",
                            l.id
                        ));
                    }
                }
                event_link_ids.insert(root, l.id.clone());
            }
        }

        let place_groups = places.groups();
        let trans_groups = transitions.groups();

        // Flat names.
        let mut place_flat: HashMap<String, String> = HashMap::new();
        for (root, members) in &place_groups {
            let name = self.flat_place_name(root, members);
            for member in members {
                place_flat.insert(member.clone(), name.clone());
            }
            if members.len() > 1 {
                fm.wires.insert(name, members.clone());
            }
        }
        let mut trans_flat: HashMap<String, String> = HashMap::new();
        for (root, members) in &trans_groups {
            let explicit = event_link_ids.get(root).cloned().unwrap_or_default();
            let name = self.flat_transition_name(root, members, &explicit);
            for member in members {
                trans_flat.insert(member.clone(), name.clone());
            }
            if members.len() > 1 {
                fm.fused_groups.insert(name.clone(), members.clone());
                fm.warnings.push(ValidationError::new(
                    WARN_EVENTLINK_CYCLE,
                    format!("transitions {} fire as one", members.join(", ")),
                    &name,
                ));
            }
        }

        for s in &subnets {
            let mut pm = HashMap::new();
            let mut tm = HashMap::new();
            for p in &s.model.places {
                pm.insert(p.id.clone(), place_flat.get(&format!("{}/{}", s.id, p.id)).cloned().unwrap_or_default());
            }
            for t in &s.model.transitions {
                tm.insert(t.id.clone(), trans_flat.get(&format!("{}/{}", s.id, t.id)).cloned().unwrap_or_default());
            }
            fm.place.insert(s.id.clone(), pm);
            fm.transition.insert(s.id.clone(), tm);
            fm.place_prefix.insert(s.id.clone(), self.prefix(&s.id));
        }

        let mut out = Model {
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            ..Default::default()
        };

        // 4. Merged places.
        out.places = merge_places(&subnets, &place_flat)?;

        // 5. Fused transitions.
        out.transitions = fuse_transitions(self, &subnets, &trans_groups, &trans_flat, &place_flat, &mut fm)?;

        // 6. Arcs.
        out.arcs = self.merge_arcs(&subnets, &place_flat, &trans_flat);

        // 7. Guard links.
        self.apply_guard_links(&mut out, &place_flat, &trans_flat)?;

        // 8. Constraints, events, simulation.
        out.constraints = self.rewrite_constraints(&subnets, &place_flat);
        out.events = merge_events(&subnets)?;
        out.simulation = self.merge_simulation(&subnets, &place_flat, &trans_flat);
        self.merge_token_display(&subnets, &mut out);

        normalize_kinds(&mut out);
        Ok((out, fm))
    }

    fn flat_place_name(&self, root: &str, members: &[String]) -> String {
        let (subnet_id, local_id) = split_qualified(root);
        if members.len() == 1 {
            self.qualified_id(subnet_id, local_id)
        } else {
            format!("wire:{}", self.qualified_id(subnet_id, local_id))
        }
    }

    fn flat_transition_name(&self, root: &str, members: &[String], explicit_id: &str) -> String {
        let (subnet_id, local_id) = split_qualified(root);
        if members.len() == 1 {
            return self.qualified_id(subnet_id, local_id);
        }
        if !explicit_id.is_empty() {
            return explicit_id.to_string();
        }
        let mut qualified: Vec<String> = members
            .iter()
            .map(|m| {
                let (s, l) = split_qualified(m);
                self.qualified_id(s, l)
            })
            .collect();
        qualified.sort();
        format!("fused:{}", qualified.join("+"))
    }

    /// Retargets every arc onto flat IDs and merges duplicates.
    fn merge_arcs(&self, subnets: &[&Subnet], place_flat: &HashMap<String, String>, trans_flat: &HashMap<String, String>) -> Vec<MArc> {
        #[derive(PartialEq, Eq, Hash, Clone)]
        struct ArcKey {
            from: String,
            to: String,
            typ: ArcType,
            kinetic: bool,
            keys: String,
            value: String,
        }

        let mut order: Vec<ArcKey> = Vec::new();
        let mut acc: HashMap<ArcKey, MArc> = HashMap::new();

        for s in subnets {
            for a in &s.model.arcs {
                let mut flat = a.clone();
                flat.from = self.translate_ref(s, &a.from, place_flat, trans_flat);
                flat.to = self.translate_ref(s, &a.to, place_flat, trans_flat);
                if flat.weight == 0 {
                    flat.weight = 1;
                }

                let key = ArcKey {
                    from: flat.from.clone(),
                    to: flat.to.clone(),
                    typ: flat.typ,
                    kinetic: flat.is_kinetic(),
                    keys: a.keys.join("\u{0}"),
                    value: a.value.clone(),
                };
                if let Some(existing) = acc.get_mut(&key) {
                    if flat.is_read() {
                        if flat.weight > existing.weight {
                            existing.weight = flat.weight;
                        }
                    } else if flat.is_inhibitor() {
                        if flat.weight < existing.weight {
                            existing.weight = flat.weight;
                        }
                    } else if self.arc_merge() == ArcMergePolicy::Max {
                        if flat.weight > existing.weight {
                            existing.weight = flat.weight;
                        }
                    } else {
                        existing.weight += flat.weight;
                    }
                    continue;
                }
                order.push(key.clone());
                acc.insert(key, flat);
            }
        }

        order.into_iter().map(|k| acc.remove(&k).expect("key present")).collect()
    }

    fn translate_ref(&self, s: &Subnet, local_id: &str, place_flat: &HashMap<String, String>, trans_flat: &HashMap<String, String>) -> String {
        let key = format!("{}/{}", s.id, local_id);
        if let Some(flat) = place_flat.get(&key) {
            return flat.clone();
        }
        if let Some(flat) = trans_flat.get(&key) {
            return flat.clone();
        }
        self.qualified_id(&s.id, local_id)
    }

    /// Lowers each guard link onto the flattened model.
    fn apply_guard_links(&self, out: &mut Model, place_flat: &HashMap<String, String>, trans_flat: &HashMap<String, String>) -> Result<(), String> {
        for l in &self.links {
            if l.kind != LinkKind::Guard {
                continue;
            }
            let from = resolve(self, &l.from)?;
            let to = resolve(self, &l.to)?;

            let flat_place = place_flat.get(&format!("{}/{}", to.subnet.id, to.place)).cloned().unwrap_or_default();
            let flat_trans = trans_flat.get(&format!("{}/{}", from.subnet.id, from.transition)).cloned().unwrap_or_default();

            let lowered = resolve_lowering(l)?;

            match lowered.strategy.as_str() {
                LOWERING_STRUCTURAL => {
                    if lowered.read > 0 {
                        out.arcs.push(MArc {
                            from: flat_place.clone(),
                            to: flat_trans.clone(),
                            weight: lowered.read,
                            typ: ArcType::Read,
                            ..Default::default()
                        });
                    }
                    if lowered.inhibit > 0 {
                        out.arcs.push(MArc {
                            from: flat_place.clone(),
                            to: flat_trans.clone(),
                            weight: lowered.inhibit,
                            typ: ArcType::Inhibitor,
                            ..Default::default()
                        });
                    }
                }
                LOWERING_EXPR => {
                    let conjunct = guard_conjunct(&flat_place, &l.condition)?;
                    let t = out
                        .transition_by_id_mut(&flat_trans)
                        .ok_or_else(|| format!("guard link targets transition {flat_trans:?}, which is not in the flattened model"))?;
                    t.guard = and_guards_two(&t.guard, &conjunct);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Namespaces constraint IDs and rewrites the place references inside
    /// their expressions.
    fn rewrite_constraints(&self, subnets: &[&Subnet], place_flat: &HashMap<String, String>) -> Vec<Constraint> {
        let mut out = Vec::new();
        for s in subnets {
            let exact = subnet_place_alias(s, place_flat);
            for c in &s.model.constraints {
                out.push(Constraint {
                    id: self.qualified_id(&s.id, &c.id),
                    expr: rewrite_place_refs(&c.expr, &exact, &self.prefix(&s.id)),
                });
            }
        }
        out.extend(self.constraints.iter().cloned());
        out
    }

    /// Carries at most one objective through, and remaps solver rate keys
    /// and player turn places onto flat IDs.
    fn merge_simulation(&self, subnets: &[&Subnet], place_flat: &HashMap<String, String>, trans_flat: &HashMap<String, String>) -> Option<Simulation> {
        let mut out: Option<Simulation> = None;

        for s in subnets {
            let sim = match &s.model.simulation {
                Some(sim) => sim,
                None => continue,
            };
            let o = out.get_or_insert_with(Simulation::default);

            if !sim.objective.is_empty() {
                o.objective = rewrite_place_refs(&sim.objective, &subnet_place_alias(s, place_flat), &self.prefix(&s.id));
            }

            if let Some(players) = &sim.players {
                for (name, player) in players {
                    let mut p = player.clone();
                    if !p.turn_place.is_empty() {
                        p.turn_place = place_flat.get(&format!("{}/{}", s.id, p.turn_place)).cloned().unwrap_or_default();
                    }
                    p.transitions = player
                        .transitions
                        .iter()
                        .map(|t| trans_flat.get(&format!("{}/{t}", s.id)).cloned().unwrap_or_default())
                        .collect();
                    o.players
                        .get_or_insert_with(HashMap::new)
                        .insert(self.qualified_id(&s.id, name), p);
                }
            }

            if let Some(solver) = &sim.solver {
                let o_solver = o.solver.get_or_insert(SolverConfig {
                    tspan: solver.tspan,
                    dt: solver.dt,
                    rates: None,
                });
                if let Some(rates) = &solver.rates {
                    for (tid, rate) in rates {
                        let flat = trans_flat
                            .get(&format!("{}/{tid}", s.id))
                            .cloned()
                            .filter(|f| !f.is_empty())
                            .unwrap_or_else(|| self.qualified_id(&s.id, tid));
                        let map = o_solver.rates.get_or_insert_with(HashMap::new);
                        // A fused transition fires no faster than its slowest participant.
                        match map.get(&flat) {
                            Some(&prev) if prev <= *rate => {}
                            _ => {
                                map.insert(flat, *rate);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn merge_token_display(&self, subnets: &[&Subnet], out: &mut Model) {
        for s in subnets {
            if out.decimals == 0 && s.model.decimals != 0 {
                out.decimals = s.model.decimals;
            }
            if out.unit.is_empty() && !s.model.unit.is_empty() {
                out.unit = s.model.unit.clone();
            }
        }
    }
}

fn and_guards_two(a: &str, b: &str) -> String {
    crate::guard::and_guards(&[a.to_string(), b.to_string()])
}

fn identity_flatten_map(b: &Bundle) -> FlattenMap {
    let mut fm = FlattenMap::default();
    let s = &b.subnets[0];
    let mut pm = HashMap::new();
    let mut tm = HashMap::new();
    for p in &s.model.places {
        pm.insert(p.id.clone(), p.id.clone());
    }
    for t in &s.model.transitions {
        tm.insert(t.id.clone(), t.id.clone());
    }
    fm.place.insert(s.id.clone(), pm);
    fm.transition.insert(s.id.clone(), tm);
    fm.place_prefix.insert(s.id.clone(), String::new());
    fm
}

fn split_qualified(id: &str) -> (&str, &str) {
    match id.find('/') {
        Some(i) => (&id[..i], &id[i + 1..]),
        None => ("", id),
    }
}

/// Folds each place class into a single `Place`. Merge rules:
///
/// - `kind` must agree
/// - `type` non-empty values must agree
/// - `initial` sums (two producers may each contribute starting stock)
/// - `capacity` is the min of non-zero bounds (fusion means both bounds hold)
/// - `exported`/`persisted`/`resource` OR together
/// - `initial_value` conflict is an error
fn merge_places(subnets: &[&Subnet], place_flat: &HashMap<String, String>) -> Result<Vec<Place>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut acc: HashMap<String, Place> = HashMap::new();
    let mut members: HashMap<String, Vec<String>> = HashMap::new();

    for s in subnets {
        for p in &s.model.places {
            let flat = place_flat.get(&format!("{}/{}", s.id, p.id)).cloned().unwrap_or_default();
            if let Some(existing) = acc.get_mut(&flat) {
                let member_key = format!("{}/{}", s.id, p.id);
                members.entry(flat.clone()).or_default().push(member_key.clone());

                if existing.is_token() != p.is_token() {
                    return Err(format!(
                        "[{ERR_KIND_MISMATCH}] fusing {flat}: {} is a {} place but {member_key} is a {} place",
                        members[&flat][0],
                        kind_name(existing),
                        kind_name(p)
                    ));
                }
                if !existing.typ.is_empty() && !p.typ.is_empty() && existing.typ != p.typ {
                    return Err(format!(
                        "[{ERR_TYPE_MISMATCH}] fusing {flat}: types {:?} and {:?} differ",
                        existing.typ, p.typ
                    ));
                }
                if existing.typ.is_empty() {
                    existing.typ = p.typ.clone();
                }
                if let (Some(ev), Some(pv)) = (&existing.initial_value, &p.initial_value) {
                    if ev != pv {
                        return Err(format!(
                            "[{ERR_INITIAL_VALUE_CONFLICT}] fusing {flat}: initial values {ev:?} and {pv:?} differ"
                        ));
                    }
                }
                if existing.initial_value.is_none() {
                    existing.initial_value = p.initial_value.clone();
                }

                existing.initial += p.initial;
                existing.capacity = min_non_zero(existing.capacity, p.capacity);
                existing.exported = existing.exported || p.exported;
                existing.persisted = existing.persisted || p.persisted;
                existing.resource = existing.resource || p.resource;
                if existing.description.is_empty() {
                    existing.description = p.description.clone();
                }
                continue;
            }
            let mut merged = p.clone();
            merged.id = flat.clone();
            acc.insert(flat.clone(), merged);
            order.push(flat.clone());
            members.insert(flat, vec![format!("{}/{}", s.id, p.id)]);
        }
    }

    let mut out = Vec::with_capacity(order.len());
    for flat in order {
        let mut p = acc.remove(&flat).expect("place present");
        if members[&flat].len() > 1 {
            p.description = describe_fusion(&p.description, "fused", &members[&flat]);
        }
        out.push(p);
    }
    Ok(out)
}

/// Treats 0 as "unbounded", so the tightest real bound wins.
fn min_non_zero(a: i64, b: i64) -> i64 {
    match (a, b) {
        (0, _) => b,
        (_, 0) => a,
        _ => a.min(b),
    }
}

pub(crate) fn describe_fusion(desc: &str, verb: &str, members: &[String]) -> String {
    let note = format!("{verb}: {}", members.join(", "));
    if desc.is_empty() {
        note
    } else {
        format!("{desc} ({note})")
    }
}

/// Unions the subnets' event definitions. Event IDs are NOT namespaced:
/// identical definitions dedupe; a genuine collision is an error.
fn merge_events(subnets: &[&Subnet]) -> Result<Vec<Event>, String> {
    let mut out = Vec::new();
    let mut seen: HashMap<String, Event> = HashMap::new();
    let mut origin: HashMap<String, String> = HashMap::new();

    for s in subnets {
        for e in &s.model.events {
            match seen.get(&e.id) {
                None => {
                    seen.insert(e.id.clone(), e.clone());
                    origin.insert(e.id.clone(), s.id.clone());
                    out.push(e.clone());
                }
                Some(prev) => {
                    if !same_event(prev, e) {
                        return Err(format!(
                            "[{ERR_EVENT_ID_COLLISION}] event {:?} is defined differently in {:?} and {:?}",
                            e.id, origin[&e.id], s.id
                        ));
                    }
                }
            }
        }
    }
    Ok(out)
}

fn same_event(a: &Event, b: &Event) -> bool {
    a.name == b.name && a.description == b.description && a.fields == b.fields
}

/// Stamps an explicit `kind` on every place, matching go-pflow's
/// `NormalizeKinds`: an unset kind is otherwise ambiguous across the
/// ecosystem. `pub` (rather than a private copy per crate) because
/// `pflow-statemachine`, `pflow-workflow` and `pflow-actor`'s `to_meta_model`
/// emitters need the exact same stamp go-pflow's `metamodel.NormalizeKinds`
/// applies before returning a `Model` — see ROADMAP.md ground rule 4's
/// "one home per language" for why this isn't reimplemented per crate.
pub fn normalize_kinds(m: &mut Model) {
    for p in &mut m.places {
        if p.kind.is_none() {
            p.kind = Some(StateKind::Token);
        }
    }
}
