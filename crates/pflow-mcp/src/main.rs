mod tools;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{schemars, tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use tokio::io::{stdin, stdout};

/// Every tool below accepts a model as DSL S-expression text (starting with
/// `(`), the tokenmodel Schema's native JSON, the pflow.xyz editor's Shape A
/// JSON, or `pflow_metamodel::Model`'s Shape B JSON — see
/// `tools::convert::parse_any_model`. Tool names and response shapes mirror
/// petri-pilot's MCP server (`pkg/mcp/`) one for one where a Go tool of the
/// same name exists, so a client cannot tell which server answered
/// (ROADMAP.md Phase 6).
const MODEL_DESC: &str =
    "Petri net model as DSL S-expression, tokenmodel Schema JSON, pflow.xyz Shape A JSON, or metamodel Shape B JSON";

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ModelOnly {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ModelMaxStates {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "State exploration limit for exhaustive checks")]
    pub max_states: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SimulateParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "Simulation end time (default: 100.0)")]
    pub t_end: Option<f64>,
    #[schemars(description = "Maximum output data points (default: 100)")]
    pub max_points: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EquilibriumParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "Maximum simulation time to search for equilibrium (default: 1000.0)")]
    pub t_max: Option<f64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FireParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "Ordered list of transition IDs to fire")]
    pub actions: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StochasticParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "Simulation horizon (default: 10.0)")]
    pub horizon: Option<f64>,
    #[schemars(description = "Number of grid points, >= 2 (default: 101)")]
    pub samples: Option<usize>,
    #[schemars(description = "Number of realizations to average (default: 10)")]
    pub realizations: Option<usize>,
    #[schemars(description = "PRNG seed; 0 is treated as 1 (default: 1)")]
    pub seed: Option<u64>,
    #[schemars(description = "Per-transition rates; missing transitions default to 1.0")]
    pub rates: Option<std::collections::HashMap<String, f64>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct VerifyParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "JSON array of properties (shorthand strings or {\"kind\":...} objects) — see petri_verify's tool description")]
    pub properties: String,
    #[schemars(description = "State exploration limit for exhaustive checks (default 20000)")]
    pub max_states: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ConformanceParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "JSON array of events: [{\"case\":\"o1\",\"activity\":\"ship\"}]. Accepted key aliases: case/caseId/case_id/trace, activity/action/task/event/transition")]
    pub log: String,
    #[schemars(description = "Include per-trace fitness detail (default true)")]
    pub include_traces: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScenarioParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "JSON object overriding the initial marking, e.g. {\"staff\": 3}")]
    pub marking: Option<String>,
    #[schemars(description = "JSON object overriding transition rates, e.g. {\"serve\": 30}")]
    pub rates: Option<String>,
    #[schemars(description = "JSON object making a rate vary over time: {\"serve\": [{\"until\": 2, \"value\": 40}]}")]
    pub schedule: Option<String>,
    #[schemars(description = "JSON array of named scenarios to compare, each with its own marking/rates/schedule. When set, the top-level marking/rates/schedule are ignored")]
    pub scenarios: Option<String>,
    #[schemars(description = "How far forward to run, in the model's time unit (default 1)")]
    pub hours: Option<f64>,
    #[schemars(description = "Time points to report (default 60)")]
    pub samples: Option<usize>,
    #[schemars(description = "Independent stochastic runs to average (default 1)")]
    pub realizations: Option<usize>,
    #[schemars(description = "Random seed (default 1)")]
    pub seed: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExtendParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "JSON array of operations: add_place, add_transition, add_arc, add_event, add_event_field, add_binding, remove_place, remove_transition, remove_arc, remove_event, remove_binding")]
    pub operations: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DiffParams {
    #[schemars(description = "First model as JSON or DSL (the 'before'/'base' model)")]
    pub model_a: String,
    #[schemars(description = "Second model as JSON or DSL (the 'after'/'new' model)")]
    pub model_b: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DatasetParams {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
    #[schemars(description = "Cases to generate (default 200)")]
    pub cases: Option<usize>,
    #[schemars(description = "Horizon per case, in the model's time unit (default 1.0)")]
    pub hours: Option<f64>,
    #[schemars(description = "PRNG seed (default 1)")]
    pub seed: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PflowServer {
    tool_router: ToolRouter<Self>,
}

impl Default for PflowServer {
    fn default() -> Self {
        Self::new()
    }
}

impl PflowServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl PflowServer {
    #[tool(
        description = "Parse a Petri net model and return its topology: places, transitions, arcs, and initial state"
    )]
    async fn pflow_build(
        &self,
        Parameters(params): Parameters<ModelOnly>,
    ) -> Result<String, String> {
        let model = params.model;
        tokio::task::spawn_blocking(move || tools::build::run(&model))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Validate a Petri net model (DSL or JSON), return structure summary and content-addressed ID (CID)"
    )]
    async fn petri_validate(
        &self,
        Parameters(params): Parameters<ModelOnly>,
    ) -> Result<String, String> {
        let model = params.model;
        tokio::task::spawn_blocking(move || tools::validate::run(&model))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Fire discrete transitions step-by-step on a Petri net. Returns token state after each step and which transitions are enabled"
    )]
    async fn pflow_fire(
        &self,
        Parameters(params): Parameters<FireParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let actions = params.actions;
        tokio::task::spawn_blocking(move || tools::fire::run(&model, &actions))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Run ODE simulation of a Petri net over time. Returns downsampled time series for all places"
    )]
    async fn petri_simulate(
        &self,
        Parameters(params): Parameters<SimulateParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let t_end = params.t_end.unwrap_or(100.0);
        let max_points = params.max_points.unwrap_or(100);
        tokio::task::spawn_blocking(move || tools::simulate::run(&model, t_end, max_points))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Find the equilibrium (steady state) of a Petri net ODE system. Reports whether equilibrium was reached and the final state"
    )]
    async fn pflow_equilibrium(
        &self,
        Parameters(params): Parameters<EquilibriumParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let t_max = params.t_max.unwrap_or(1000.0);
        tokio::task::spawn_blocking(move || tools::equilibrium::run(&model, t_max))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Run the portable Gillespie SSA (discrete-stochastic, byte-exact across go-pflow/pflow-rs/pflow-xyz/pflow-jl for a given seed). Returns the ensemble mean and stddev per place on a fixed time grid"
    )]
    async fn petri_stochastic(
        &self,
        Parameters(params): Parameters<StochasticParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let horizon = params.horizon.unwrap_or(10.0);
        let samples = params.samples.unwrap_or(101);
        let realizations = params.realizations.unwrap_or(10);
        let seed = params.seed.unwrap_or(1);
        let rates = params.rates.unwrap_or_default();
        tokio::task::spawn_blocking(move || {
            tools::stochastic::run(&model, horizon, samples, realizations, seed, &rates)
        })
        .await
        .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Analyze Petri net structure: incidence matrix (input/output/delta per transition), enabled transitions at initial marking"
    )]
    async fn petri_analyze(
        &self,
        Parameters(params): Parameters<ModelOnly>,
    ) -> Result<String, String> {
        let model = params.model;
        tokio::task::spawn_blocking(move || tools::analyze::run(&model))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Check whether a Petri net model satisfies stated correctness properties. Returns proved/refuted/unknown per property, each with the proof method (structural/exhaustive/witness) and a replayable firing sequence on refutation. Shorthand strings: deadlock-free, bounded, live, terminating, conserves, reachable:a=1,b=1, unreachable:a=1,b=1, mutex:p1,p2 (or mutex:p1,p2<=2), or a bare linear expression like \"a + 2*b == 10\". Object form: {\"kind\":\"invariant\",\"expr\":\"...\"}, {\"kind\":\"unreachable\",\"target\":{...}}, {\"kind\":\"mutual-exclusion\",\"places\":[...],\"bound\":1}"
    )]
    async fn petri_verify(
        &self,
        Parameters(params): Parameters<VerifyParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let properties = params.properties;
        let max_states = params.max_states;
        tokio::task::spawn_blocking(move || tools::verify::run(&model, &properties, max_states))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Compute the model's minimal-support P-invariants (conservation laws over places) and T-invariants (firing-count cycles), plus whether every place is covered by some invariant (a sufficient condition for structural boundedness)"
    )]
    async fn petri_invariants(
        &self,
        Parameters(params): Parameters<ModelOnly>,
    ) -> Result<String, String> {
        let model = params.model;
        tokio::task::spawn_blocking(move || tools::invariants::run(&model))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Check how well a Petri net model matches real observed behavior, by replaying an event log against it. Returns fitness (can the model reproduce the observed traces?), precision (does the model allow behavior never observed?), their F-score, and per-trace diagnostics naming activities that could not be replayed"
    )]
    async fn petri_conformance(
        &self,
        Parameters(params): Parameters<ConformanceParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let log = params.log;
        let include_traces = params.include_traces.unwrap_or(true);
        tokio::task::spawn_blocking(move || tools::conformance::run(&model, &log, include_traces))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Answer a what-if about a Petri net: override the initial marking, rates, or a rate schedule, then run it forward on the model's own stochastic engine. Returns a trajectory summary — final state, throughput, depletion, contention — per named scenario. Use `scenarios` (a JSON array) to compare several on one seed"
    )]
    async fn petri_scenario(
        &self,
        Parameters(params): Parameters<ScenarioParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let marking = parse_json_map_i64(params.marking.as_deref())?;
        let rates = parse_json_map_f64(params.rates.as_deref())?;
        let schedule = parse_json_map_schedule(params.schedule.as_deref())?;
        let scenarios = parse_json_scenarios(params.scenarios.as_deref())?;
        let hours = params.hours.unwrap_or(1.0);
        let samples = params.samples.unwrap_or(60);
        let realizations = params.realizations.unwrap_or(1);
        let seed = params.seed.unwrap_or(1);
        tokio::task::spawn_blocking(move || {
            tools::scenario::run(&model, marking, rates, schedule, scenarios, hours, samples, realizations, seed)
        })
        .await
        .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Modify an existing Petri net model by applying structural operations. Operations: add_place, add_transition, add_arc, add_event, add_event_field, add_binding, remove_place, remove_transition, remove_arc, remove_event, remove_binding. Returns the modified model plus which operations applied and which failed"
    )]
    async fn petri_extend(
        &self,
        Parameters(params): Parameters<ExtendParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let operations = params.operations;
        tokio::task::spawn_blocking(move || tools::extend::run(&model, &operations))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Compare two Petri net models and show structural differences: added/removed places, transitions, and arcs"
    )]
    async fn petri_diff(
        &self,
        Parameters(params): Parameters<DiffParams>,
    ) -> Result<String, String> {
        let model_a = params.model_a;
        let model_b = params.model_b;
        tokio::task::spawn_blocking(move || tools::diff::run(&model_a, &model_b))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Compute a content-addressed identifier for a model. NOTE: reports pflow-rs's identity hash, not the ecosystem's URDNA2015+CIDv1 canonical CID (no JSON-LD canonicaliser is ported into this workspace yet)"
    )]
    async fn petri_canonical(
        &self,
        Parameters(params): Parameters<ModelOnly>,
    ) -> Result<String, String> {
        let model = params.model;
        tokio::task::spawn_blocking(move || tools::canonical::run(&model))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Check ordinary lumpability of the CTMC a model's stochastic engine samples: is there a coarser partition of the reachable state space that is itself a valid, exact-answer CTMC? Returns the coarsest such partition found (bounded by max_states)"
    )]
    async fn petri_lumping(
        &self,
        Parameters(params): Parameters<ModelMaxStates>,
    ) -> Result<String, String> {
        let model = params.model;
        let max_states = params.max_states;
        tokio::task::spawn_blocking(move || tools::lumping::run(&model, max_states))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }

    #[tool(
        description = "Generate a synthetic event log from a model by playing out its stochastic engine for `cases` independent realizations. Deterministic (same model/seed/cases -> same bytes). Returns CSV (case_id,activity,timestamp), the shape petri_conformance's log input and pflow-eventlog's CSV reader both understand"
    )]
    async fn petri_dataset(
        &self,
        Parameters(params): Parameters<DatasetParams>,
    ) -> Result<String, String> {
        let model = params.model;
        let cases = params.cases.unwrap_or(200);
        let hours = params.hours.unwrap_or(1.0);
        let seed = params.seed.unwrap_or(1);
        tokio::task::spawn_blocking(move || tools::dataset::run(&model, cases, hours, seed))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }
}

fn parse_json_map_i64(s: Option<&str>) -> Result<std::collections::HashMap<String, i64>, String> {
    match s {
        None | Some("") => Ok(std::collections::HashMap::new()),
        Some(s) => serde_json::from_str(s).map_err(|e| format!("invalid marking JSON: {e}")),
    }
}

fn parse_json_map_f64(s: Option<&str>) -> Result<std::collections::HashMap<String, f64>, String> {
    match s {
        None | Some("") => Ok(std::collections::HashMap::new()),
        Some(s) => serde_json::from_str(s).map_err(|e| format!("invalid rates JSON: {e}")),
    }
}

fn parse_json_map_schedule(
    s: Option<&str>,
) -> Result<std::collections::HashMap<String, Vec<pflow_metamodel::RateSegment>>, String> {
    match s {
        None | Some("") => Ok(std::collections::HashMap::new()),
        Some(s) => serde_json::from_str(s).map_err(|e| format!("invalid schedule JSON: {e}")),
    }
}

fn parse_json_scenarios(s: Option<&str>) -> Result<Vec<tools::scenario::ScenarioOverride>, String> {
    match s {
        None | Some("") => Ok(Vec::new()),
        Some(s) => serde_json::from_str(s).map_err(|e| format!("invalid scenarios JSON: {e}")),
    }
}

impl ServerHandler for PflowServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(format!(
                "pflow MCP server: build, simulate, verify, and analyze Petri net models. \
                 Tool names and response shapes mirror petri-pilot's MCP server (petri_*) where a \
                 matching tool exists; pflow_* tools are pflow-rs-only extras. {MODEL_DESC}."
            )),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ToolCallContext::new(self, request, context);
        self.tool_router.call(ctx).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let server = PflowServer::new();
    let transport = (stdin(), stdout());
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
