//! In-process message bus, ported from go-pflow's `actor/bus.go`.
//!
//! go-pflow's `Bus` is goroutine-backed: `Publish` enqueues onto a buffered
//! channel a background `processLoop` drains, and `PublishSync` dispatches
//! inline. This port is single-threaded (a library should not spawn
//! background workers on a caller's behalf — the same call this crate's
//! `pflow-workflow::monitor` module doc makes about go-pflow's ticker
//! loop): [`Bus::publish`] enqueues onto an internal `Vec`, and
//! [`Bus::drain`] (or [`Bus::publish_sync`], which dispatches immediately)
//! is what actually runs handlers. Middleware also changes shape: Go's
//! `func(*Signal, next func(*Signal))` continuation lets a middleware call
//! `next` zero, one or many times; this port's [`Middleware`] is a plain
//! `Fn(&Signal) -> Option<Signal>` applied once per signal in sequence
//! (`None` drops it, `Some` may transform it) — covers logging, filtering
//! and transforming, the three concrete middlewares go-pflow itself ships,
//! at the cost of the fan-out case none of them use.
//!
//! Because handlers need mutable access to the target [`Actor`]'s state
//! (`ActorContext::state`), and a `Bus` does not own the actors in this
//! port (an [`crate::ActorSystem`] does — see its module doc), dispatch
//! takes the actor map as a parameter rather than reaching into a `self`
//! field, the same adaptation `pflow-workflow::engine::Engine` makes for
//! its handler `&Case` borrows.

use std::collections::{HashMap, VecDeque};

use crate::types::{Actor, ActorContext, Signal, SignalHandler, SignalPredicate, Subscription};

pub type Middleware = Box<dyn Fn(&Signal) -> Option<Signal>>;

#[derive(Debug, Default, Clone, Copy)]
pub struct BusStats {
    pub signal_count: u64,
    pub error_count: u64,
    pub subscription_count: usize,
    pub queue_size: usize,
}

/// A message bus for actor communication.
#[derive(Default)]
pub struct Bus {
    name: String,
    subscriptions: HashMap<String, Vec<Subscription>>,
    middleware: Vec<Middleware>,
    queue: VecDeque<Signal>,
    signal_count: u64,
    error_count: u64,
}

impl Bus {
    pub fn new(name: impl Into<String>) -> Self {
        Bus {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Registers `actor_id`'s interest in `signal_type`.
    pub fn subscribe(&mut self, actor_id: impl Into<String>, signal_type: impl Into<String>, handler: SignalHandler) {
        self.subscribe_with(actor_id, signal_type, handler, None, 0);
    }

    pub fn subscribe_with_filter(
        &mut self,
        actor_id: impl Into<String>,
        signal_type: impl Into<String>,
        handler: SignalHandler,
        filter: SignalPredicate,
    ) {
        self.subscribe_with(actor_id, signal_type, handler, Some(filter), 0);
    }

    /// Subscribes with a priority (higher fires first).
    pub fn subscribe_with_priority(
        &mut self,
        actor_id: impl Into<String>,
        signal_type: impl Into<String>,
        handler: SignalHandler,
        priority: i32,
    ) {
        self.subscribe_with(actor_id, signal_type, handler, None, priority);
    }

    fn subscribe_with(
        &mut self,
        actor_id: impl Into<String>,
        signal_type: impl Into<String>,
        handler: SignalHandler,
        filter: Option<SignalPredicate>,
        priority: i32,
    ) {
        let signal_type = signal_type.into();
        let subs = self.subscriptions.entry(signal_type.clone()).or_default();
        subs.push(Subscription {
            actor_id: actor_id.into(),
            signal_type,
            priority,
            handler,
            filter,
        });
        subs.sort_by_key(|s| std::cmp::Reverse(s.priority));
    }

    pub fn unsubscribe(&mut self, actor_id: &str, signal_type: &str) {
        if let Some(subs) = self.subscriptions.get_mut(signal_type) {
            subs.retain(|s| s.actor_id != actor_id);
        }
    }

    pub fn unsubscribe_all(&mut self, actor_id: &str) {
        for subs in self.subscriptions.values_mut() {
            subs.retain(|s| s.actor_id != actor_id);
        }
    }

    pub fn use_middleware(&mut self, mw: Middleware) {
        self.middleware.push(mw);
    }

    /// Enqueues a signal for later dispatch via [`Bus::drain`].
    pub fn publish(&mut self, signal: Signal) {
        self.queue.push_back(signal);
    }

    /// Dispatches a signal to matching subscribers immediately, against
    /// `actors` (see the module doc for why the bus does not own them).
    /// Returns the last handler error, if any.
    pub fn publish_sync(&mut self, actors: &mut HashMap<String, Actor>, signal: Signal) -> Option<String> {
        let signal = self.apply_middleware(signal)?;
        self.signal_count += 1;
        self.dispatch(actors, &signal)
    }

    /// Runs every queued signal through [`Bus::publish_sync`], in FIFO
    /// order.
    pub fn drain(&mut self, actors: &mut HashMap<String, Actor>) {
        while let Some(signal) = self.queue.pop_front() {
            let Some(signal) = self.apply_middleware(signal) else {
                continue;
            };
            self.signal_count += 1;
            self.dispatch(actors, &signal);
        }
    }

    fn apply_middleware(&self, mut signal: Signal) -> Option<Signal> {
        for mw in &self.middleware {
            match mw(&signal) {
                Some(s) => signal = s,
                None => return None,
            }
        }
        Some(signal)
    }

    fn dispatch(&mut self, actors: &mut HashMap<String, Actor>, signal: &Signal) -> Option<String> {
        let subs = self.subscriptions.get_mut(&signal.typ)?;
        let mut last_err = None;
        for sub in subs.iter_mut() {
            if !signal.target.is_empty() && signal.target != sub.actor_id {
                continue;
            }
            if let Some(filter) = &sub.filter {
                if !filter(signal) {
                    continue;
                }
            }
            let Some(actor) = actors.get_mut(&sub.actor_id) else {
                continue;
            };
            let mut ctx = ActorContext {
                actor_id: &sub.actor_id,
                signal,
                state: &mut actor.state,
            };
            if let Err(e) = (sub.handler)(&mut ctx, signal) {
                self.error_count += 1;
                last_err = Some(e);
            }
        }
        last_err
    }

    pub fn stats(&self) -> BusStats {
        BusStats {
            signal_count: self.signal_count,
            error_count: self.error_count,
            subscription_count: self.subscriptions.values().map(|v| v.len()).sum(),
            queue_size: self.queue.len(),
        }
    }

    pub(crate) fn subscriptions(&self) -> &HashMap<String, Vec<Subscription>> {
        &self.subscriptions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn actor(id: &str) -> Actor {
        Actor {
            id: id.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn dispatches_to_matching_subscriber() {
        let mut bus = Bus::new("b");
        let mut actors = HashMap::new();
        actors.insert("worker".to_string(), actor("worker"));

        let received = Rc::new(RefCell::new(Vec::new()));
        let received2 = received.clone();
        bus.subscribe(
            "worker",
            "task",
            Box::new(move |_ctx, sig| {
                received2.borrow_mut().push(sig.id.clone());
                Ok(())
            }),
        );

        bus.publish_sync(
            &mut actors,
            Signal {
                id: "s1".into(),
                typ: "task".into(),
                ..Default::default()
            },
        );

        assert_eq!(*received.borrow(), vec!["s1".to_string()]);
    }

    #[test]
    fn target_filters_out_other_actors() {
        let mut bus = Bus::new("b");
        let mut actors = HashMap::new();
        actors.insert("a".to_string(), actor("a"));
        actors.insert("b".to_string(), actor("b"));

        let hits = Rc::new(RefCell::new(0));
        for id in ["a", "b"] {
            let hits2 = hits.clone();
            bus.subscribe(id, "ping", Box::new(move |_c, _s| { *hits2.borrow_mut() += 1; Ok(()) }));
        }

        bus.publish_sync(&mut actors, Signal { typ: "ping".into(), target: "a".into(), ..Default::default() });
        assert_eq!(*hits.borrow(), 1);
    }

    #[test]
    fn priority_orders_handlers() {
        let mut bus = Bus::new("b");
        let mut actors = HashMap::new();
        actors.insert("a".to_string(), actor("a"));
        actors.insert("b".to_string(), actor("b"));

        let order = Rc::new(RefCell::new(Vec::new()));
        let o1 = order.clone();
        bus.subscribe_with_priority("a", "x", Box::new(move |_c, _s| { o1.borrow_mut().push("a"); Ok(()) }), 1);
        let o2 = order.clone();
        bus.subscribe_with_priority("b", "x", Box::new(move |_c, _s| { o2.borrow_mut().push("b"); Ok(()) }), 5);

        bus.publish_sync(&mut actors, Signal { typ: "x".into(), ..Default::default() });
        assert_eq!(*order.borrow(), vec!["b", "a"]);
    }

    #[test]
    fn middleware_can_drop_a_signal() {
        let mut bus = Bus::new("b");
        bus.use_middleware(Box::new(|s| if s.typ == "blocked" { None } else { Some(s.clone()) }));
        let mut actors = HashMap::new();
        actors.insert("a".to_string(), actor("a"));
        let hits = Rc::new(RefCell::new(0));
        let h = hits.clone();
        bus.subscribe("a", "blocked", Box::new(move |_c, _s| { *h.borrow_mut() += 1; Ok(()) }));

        bus.publish_sync(&mut actors, Signal { typ: "blocked".into(), ..Default::default() });
        assert_eq!(*hits.borrow(), 0);
    }

    #[test]
    fn publish_then_drain_runs_queued_signals() {
        let mut bus = Bus::new("b");
        let mut actors = HashMap::new();
        actors.insert("a".to_string(), actor("a"));
        let hits = Rc::new(RefCell::new(0));
        let h = hits.clone();
        bus.subscribe("a", "x", Box::new(move |_c, _s| { *h.borrow_mut() += 1; Ok(()) }));

        bus.publish(Signal { typ: "x".into(), ..Default::default() });
        bus.publish(Signal { typ: "x".into(), ..Default::default() });
        assert_eq!(bus.stats().queue_size, 2);

        bus.drain(&mut actors);
        assert_eq!(*hits.borrow(), 2);
        assert_eq!(bus.stats().queue_size, 0);
    }

    #[test]
    fn unsubscribe_removes_the_handler() {
        let mut bus = Bus::new("b");
        let mut actors = HashMap::new();
        actors.insert("a".to_string(), actor("a"));
        let hits = Rc::new(RefCell::new(0));
        let h = hits.clone();
        bus.subscribe("a", "x", Box::new(move |_c, _s| { *h.borrow_mut() += 1; Ok(()) }));
        bus.unsubscribe("a", "x");

        bus.publish_sync(&mut actors, Signal { typ: "x".into(), ..Default::default() });
        assert_eq!(*hits.borrow(), 0);
    }
}
