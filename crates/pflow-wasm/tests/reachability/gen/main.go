// Command gen writes go-pflow's reachability answer for each net in ../nets as
// ../go/<name>.json, in exactly the shape pflow.xyz/mcp's pflow_reachability
// returns (cmd/webserver/mcp.go mcpReachability, copied below), so pflow-rs's
// wasm engine can be held to the hosted Go engine's verdicts.
//
//	cd crates/pflow-wasm/tests/reachability/gen && go run .
//
// go.mod's replace points at the sibling go-pflow checkout as seen from the
// main pflow-rs checkout (~/Workspace/go-pflow); from anywhere else, run
// `go mod edit -replace github.com/pflow-xyz/go-pflow=<path>` on a copy, or set
// GO_PFLOW for the commit stamp. The commit used is written into ../go/SOURCE.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"

	"github.com/pflow-xyz/go-pflow/parser"
	"github.com/pflow-xyz/go-pflow/petri"
	"github.com/pflow-xyz/go-pflow/reachability"
)

// Budgets per net; everything else gets the hosted default, 2000.
var budgets = map[string]int{"source": 50}

func main() {
	files, _ := filepath.Glob("../nets/*.json")
	sort.Strings(files)
	os.MkdirAll("../go", 0o755)
	for _, f := range files {
		name := strings.TrimSuffix(filepath.Base(f), ".json")
		raw, err := os.ReadFile(f)
		check(err)
		net, err := parser.FromJSON(raw)
		check(err)
		max := 2000
		if b, ok := budgets[name]; ok {
			max = b
		}
		out := answer(net, max)
		out["max_states_asked"] = max
		b, err := json.MarshalIndent(out, "", "  ")
		check(err)
		check(os.WriteFile(filepath.Join("../go", name+".json"), append(b, '\n'), 0o644))
		fmt.Println(name, out["states"], out["deadlock_free"], out["quasi_live"], out["live"])
	}
	rev, _ := exec.Command("git", "-C", goPflow(), "rev-parse", "HEAD").Output()
	check(os.WriteFile("../go/SOURCE", []byte("github.com/pflow-xyz/go-pflow @ "+strings.TrimSpace(string(rev))+"\n"), 0o644))
}

// answer is pflow-xyz cmd/webserver/mcp.go mcpReachability's body.
func answer(net *petri.PetriNet, max int) map[string]any {
	res := reachability.NewAnalyzer(net).WithMaxStates(max).Analyze()
	l := res.Liveness
	out := map[string]any{
		"states": res.StateCount, "edges": res.EdgeCount, "complete": res.IsComplete, "bounded": res.Bounded,
		"max_tokens": res.MaxTokens, "deadlocks": len(res.DeadMarkings),
		"deadlock_free": verdictValue(l.DeadlockFree), "live": verdictValue(l.Live), "quasi_live": verdictValue(l.QuasiLive),
		"witness": l.Witness, "not_live": l.NotLive, "dead_transitions": res.DeadTrans,
		"note": "untimed: rates play no part; a claim about every state holds only when complete is true",
	}
	if res.Truncated {
		out["truncated"] = res.TruncateMsg
		out["bound"] = map[string]any{"max_states": res.ExplorationStats.StatesLimit, "max_tokens": res.ExplorationStats.TokensLimit}
	}
	if !res.IsComplete {
		out["bounded"] = "unknown: the search stopped at max_states"
	}
	if len(res.DeadMarkings) > 0 {
		out["first_deadlock"] = res.DeadMarkings[0].Marking
	}
	return out
}

func verdictValue(v reachability.Verdict) any {
	switch v {
	case reachability.VerdictTrue:
		return true
	case reachability.VerdictFalse:
		return false
	}
	return "unknown"
}

func check(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func goPflow() string {
	if p := os.Getenv("GO_PFLOW"); p != "" {
		return p
	}
	return "../../../../../../go-pflow"
}
