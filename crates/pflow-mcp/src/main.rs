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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ModelOnly {
    #[schemars(description = "Petri net model as DSL S-expression or JSON schema")]
    pub model: String,
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

#[derive(Debug, Clone)]
pub struct PflowServer {
    tool_router: ToolRouter<Self>,
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
    async fn pflow_validate(
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
    async fn pflow_simulate(
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
        description = "Analyze Petri net structure: incidence matrix (input/output/delta per transition), enabled transitions at initial marking"
    )]
    async fn pflow_analyze(
        &self,
        Parameters(params): Parameters<ModelOnly>,
    ) -> Result<String, String> {
        let model = params.model;
        tokio::task::spawn_blocking(move || tools::analyze::run(&model))
            .await
            .map_err(|e| format!("Task error: {e}"))?
    }
}

impl ServerHandler for PflowServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "pflow MCP server: build, simulate, and analyze Petri net models. \
                 Pass models as DSL S-expressions (starting with '(') or JSON schemas."
                    .into(),
            ),
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
