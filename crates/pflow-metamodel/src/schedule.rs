//! Piecewise-constant transition rates. Ported from go-pflow's
//! `metamodel/schedule.go`.

use serde::{Deserialize, Serialize};

use crate::schema::{Model, Transition};

/// One piece of a piecewise-constant rate: `value` firings per unit time
/// until model time `until`. The last segment of a schedule holds to
/// whatever horizon a run uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RateSegment {
    pub until: f64,
    pub value: f64,
}

impl Transition {
    /// The declared rate at model time `at`: the first segment whose
    /// `until` exceeds `at`, or the last segment's value past the end.
    /// `None` when the transition declares no schedule.
    pub fn scheduled_rate(&self, at: f64) -> Option<f64> {
        if self.schedule.is_empty() {
            return None;
        }
        for seg in &self.schedule {
            if at < seg.until {
                return Some(seg.value);
            }
        }
        self.schedule.last().map(|s| s.value)
    }

    /// The duration-weighted mean of the declared segments — the one number
    /// to display where a single rate is expected. `None` when the
    /// transition declares no schedule.
    pub fn schedule_average(&self) -> Option<f64> {
        if self.schedule.is_empty() {
            return None;
        }
        let mut sum = 0.0;
        let mut span = 0.0;
        let mut from = 0.0;
        for seg in &self.schedule {
            sum += seg.value * (seg.until - from);
            span += seg.until - from;
            from = seg.until;
        }
        if span <= 0.0 {
            return Some(self.schedule[0].value);
        }
        Some(sum / span)
    }
}

impl Model {
    /// Whether any transition declares a schedule.
    pub fn has_schedules(&self) -> bool {
        self.transitions.iter().any(|t| !t.schedule.is_empty())
    }

    /// Every defect in the declared schedules: a negative rate, a segment
    /// boundary that does not advance, or a first boundary at or below
    /// zero.
    pub fn validate_schedules(&self) -> Vec<String> {
        let mut errs = Vec::new();
        for t in &self.transitions {
            if t.schedule.is_empty() {
                continue;
            }
            let mut prev = 0.0;
            for (j, seg) in t.schedule.iter().enumerate() {
                if seg.value < 0.0 {
                    errs.push(format!(
                        "schedule for {:?}: segment {j} has a negative rate",
                        t.id
                    ));
                }
                if seg.until <= prev {
                    errs.push(format!(
                        "schedule for {:?}: segment {j} ends at {}, not after the previous segment's {}",
                        t.id, seg.until, prev
                    ));
                }
                prev = seg.until;
            }
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Model;

    fn scheduled(segments: Vec<RateSegment>) -> Transition {
        Transition {
            id: "t".into(),
            schedule: segments,
            ..Default::default()
        }
    }

    #[test]
    fn no_schedule_means_none() {
        let t = Transition {
            id: "t".into(),
            ..Default::default()
        };
        assert_eq!(t.scheduled_rate(5.0), None);
        assert_eq!(t.schedule_average(), None);
    }

    #[test]
    fn scheduled_rate_picks_the_first_segment_that_has_not_ended() {
        let t = scheduled(vec![
            RateSegment {
                until: 4.0,
                value: 2.0,
            },
            RateSegment {
                until: 8.0,
                value: 5.0,
            },
        ]);
        assert_eq!(t.scheduled_rate(0.0), Some(2.0));
        assert_eq!(t.scheduled_rate(3.9), Some(2.0));
        assert_eq!(t.scheduled_rate(4.0), Some(5.0));
        // Past the end, the last segment holds.
        assert_eq!(t.scheduled_rate(100.0), Some(5.0));
    }

    #[test]
    fn schedule_average_is_duration_weighted() {
        let t = scheduled(vec![
            RateSegment {
                until: 1.0,
                value: 0.0,
            },
            RateSegment {
                until: 4.0,
                value: 6.0,
            },
        ]);
        // span [0,1) at 0, span [1,4) at 6: mean = (0*1 + 6*3) / 4 = 4.5
        assert_eq!(t.schedule_average(), Some(4.5));
    }

    #[test]
    fn has_schedules_reports_any_transition() {
        let mut m = Model {
            name: "m".into(),
            transitions: vec![
                Transition {
                    id: "a".into(),
                    ..Default::default()
                },
                scheduled(vec![RateSegment {
                    until: 1.0,
                    value: 1.0,
                }]),
            ],
            ..Default::default()
        };
        m.transitions[1].id = "b".into();
        assert!(m.has_schedules());

        let none = Model {
            name: "m".into(),
            transitions: vec![Transition {
                id: "a".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!none.has_schedules());
    }

    #[test]
    fn validate_schedules_flags_non_advancing_and_negative_segments() {
        let m = Model {
            name: "m".into(),
            transitions: vec![Transition {
                id: "t".into(),
                schedule: vec![
                    RateSegment {
                        until: 4.0,
                        value: -1.0,
                    },
                    RateSegment {
                        until: 4.0,
                        value: 1.0,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let errs = m.validate_schedules();
        assert!(errs.iter().any(|e| e.contains("negative rate")), "{errs:?}");
        assert!(
            errs.iter()
                .any(|e| e.contains("not after the previous segment")),
            "{errs:?}"
        );
    }
}
