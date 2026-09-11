//! `EventLink` transition fusion, ported from go-pflow's
//! `metamodel/compose_fuse.go`.
//!
//! "When one fires, the other fires too" (ch04). The transitions in an
//! EventLink class become one transition whose pre/postset is the union of
//! theirs and whose guard is their conjunction — a rendezvous, enabled only
//! when every participant is.
//!
//! Fusion is computed over equivalence classes, not pairwise, which is what
//! makes it associative.

use std::collections::HashMap;

use pflow_metamodel::{Binding, Transition, TransitionField};

use crate::bundle::{Bundle, FlattenMap, LinkKind, Subnet, ValidationError, WARN_ROUTE_DROPPED};
use crate::exprrewrite::rewrite_place_refs;
use crate::guard::and_guards;
use crate::validate::resolve;

/// One transition inside an EventLink equivalence class, kept with its
/// owning subnet so guards and bindings can be rewritten in context.
struct FusionMember<'a> {
    subnet: &'a Subnet,
    trans: &'a Transition,
}

pub(crate) fn subnet_place_alias(s: &Subnet, place_flat: &HashMap<String, String>) -> HashMap<String, String> {
    s.model
        .places
        .iter()
        .map(|p| {
            let key = format!("{}/{}", s.id, p.id);
            (p.id.clone(), place_flat.get(&key).cloned().unwrap_or_default())
        })
        .collect()
}

/// Builds one `Transition` per equivalence class.
pub(crate) fn fuse_transitions(
    b: &Bundle,
    subnets: &[&Subnet],
    groups: &std::collections::BTreeMap<String, Vec<String>>,
    trans_flat: &HashMap<String, String>,
    place_flat: &HashMap<String, String>,
    fm: &mut FlattenMap,
) -> Result<Vec<Transition>, String> {
    let mut index: HashMap<String, FusionMember> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut emitted = std::collections::HashSet::new();

    for s in subnets {
        for t in &s.model.transitions {
            let key = format!("{}/{}", s.id, t.id);
            index.insert(key.clone(), FusionMember { subnet: s, trans: t });

            let flat = trans_flat.get(&key).cloned().unwrap_or_default();
            if emitted.insert(flat.clone()) {
                order.push(flat);
            }
        }
    }

    let mut class_of: HashMap<String, Vec<String>> = HashMap::new();
    for members in groups.values() {
        let flat = trans_flat.get(&members[0]).cloned().unwrap_or_default();
        class_of.insert(flat, members.clone());
    }

    let mut out = Vec::with_capacity(order.len());
    for flat in &order {
        let members = class_of.get(flat).cloned().unwrap_or_default();
        if members.len() == 1 {
            let m = index.get(&members[0]).expect("member indexed");
            let mut t = m.trans.clone();
            t.id = flat.clone();
            t.guard = rewrite_place_refs(&t.guard, &subnet_place_alias(m.subnet, place_flat), &b.prefix(&m.subnet.id));
            out.push(t);
            continue;
        }

        let fused = fuse_class(b, flat, &members, &index, place_flat, fm)?;
        out.push(fused);
    }
    Ok(out)
}

fn fuse_class(
    b: &Bundle,
    flat: &str,
    members: &[String],
    index: &HashMap<String, FusionMember>,
    place_flat: &HashMap<String, String>,
    fm: &mut FlattenMap,
) -> Result<Transition, String> {
    let mut sorted = members.to_vec();
    sorted.sort();

    let mut fused = Transition {
        id: flat.to_string(),
        ..Default::default()
    };

    let mut guards: Vec<String> = Vec::new();
    let mut descriptions: Vec<String> = Vec::new();
    let mut emits: Vec<String> = Vec::new();
    let mut bindings: Vec<Binding> = Vec::new();
    let mut binding_index: HashMap<String, usize> = HashMap::new();
    let mut fields: Vec<TransitionField> = Vec::new();
    let mut field_index: HashMap<String, usize> = HashMap::new();
    let mut rate = 0.0;
    let mut have_rate = false;

    let initiator = initiator_of(b, &sorted);

    for key in &sorted {
        let m = index.get(key).ok_or_else(|| format!("internal: fused member {key:?} not found"))?;
        let t = m.trans;
        let alias = subnet_place_alias(m.subnet, place_flat);
        let rename = renames_for(b, key);

        let g = rewrite_place_refs(&t.guard, &alias, &b.prefix(&m.subnet.id));
        if !g.is_empty() {
            guards.push(g);
        }
        if !t.description.is_empty() {
            descriptions.push(t.description.clone());
        }

        // Each component keeps emitting its own event.
        let evt = component_event(t);
        if !evt.is_empty() {
            emits.push(evt);
        }

        merge_bindings(&mut bindings, &mut binding_index, &t.bindings, &rename, &alias, key)?;
        merge_transition_fields(&mut fields, &mut field_index, &t.fields, key)?;

        // The initiator owns the HTTP route.
        if key == &initiator {
            fused.http_method = t.http_method.clone();
            fused.http_path = t.http_path.clone();
            fused.event = t.event.clone();
        } else if !t.http_path.is_empty() {
            fm.warnings.push(ValidationError::new(
                WARN_ROUTE_DROPPED,
                format!(
                    "{} {} from {key} is dropped: it fused into {flat}, whose route comes from the initiator {initiator}",
                    t.http_method, t.http_path
                ),
                flat,
            ));
        }

        if t.rate != 0.0 && (!have_rate || t.rate < rate) {
            rate = t.rate;
            have_rate = true;
        }
        fused.clears_history = fused.clears_history || t.clears_history;
        fused.guard_unrepresentable = fused.guard_unrepresentable || t.guard_unrepresentable;
    }

    let dur = merge_durations(&sorted, index, flat)?;
    fused.duration = dur.duration;
    fused.min_duration = dur.min;
    fused.max_duration = dur.max;

    fused.guard = and_guards(&guards);
    fused.bindings = bindings;
    fused.fields = fields;
    fused.emits = emits.clone();
    fused.rate = rate;
    fused.description = crate::flatten::describe_fusion(&descriptions.join(" \u{2297} "), "fires with", &sorted);

    if !emits.is_empty() {
        fm.member_events.insert(flat.to_string(), emits);
    }
    Ok(fused)
}

fn component_event(t: &Transition) -> String {
    if !t.event.is_empty() {
        t.event.clone()
    } else if !t.event_type.is_empty() {
        t.event_type.clone()
    } else {
        t.id.clone()
    }
}

/// Picks the class member that owns the fused HTTP route: the unique member
/// with no incoming EventLink. On a cycle, or with several sources, the
/// canonically smallest member wins so the result stays deterministic.
fn initiator_of(b: &Bundle, members: &[String]) -> String {
    let in_class: std::collections::HashSet<&str> = members.iter().map(|s| s.as_str()).collect();
    let mut has_incoming = std::collections::HashSet::new();

    for l in &b.links {
        if l.kind != LinkKind::Event {
            continue;
        }
        if let Ok(to) = resolve(b, &l.to) {
            let key = format!("{}/{}", to.subnet.id, to.transition);
            if in_class.contains(key.as_str()) {
                has_incoming.insert(key);
            }
        }
    }

    for m in members {
        if !has_incoming.contains(m) {
            return m.clone();
        }
    }
    members[0].clone()
}

/// Collects the binding renames that apply to one class member, from every
/// EventLink whose `from` side is that member.
fn renames_for(b: &Bundle, member_key: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for l in &b.links {
        if l.kind != LinkKind::Event || l.rename.is_empty() {
            continue;
        }
        if let Ok(from) = resolve(b, &l.from) {
            let key = format!("{}/{}", from.subnet.id, from.transition);
            if key != member_key {
                continue;
            }
            for (k, v) in &l.rename {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

/// Unifies bindings by name: same name means same variable (CPN variable
/// unification, restricted to this schema's flat binding model).
fn merge_bindings(
    out: &mut Vec<Binding>,
    index: &mut HashMap<String, usize>,
    inp: &[Binding],
    rename: &HashMap<String, String>,
    alias: &HashMap<String, String>,
    member: &str,
) -> Result<(), String> {
    for bnd in inp {
        let mut b = bnd.clone();
        if let Some(renamed) = rename.get(&b.name) {
            b.name = renamed.clone();
        }
        if !b.place.is_empty() {
            if let Some(flat) = alias.get(&b.place) {
                b.place = flat.clone();
            }
        }

        match index.get(&b.name) {
            None => {
                index.insert(b.name.clone(), out.len());
                out.push(b);
            }
            Some(&pos) => {
                let prev = out[pos].clone();
                if prev.typ != b.typ {
                    return Err(format!(
                        "binding {:?} is {:?} in one fused transition and {:?} in {member}",
                        b.name, prev.typ, b.typ
                    ));
                }
                if prev.value != b.value {
                    return Err(format!(
                        "binding {:?} is the transfer value in one fused transition but not in {member}",
                        b.name
                    ));
                }
                if prev.keys != b.keys {
                    return Err(format!(
                        "binding {:?} has keys {:?} in one fused transition and {:?} in {member}",
                        b.name, prev.keys, b.keys
                    ));
                }
                if prev.place != b.place {
                    return Err(format!(
                        "binding {:?} reads place {:?} in one fused transition and {:?} in {member}; fuse those places with a data link, or rename one binding on the event link",
                        b.name, prev.place, b.place
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Unions input form fields by name, so the fused action presents one form.
fn merge_transition_fields(
    out: &mut Vec<TransitionField>,
    index: &mut HashMap<String, usize>,
    inp: &[TransitionField],
    member: &str,
) -> Result<(), String> {
    for f in inp {
        match index.get(&f.name) {
            None => {
                index.insert(f.name.clone(), out.len());
                out.push(f.clone());
            }
            Some(&pos) => {
                let prev = &out[pos];
                if prev.typ != f.typ || prev.required != f.required {
                    return Err(format!(
                        "input field {:?} is declared differently in {member} (type {:?}/{:?}, required {}/{})",
                        f.name, prev.typ, f.typ, prev.required, f.required
                    ));
                }
            }
        }
    }
    Ok(())
}

struct Durations {
    duration: String,
    min: String,
    max: String,
}

/// Combines SLA timings across a fused class: the slowest expected
/// duration, the tightest minimum and the tightest maximum.
fn merge_durations(members: &[String], index: &HashMap<String, FusionMember>, flat: &str) -> Result<Durations, String> {
    let mut d = Durations {
        duration: String::new(),
        min: String::new(),
        max: String::new(),
    };
    let mut min_source = String::new();
    let mut max_source = String::new();

    for key in members {
        let t = index[key].trans;
        if d.duration.is_empty() {
            d.duration = t.duration.clone();
        }
        if !t.min_duration.is_empty() {
            if d.min.is_empty() {
                d.min = t.min_duration.clone();
                min_source = key.clone();
            } else if d.min != t.min_duration {
                return Err(format!(
                    "fused transition {flat} has incompatible minimum durations: {:?} from {min_source} and {:?} from {key}",
                    d.min, t.min_duration
                ));
            }
        }
        if !t.max_duration.is_empty() {
            if d.max.is_empty() {
                d.max = t.max_duration.clone();
                max_source = key.clone();
            } else if d.max != t.max_duration {
                return Err(format!(
                    "fused transition {flat} has incompatible maximum durations: {:?} from {max_source} and {:?} from {key}",
                    d.max, t.max_duration
                ));
            }
        }
    }
    Ok(d)
}
