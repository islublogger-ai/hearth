export type Mode = 'chat' | 'agent';
export type Theme = 'system' | 'light' | 'dark';
export interface EndpointConfig { id: string; name: string; kind: 'lmstudio' | 'omlx' | 'ollama' | 'custom'; url: string; enabled: boolean }
export interface Settings {
  theme: Theme; accent: string; contextWindow: number; maxOutputTokens: number; temperature: number;
  systemPrompt: string; memoryEnabled: boolean; networkTools: boolean; workspace: string;
  maxSteps: number; backends: EndpointConfig[]; defaultBackendId: string; defaultModelId: string;
  sendKey: 'enter' | 'cmd_enter'; autoExtract: boolean;
}
export interface Stats { promptTokens: number; completionTokens: number; ttftMs: number; durationMs: number; tokensPerSecond: number; estimated: boolean; backendId: string; modelId: string }
export interface ToolStep { id: string; name: string; args: Record<string, unknown>; output: string; status: 'pending' | 'running' | 'complete' | 'denied' | 'error'; durationMs: number; preview: string }
export interface Message { id: string; role: 'user' | 'assistant'; content: string; thinking: string; createdAt: number; status: 'complete' | 'streaming' | 'stopped' | 'error'; stats?: Stats | null; tools: ToolStep[]; memoriesUsed: string[] }
export interface Conversation { id: string; title: string; mode: Mode; modelId: string; backendId: string; pinned: boolean; incognito: boolean; memoryEnabled: boolean; thinking: boolean; createdAt: number; updatedAt: number; messages: Message[] }
export interface Memory { id: string; text: string; category: 'identity' | 'preference' | 'project' | 'instruction' | 'other'; status: 'active' | 'candidate' | 'archived'; pinned: boolean; enabled: boolean; createdAt: number; updatedAt: number }
export interface ModelInfo { id: string; name: string; backendId: string; canonicalKey: string; loaded: boolean | null; sizeBytes: number | null; contextWindow: number | null; format: string | null }
export interface BackendStatus { id: string; name: string; url: string; online: boolean; error: string | null; models: ModelInfo[] }
export interface SystemInfo { totalMemoryBytes: number; gpuLimitBytes: number | null; chip: string; platform: string }
export interface Bootstrap { conversations: Conversation[]; memories: Memory[]; settings: Settings; system: SystemInfo }
export interface PromptInspection { messages: { role: string; content: string }[]; systemTokens: number; memoryTokens: number; historyTokens: number; outputTokens: number; totalTokens: number; contextWindow: number; estimated: boolean; memoriesUsed: string[] }
export interface GenerationRequest { runId: string; conversation: Conversation; settings: Settings }
export type StreamEvent = { runId: string } & (
  | { type: 'delta'; text: string; thinking: string }
  | { type: 'context'; inspection: PromptInspection }
  | { type: 'tool'; step: ToolStep }
  | { type: 'approval'; approvalId: string; toolName: string; args: Record<string, unknown>; preview: string }
  | { type: 'done'; content: string; thinking: string; stats: Stats; status: 'complete' | 'stopped' }
  | { type: 'error'; message: string }
  | { type: 'status'; message: string }
);
export interface Approval { runId: string; approvalId: string; toolName: string; args: Record<string, unknown>; preview: string }
