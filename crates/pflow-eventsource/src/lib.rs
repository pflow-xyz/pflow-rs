//! In-memory event-sourcing store, ported from go-pflow's `eventsource`
//! package — **the in-memory half only**.
//!
//! ROADMAP.md's *Not ported, on purpose* table is explicit about the other
//! half: go-pflow's SQLite-backed `Store` (`eventsource/sqlite.go`) is
//! persistence belonging to a service, not this library, and stays
//! unported; the memory store ships here in Phase 5. See go-pflow's
//! `eventsource/memory.go` for the reference implementation this mirrors.
//!
//! # What is ported
//!
//! - [`Event`], [`EventFilter`] (`event.go`) — field-for-field, except
//!   `Metadata` (a `map[string]string`) is omitted: nothing in this crate's
//!   own scope (`pflow-monitoring`) produces or reads it, and it is not
//!   part of the `Store` contract itself.
//! - [`Store`] (`store.go`)'s five data-plane methods — `append`, `read`,
//!   `read_all`, `stream_version`, `delete_stream` — plus `close`.
//! - [`MemoryStore`] (`memory.go`): the same optimistic-concurrency append
//!   rule (`expected_version >= 0` must match the current version), the
//!   same "absent stream reads as empty, not an error" contract for `read`,
//!   and the same filter semantics in `read_all` (`from_version`/`to_version`
//!   are 1-indexed inclusive bounds when set, `from_time`/`to_time`,
//!   `types`, `limit`).
//! - [`AdminStore`] (`list_instances`/`get_stats`) — the same "not tracked
//!   in memory" `by_place` gap go-pflow's own comment names.
//!
//! # What is not ported, and why
//!
//! **`Subscribe`, the live channel-based push subscription.** Go's version
//! is a goroutine registered on the store, fed by every future `Append` for
//! the lifetime of a `context.Context`. Nothing in this crate's own
//! consumers (`pflow-monitoring`'s `Monitor`/`Predictor`) needs a push
//! subscription — a monitor calls `record_event` directly rather than
//! listening for one — so this is a documented narrowing rather than a
//! silent drop, the same shape `pflow-compose::queue`'s missing `Payload`
//! binding and `pflow-mining::timing`'s unported `FitRateFunctionsFromLog`
//! stub are recorded in their own crates. A caller that needs to react to
//! appends can poll [`MemoryStore::read`] from `stream_version` forward.
//!
//! **`Snapshot`/`SnapshotStore`.** Go declares the types but nothing in
//! `memory.go` implements the interface; there is no in-memory behaviour to
//! port yet.
//!
//! **UUID-shaped event IDs.** Go's `MemoryStore.Append` calls
//! `uuid.New().String()`. Pulling in a UUID dependency for one field with no
//! cross-language identity contract (unlike the SSA's pinned PRNG) is not
//! worth it; [`MemoryStore`] instead assigns a monotonically increasing
//! store-wide counter formatted as a string, which is unique and stable
//! within one store's lifetime — the only property [`Event::id`] is used
//! for anywhere in this crate or its consumers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A domain event in an event-sourced stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub stream_id: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub version: i64,
    /// Unix-epoch milliseconds. go-pflow uses `time.Time`; this crate has no
    /// clock dependency, so the caller supplies a timestamp
    /// ([`Event::new`] defaults to `0`, meaning "unset" — set it before
    /// appending if ordering-by-time matters).
    pub timestamp_ms: i64,
    /// The event payload, already-parsed JSON (go-pflow's `json.RawMessage`
    /// is bytes-in-bytes-out; this crate is Rust-native throughout, so the
    /// parsed form is more useful and just as lossless).
    pub data: serde_json::Value,
}

impl Event {
    pub fn new(stream_id: impl Into<String>, typ: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            id: String::new(),
            stream_id: stream_id.into(),
            typ: typ.into(),
            version: 0,
            timestamp_ms: 0,
            data,
        }
    }

    /// Deserializes [`Event::data`] into `T`.
    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> serde_json::Result<T> {
        serde_json::from_value(self.data.clone())
    }
}

/// Filters events read via [`Store::read_all`].
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub stream_id: String,
    pub types: Vec<String>,
    pub from_version: i64,
    pub to_version: i64,
    pub from_time_ms: Option<i64>,
    pub to_time_ms: Option<i64>,
    pub limit: usize,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("eventsource: stream not found")]
    StreamNotFound,
    #[error("eventsource: concurrency conflict: expected version mismatch")]
    ConcurrencyConflict,
    #[error("eventsource: event not found")]
    EventNotFound,
    #[error("eventsource: store is closed")]
    StoreClosed,
}

pub type Result<T> = std::result::Result<T, Error>;

/// An aggregate instance summary, for [`AdminStore::list_instances`].
#[derive(Debug, Clone, Default)]
pub struct Instance {
    pub id: String,
    pub version: i64,
    pub state: HashMap<String, i64>,
    pub updated_at_ms: i64,
}

/// Store-wide statistics, for [`AdminStore::get_stats`].
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub total_instances: usize,
    /// Not tracked by [`MemoryStore`] — matches go-pflow's own comment on
    /// `GetStats`: "Not tracked in memory store".
    pub by_place: HashMap<String, i64>,
}

/// The event storage interface. Mirrors go-pflow's `eventsource.Store`,
/// minus `Subscribe` (see the module doc).
pub trait Store {
    /// Appends events to a stream with optimistic concurrency control.
    /// `expected_version` must match the stream's current version, or be
    /// negative for "no check" / a brand-new stream. Returns the new
    /// stream version.
    fn append(&self, stream_id: &str, expected_version: i64, events: Vec<Event>) -> Result<i64>;

    /// Events from `from_version` (inclusive) to the end of the stream. An
    /// unknown stream reads as empty, not an error.
    fn read(&self, stream_id: &str, from_version: i64) -> Result<Vec<Event>>;

    /// All events across every stream matching `filter`.
    fn read_all(&self, filter: &EventFilter) -> Result<Vec<Event>>;

    /// The current version of a stream, or `-1` if it does not exist.
    fn stream_version(&self, stream_id: &str) -> Result<i64>;

    /// Removes all events for a stream.
    fn delete_stream(&self, stream_id: &str) -> Result<()>;

    /// Releases resources; further calls return [`Error::StoreClosed`].
    fn close(&self) -> Result<()>;
}

/// Administrative queries over a [`Store`], for tooling/dashboards.
pub trait AdminStore {
    fn list_instances(
        &self,
        place: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<Instance>, usize)>;

    fn get_stats(&self) -> Result<Stats>;
}

#[derive(Default)]
struct Streams {
    by_id: HashMap<String, Vec<Event>>,
    closed: bool,
}

/// An in-memory event store for testing, development and this crate's own
/// consumers. Thread-safe (`Send + Sync`) via an internal [`Mutex`], the
/// same contract go-pflow's `sync.RWMutex`-guarded `MemoryStore` gives.
pub struct MemoryStore {
    streams: Mutex<Streams>,
    next_id: AtomicU64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            streams: Mutex::new(Streams::default()),
            next_id: AtomicU64::new(1),
        }
    }

    fn matches_filter(event: &Event, filter: &EventFilter) -> bool {
        if filter.from_version > 0 && event.version < filter.from_version {
            return false;
        }
        if filter.to_version > 0 && event.version > filter.to_version {
            return false;
        }
        if let Some(from) = filter.from_time_ms {
            if event.timestamp_ms < from {
                return false;
            }
        }
        if let Some(to) = filter.to_time_ms {
            if event.timestamp_ms > to {
                return false;
            }
        }
        if !filter.types.is_empty() && !filter.types.iter().any(|t| t == &event.typ) {
            return false;
        }
        true
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Store for MemoryStore {
    fn append(&self, stream_id: &str, expected_version: i64, events: Vec<Event>) -> Result<i64> {
        let mut guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }

        let stream = guard.by_id.entry(stream_id.to_string()).or_default();
        let current_version = stream.len() as i64 - 1;

        if expected_version >= 0 && current_version != expected_version {
            return Err(Error::ConcurrencyConflict);
        }

        for (i, mut event) in events.into_iter().enumerate() {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            event.id = id.to_string();
            event.stream_id = stream_id.to_string();
            event.version = current_version + i as i64 + 1;
            stream.push(event);
        }

        Ok(stream.len() as i64 - 1)
    }

    fn read(&self, stream_id: &str, from_version: i64) -> Result<Vec<Event>> {
        let guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }

        let Some(stream) = guard.by_id.get(stream_id) else {
            return Ok(Vec::new());
        };

        let from = from_version.max(0) as usize;
        if from >= stream.len() {
            return Ok(Vec::new());
        }
        Ok(stream[from..].to_vec())
    }

    fn read_all(&self, filter: &EventFilter) -> Result<Vec<Event>> {
        let guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }

        let mut result = Vec::new();
        for (stream_id, stream) in &guard.by_id {
            if !filter.stream_id.is_empty() && filter.stream_id != *stream_id {
                continue;
            }
            for event in stream {
                if Self::matches_filter(event, filter) {
                    result.push(event.clone());
                    if filter.limit > 0 && result.len() >= filter.limit {
                        return Ok(result);
                    }
                }
            }
        }
        Ok(result)
    }

    fn stream_version(&self, stream_id: &str) -> Result<i64> {
        let guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }
        match guard.by_id.get(stream_id) {
            Some(stream) => Ok(stream.len() as i64 - 1),
            None => Ok(-1),
        }
    }

    fn delete_stream(&self, stream_id: &str) -> Result<()> {
        let mut guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }
        guard.by_id.remove(stream_id);
        Ok(())
    }

    fn close(&self) -> Result<()> {
        let mut guard = self.streams.lock().unwrap();
        guard.closed = true;
        Ok(())
    }
}

impl AdminStore for MemoryStore {
    fn list_instances(
        &self,
        place: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<Instance>, usize)> {
        let guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }

        let mut instances: Vec<Instance> = guard
            .by_id
            .iter()
            .filter_map(|(id, events)| {
                if events.is_empty() {
                    return None;
                }
                // Best-effort state reconstruction: fold every event whose
                // data carries a top-level "state" object, matching go-pflow's
                // ListInstances.
                let mut state: HashMap<String, i64> = HashMap::new();
                for e in events {
                    if let Some(obj) = e.data.get("state").and_then(|v| v.as_object()) {
                        for (k, v) in obj {
                            if let Some(n) = v.as_i64() {
                                state.insert(k.clone(), n);
                            } else if let Some(f) = v.as_f64() {
                                state.insert(k.clone(), f as i64);
                            }
                        }
                    }
                }
                if !place.is_empty() {
                    match state.get(place) {
                        Some(n) if *n > 0 => {}
                        _ => return None,
                    }
                }
                Some(Instance {
                    id: id.clone(),
                    version: events.len() as i64 - 1,
                    state,
                    updated_at_ms: events.last().map(|e| e.timestamp_ms).unwrap_or(0),
                })
            })
            .collect();

        instances.sort_by(|a, b| a.id.cmp(&b.id));
        let total = instances.len();

        let page = page.max(1);
        let per_page = if per_page == 0 { 50 } else { per_page };
        let start = (page - 1) * per_page;
        if start >= instances.len() {
            return Ok((Vec::new(), total));
        }
        let end = (start + per_page).min(instances.len());
        Ok((instances[start..end].to_vec(), total))
    }

    fn get_stats(&self) -> Result<Stats> {
        let guard = self.streams.lock().unwrap();
        if guard.closed {
            return Err(Error::StoreClosed);
        }
        Ok(Stats {
            total_instances: guard.by_id.len(),
            by_place: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(typ: &str) -> Event {
        Event::new("s1", typ, serde_json::json!({}))
    }

    #[test]
    fn append_assigns_sequential_versions() {
        let store = MemoryStore::new();
        let v = store.append("s1", -1, vec![ev("a"), ev("b")]).unwrap();
        assert_eq!(v, 1);
        let events = store.read("s1", 0).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].version, 0);
        assert_eq!(events[1].version, 1);
    }

    #[test]
    fn append_rejects_a_stale_expected_version() {
        let store = MemoryStore::new();
        store.append("s1", -1, vec![ev("a")]).unwrap(); // version 0
        store.append("s1", -1, vec![ev("b")]).unwrap(); // version 1 (negative = no check)

        // Stream is at version 1; claiming 0 is stale.
        let err = store.append("s1", 0, vec![ev("c")]).unwrap_err();
        assert_eq!(err, Error::ConcurrencyConflict);

        // The correct expected version succeeds.
        store.append("s1", 1, vec![ev("c")]).unwrap();
        assert_eq!(store.stream_version("s1").unwrap(), 2);
    }

    #[test]
    fn read_on_an_unknown_stream_is_empty_not_an_error() {
        let store = MemoryStore::new();
        assert_eq!(store.read("nope", 0).unwrap(), Vec::new());
        assert_eq!(store.stream_version("nope").unwrap(), -1);
    }

    #[test]
    fn read_all_filters_by_type_and_stream() {
        let store = MemoryStore::new();
        store.append("s1", -1, vec![ev("a"), ev("b")]).unwrap();
        store.append("s2", -1, vec![ev("a")]).unwrap();

        let filtered = store
            .read_all(&EventFilter {
                stream_id: "s1".into(),
                types: vec!["b".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].typ, "b");
    }

    #[test]
    fn delete_stream_clears_it() {
        let store = MemoryStore::new();
        store.append("s1", -1, vec![ev("a")]).unwrap();
        store.delete_stream("s1").unwrap();
        assert_eq!(store.stream_version("s1").unwrap(), -1);
    }

    #[test]
    fn closed_store_refuses_every_call() {
        let store = MemoryStore::new();
        store.close().unwrap();
        assert_eq!(store.append("s1", -1, vec![ev("a")]), Err(Error::StoreClosed));
        assert_eq!(store.read("s1", 0), Err(Error::StoreClosed));
    }

    #[test]
    fn list_instances_paginates_and_filters_by_place() {
        let store = MemoryStore::new();
        store
            .append(
                "a",
                -1,
                vec![Event::new("a", "t", serde_json::json!({"state": {"p": 1}}))],
            )
            .unwrap();
        store
            .append(
                "b",
                -1,
                vec![Event::new("b", "t", serde_json::json!({"state": {"p": 0}}))],
            )
            .unwrap();

        let (instances, total) = store.list_instances("p", 1, 50).unwrap();
        assert_eq!(total, 1, "only stream a has p > 0");
        assert_eq!(instances[0].id, "a");
    }
}
