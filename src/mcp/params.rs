use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowInput {
    #[serde(default = "default_overview")]
    pub phase: String,
}

impl Default for WorkflowInput {
    fn default() -> Self {
        Self {
            phase: default_overview(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ScanInput {
    pub path: Option<String>,
    pub content: Option<String>,
    pub variable_name: Option<String>,
    #[serde(default)]
    pub virtual_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionIdInput {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SliceInput {
    pub session_id: String,
    pub chunk_id: String,
    pub start_line: u64,
    pub end_line: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReplExecuteInput {
    pub session_id: String,
    pub code: String,
    #[serde(default = "default_text")]
    pub language: String,
    #[serde(default)]
    pub backend: ReplBackend,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TransformInput {
    pub session_id: String,
    pub operation: TransformOperation,
    #[serde(default)]
    pub params: Option<Value>,
    pub chunk_id: Option<String>,
    pub artifact_name: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactWriteInput {
    pub session_id: String,
    pub name: String,
    pub content: Option<String>,
    pub source_chunk_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactReadInput {
    pub session_id: String,
    pub name: String,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChunkInput {
    pub session_id: String,
    pub file_pattern: Option<String>,
    pub chunk_ids: Option<Vec<String>>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "default_chunk_limit")]
    pub limit: u64,
    #[serde(default = "default_true")]
    pub include_metadata: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PeekInput {
    pub session_id: String,
    pub query: Option<String>,
    pub path_filter: Option<String>,
    pub glob: Option<String>,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub bm25: bool,
    #[serde(default = "default_true")]
    pub case_sensitive: bool,
    pub line_start: Option<u64>,
    pub line_end: Option<u64>,
    #[serde(default = "default_context_radius")]
    pub context_radius: u64,
    #[serde(default = "default_peek_limit")]
    pub limit: u64,
    #[serde(default)]
    pub include_content: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MapPlanInput {
    pub session_id: String,
    pub chunk_ids: Option<Vec<String>>,
    pub file_pattern: Option<String>,
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MapClaimInput {
    pub plan_id: String,
    pub worker_id: String,
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MapCompleteInput {
    pub plan_id: String,
    pub worker_id: String,
    pub batch_id: String,
    pub output: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ReduceMergeInput {
    #[serde(default)]
    pub worker_outputs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionImportInput {
    pub session: Value,
    #[serde(default)]
    pub preserve_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCreateInput {
    pub session_id: String,
    pub prompt: String,
    #[serde(default)]
    pub chunk_ids: Vec<String>,
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub provider: ProviderName,
    #[serde(default = "default_true")]
    pub execute: bool,
    pub budget: Option<TaskBudgetInput>,
    pub budget_mode: Option<BudgetModeInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TaskListInput {
    pub session_id: Option<String>,
    pub root_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskIdInput {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RootIdInput {
    pub root_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCancelInput {
    pub root_id: String,
    #[serde(default = "default_cancel_reason")]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BudgetConfigureInput {
    pub session_id: String,
    #[serde(default)]
    pub mode: BudgetModeInput,
    #[serde(default = "default_max_chunks_read")]
    pub max_chunks_read: u64,
    #[serde(default = "default_max_sub_calls")]
    pub max_sub_calls: u64,
    #[serde(default = "default_max_total_tokens_est")]
    pub max_total_tokens_est: u64,
    #[serde(default = "default_max_wall_secs")]
    pub max_wall_secs: u64,
    pub task_budget: Option<TaskBudgetInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrajectoryGetInput {
    pub session_id: String,
    #[serde(default)]
    pub format: TrajectoryFormat,
    #[serde(default = "default_true")]
    pub redact: bool,
    #[serde(default)]
    pub redact_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrajectoryFinalInput {
    pub session_id: String,
    pub answer: String,
    #[serde(default)]
    pub evidence_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BenchmarkRunInput {
    #[serde(default)]
    pub suite: BenchmarkSuite,
    #[serde(default)]
    pub fixture_size: FixtureSize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct TaskBudgetInput {
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    #[serde(default = "default_max_fanout")]
    pub max_fanout: u32,
    #[serde(default = "default_max_subcalls")]
    pub max_subcalls: u32,
    #[serde(default = "default_max_input_bytes")]
    pub max_input_bytes: usize,
    #[serde(default = "default_task_max_wall_secs")]
    pub max_wall_secs: u64,
}

impl Default for TaskBudgetInput {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            max_fanout: default_max_fanout(),
            max_subcalls: default_max_subcalls(),
            max_input_bytes: default_max_input_bytes(),
            max_wall_secs: default_task_max_wall_secs(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetModeInput {
    #[default]
    FailFast,
    SoftWarning,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderName {
    #[default]
    Mock,
    DryRun,
    Command,
    Openai,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrajectoryFormat {
    #[default]
    Json,
    Jsonl,
    Replay,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplBackend {
    #[default]
    Command,
    Python,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkSuite {
    #[default]
    Sniah,
    Oolong,
    Codeqa,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FixtureSize {
    #[default]
    Mini,
    Small,
    Large,
    Nightly,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransformOperation {
    #[default]
    DedupeLines,
    SortLines,
    FilterLines,
    HeadLines,
    TailLines,
    TruncateChars,
    AddLineNumbers,
    CountLines,
    NormalizeWhitespace,
}

fn default_overview() -> String {
    "overview".into()
}
fn default_text() -> String {
    "text".into()
}
fn default_true() -> bool {
    true
}
fn default_chunk_limit() -> u64 {
    5
}
fn default_peek_limit() -> u64 {
    20
}
fn default_context_radius() -> u64 {
    2
}
fn default_batch_size() -> u64 {
    3
}
fn default_cancel_reason() -> String {
    "cancelled by agent".into()
}
fn default_max_chunks_read() -> u64 {
    500
}
fn default_max_sub_calls() -> u64 {
    64
}
fn default_max_total_tokens_est() -> u64 {
    500_000
}
fn default_max_wall_secs() -> u64 {
    600
}
fn default_max_depth() -> u32 {
    4
}
fn default_max_fanout() -> u32 {
    8
}
fn default_max_subcalls() -> u32 {
    32
}
fn default_max_input_bytes() -> usize {
    256 * 1024
}
fn default_task_max_wall_secs() -> u64 {
    300
}
