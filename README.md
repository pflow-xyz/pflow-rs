# pflow-rs

Rust port of [go-pflow](https://github.com/pflow-xyz/go-pflow) — Petri net modeling with ODE simulation and token model DSL.

## Crates

| Crate | Description |
|-------|-------------|
| `pflow-core` | Petri net types (Place, Transition, Arc), fluent Builder API, state map utilities |
| `pflow-solver` | ODE solvers (Tsit5, RK45, RK4, Euler, Heun, Midpoint, BS32), implicit methods, equilibrium detection |
| `pflow-tokenmodel` | Token model schema, snapshot, runtime execution, validation, content-addressed identity |
| `pflow-dsl` | S-expression DSL: lexer, parser, interpreter, builder, codegen |
| `pflow-macros` | `schema!` proc macro — compile-time DSL parsing with zero runtime overhead |
| `pflow` | Umbrella crate re-exporting all of the above |

## Quick Start

```rust
use pflow::*;

// Build an SIR epidemic model
let (net, rates) = PetriNet::build()
    .sir(999.0, 1.0, 0.0)
    .with_rates(1.0);

// Solve to equilibrium
let state = net.set_state(None);
let prob = Problem::new(net, state, [0.0, 100.0], rates);
let (final_state, reached) = find_equilibrium(&prob);

assert!(reached);
// S + I + R = 1000 (conserved)
```

## Petri Net Builder

```rust
use pflow_core::PetriNet;

let net = PetriNet::build()
    .place("A", 10.0)
    .place("B", 0.0)
    .transition("t1")
    .arc("A", "t1", 1.0)
    .arc("t1", "B", 1.0)
    .done();
```

Chain helper for linear sequences:

```rust
let net = PetriNet::build()
    .chain(1.0, &["start", "t1", "middle", "t2", "end"])
    .done();
```

## ODE Solver

Seven explicit Runge-Kutta methods plus implicit solvers for stiff systems:

```rust
use pflow_solver::*;

let prob = Problem::new(net, state, [0.0, 100.0], rates);

// Explicit (adaptive step size)
let sol = solve(&prob, &methods::tsit5(), &Options::default_opts());

// Implicit (stiff systems)
let sol = implicit::implicit_euler(&prob, &Options::stiff());
let sol = implicit::trbdf2(&prob, &Options::stiff());

// Auto-detect stiffness
let sol = implicit::solve_implicit(&prob, &Options::default_opts());
```

**Solver presets:**

| Preset | Use Case |
|--------|----------|
| `Options::default_opts()` | General purpose |
| `Options::fast()` | Game AI, interactive (~10x faster) |
| `Options::accurate()` | Research, publishing |
| `Options::game_ai()` | Move evaluation |
| `Options::epidemic()` | SIR/SEIR models |

## Token Model DSL

Define token model schemas using an S-expression DSL. The `schema!` macro parses and validates the DSL at compile time — syntax errors become compiler errors, and the generated code constructs the `Schema` directly with zero runtime parsing.

```rust
use pflow::schema;

let s = schema!(r#"
(schema ERC-020
  (version v1.0.0)
  (states
    (state balances :type map[address]uint256 :exported)
    (state totalSupply :type uint256)
  )
  (actions
    (action transfer :guard {balances[from] >= amount})
  )
  (arcs
    (arc balances -> transfer :keys (from))
    (arc transfer -> balances :keys (to))
  )
  (constraints
    (constraint conservation {sum(balances) == totalSupply})
  )
)
"#);

assert_eq!(s.name, "ERC-020");
assert_eq!(s.actions[0].guard, "balances[from] >= amount");
```

Invalid DSL is caught at compile time:

```rust
// This won't compile:
let s = schema!(r#"(bad input)"#);
// error: DSL parse error: expected symbol "schema", got "bad"
```

### DSL Syntax Reference

```scheme
(schema <name>
  (version <version>)

  (states
    (state <id> :kind token :initial <n>)          ; token state with initial count
    (state <id> :type <type> :exported)             ; data state, exported
  )

  (actions
    (action <id>)                                    ; simple action
    (action <id> :guard {<expr>})                    ; guarded action
  )

  (arcs
    (arc <source> -> <target>)                       ; simple arc
    (arc <source> -> <target> :keys (<k1> <k2>))     ; arc with map keys
    (arc <source> -> <target> :value <binding>)       ; arc with value binding
  )

  (constraints
    (constraint <id> {<expr>})                       ; invariant constraint
  )
)
```

### Alternative: Fluent Builder

For dynamic schema construction, use the builder API directly:

```rust
use pflow_dsl::Builder;

let schema = Builder::new("ERC-020")
    .data("balances", "map[address]uint256").exported()
    .data("totalSupply", "uint256")
    .action("transfer").guard("balances[from] >= amount")
    .flow("balances", "transfer").keys(&["from"])
    .flow("transfer", "balances").keys(&["to"])
    .constraint("conservation", "sum(balances) == totalSupply")
    .must_schema();
```

### Runtime Parsing

For DSL strings loaded at runtime (e.g. from files):

```rust
use pflow_dsl::parse_schema;

let schema = parse_schema(&dsl_string).unwrap();
```

## Content-Addressed Identity

Schemas produce deterministic content identifiers (CIDs) via SHA-256. Insertion order doesn't matter — schemas with the same structure always produce the same hash.

```rust
use pflow::schema;

let s = schema!(r#"(schema counter
  (states (state count :kind token :initial 5))
  (actions (action inc))
  (arcs (arc inc -> count))
)"#);

// Full CID (includes name/version)
let cid = s.cid();           // "cid:a1b2c3..."

// Structural fingerprint (ignores name/version)
let idh = s.identity_hash(); // "idh:d4e5f6..."

// Compare schemas
assert!(s.equal(&s));                // same CID
assert!(s.structurally_equal(&s));   // same structure
```

## Runtime Execution

```rust
use pflow_tokenmodel::{Runtime, Bindings};
use serde_json::Value;

let mut rt = Runtime::new(schema);

// Set initial balance
rt.snapshot.set_data_map_value("balances", "alice", Value::Number(1000.into()));

// Execute a transfer
let mut bindings = Bindings::new();
bindings.insert("from".into(), Value::String("alice".into()));
bindings.insert("to".into(), Value::String("bob".into()));
bindings.insert("amount".into(), Value::Number(250.into()));

rt.execute_with_bindings("transfer", &bindings).unwrap();
// alice: 750, bob: 250
```

## State Utilities

```rust
use pflow_core::stateutil;

let state = /* ... */;
let updated = stateutil::apply(&state, &updates);
let total = stateutil::sum(&state);
let changes = stateutil::diff(&before, &after);
let history = stateutil::filter(&state, |k| k.starts_with('_'));
```

## Code Generation

Generate Rust source from DSL definitions:

```rust
use pflow_dsl::generate_rust_from_dsl;

let code = generate_rust_from_dsl(dsl_input, "mymodule", "make_schema").unwrap();
// Outputs a Rust function that constructs the schema
```

## Dependencies

Minimal external dependencies:

- `serde`, `serde_json` — serialization
- `sha2`, `hex` — content-addressed identity
- `thiserror` — error types

## License

MIT
