#!/bin/sh
# Writes the four plan nets (SearchPlan::net, by the plan_net example) to
# crates/pflow-metamodel/tests/golden/plan-*.json, and draws each with
# pflow-native's string-diagram view to docs/diagrams/plan-*.svg.
#
#   docs/diagrams/render.sh /path/to/pflow-native-cli
#
# The CLI is pflow-native's (cargo build --release -p pflow-native-cli in a
# pflow-native checkout). tests/search_plan.rs holds the nets to these files.
set -eu
cli=$1
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
golden=$root/crates/pflow-metamodel/tests/golden

draw() {
  cargo run -q --manifest-path "$root/Cargo.toml" -p pflow-metamodel --example plan_net -- "$2" \
    > "$golden/$1.json"
  # The SVG's path goes into the batch JSON as a string: escape \ and ".
  svg=$(printf '%s' "$here/$1.svg" | sed 's/\\/\\\\/g; s/"/\\"/g')
  printf '[{"command": "view.string_diagram", "args": {}}, {"command": "export.artifact", "args": {"format": "svg", "path": "%s"}}]' \
    "$svg" | "$cli" -i "$golden/$1.json" batch - > /dev/null
  echo "$1"
}

draw plan-default '{}'
draw plan-connect4-exact '{"mode":"exact","order":["tt_best","immediate_win","centrality","history"],"drivers":["null_window",{"budget":{"nodes":1000000}}],"memo":{"entries":500000}}'
draw plan-root-ode '{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"]}'
draw plan-doc-example '{"mode": "exact", "order": ["tt_best", "immediate_win", "centrality", "history"], "filters": ["must_win", "non_losing"], "root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration", {"budget": {"nodes": 1000000}}], "memo": {"entries": 500000}}'
