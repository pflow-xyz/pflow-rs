//! Pipeline-shape discovery: given a stream of timestamped `(key, ts)`
//! records emitted by a streaming source, [`discover_pipeline`] infers a
//! plausible pipeline spec — window strategy, window size, key set,
//! recommended trigger. A port of go-pflow's `mining/dataflow_discovery.go`.
//!
//! This is distinct from classical process mining, which discovers
//! transition graphs from case traces; here the "case" is a single pipeline
//! run and the interesting structure is in the per-key timing distribution.
//!
//! The discovery is intentionally heuristic: it picks between fixed and
//! sessions windows by inter-arrival burstiness (P95/P50 ratio), sizes fixed
//! windows so each holds ~`ideal_events_per_window` events, and chooses a
//! session gap large enough to keep the per-key session count bounded.
//! Sliding windows are NOT inferred — they're ambiguous with fixed and
//! require a caller hint.
//!
//! `PipelineSpec`/`WindowSpec`/`TriggerSpec` are minimal, local types
//! carrying just the fields this discovery produces — go-pflow's originals
//! live in `tokenmodel/dataflow`, a streaming-pipeline execution package with
//! no Rust port yet (out of Phase 4's scope, which is eventlog + mining).

use std::collections::HashMap;

use pflow_eventlog::{Event, EventLog};

/// A windowing strategy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowSpec {
    /// `"fixed"`, `"sliding"` or `"sessions"`.
    pub kind: String,
    pub size: i64,
    pub period: i64,
    pub gap: i64,
}

/// An emit trigger. `"any"`/`"all"` are composite forms over `children`.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerSpec {
    pub kind: String,
    pub n: i64,
    pub delay: i64,
    pub children: Vec<TriggerSpec>,
}

impl TriggerSpec {
    fn after_watermark() -> Self {
        Self {
            kind: "after_watermark".to_string(),
            n: 0,
            delay: 0,
            children: vec![],
        }
    }

    fn after_count(n: i64) -> Self {
        Self {
            kind: "after_count".to_string(),
            n,
            delay: 0,
            children: vec![],
        }
    }

    fn any(children: Vec<TriggerSpec>) -> Self {
        Self {
            kind: "any".to_string(),
            n: 0,
            delay: 0,
            children,
        }
    }
}

/// The declarative form of a discovered pipeline.
#[derive(Debug, Clone)]
pub struct PipelineSpec {
    pub context: String,
    pub typ: String,
    pub name: String,
    pub keys: Vec<String>,
    pub window: WindowSpec,
    pub horizon: i64,
    pub trigger: Option<TriggerSpec>,
    pub stage: String,
}

pub const PIPELINE_CONTEXT: &str = "https://pflow.xyz/schema";
pub const PIPELINE_TYPE: &str = "DataflowPipeline";
pub const STAGE_COUNT_PER_KEY: &str = "count_per_key";

/// Options tuning the discovery heuristic. Use [`PipelineDiscoveryOptions::default`]
/// and override just the fields you need.
#[derive(Debug, Clone)]
pub struct PipelineDiscoveryOptions {
    pub name: String,
    pub ideal_events_per_window: i64,
    pub max_sessions_per_key: i64,
    /// Events/unit-time above which early fire is recommended; `0` disables it.
    pub early_fire_rate: f64,
    pub prefer_sessions: bool,
    pub prefer_fixed: bool,
}

impl Default for PipelineDiscoveryOptions {
    fn default() -> Self {
        Self {
            name: "discovered".to_string(),
            ideal_events_per_window: 50,
            max_sessions_per_key: 20,
            early_fire_rate: 0.0,
            prefer_sessions: false,
            prefer_fixed: false,
        }
    }
}

/// The produced spec plus diagnostics.
#[derive(Debug, Clone)]
pub struct PipelineDiscoveryResult {
    pub spec: PipelineSpec,
    pub score: f64,
    pub reasoning: Vec<String>,
    pub stats: PipelineDiscoveryStats,
}

/// Raw measurements of the input stream, surfaced so callers can plug their
/// own picker if they don't trust the heuristic.
#[derive(Debug, Clone, Default)]
pub struct PipelineDiscoveryStats {
    pub num_keys: usize,
    pub num_events: usize,
    pub time_range_min: i64,
    pub time_range_max: i64,
    pub p50_inter_arrival: i64,
    pub p95_inter_arrival: i64,
}

/// Analyses an event log of `(key, ts)` send-style records and returns a
/// spec whose window/trigger would plausibly produce the observed stream.
pub fn discover_pipeline(log: &EventLog, opts: &PipelineDiscoveryOptions) -> Result<PipelineDiscoveryResult, String> {
    if opts.prefer_sessions && opts.prefer_fixed {
        return Err("PreferSessions and PreferFixed are mutually exclusive".to_string());
    }

    let mut per_key: HashMap<String, Vec<i64>> = HashMap::new();
    for trace in log.traces() {
        for ev in &trace.events {
            if let Some((key, ts)) = extract_key_ts(ev) {
                per_key.entry(key).or_default().push(ts);
            }
        }
    }
    if per_key.is_empty() {
        return Err("event log has no usable (key, ts) records".to_string());
    }

    let mut keys: Vec<String> = per_key.keys().cloned().collect();
    keys.sort();
    let mut all_ts: Vec<i64> = Vec::new();
    for ts in per_key.values_mut() {
        ts.sort();
        all_ts.extend(ts.iter());
    }
    all_ts.sort();

    let mut gaps: Vec<i64> = Vec::new();
    for ts in per_key.values() {
        for w in ts.windows(2) {
            let d = w[1] - w[0];
            if d > 0 {
                gaps.push(d);
            }
        }
    }
    gaps.sort();

    let stats = PipelineDiscoveryStats {
        num_keys: keys.len(),
        num_events: all_ts.len(),
        time_range_min: all_ts[0],
        time_range_max: all_ts[all_ts.len() - 1],
        p50_inter_arrival: percentile(&gaps, 0.50),
        p95_inter_arrival: percentile(&gaps, 0.95),
    };

    let mut total_span = stats.time_range_max - stats.time_range_min;
    if total_span <= 0 {
        total_span = 1;
    }

    let burstiness = if stats.p50_inter_arrival > 0 {
        stats.p95_inter_arrival as f64 / stats.p50_inter_arrival as f64
    } else {
        1.0
    };

    let mut reasoning = vec![
        format!(
            "observed {} events across {} keys, time range [{}, {}]",
            stats.num_events, stats.num_keys, stats.time_range_min, stats.time_range_max
        ),
        format!(
            "inter-arrival p50={} p95={} burstiness={:.2}",
            stats.p50_inter_arrival, stats.p95_inter_arrival, burstiness
        ),
    ];

    let use_sessions = if opts.prefer_sessions {
        reasoning.push("PreferSessions=true: forcing sessions window".to_string());
        true
    } else if opts.prefer_fixed {
        reasoning.push("PreferFixed=true: forcing fixed window".to_string());
        false
    } else if burstiness > 5.0 {
        reasoning.push("burstiness > 5: choosing sessions window".to_string());
        true
    } else {
        reasoning.push("burstiness <= 5: choosing fixed window".to_string());
        false
    };

    let (window, horizon, shape_score) = if use_sessions {
        let gap = choose_session_gap(&per_key, &gaps, opts.max_sessions_per_key);
        reasoning.push(format!(
            "sessions gap={gap} (cap {} sessions/key)",
            opts.max_sessions_per_key
        ));
        let score = clamp01((1.0 + burstiness).ln() / 21f64.ln());
        (
            WindowSpec {
                kind: "sessions".to_string(),
                gap,
                ..Default::default()
            },
            stats.time_range_max + gap,
            score,
        )
    } else {
        let size = choose_fixed_size(total_span, stats.num_events as i64, opts.ideal_events_per_window);
        reasoning.push(format!(
            "fixed size={size} (target {} events/window)",
            opts.ideal_events_per_window
        ));
        let score = clamp01(1.0 - (burstiness - 1.0) / 5.0);
        (
            WindowSpec {
                kind: "fixed".to_string(),
                size,
                ..Default::default()
            },
            stats.time_range_max + size,
            score,
        )
    };

    let rate = stats.num_events as f64 / total_span as f64;
    let trigger = if opts.early_fire_rate > 0.0 && rate > opts.early_fire_rate {
        let early_n = (opts.ideal_events_per_window / 2).max(1);
        reasoning.push(format!(
            "event rate {rate:.3}/unit > EarlyFireRate {:.3}: trigger=any(after_count={early_n}, after_watermark)",
            opts.early_fire_rate
        ));
        Some(TriggerSpec::any(vec![
            TriggerSpec::after_count(early_n),
            TriggerSpec::after_watermark(),
        ]))
    } else {
        reasoning.push("trigger=after_watermark (default)".to_string());
        Some(TriggerSpec::after_watermark())
    };

    let consistency = key_consistency(&per_key);
    let score = 0.5 * consistency + 0.5 * shape_score;
    reasoning.push(format!(
        "score={score:.2} (consistency={consistency:.2}, shape={shape_score:.2})"
    ));

    let spec = PipelineSpec {
        context: PIPELINE_CONTEXT.to_string(),
        typ: PIPELINE_TYPE.to_string(),
        name: opts.name.clone(),
        keys,
        window,
        horizon,
        trigger,
        stage: STAGE_COUNT_PER_KEY.to_string(),
    };

    Ok(PipelineDiscoveryResult {
        spec,
        score,
        reasoning,
        stats,
    })
}

/// Extracts `(key, ts)` from an event: primarily from `attributes["key"]`/
/// `attributes["ts"]` (a Pipeline-originated log tags an `"op"` attribute;
/// non-`"send"` ops are control-plane and skipped), falling back to the
/// event's own activity/timestamp for logs that didn't originate from a
/// pipeline.
fn extract_key_ts(ev: &Event) -> Option<(String, i64)> {
    if let Some(op) = ev.attributes.get("op").and_then(|v| v.as_str()) {
        if op != "send" {
            return None;
        }
    }

    let mut key = ev
        .attributes
        .get("key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut ts = ev.attributes.get("ts").and_then(|v| {
        if let Some(i) = v.as_i64() {
            Some(i)
        } else {
            v.as_f64().map(|f| f as i64)
        }
    });

    if key.is_none() {
        if ev.activity.is_empty() {
            return None;
        }
        key = Some(ev.activity.clone());
    }
    if ts.is_none() || ts == Some(0) {
        ts = Some(ev.timestamp);
    }

    key.map(|k| (k, ts.unwrap()))
}

fn percentile(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    if p <= 0.0 {
        return sorted[0];
    }
    if p >= 1.0 {
        return sorted[sorted.len() - 1];
    }
    let idx = ((p * sorted.len() as f64).ceil() as i64 - 1).clamp(0, sorted.len() as i64 - 1) as usize;
    sorted[idx]
}

/// "Nice numbers" fixed window sizes snap to — matches go-pflow's list.
const NICE_SIZES: &[i64] = &[
    1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000, 100000,
];

fn choose_fixed_size(total_span: i64, num_events: i64, ideal: i64) -> i64 {
    let windows = (num_events as f64 / ideal as f64).max(1.0);
    let raw = (total_span as f64 / windows).max(1.0);

    let mut best = NICE_SIZES[0];
    let mut best_dist = (raw.ln() - (best as f64).ln()).abs();
    for &n in &NICE_SIZES[1..] {
        let d = (raw.ln() - (n as f64).ln()).abs();
        if d < best_dist {
            best = n;
            best_dist = d;
        }
    }
    best
}

fn choose_session_gap(per_key: &HashMap<String, Vec<i64>>, gaps: &[i64], max_per_key: i64) -> i64 {
    if gaps.is_empty() {
        return 1;
    }
    let floor = percentile(gaps, 0.50) + 1;
    for &p in &[0.75, 0.90, 0.95, 0.99] {
        let g = percentile(gaps, p);
        if g < floor {
            continue;
        }
        if avg_sessions_per_key(per_key, g) <= max_per_key as f64 {
            return g;
        }
    }
    gaps[gaps.len() - 1]
}

fn avg_sessions_per_key(per_key: &HashMap<String, Vec<i64>>, gap: i64) -> f64 {
    if per_key.is_empty() {
        return 0.0;
    }
    let mut total = 0i64;
    for ts in per_key.values() {
        if ts.is_empty() {
            continue;
        }
        let mut sessions = 1;
        for w in ts.windows(2) {
            if w[1] - w[0] > gap {
                sessions += 1;
            }
        }
        total += sessions;
    }
    total as f64 / per_key.len() as f64
}

fn key_consistency(per_key: &HashMap<String, Vec<i64>>) -> f64 {
    if per_key.is_empty() {
        return 0.0;
    }
    let counts: Vec<f64> = per_key.values().map(|ts| ts.len() as f64).collect();
    let mean = counts.iter().sum::<f64>() / counts.len() as f64;
    if mean == 0.0 {
        return 0.0;
    }
    let variance = counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / counts.len() as f64;
    let cv = variance.sqrt() / mean;
    clamp01(1.0 - cv)
}

fn clamp01(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev_with_key_ts(case: &str, key: &str, ts: i64) -> Event {
        let mut e = Event::new(case, "send", ts);
        e.attributes.insert("op".to_string(), json!("send"));
        e.attributes.insert("key".to_string(), json!(key));
        e.attributes.insert("ts".to_string(), json!(ts));
        e
    }

    fn log_of(events: Vec<Event>) -> EventLog {
        let mut log = EventLog::new();
        for e in events {
            log.add_event(e);
        }
        log
    }

    #[test]
    fn rejects_when_prefer_sessions_and_prefer_fixed_both_set() {
        let log = log_of(vec![ev_with_key_ts("c1", "k1", 1)]);
        let opts = PipelineDiscoveryOptions {
            prefer_sessions: true,
            prefer_fixed: true,
            ..Default::default()
        };
        assert!(discover_pipeline(&log, &opts).is_err());
    }

    #[test]
    fn rejects_a_log_with_no_usable_records() {
        let log = EventLog::new();
        assert!(discover_pipeline(&log, &PipelineDiscoveryOptions::default()).is_err());
    }

    #[test]
    fn regular_stream_chooses_fixed_window() {
        // Steady arrivals every 10ms for one key -> low burstiness.
        let mut events = Vec::new();
        for i in 0..100 {
            events.push(ev_with_key_ts("c1", "k1", i * 10));
        }
        let log = log_of(events);
        let result = discover_pipeline(&log, &PipelineDiscoveryOptions::default()).unwrap();
        assert_eq!(result.spec.window.kind, "fixed");
    }

    #[test]
    fn bursty_stream_chooses_sessions_window() {
        // Ten events at gap=1, then a huge pause, repeated.
        let mut events = Vec::new();
        let mut t = 0i64;
        for burst in 0..5 {
            for _ in 0..10 {
                events.push(ev_with_key_ts("c1", "k1", t));
                t += 1;
            }
            t += 1000 * (burst + 1);
        }
        let log = log_of(events);
        let result = discover_pipeline(&log, &PipelineDiscoveryOptions::default()).unwrap();
        assert_eq!(result.spec.window.kind, "sessions");
    }

    #[test]
    fn prefer_fixed_overrides_the_heuristic() {
        let mut events = Vec::new();
        let mut t = 0i64;
        for _ in 0..10 {
            events.push(ev_with_key_ts("c1", "k1", t));
            t += 1;
        }
        t += 10000;
        events.push(ev_with_key_ts("c1", "k1", t));
        let log = log_of(events);
        let opts = PipelineDiscoveryOptions {
            prefer_fixed: true,
            ..Default::default()
        };
        let result = discover_pipeline(&log, &opts).unwrap();
        assert_eq!(result.spec.window.kind, "fixed");
    }

    #[test]
    fn fallback_extraction_uses_activity_and_timestamp() {
        let mut log = EventLog::new();
        log.add_event(Event::new("c1", "arrive", 0));
        log.add_event(Event::new("c1", "arrive", 100));
        log.add_event(Event::new("c1", "arrive", 200));
        let result = discover_pipeline(&log, &PipelineDiscoveryOptions::default()).unwrap();
        assert_eq!(result.spec.keys, vec!["arrive".to_string()]);
    }

    #[test]
    fn control_plane_events_are_skipped() {
        let mut e = Event::new("c1", "watermark", 5);
        e.attributes.insert("op".to_string(), json!("advance_watermark"));
        let log = log_of(vec![e]);
        assert!(discover_pipeline(&log, &PipelineDiscoveryOptions::default()).is_err());
    }

    #[test]
    fn percentile_edge_cases() {
        assert_eq!(percentile(&[], 0.5), 0);
        assert_eq!(percentile(&[1, 2, 3], 0.0), 1);
        assert_eq!(percentile(&[1, 2, 3], 1.0), 3);
    }
}
