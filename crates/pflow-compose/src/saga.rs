//! `Saga`: an ordered list of compensable steps as a composable subnet,
//! ported from go-pflow's `saga` package (`saga/saga.go`,
//! `saga/metasubnet.go`, added in go-pflow `4a827f4`; ported from
//! `c802f34`).
//!
//! A saga is the standard answer to "a business transaction that spans
//! several systems, none of which can roll the others back": run the steps
//! in order, and if step k fails, run the compensations of steps k-1, k-2,
//! ..., 1 in reverse. Here it is a net, so that the properties a saga exists
//! to provide — no stranded half-done state, never both committed and rolled
//! back, funds conserved — are things the analysers prove rather than things
//! a code review hopes for.
//!
//! - [`Step`]: a forward action and the compensation that undoes it.
//! - [`Move`]: a transfer of tokens between two accounts. A step's forward
//!   transition applies its moves; its compensation applies them reversed.
//! - [`Account`]: a place holding a token balance. Owned by the saga, or
//!   shared with another subnet so a step can be a transfer in a larger
//!   system.
//!
//! Structure, for steps s1 ... sn:
//!
//! ```text
//! control token (exactly one, always):
//!   next_s1 -> do_s1 -> next_s2 -> ... -> do_sn -> committing -> commit -> completed
//!   fail_si:     next_si -> unwind_s(i-1)      (rolled_back when i == 1)
//!   fail_commit: committing -> unwind_sn
//!   undo_si:     unwind_si -> unwind_s(i-1)    (rolled_back when i == 1)
//!
//! per step (exactly one token among the four):
//!   pending -do-> done -undo-> compensated
//!   pending -fail-> failed
//! ```
//!
//! The control token is the whole ordering argument: it walks forward
//! through the `next_*` places, and on failure it is handed to the
//! `unwind_*` chain, which only runs downward — so compensation order is a
//! property of the arcs. Because it is a single token, `completed` and
//! `rolled_back` cannot both be marked: a P-invariant Farkas derives.
//!
//! Like [`crate::new_resource_pool`], this builds the [`Subnet`] directly.
//! Every generated name, the order of places, transitions, arcs, ports and
//! constraints, and every description string match go-pflow's
//! `ToMetaModel`/`ToMetaSubnet` exactly. Not ported: `saga/analyze.go`'s
//! `Stranded`, a reachability-graph query rather than part of the net; the
//! integration tests carry an equivalent helper (`tests/saga.rs`).

use std::collections::BTreeMap;

use pflow_metamodel::{Arc as MArc, Constraint, Model, Place, StateKind, Transition};

use crate::bundle::{NetType, Port, PortKind, PortTarget, Subnet, SUBNET_TYPE};
use crate::flatten::normalize_kinds;

/// Holds the control token after the last step has run, while the saga waits
/// for its final acknowledgement.
pub const SAGA_COMMITTING: &str = "committing";
/// The terminal place of a saga that ran to the end.
pub const SAGA_COMPLETED: &str = "completed";
/// The terminal place of a saga that was fully compensated. It doubles as
/// "the compensation cursor has passed step 1".
pub const SAGA_ROLLED_BACK: &str = "rolled_back";
/// Takes the saga from `committing` to `completed`.
pub const SAGA_COMMIT: &str = "commit";
/// Fails the final acknowledgement, which compensates every step including
/// the last.
pub const SAGA_FAIL_COMMIT: &str = "fail_commit";

/// `<step>_pending`: 1 while the step has not been attempted.
pub fn pending_place(step: &str) -> String {
    format!("{step}_pending")
}

/// `<step>_done`: 1 while the step has run and not been compensated.
pub fn done_place(step: &str) -> String {
    format!("{step}_done")
}

/// `<step>_failed`: 1 if the step was attempted and failed.
pub fn failed_place(step: &str) -> String {
    format!("{step}_failed")
}

/// `<step>_compensated`: 1 once the step's compensation has run.
pub fn compensated_place(step: &str) -> String {
    format!("{step}_compensated")
}

/// `next_<step>`: holds the control token while the step is the next to run.
pub fn next_place(step: &str) -> String {
    format!("next_{step}")
}

/// `unwind_<step>`: holds the control token while the step is the next to
/// compensate.
pub fn unwind_place(step: &str) -> String {
    format!("unwind_{step}")
}

/// `do_<step>`: runs the step forward.
pub fn do_transition(step: &str) -> String {
    format!("do_{step}")
}

/// `fail_<step>`: fails the step.
pub fn fail_transition(step: &str) -> String {
    format!("fail_{step}")
}

/// `undo_<step>`: compensates the step.
pub fn undo_transition(step: &str) -> String {
    format!("undo_{step}")
}

/// `account:<id>`: the port ID of an account.
pub fn account_port(account: &str) -> String {
    format!("account:{account}")
}

/// Transfers `amount` tokens `from` one account `to` another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub from: String,
    pub to: String,
    pub amount: i64,
}

/// One forward action paired with its compensation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    /// Prose for the forward action (e.g. "sign"). Lands in the `do_`
    /// transition's description; empty reads as `"run <id>"`.
    pub name: String,
    /// Prose for the undo (e.g. "void signature"). Lands in the `undo_`
    /// transition's description; empty reads as `"compensate <id>"`.
    pub compensation: String,
    /// Applied atomically when the step runs, reversed atomically when it is
    /// compensated. A step with no moves is a pure state change.
    pub moves: Vec<Move>,
}

/// A place holding a balance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    /// The starting balance. A shared account has `initial` 0: the subnet
    /// that owns the balance supplies it, and fusing places sums initial
    /// markings, so a second copy here would double-count.
    pub initial: i64,
    /// Marks an account whose balance lives in another subnet. It is a
    /// boundary place only; link it with a `TokenLink`. A saga with a shared
    /// account is not closed, so it carries no local conservation
    /// constraint.
    pub shared: bool,
}

/// An ordered list of compensable steps and the accounts they move tokens
/// between.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Saga {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Steps run in order. Compensation runs in the reverse order.
    pub steps: Vec<Step>,
    /// Keyed by account ID. A `BTreeMap` so iteration is the sorted order
    /// go-pflow's `sortedAccountIDs` imposes on its map.
    pub accounts: BTreeMap<String, Account>,
}

/// A fluent builder for [`Saga`]s, go-pflow's `saga.Builder`.
///
/// ```
/// use pflow_compose::SagaBuilder;
///
/// let saga = SagaBuilder::new("pay")
///     .account("a", 10)
///     .account("b", 0)
///     .step("one").move_tokens("a", "b", 3).done()
///     .must_build();
/// let sub = saga.to_meta_subnet();
/// assert!(sub.port_by_id("account:a").is_some());
/// ```
#[derive(Debug, Clone)]
pub struct SagaBuilder {
    saga: Saga,
    errors: Vec<String>,
}

/// Builds one step. [`StepBuilder::done`] returns to the saga's builder.
#[derive(Debug, Clone)]
pub struct StepBuilder {
    parent: SagaBuilder,
    step: Step,
}

impl SagaBuilder {
    /// go-pflow's `saga.New`.
    pub fn new(id: impl Into<String>) -> Self {
        SagaBuilder {
            saga: Saga {
                id: id.into(),
                ..Default::default()
            },
            errors: Vec::new(),
        }
    }

    /// Sets the saga's display name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.saga.name = name.into();
        self
    }

    /// Sets the saga's description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.saga.description = desc.into();
        self
    }

    /// Declares an account the saga owns, holding `initial` tokens.
    pub fn account(mut self, id: impl Into<String>, initial: i64) -> Self {
        let id = id.into();
        self.add_account(Account {
            id,
            initial,
            shared: false,
        });
        self
    }

    /// Declares an account whose balance lives in another subnet. Link the
    /// saga's `account:<id>` port to that subnet's balance place.
    pub fn shared_account(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        self.add_account(Account {
            id,
            initial: 0,
            shared: true,
        });
        self
    }

    fn add_account(&mut self, a: Account) {
        if self.saga.accounts.contains_key(&a.id) {
            self.errors.push(format!("duplicate account {:?}", a.id));
            return;
        }
        self.saga.accounts.insert(a.id.clone(), a);
    }

    /// Starts a step. Steps run in the order they are added.
    pub fn step(self, id: impl Into<String>) -> StepBuilder {
        StepBuilder {
            parent: self,
            step: Step {
                id: id.into(),
                ..Default::default()
            },
        }
    }

    /// Returns the saga without validating it.
    pub fn build(self) -> Saga {
        self.saga
    }

    /// Validates and returns the saga.
    pub fn build_validated(self) -> Result<Saga, String> {
        validate(&self.saga, &self.errors)?;
        Ok(self.saga)
    }

    /// Builds with validation, panicking on an ill-formed saga.
    pub fn must_build(self) -> Saga {
        match self.build_validated() {
            Ok(s) => s,
            Err(e) => panic!("{e}"),
        }
    }
}

impl StepBuilder {
    /// Describes the forward action.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.step.name = name.into();
        self
    }

    /// Describes the undo.
    pub fn compensate(mut self, desc: impl Into<String>) -> Self {
        self.step.compensation = desc.into();
        self
    }

    /// Adds a transfer of `amount` tokens from one account to another
    /// (go-pflow's `StepBuilder.Move`; `move` is a Rust keyword).
    pub fn move_tokens(
        mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        amount: i64,
    ) -> Self {
        self.step.moves.push(Move {
            from: from.into(),
            to: to.into(),
            amount,
        });
        self
    }

    /// Finishes the step and returns to the saga builder.
    pub fn done(mut self) -> SagaBuilder {
        self.parent.saga.steps.push(self.step);
        self.parent
    }
}

/// go-pflow's `(*Builder).validate`: `prior` are errors the builder recorded
/// while the saga was assembled (duplicate accounts). Every error is
/// collected; they are joined with `"; "` under `saga "<id>": `.
fn validate(s: &Saga, prior: &[String]) -> Result<(), String> {
    let mut errs: Vec<String> = prior.to_vec();

    if s.id.is_empty() {
        errs.push("saga has no ID".to_string());
    }
    if s.steps.is_empty() {
        errs.push("saga has no steps".to_string());
    }

    // Every ID the saga generates must be unique, or two steps would
    // silently share a place and the net would mean something else.
    let mut taken: BTreeMap<String, String> = [
        (SAGA_COMMITTING, "the committing place"),
        (SAGA_COMPLETED, "the completed place"),
        (SAGA_ROLLED_BACK, "the rolled_back place"),
        (SAGA_COMMIT, "the commit transition"),
        (SAGA_FAIL_COMMIT, "the fail_commit transition"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let mut claim = |errs: &mut Vec<String>, id: String, owner: String| {
        if let Some(prev) = taken.get(&id) {
            errs.push(format!(
                "{owner} collides with {prev}: both would be {id:?}"
            ));
            return;
        }
        taken.insert(id, owner);
    };

    for id in s.accounts.keys() {
        claim(&mut errs, id.clone(), format!("account {id:?}"));
    }

    let mut seen = std::collections::HashSet::new();
    for st in &s.steps {
        if st.id.is_empty() {
            errs.push("a step has no ID".to_string());
            continue;
        }
        if seen.contains(&st.id) {
            errs.push(format!("duplicate step {:?}", st.id));
            continue;
        }
        if st
            .id
            .chars()
            .any(|c| matches!(c, ' ' | '\t' | '\n' | '"' | '\''))
        {
            errs.push(format!(
                "step ID {:?} must not contain whitespace or quotes",
                st.id
            ));
            continue;
        }
        seen.insert(st.id.clone());

        for generated in [
            pending_place(&st.id),
            done_place(&st.id),
            failed_place(&st.id),
            compensated_place(&st.id),
            next_place(&st.id),
            unwind_place(&st.id),
            do_transition(&st.id),
            fail_transition(&st.id),
            undo_transition(&st.id),
        ] {
            claim(&mut errs, generated, format!("step {:?}", st.id));
        }

        for (i, mv) in st.moves.iter().enumerate() {
            if !s.accounts.contains_key(&mv.from) {
                errs.push(format!(
                    "step {:?} move {i}: unknown account {:?}",
                    st.id, mv.from
                ));
            }
            if !s.accounts.contains_key(&mv.to) {
                errs.push(format!(
                    "step {:?} move {i}: unknown account {:?}",
                    st.id, mv.to
                ));
            }
            if mv.amount < 1 {
                errs.push(format!(
                    "step {:?} move {i}: amount {} must be positive",
                    st.id, mv.amount
                ));
            }
            if mv.from == mv.to {
                errs.push(format!(
                    "step {:?} move {i}: from and to are both {:?}",
                    st.id, mv.from
                ));
            }
        }
    }

    for (id, a) in &s.accounts {
        if a.initial < 0 {
            errs.push(format!("account {id:?}: initial {} is negative", a.initial));
        }
        if a.shared && a.initial != 0 {
            errs.push(format!(
                "account {id:?}: a shared account's balance lives in the subnet it is linked to, not here"
            ));
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(format!("saga {:?}: {}", s.id, errs.join("; ")))
    }
}

/// A step's effect on each account once its moves are netted out. Netting
/// makes a pass-through (A->B then B->C in one step) cost B nothing, which is
/// what "atomic" means for a step; the alternative, a consuming arc and a
/// producing arc on B, would demand B already hold the amount. Sorted by
/// account, and accounts that net to zero are absent.
fn net_delta(step: &Step) -> BTreeMap<String, i64> {
    let mut delta: BTreeMap<String, i64> = BTreeMap::new();
    for mv in &step.moves {
        *delta.entry(mv.from.clone()).or_insert(0) -= mv.amount;
        *delta.entry(mv.to.clone()).or_insert(0) += mv.amount;
    }
    delta.retain(|_, v| *v != 0);
    delta
}

impl Saga {
    /// Starts a [`SagaBuilder`].
    pub fn builder(id: impl Into<String>) -> SagaBuilder {
        SagaBuilder::new(id)
    }

    /// Checks the saga is well formed enough to emit a net. Unlike
    /// [`SagaBuilder::build_validated`], it cannot see errors the builder
    /// recorded (a duplicate account), exactly as go-pflow's `Saga.Validate`.
    pub fn validate(&self) -> Result<(), String> {
        validate(self, &[])
    }

    /// Every place the single control token can occupy, in the order it
    /// visits them going forward, then the unwind chain downward, then the
    /// terminals. Their sum is 1 in every reachable marking.
    pub fn control_places(&self) -> Vec<String> {
        let mut out: Vec<String> = self.steps.iter().map(|st| next_place(&st.id)).collect();
        out.push(SAGA_COMMITTING.to_string());
        for st in self.steps.iter().rev() {
            out.push(unwind_place(&st.id));
        }
        out.push(SAGA_COMPLETED.to_string());
        out.push(SAGA_ROLLED_BACK.to_string());
        out
    }

    /// Where the control token goes when the step at index `i` is the last
    /// thing that has to be undone below it: unwind of the step before it,
    /// or `rolled_back` when there is none.
    fn unwind_target(&self, i: usize) -> String {
        if i == 0 {
            SAGA_ROLLED_BACK.to_string()
        } else {
            unwind_place(&self.steps[i - 1].id)
        }
    }

    /// Emits the saga as a `Model`, with no boundary.
    ///
    /// # Panics
    ///
    /// If the saga does not validate; call [`Saga::validate`] (or build with
    /// [`SagaBuilder::build_validated`]) first when the saga comes from
    /// outside the program. The alternative — emitting a net from an
    /// ill-formed saga — would silently give two steps one place.
    pub fn to_meta_model(&self) -> Model {
        if let Err(e) = self.validate() {
            panic!("{e}");
        }
        let mut m = Model {
            name: self.id.clone(),
            description: self.description.clone(),
            ..Default::default()
        };
        self.add_places(&mut m);
        self.add_transitions(&mut m);
        self.add_constraints(&mut m);
        normalize_kinds(&mut m);
        m
    }

    /// Emits the saga as a composable [`Subnet`].
    ///
    /// Boundary:
    /// - `account:<id>` inout ports on every account (schema `account`), so a
    ///   balance can be shared with another subnet through a `TokenLink`
    ///   rather than duplicated.
    /// - `completed` and `rolled_back` out ports (schema `saga:outcome`), so
    ///   the outcome is observable.
    /// - transition ports `commit` and `fail_commit` (in), then per step
    ///   `do:<step>`, `fail:<step>` (in) and `undo:<step>` (out), so a step
    ///   can be fused with the external action it stands for through an
    ///   `EventLink`.
    ///
    /// The net type is [`NetType::Resource`]: it is the type that lets the
    /// account ports token-link. Only the account ports are advertised for
    /// token links, which is the guard against linking a control place.
    pub fn to_meta_subnet(&self) -> Subnet {
        let mut m = self.to_meta_model();

        let export = |m: &mut Model, id: &str| {
            if let Some(p) = m.place_by_id_mut(id) {
                p.exported = true;
            }
        };

        let mut ports = Vec::new();
        for id in self.accounts.keys() {
            export(&mut m, id);
            ports.push(Port {
                id: account_port(id),
                kind: Some(PortKind::Inout),
                place: id.clone(),
                schema: "account".to_string(),
                ..Default::default()
            });
        }
        for terminal in [SAGA_COMPLETED, SAGA_ROLLED_BACK] {
            export(&mut m, terminal);
            ports.push(Port {
                id: terminal.to_string(),
                kind: Some(PortKind::Out),
                place: terminal.to_string(),
                schema: "saga:outcome".to_string(),
                ..Default::default()
            });
        }
        let trans_port = |id: String, kind: PortKind, transition: String| Port {
            id,
            kind: Some(kind),
            target: Some(PortTarget::Transition),
            transition,
            ..Default::default()
        };
        ports.push(trans_port(
            SAGA_COMMIT.to_string(),
            PortKind::In,
            SAGA_COMMIT.to_string(),
        ));
        ports.push(trans_port(
            SAGA_FAIL_COMMIT.to_string(),
            PortKind::In,
            SAGA_FAIL_COMMIT.to_string(),
        ));
        for st in &self.steps {
            ports.push(trans_port(
                format!("do:{}", st.id),
                PortKind::In,
                do_transition(&st.id),
            ));
            ports.push(trans_port(
                format!("fail:{}", st.id),
                PortKind::In,
                fail_transition(&st.id),
            ));
            ports.push(trans_port(
                format!("undo:{}", st.id),
                PortKind::Out,
                undo_transition(&st.id),
            ));
        }

        Subnet {
            typ: SUBNET_TYPE.to_string(),
            id: self.id.clone(),
            net_type: NetType::Resource,
            model: m,
            ports,
        }
    }

    fn add_places(&self, m: &mut Model) {
        let token = |m: &mut Model, id: String, initial: i64, desc: String| {
            m.places.push(Place {
                id,
                kind: Some(StateKind::Token),
                initial,
                description: desc,
                ..Default::default()
            });
        };

        for (i, st) in self.steps.iter().enumerate() {
            let first = if i == 0 { 1 } else { 0 };
            token(
                m,
                next_place(&st.id),
                first,
                format!("control: {} runs next", st.id),
            );
        }
        token(
            m,
            SAGA_COMMITTING.to_string(),
            0,
            "control: every step ran; awaiting final acknowledgement".to_string(),
        );
        for st in self.steps.iter().rev() {
            token(
                m,
                unwind_place(&st.id),
                0,
                format!("control: {} is compensated next", st.id),
            );
        }
        token(
            m,
            SAGA_COMPLETED.to_string(),
            0,
            "terminal: the saga ran to the end".to_string(),
        );
        token(
            m,
            SAGA_ROLLED_BACK.to_string(),
            0,
            "terminal: every step that ran was compensated".to_string(),
        );

        for st in &self.steps {
            token(
                m,
                pending_place(&st.id),
                1,
                format!("{} not yet attempted", st.id),
            );
            token(
                m,
                done_place(&st.id),
                0,
                format!("{} ran and is not compensated", st.id),
            );
            token(
                m,
                failed_place(&st.id),
                0,
                format!("{} was attempted and failed", st.id),
            );
            token(
                m,
                compensated_place(&st.id),
                0,
                format!("{} was compensated", st.id),
            );
        }

        for (id, a) in &self.accounts {
            let desc = if a.shared {
                "shared account; balance lives in the linked subnet"
            } else {
                "account"
            };
            m.places.push(Place {
                id: id.clone(),
                kind: Some(StateKind::Token),
                initial: a.initial,
                resource: true,
                description: desc.to_string(),
                ..Default::default()
            });
        }
    }

    fn add_transitions(&self, m: &mut Model) {
        let tr = |m: &mut Model, id: String, desc: String| {
            m.transitions.push(Transition {
                id,
                description: desc,
                ..Default::default()
            });
        };
        let arc = |m: &mut Model, from: String, to: String, weight: i64| {
            m.arcs.push(MArc {
                from,
                to,
                weight,
                ..Default::default()
            });
        };

        let n = self.steps.len();
        for (i, st) in self.steps.iter().enumerate() {
            let (do_t, fail_t, undo_t) = (
                do_transition(&st.id),
                fail_transition(&st.id),
                undo_transition(&st.id),
            );

            let forward = if st.name.is_empty() {
                format!("run {}", st.id)
            } else {
                st.name.clone()
            };
            let backward = if st.compensation.is_empty() {
                format!("compensate {}", st.id)
            } else {
                st.compensation.clone()
            };
            tr(m, do_t.clone(), forward);
            tr(m, fail_t.clone(), format!("{} fails", st.id));
            tr(m, undo_t.clone(), backward);

            // Forward: the control token moves on and the step becomes done.
            let next = if i + 1 < n {
                next_place(&self.steps[i + 1].id)
            } else {
                SAGA_COMMITTING.to_string()
            };
            arc(m, next_place(&st.id), do_t.clone(), 1);
            arc(m, pending_place(&st.id), do_t.clone(), 1);
            arc(m, do_t.clone(), done_place(&st.id), 1);
            arc(m, do_t.clone(), next, 1);

            // Failure: the step is marked failed and the control token turns
            // round.
            arc(m, next_place(&st.id), fail_t.clone(), 1);
            arc(m, pending_place(&st.id), fail_t.clone(), 1);
            arc(m, fail_t.clone(), failed_place(&st.id), 1);
            arc(m, fail_t, self.unwind_target(i), 1);

            // Compensation: only the step the control token is standing on,
            // and only if it ran.
            arc(m, unwind_place(&st.id), undo_t.clone(), 1);
            arc(m, done_place(&st.id), undo_t.clone(), 1);
            arc(m, undo_t.clone(), compensated_place(&st.id), 1);
            arc(m, undo_t.clone(), self.unwind_target(i), 1);

            // The accounts: do applies the step's net effect, undo reverses
            // it.
            for (acct, d) in net_delta(st) {
                if d < 0 {
                    arc(m, acct.clone(), do_t.clone(), -d);
                    arc(m, undo_t.clone(), acct, -d);
                } else {
                    arc(m, do_t.clone(), acct.clone(), d);
                    arc(m, acct, undo_t.clone(), d);
                }
            }
        }

        tr(
            m,
            SAGA_COMMIT.to_string(),
            "final acknowledgement succeeds".to_string(),
        );
        arc(m, SAGA_COMMITTING.to_string(), SAGA_COMMIT.to_string(), 1);
        arc(m, SAGA_COMMIT.to_string(), SAGA_COMPLETED.to_string(), 1);

        tr(
            m,
            SAGA_FAIL_COMMIT.to_string(),
            "final acknowledgement fails; compensate every step".to_string(),
        );
        arc(
            m,
            SAGA_COMMITTING.to_string(),
            SAGA_FAIL_COMMIT.to_string(),
            1,
        );
        arc(
            m,
            SAGA_FAIL_COMMIT.to_string(),
            unwind_place(&self.steps[n - 1].id),
            1,
        );
    }

    fn add_constraints(&self, m: &mut Model) {
        let sum = |places: &[String]| -> String {
            places
                .iter()
                .map(|p| format!("tokens({p:?})"))
                .collect::<Vec<_>>()
                .join(" + ")
        };

        // One control token: this is what makes completed and rolled_back
        // exclusive, and what makes compensation order a structural fact.
        m.constraints.push(Constraint {
            id: "saga_control".to_string(),
            expr: format!("{} == 1", sum(&self.control_places())),
        });

        // Each step is in exactly one of four states.
        for st in &self.steps {
            m.constraints.push(Constraint {
                id: format!("saga_step_{}", st.id),
                expr: format!(
                    "{} == 1",
                    sum(&[
                        pending_place(&st.id),
                        done_place(&st.id),
                        failed_place(&st.id),
                        compensated_place(&st.id),
                    ])
                ),
            });
        }

        // Conservation over the accounts the steps touch. Only claimed for a
        // closed saga: once an account is shared another subnet can move
        // tokens in or out, and the law is then a property of the
        // composite, derived there.
        let mut touched: BTreeMap<String, ()> = BTreeMap::new();
        for st in &self.steps {
            for acct in net_delta(st).into_keys() {
                touched.insert(acct, ());
            }
        }
        if touched.is_empty() {
            return;
        }
        let mut accounts = Vec::new();
        let mut total = 0;
        for id in touched.keys() {
            let a = &self.accounts[id];
            if a.shared {
                return;
            }
            accounts.push(id.clone());
            total += a.initial;
        }
        m.constraints.push(Constraint {
            id: "saga_conservation".to_string(),
            expr: format!("{} == {total}", sum(&accounts)),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TestNetDeltaNetsOutPassThrough` (go-pflow `saga/saga_test.go:88`
    /// @ `c802f34`): b and d cancel and must not appear.
    #[test]
    fn net_delta_nets_out_pass_through() {
        let mv = |from: &str, to: &str, amount| Move {
            from: from.into(),
            to: to.into(),
            amount,
        };
        let st = Step {
            id: "s".into(),
            moves: vec![
                mv("a", "b", 5),
                mv("b", "c", 5),
                mv("c", "d", 2),
                mv("d", "c", 2),
            ],
            ..Default::default()
        };
        let got = net_delta(&st);
        let want: BTreeMap<String, i64> = [("a".to_string(), -5), ("c".to_string(), 5)]
            .into_iter()
            .collect();
        assert_eq!(got, want);
    }
}
