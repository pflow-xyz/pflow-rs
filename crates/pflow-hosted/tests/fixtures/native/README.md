# pflow-native goldens for the workbench tools

`workbench_goldens.json` is the `pflow_sde` and `pflow_what_if` subset (16
cases each, 32 in all) of pflow-xyz's `parity/native/mcp_goldens.json`, the
answers pflow-native gives to each hosted /mcp call. The cases are copied
verbatim (same objects, same doubles); only the other tools' cases and the
`compositions` block are left out. `tests/workbench_parity.rs` replays them
through `pflow_hosted::call` with the comparison rules of pflow-xyz's
`cmd/webserver/mcp_workbench_parity_test.go`.

| | |
|---|---|
| source repo | `~/Workspace/pflow-xyz` (github.com/pflow-xyz/pflow-xyz) |
| source file | `parity/native/mcp_goldens.json` |
| pflow-xyz commit | `25647767ce7052606bc274f42b42604686a3b15c` |
| sha256 of `mcp_goldens.json` | `d2bb2577a8a45b3471eb4f1f3a8ccbb9d7e2713695fb1fcc323f49bdf58eca5e` |
| generated from pflow-native | `d0f4a477c0e0e2c861363b7b4c53bc9388b82d84` (its `pflow_native` field) |

`nets/` holds byte-identical copies of the fixtures the cases name. A case's
`fixture` is its path under pflow-xyz's `parity/`; the copy is
`nets/<basename>`:

| fixture | sha256 |
|---|---|
| `native/fixtures/sir.jsonld` | `f43a06f99555da97e7d932edee84fe9b0009a5179c8813f812781907ae9bbe56` |
| `native/fixtures/colored.jsonld` | `5cd4e105a99a1c7a384388dd47869cef1e029aede735cf56610356c0d00b4a86` |
| `native/fixtures/inert.jsonld` | `f64bf88a6b2bf8922f5ebbf81de7db4b82577e93942ac5f5efef7045c56743b0` |
| `native/fixtures/gated.jsonld` | `29e85d111181897ce76c2c22fc9fde768318cdd2db552d3bc96a02c6cfbaf426` |
| `native/fixtures/explode.jsonld` | `408da6dd4aff510a8b40681e2c5f1734fba3f1f134ae6b070ccf7fb93417bc6c` |
| `fixtures/knapsack.jsonld` | `7ceb53638f892c798febfd00faeef0fd9e64713618a75f852aa99ba3871509c3` |
| `fixtures/with-rates.jsonld` | `b6d12525d1b443272304276d292abc8fd8d571d58bea9b7bd96628ae63c0a09f` |

A mismatch is a bug here, never a reason to regenerate. To refresh after
pflow-xyz regenerates its goldens, re-extract the two tools' cases from the
new `mcp_goldens.json`, recopy the fixtures, and update the table above.
