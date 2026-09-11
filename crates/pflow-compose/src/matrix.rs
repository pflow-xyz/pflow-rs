//! The net-type x link-kind legality matrix, ported from go-pflow's
//! `metamodel/compose_matrix.go`.
//!
//! This is what makes composition *typed* rather than ad hoc: a workflow
//! cursor cannot be accidentally linked to an inventory counter.
//! [`NetType::Untyped`] is legal with everything, so models written before
//! net types existed keep composing; `Validate` warns rather than failing.

use crate::bundle::{LinkKind, NetType};

/// Reports whether a link of the given kind may connect the two net types,
/// and if not, why.
pub fn link_legal(kind: LinkKind, from: NetType, to: NetType) -> (bool, String) {
    if from == NetType::Untyped || to == NetType::Untyped {
        return (true, String::new());
    }

    match kind {
        LinkKind::Token => {
            if !counts_tokens(from) {
                return (
                    false,
                    format!(
                        "a token link cannot originate in a {from} — its places are not fungible counters (use an event link to synchronise firing, or a guard link to gate on state)"
                    ),
                );
            }
            if !counts_tokens(to) {
                return (
                    false,
                    format!(
                        "a token link cannot terminate in a {to} — its places are not fungible counters (use an event link to synchronise firing, or a guard link to gate on state)"
                    ),
                );
            }
            (true, String::new())
        }
        LinkKind::Data => (true, String::new()),
        LinkKind::Event => {
            if from == NetType::Computation || to == NetType::Computation {
                return (
                    false,
                    format!(
                        "an event link cannot involve a {} — its transitions fire continuously under rates, so there is no discrete instant to synchronise (use a data link to observe its state)",
                        NetType::Computation
                    ),
                );
            }
            (true, String::new())
        }
        LinkKind::Guard => (true, String::new()),
    }
}

/// Reports whether a net type's places hold fungible, transferable tokens.
fn counts_tokens(t: NetType) -> bool {
    matches!(t, NetType::Resource | NetType::Game)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untyped_is_legal_with_everything() {
        let (ok, _) = link_legal(LinkKind::Token, NetType::Untyped, NetType::Workflow);
        assert!(ok);
    }

    #[test]
    fn token_link_rejects_workflow_net() {
        let (ok, why) = link_legal(LinkKind::Token, NetType::Workflow, NetType::Resource);
        assert!(!ok);
        assert!(why.contains("WorkflowNet"), "{why}");
    }

    #[test]
    fn event_link_rejects_computation_net() {
        let (ok, why) = link_legal(LinkKind::Event, NetType::Computation, NetType::Resource);
        assert!(!ok);
        assert!(why.contains("ComputationNet"), "{why}");
    }

    #[test]
    fn data_and_guard_links_are_universally_legal() {
        assert!(link_legal(LinkKind::Data, NetType::Workflow, NetType::Computation).0);
        assert!(link_legal(LinkKind::Guard, NetType::Computation, NetType::Workflow).0);
    }
}
