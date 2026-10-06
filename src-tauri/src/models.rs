use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointConfig { pub id: String, pub name: String, pub kind: String, pub url: String, pub enabled: bool }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String, pub accent: String, pub context_window: usize, pub max_output_tokens: usize,
    pub temperature: f64, pub system_prompt: String, pub memory_enabled: bool, pub network_tools: bool,
    pub workspace: String, pub max_steps: usize, pub backends: Vec<EndpointConfig>,
    pub default_backend_id: String, pub default_model_id: String, pub send_key: String, pub auto_extract: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(), accent: "#b77543".into(), context_window: 8192, max_output_tokens: 2048,
            temperature: 0.7, system_prompt: "You are Hearth, a helpful assistant running locally on the user's Mac. Be accurate and concise. Use markdown when helpful. If unsure, say so. Never invent facts about the user. Use known user notes only when relevant.".into(),
            memory_enabled: true, network_tools: false, workspace: String::new(), max_steps: 10,
            backends: vec![
                EndpointConfig {id:"lmstudio".into(), name:"LM Studio".into(), kind:"lmstudio".into(), url:"http://127.0.0.1:1234/v1".into(),enabled:true},
                EndpointConfig {id:"omlx".into(),name:"oMLX".into(),kind:"omlx".into(),url:"http://127.0.0.1:8000/v1".into(),enabled:true},
                EndpointConfig {id:"ollama".into(),name:"Ollama".into(),kind:"ollama".into(),url:"http://127.0.0.1:11434/v1".into(),enabled:true},
            ], default_backend_id:"lmstudio".into(),default_model_id:String::new(),send_key:"enter".into(),auto_extract:false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats { pub prompt_tokens: usize, pub completion_tokens: usize, pub ttft_ms: u64, pub duration_ms: u64, pub tokens_per_second: f64, pub estimated: bool, pub backend_id: String, pub model_id: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStep { pub id: String, pub name: String, pub args: Value, pub output: String, pub status: String, pub duration_ms: u64, pub preview: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message { pub id: String, pub role: String, pub content: String, pub thinking: String, pub created_at: i64, pub status: String, pub stats: Option<Stats>, #[serde(default)] pub tools: Vec<ToolStep>, #[serde(default)] pub memories_used: Vec<String> }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation { pub id: String, pub title: String, pub mode: String, pub model_id: String, pub backend_id: String, pub pinned: bool, pub incognito: bool, pub memory_enabled: bool, pub thinking: bool, pub created_at: i64, pub updated_at: i64, #[serde(default)] pub messages: Vec<Message> }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory { pub id: String, pub text: String, pub category: String, pub status: String, pub pinned: bool, pub enabled: bool, pub created_at: i64, pub updated_at: i64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo { pub id: String, pub name: String, pub backend_id: String, pub canonical_key: String, pub loaded: Option<bool>, pub size_bytes: Option<u64>, pub context_window: Option<usize>, pub format: Option<String> }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus { pub id: String, pub name: String, pub url: String, pub online: bool, pub error: Option<String>, pub models: Vec<ModelInfo> }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo { pub total_memory_bytes: u64, pub gpu_limit_bytes: Option<u64>, pub chip: String, pub platform: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap { pub conversations: Vec<Conversation>, pub memories: Vec<Memory>, pub settings: Settings, pub system: SystemInfo }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptMessage { pub role: String, pub content: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInspection { pub messages: Vec<PromptMessage>, pub system_tokens: usize, pub memory_tokens: usize, pub history_tokens: usize, pub output_tokens: usize, pub total_tokens: usize, pub context_window: usize, pub estimated: bool, pub memories_used: Vec<String> }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRequest { pub run_id: String, pub conversation: Conversation, pub settings: Settings }

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventPayload {
    Delta { text: String, thinking: String },
    Context { inspection: PromptInspection },
    Tool { step: ToolStep },
    #[serde(rename_all = "camelCase")]
    Approval { approval_id: String, tool_name: String, args: Value, preview: String },
    Done { content: String, thinking: String, stats: Stats, status: String },
    Error { message: String },
    Status { message: String },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent { pub run_id: String, #[serde(flatten)] pub payload: EventPayload }
