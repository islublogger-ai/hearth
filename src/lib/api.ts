import { Channel, invoke } from '@tauri-apps/api/core';
import type { Approval, BackendStatus, Bootstrap, Conversation, GenerationRequest, Memory, Settings, StreamEvent } from './types';

export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const defaultSettings: Settings = {
  theme: 'system', accent: '#b77543', contextWindow: 8192, maxOutputTokens: 2048, temperature: 0.7,
  systemPrompt: "You are Hearth, a helpful assistant running locally on the user's Mac. Be accurate and concise. Use markdown when helpful. If unsure, say so. Never invent facts about the user. Use known user notes only when relevant.",
  memoryEnabled: true, networkTools: false, workspace: '', maxSteps: 10,
  backends: [
    { id: 'lmstudio', name: 'LM Studio', kind: 'lmstudio', url: 'http://127.0.0.1:1234/v1', enabled: true },
    { id: 'omlx', name: 'oMLX', kind: 'omlx', url: 'http://127.0.0.1:8000/v1', enabled: true },
    { id: 'ollama', name: 'Ollama', kind: 'ollama', url: 'http://127.0.0.1:11434/v1', enabled: true },
  ],
  defaultBackendId: 'lmstudio', defaultModelId: '', sendKey: 'enter', autoExtract: false,
};

interface PreviewData { conversations: Conversation[]; memories: Memory[]; settings: Settings }
const previewKey = 'hearth-preview-v1';
function readPreview(): PreviewData {
  const empty = { conversations: [], memories: [], settings: structuredClone(defaultSettings) };
  try { const parsed = JSON.parse(localStorage.getItem(previewKey) ?? 'null'); return parsed ? { ...empty, ...parsed, settings: { ...empty.settings, ...parsed.settings } } : empty; }
  catch { return empty; }
}
function writePreview(data: PreviewData) { localStorage.setItem(previewKey, JSON.stringify(data)); }
export function id() { return crypto.randomUUID(); }
export function newConversation(settings: Settings, incognito = false): Conversation {
  const now = Date.now();
  return { id: id(), title: 'New conversation', mode: 'chat', modelId: settings.defaultModelId, backendId: settings.defaultBackendId, pinned: false, incognito, memoryEnabled: !incognito && settings.memoryEnabled, thinking: false, createdAt: now, updatedAt: now, messages: [] };
}
export async function bootstrap(): Promise<Bootstrap> {
  if (isDesktop) return invoke('bootstrap');
  return { ...readPreview(), system: { totalMemoryBytes: 0, gpuLimitBytes: null, chip: 'Browser preview', platform: 'preview' } };
}
export async function saveConversation(conversation: Conversation): Promise<void> {
  if (conversation.incognito) return;
  if (isDesktop) return invoke('save_conversation', { conversation });
  const data = readPreview(); const index = data.conversations.findIndex(c => c.id === conversation.id);
  if (index < 0) data.conversations.unshift(conversation); else data.conversations[index] = conversation;
  writePreview(data);
}
export async function deleteConversation(conversationId: string): Promise<void> {
  if (isDesktop) return invoke('delete_conversation', { conversationId });
  const data = readPreview(); data.conversations = data.conversations.filter(c => c.id !== conversationId); writePreview(data);
}
export async function saveMemory(memory: Memory): Promise<void> {
  if (isDesktop) return invoke('save_memory', { memory });
  const data = readPreview(); const index = data.memories.findIndex(m => m.id === memory.id);
  if (index < 0) data.memories.unshift(memory); else data.memories[index] = memory; writePreview(data);
}
export async function deleteMemory(memoryId: string): Promise<void> {
  if (isDesktop) return invoke('delete_memory', { memoryId });
  const data = readPreview(); data.memories = data.memories.filter(m => m.id !== memoryId); writePreview(data);
}
export async function saveSettings(settings: Settings): Promise<void> {
  if (isDesktop) return invoke('save_settings', { settings });
  const data = readPreview(); data.settings = settings; writePreview(data);
}
export async function discoverBackends(settings: Settings): Promise<BackendStatus[]> {
  if (isDesktop) return invoke('discover_backends', { backends: settings.backends });
  return settings.backends.filter(b => b.enabled).map(b => ({ id: b.id, name: b.name, url: b.url, online: false, error: 'Connect to local models in the desktop app.', models: [] }));
}
export async function searchConversations(query: string): Promise<string[]> {
  if (isDesktop) return invoke('search_conversations', { query });
  const term = query.toLowerCase(); return readPreview().conversations.filter(c => c.title.toLowerCase().includes(term) || c.messages.some(m => m.content.toLowerCase().includes(term))).map(c => c.id);
}
export async function generate(request: GenerationRequest, onEvent: (event: StreamEvent) => void): Promise<void> {
  if (!isDesktop) throw new Error('Open the Hearth desktop app to generate with a local model. Browser preview supports conversations, memory and settings.');
  const onEventChannel = new Channel<StreamEvent>(); onEventChannel.onmessage = onEvent;
  return invoke('generate', { request, onEvent: onEventChannel });
}
export async function stopGeneration(runId: string): Promise<void> { if (isDesktop) return invoke('stop_generation', { runId }); }
export async function approveTool(approval: Pick<Approval, 'runId' | 'approvalId'>, allow: boolean): Promise<void> {
  if (isDesktop) return invoke('approve_tool', { runId: approval.runId, approvalId: approval.approvalId, allow });
}
export async function exportText(filename: string, content: string): Promise<string> {
  if (isDesktop) return invoke('export_text', { filename, content });
  const blob = new Blob([content], { type: filename.endsWith('.json') ? 'application/json' : 'text/markdown' });
  const url = URL.createObjectURL(blob); const a = document.createElement('a'); a.href = url; a.download = filename; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); return filename;
}
export function markdownExport(conversation: Conversation): string {
  return `# ${conversation.title}\n\n` + conversation.messages.map(m => `## ${m.role === 'user' ? 'You' : 'Hearth'}\n\n${m.content}\n`).join('\n');
}
export async function openExternal(url: string): Promise<void> {
  if (isDesktop) return invoke('open_external', { url });
  const parsed = new URL(url); if (!['https:', 'http:'].includes(parsed.protocol)) throw new Error('Only web links may be opened.');
  window.open(url, '_blank', 'noopener,noreferrer');
}
