//! Ranking loss: `learn/ranking.go`.

/// One labeled choice point: a score per option and which options a reference labeler
/// prefers. Scores typically come from evaluating a model per option (for a Petri net,
/// one ODE solve per candidate transition firing); the preferred set from an oracle —
/// an exact search, an expert log, a slower method being distilled.
pub struct Decision {
    pub scores: Vec<f64>,
    pub preferred: Vec<bool>,
}

/// Sums, per decision, the worst violation of "some preferred option must outscore
/// every non-preferred option by margin". A decision whose best preferred score clears
/// the best non-preferred score by at least `margin` contributes zero; decisions with
/// no preferred or no non-preferred options contribute zero (there is nothing to
/// rank). The result is a calibration objective for [`crate::fit::fit`]/[`crate::optim`]:
/// parameters that rank every decision correctly with margin to spare reach loss zero.
///
/// The loss can overstate failure: a positive total means some margins are thin, not
/// necessarily that any argmax is wrong. Callers deciding "does the fitted model
/// actually choose correctly" should check the argmax directly.
///
/// Mirrors go-pflow's `learn.HingeRankLoss` exactly, including using
/// `min(len(scores), len(preferred))` when the two vectors disagree in length.
pub fn hinge_rank_loss(decisions: &[Decision], margin: f64) -> f64 {
    let mut loss = 0.0;
    for d in decisions {
        let mut best_pref = f64::NEG_INFINITY;
        let mut best_non = f64::NEG_INFINITY;
        let n = d.scores.len().min(d.preferred.len());
        for i in 0..n {
            if d.preferred[i] {
                if d.scores[i] > best_pref {
                    best_pref = d.scores[i];
                }
            } else if d.scores[i] > best_non {
                best_non = d.scores[i];
            }
        }
        if best_pref == f64::NEG_INFINITY || best_non == f64::NEG_INFINITY {
            continue;
        }
        let v = margin + best_non - best_pref;
        if v > 0.0 {
            loss += v;
        }
    }
    loss
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_loss_when_margin_is_cleared() {
        let decisions = vec![Decision {
            scores: vec![0.1, 0.9, 0.2],
            preferred: vec![false, true, false],
        }];
        // best_pref=0.9, best_non=0.2, margin 0.5 => 0.5 + 0.2 - 0.9 = -0.2 <= 0.
        assert_eq!(hinge_rank_loss(&decisions, 0.5), 0.0);
    }

    #[test]
    fn positive_loss_when_margin_is_violated() {
        let decisions = vec![Decision {
            scores: vec![0.6, 0.5],
            preferred: vec![true, false],
        }];
        // best_pref=0.6, best_non=0.5, margin 0.5 => 0.5 + 0.5 - 0.6 = 0.4.
        let loss = hinge_rank_loss(&decisions, 0.5);
        assert!((loss - 0.4).abs() < 1e-12, "loss={loss}");
    }

    #[test]
    fn decisions_with_no_preferred_or_no_nonpreferred_contribute_zero() {
        let all_preferred = Decision {
            scores: vec![0.1, 0.2],
            preferred: vec![true, true],
        };
        let none_preferred = Decision {
            scores: vec![0.1, 0.2],
            preferred: vec![false, false],
        };
        assert_eq!(hinge_rank_loss(&[all_preferred, none_preferred], 1.0), 0.0);
    }

    #[test]
    fn sums_across_multiple_decisions() {
        let decisions = vec![
            Decision {
                scores: vec![0.6, 0.5],
                preferred: vec![true, false],
            },
            Decision {
                scores: vec![0.3, 0.9],
                preferred: vec![true, false],
            },
        ];
        // First: 0.5+0.5-0.6=0.4. Second: 0.5+0.9-0.3=1.1.
        let loss = hinge_rank_loss(&decisions, 0.5);
        assert!((loss - 1.5).abs() < 1e-12, "loss={loss}");
    }

    #[test]
    fn mismatched_lengths_use_the_shorter() {
        let decisions = vec![Decision {
            scores: vec![0.6, 0.5, 0.9], // extra score, no matching `preferred` entry
            preferred: vec![true, false],
        }];
        // Only the first 2 entries are considered: best_pref=0.6, best_non=0.5.
        let loss = hinge_rank_loss(&decisions, 0.5);
        assert!((loss - 0.4).abs() < 1e-12, "loss={loss}");
    }
}
