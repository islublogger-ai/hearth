<script lang="ts">
  import { onMount } from 'svelte';
  import {
    approveTool, bootstrap, defaultSettings, deleteConversation, deleteMemory, discoverBackends,
    exportText, generate, id, isDesktop, markdownExport, newConversation, openExternal,
    saveConversation, saveMemory, saveSettings, searchConversations, stopGeneration,
  } from './lib/api';
  import { createCheckpoint } from './lib/components/checkpoint';
  import { endpointError } from './lib/components/endpoint';
  import { fitLabel, hardwareLine, loadedLabel } from './lib/components/format';
  import { sendsOnEnter } from './lib/components/keys';
  import { renderMarkdown } from './lib/markdown';
  import type {
    Approval, BackendStatus, Conversation, EndpointConfig, Memory, Message, PromptInspection,
    Settings, StreamEvent, SystemInfo, ToolStep,
  } from './lib/types';

  type Tab = 'memory' | 'context' | 'tools';
  type Command = { id: string; label: string; hint: string; disabled: boolean; run: () => void };

  let ready = $state(false);
  let loadError = $state('');
  let conversations = $state<Conversation[]>([]);
  let memories = $state<Memory[]>([]);
  let settings = $state<Settings>(structuredClone(defaultSettings));
  let system = $state<SystemInfo>({ totalMemoryBytes: 0, gpuLimitBytes: null, chip: '', platform: '' });
  let selectedId = $state<string | null>(null);
  let backends = $state<BackendStatus[]>([]);
  let discovery = $state<'idle' | 'checking' | 'ready' | 'failed'>('idle');
  let discoverError = $state('');
  let searchQuery = $state('');
  let matchIds = $state<string[] | null>(null);
  let draft = $state('');
  let editingIndex = $state<number | null>(null);
  let activeRunId = $state<string | null>(null);
  let stopRequested = $state(false);
  let approving = $state(false);
  let pending = $state<Approval | null>(null);
  let failures = $state<Record<string, string>>({});
  let inspections = $state<Record<string, PromptInspection>>({});
  let live = $state('');
  let settingsOpen = $state(false);
  let settingsDraft = $state<Settings | null>(null);
  let settingsError = $state('');
  let paletteOpen = $state(false);
  let paletteQuery = $state('');
  let paletteIndex = $state(0);
  let sideOpen = $state(false);
  let inspectorOpen = $state(false);
  let tab = $state<Tab>('memory');
  let pendingDeleteId = $state<string | null>(null);
  let renamingId = $state<string | null>(null);
  let renameValue = $state('');
  let memoryText = $state('');
  let memoryCategory = $state<Memory['category']>('other');
  let memoryEditId = $state<string | null>(null);
  let memoryEditText = $state('');
  let runError = $state('');
  let memoryDeleteId = $state<string | null>(null);
  let toast = $state<{ text: string; undo?: () => Promise<void> } | null>(null);
  let scroller = $state<HTMLDivElement | null>(null);
  let paletteInput = $state<HTMLInputElement | null>(null);
  let toastToken = 0;
  let restoreFocus = $state<HTMLElement | null>(null);
  let searchSeq = 0;
  let discoverSeq = 0;

  const selected = $derived(conversations.find((item) => item.id === selectedId) ?? null);
  const inspection = $derived(selected ? inspections[selected.id] ?? null : null);
  const models = $derived(backends.flatMap((backend) => backend.models.map((model) => ({ ...model, backendName: backend.name }))));
  const currentModel = $derived(models.find((model) => model.id === selected?.modelId && model.backendId === selected?.backendId) ?? null);
  const fit = $derived(currentModel ? fitLabel(currentModel.sizeBytes, system.totalMemoryBytes) : null);
  const memoryLocked = $derived(Boolean(selected?.incognito));
  const visibleChats = $derived.by(() => {
    const sorted = [...conversations].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updatedAt - a.updatedAt);
    if (!searchQuery.trim() || matchIds === null) return sorted;
    const ids = new Set(matchIds);
    return sorted.filter((item) => ids.has(item.id));
  });
  const pinnedChats = $derived(visibleChats.filter((item) => item.pinned));
  const recentChats = $derived(visibleChats.filter((item) => !item.pinned));
  const categories: Memory['category'][] = ['identity', 'preference', 'project', 'instruction', 'other'];

  function plain<T>(value: T): T {
    return structuredClone($state.snapshot(value)) as T;
  }

  function showToast(text: string, undo?: () => Promise<void>) {
    const token = ++toastToken;
    toast = { text, undo };
    setTimeout(() => { if (token === toastToken) toast = null; }, 7000);
  }

  function normalizeMessage(message: Message): Message {
    return {
      ...message,
      content: message.content ?? '',
      thinking: message.thinking ?? '',
      tools: message.tools ?? [],
      memoriesUsed: message.memoriesUsed ?? [],
      status: message.status === 'streaming' ? 'stopped' : message.status,
    };
  }

  function messageOf(conversationId: string, messageId: string): Message | undefined {
    return conversations.find((item) => item.id === conversationId)?.messages.find((item) => item.id === messageId);
  }

  function lastUserIndex(messages: Message[]): number {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index].role === 'user') return index;
    }
    return -1;
  }

  function titleFrom(text: string): string {
    const line = text.trim().split('\n')[0]?.replace(/\s+/g, ' ') ?? '';
    return line.slice(0, 48) || 'New conversation';
  }

  function exportName(title: string, extension: 'md' | 'json'): string {
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 48);
    return `${slug || 'conversation'}.${extension}`;
  }

  function safeAccent(value: string): string {
    return /^#[0-9a-fA-F]{6}$/.test(value) ? value : '#b77543';
  }

  function workspaceIssue(path: string): string | null {
    const value = path.trim();
    if (!value) return null;
    if (!value.startsWith('/') || value.includes('\0') || value.split('/').includes('..')) {
      return 'Workspace must be an absolute path, without .., to a folder that already exists.';
    }
    return null;
  }

  function validateSettings(next: Settings): string | null {
    if (!['system', 'light', 'dark'].includes(next.theme)) return 'Choose system, light, or dark.';
    if (!/^#[0-9a-fA-F]{6}$/.test(next.accent)) return 'Accent must be a 6-digit hex color.';
    if (!Number.isInteger(next.contextWindow) || next.contextWindow < 512 || next.contextWindow > 131072) {
      return 'Context window must be a whole number from 512 to 131072.';
    }
    if (!Number.isInteger(next.maxOutputTokens) || next.maxOutputTokens < 16 || next.maxOutputTokens >= next.contextWindow) {
      return 'Output tokens must be a whole number from 16 up to, but not including, the context window.';
    }
    if (!Number.isInteger(next.maxSteps) || next.maxSteps < 1 || next.maxSteps > 20) return 'Max steps must be from 1 to 20.';
    if (!Number.isFinite(next.temperature) || next.temperature < 0 || next.temperature > 2) return 'Temperature must be between 0 and 2.';
    const workspace = workspaceIssue(next.workspace);
    if (workspace) return workspace;
    for (const endpoint of next.backends) {
      const issue = endpointError(endpoint.url);
      if (issue) return `${endpoint.name}: ${issue}`;
    }
    return null;
  }

  function pillFor(endpoint: EndpointConfig): { detail: string; on: boolean } {
    if (!endpoint.enabled) return { detail: 'Disabled', on: false };
    const status = backends.find((item) => item.id === endpoint.id);
    if (!status) return { detail: discovery === 'checking' ? 'Checking…' : 'Not checked', on: false };
    if (status.online) return { detail: status.models.length === 1 ? '1 model' : `${status.models.length} models`, on: true };
    return { detail: status.error ?? 'Offline', on: false };
  }

  async function persist(conversation: Conversation) {
    if (conversation.incognito) return;
    await saveConversation(plain(conversation));
  }

  const saver = createCheckpoint((conversationId) => {
    const conversation = conversations.find((item) => item.id === conversationId);
    if (!conversation || conversation.incognito) return;
    void saveConversation(plain(conversation)).catch((error: unknown) => {
      live = error instanceof Error ? error.message : 'Could not save the conversation.';
    });
  });

  async function rediscover(source: Settings = settings) {
    const seq = ++discoverSeq;
    discovery = 'checking';
    try {
      const list = await discoverBackends(plain(source));
      if (seq !== discoverSeq) return;
      backends = list;
      discovery = 'ready';
      discoverError = '';
    } catch (error) {
      if (seq !== discoverSeq) return;
      discovery = 'failed';
      discoverError = error instanceof Error ? error.message : 'Could not check servers.';
    }
  }

  async function load() {
    try {
      const data = await bootstrap();
      settings = {
        ...structuredClone(defaultSettings),
        ...data.settings,
        backends: data.settings.backends?.length ? data.settings.backends : structuredClone(defaultSettings.backends),
      };
      memories = data.memories ?? [];
      conversations = (data.conversations ?? []).map((conversation) => ({
        ...conversation,
        messages: (conversation.messages ?? []).map(normalizeMessage),
      }));
      system = data.system;
      for (const conversation of conversations) {
        const original = (data.conversations ?? []).find((item) => item.id === conversation.id);
        if (original?.messages?.some((message) => message.status === 'streaming')) {
          try { await persist(conversation); } catch { /* The local label is already stopped. */ }
        }
      }
      selectedId = [...conversations].sort((a, b) => b.updatedAt - a.updatedAt)[0]?.id ?? null;
    } catch (error) {
      loadError = error instanceof Error ? error.message : 'Could not open the library.';
    } finally {
      ready = true;
      if (!loadError) void rediscover();
    }
  }

  function selectChat(next: string) {
    if (activeRunId && next !== selectedId) {
      live = 'Stop the reply before switching chats.';
      return;
    }
    if (next === selectedId) return;
    runError = '';
    selectedId = next;
    editingIndex = null;
    draft = '';
    sideOpen = false;
  }

  async function createChat(incognito = false) {
    if (activeRunId) {
      live = 'Stop the reply before starting another chat.';
      return;
    }
    runError = '';
    const conversation = newConversation(settings, incognito);
    conversations = [conversation, ...conversations];
    selectedId = conversation.id;
    draft = '';
    editingIndex = null;
    paletteOpen = false;
    sideOpen = false;
    if (!incognito) {
      try { await persist(conversation); }
      catch (error) { live = error instanceof Error ? error.message : 'Could not save the conversation.'; }
    }
  }

  async function commitRename(conversation: Conversation) {
    if (activeRunId) return;
    const title = renameValue.trim();
    renamingId = null;
    if (!title || title === conversation.title) return;
    conversation.title = title;
    conversation.updatedAt = Date.now();
    try { await persist(conversation); }
    catch (error) { live = error instanceof Error ? error.message : 'Could not rename the conversation.'; }
  }

  async function togglePin(conversation: Conversation) {
    if (activeRunId) return;
    conversation.pinned = !conversation.pinned;
    conversation.updatedAt = Date.now();
    try { await persist(conversation); }
    catch (error) { live = error instanceof Error ? error.message : 'Could not update the pin.'; }
  }

  async function confirmDelete(conversation: Conversation) {
    if (activeRunId) {
      live = 'Stop the reply before deleting a chat.';
      return;
    }
    pendingDeleteId = null;
    const snapshot = plain(conversation);
    conversations = conversations.filter((item) => item.id !== conversation.id);
    if (selectedId === conversation.id) selectedId = conversations[0]?.id ?? null;
    if (!snapshot.incognito) {
      try { await deleteConversation(snapshot.id); }
      catch (error) {
        conversations = [snapshot, ...conversations];
        selectedId = snapshot.id;
        live = error instanceof Error ? error.message : 'Could not delete the conversation.';
        return;
      }
    }
    showToast('Conversation deleted.', async () => {
      conversations = [snapshot, ...conversations];
      selectedId = snapshot.id;
      if (!snapshot.incognito) await saveConversation(snapshot);
    });
  }

  async function patchSelected(mutate: (conversation: Conversation) => void) {
    const conversation = selected;
    if (!conversation || activeRunId) return;
    mutate(conversation);
    conversation.updatedAt = Date.now();
    try { await persist(conversation); }
    catch (error) { live = error instanceof Error ? error.message : 'Could not save the conversation.'; }
  }

  async function setIncognito(next: boolean) {
    const conversation = selected;
    if (!conversation || activeRunId || next === conversation.incognito) return;
    if (next) {
      const memoryEnabled = conversation.memoryEnabled;
      conversation.incognito = true;
      conversation.memoryEnabled = false;
      try { await deleteConversation(conversation.id); }
      catch (error) {
        conversation.incognito = false;
        conversation.memoryEnabled = memoryEnabled;
        live = error instanceof Error ? error.message : 'Could not remove the saved copy.';
      }
      return;
    }
    conversation.incognito = false;
    conversation.memoryEnabled = settings.memoryEnabled;
    try { await persist(conversation); }
    catch (error) {
      conversation.incognito = true;
      conversation.memoryEnabled = false;
      live = error instanceof Error ? error.message : 'Could not save the conversation.';
    }
  }

  function onModelChange(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (!/^\d+$/.test(value)) return;
    const model = models[Number(value)];
    if (!model) return;
    void patchSelected((conversation) => {
      conversation.modelId = model.id;
      conversation.backendId = model.backendId;
    });
  }

  function applyEvent(token: string, conversationId: string, assistantId: string, event: StreamEvent) {
    if (event.runId !== token || activeRunId !== token) return;
    const message = messageOf(conversationId, assistantId);
    if (!message || message.status !== 'streaming') return;
    if (event.type === 'delta') {
      message.content += event.text;
      message.thinking += event.thinking;
      message.status = 'streaming';
      if (live === 'Starting…') live = 'Generating';
    } else if (event.type === 'context') {
      inspections = { ...inspections, [conversationId]: event.inspection };
      message.memoriesUsed = event.inspection.memoriesUsed;
    } else if (event.type === 'tool') {
      const index = message.tools.findIndex((step) => step.id === event.step.id);
      message.tools = index < 0 ? [...message.tools, event.step] : message.tools.map((step, stepIndex) => stepIndex === index ? event.step : step);
    } else if (event.type === 'approval') {
      pending = { runId: event.runId, approvalId: event.approvalId, toolName: event.toolName, args: event.args, preview: event.preview };
      live = `Approval needed for ${event.toolName}`;
    } else if (event.type === 'done') {
      if (event.content) message.content = event.content;
      if (event.thinking) message.thinking = event.thinking;
      message.stats = event.stats;
      message.status = event.status;
      if (pending?.runId === event.runId) pending = null;
      live = event.status === 'stopped' ? 'Stopped' : 'Finished';
    } else if (event.type === 'error') {
      message.status = 'error';
      failures = { ...failures, [message.id]: event.message };
      if (pending?.runId === event.runId) pending = null;
      live = event.message;
    } else {
      live = event.message;
    }
    saver.schedule(conversationId);
  }

  async function beginRun(conversation: Conversation) {
    if (activeRunId || stopRequested) return;
    const assistant: Message = {
      id: id(), role: 'assistant', content: '', thinking: '', createdAt: Date.now(), status: 'streaming', tools: [], memoriesUsed: [],
    };
    conversation.messages = [...conversation.messages, assistant];
    conversation.updatedAt = Date.now();
    const token = id();
    const conversationId = conversation.id;
    activeRunId = token;
    pending = null;
    runError = '';
    live = 'Starting…';
    const requestConversation = plain(conversation);
    if (requestConversation.mode === 'agent') requestConversation.thinking = false;
    requestConversation.messages = requestConversation.messages.filter((message) => message.id !== assistant.id);
    try {
      if (!conversation.incognito) await saveConversation(requestConversation);
      await generate({ runId: token, conversation: requestConversation, settings: plain(settings) }, (event) => applyEvent(token, conversationId, assistant.id, event));
    } catch (error) {
      const message = messageOf(conversationId, assistant.id);
      const text = error instanceof Error ? error.message : 'Generation failed.';
      if (message && message.status === 'streaming') message.status = 'error';
      if (message) failures = { ...failures, [message.id]: text };
      live = text;
    } finally {
      const message = messageOf(conversationId, assistant.id);
      if (message?.status === 'streaming') {
        message.status = 'error';
        failures = { ...failures, [assistant.id]: failures[assistant.id] ?? 'The run ended before a final status arrived.' };
      }
      if (activeRunId === token) activeRunId = null;
      stopRequested = false;
      pending = null;
      saver.flush();
      const latest = conversations.find((item) => item.id === conversationId);
      if (latest && !latest.incognito) {
        try { await saveConversation(plain(latest)); }
        catch (error) { live = error instanceof Error ? error.message : 'Could not save the conversation.'; }
      }
    }
  }

  async function submit() {
    const conversation = selected;
    if (!conversation || activeRunId || stopRequested) return;
    const text = draft.trim();
    if (!text) return;
    if (editingIndex !== null) {
      conversation.messages = conversation.messages.slice(0, editingIndex);
      editingIndex = null;
    }
    if (conversation.title === 'New conversation') conversation.title = titleFrom(text);
    const user: Message = {
      id: id(), role: 'user', content: text, thinking: '', createdAt: Date.now(), status: 'complete', tools: [], memoriesUsed: [],
    };
    conversation.messages = [...conversation.messages, user];
    draft = '';
    await beginRun(conversation);
  }

  async function regenerate() {
    const conversation = selected;
    if (!conversation || activeRunId) return;
    const last = conversation.messages[conversation.messages.length - 1];
    if (!last || last.role !== 'assistant') return;
    conversation.messages = conversation.messages.slice(0, -1);
    await beginRun(conversation);
  }

  function beginEdit() {
    const conversation = selected;
    if (!conversation || activeRunId) return;
    const index = lastUserIndex(conversation.messages);
    if (index < 0) return;
    editingIndex = index;
    draft = conversation.messages[index].content;
  }

  async function stop() {
    if (!activeRunId || stopRequested) return;
    stopRequested = true;
    try { await stopGeneration(activeRunId); }
    catch (error) {
      stopRequested = false;
      live = error instanceof Error ? error.message : 'Could not stop.';
    }
  }

  async function resolveApproval(allow: boolean) {
    if (!pending || approving) return;
    approving = true;
    try {
      await approveTool({ runId: pending.runId, approvalId: pending.approvalId }, allow);
      pending = null;
    } catch (error) {
      live = error instanceof Error ? error.message : 'Could not answer the approval.';
    } finally {
      approving = false;
    }
  }

  async function remember(message: Message) {
    const conversation = selected;
    if (!conversation || conversation.incognito || !settings.memoryEnabled || !conversation.memoryEnabled) return;
    const text = message.content.trim();
    if (!text) return;
    if (text.length > 4000) {
      live = 'That message is too long to store as one memory. Save a shorter note in the inspector.';
      tab = 'memory';
      inspectorOpen = true;
      return;
    }
    const now = Date.now();
    const memory: Memory = { id: id(), text, category: 'other', status: 'active', pinned: false, enabled: true, createdAt: now, updatedAt: now };
    try {
      await saveMemory(memory);
      memories = [memory, ...memories];
      live = 'Memory saved.';
    } catch (error) {
      live = error instanceof Error ? error.message : 'Could not save the memory.';
    }
  }

  async function copyText(text: string) {
    try { await navigator.clipboard.writeText(text); live = 'Copied.'; }
    catch { live = 'Could not copy.'; }
  }

  async function exportConversation(kind: 'md' | 'json') {
    const conversation = selected;
    if (!conversation || conversation.incognito) return;
    const snapshot = plain(conversation);
    const content = kind === 'md' ? markdownExport(snapshot) : JSON.stringify(snapshot, null, 2);
    try { live = `Exported ${await exportText(exportName(snapshot.title, kind), content)}`; }
    catch (error) { live = error instanceof Error ? error.message : 'Export failed.'; }
  }

  function openSettings() {
    restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    settingsDraft = plain(settings);
    settingsError = '';
    settingsOpen = true;
    paletteOpen = false;
  }

  function closeSettings() {
    settingsOpen = false;
    settingsDraft = null;
    const back = restoreFocus;
    restoreFocus = null;
    queueMicrotask(() => back?.focus());
    void rediscover(settings);
  }

  async function commitSettings() {
    if (!settingsDraft) return;
    const next = plain(settingsDraft);
    next.workspace = next.workspace.trim();
    next.accent = next.accent.trim();
    next.contextWindow = Number(next.contextWindow);
    next.maxOutputTokens = Number(next.maxOutputTokens);
    next.temperature = Number(next.temperature);
    next.maxSteps = Number(next.maxSteps);
    next.backends = next.backends.map((endpoint) => ({ ...endpoint, url: endpoint.url.trim() }));
    next.networkTools = false;
    next.autoExtract = false;
    const problem = validateSettings(next);
    if (problem) { settingsError = problem; return; }
    try {
      await saveSettings(next);
      settings = next;
      settingsOpen = false;
      settingsDraft = null;
      settingsError = '';
      const back = restoreFocus;
      restoreFocus = null;
      queueMicrotask(() => back?.focus());
      await rediscover(next);
      live = 'Settings saved.';
    } catch (error) {
      settingsError = error instanceof Error ? error.message : 'Could not save settings.';
    }
  }

  async function saveManualMemory() {
    if (memoryLocked) return;
    const text = memoryText.trim();
    if (!text) return;
    const now = Date.now();
    const memory: Memory = { id: id(), text, category: memoryCategory, status: 'active', pinned: false, enabled: true, createdAt: now, updatedAt: now };
    try {
      await saveMemory(memory);
      memories = [memory, ...memories];
      memoryText = '';
    } catch (error) {
      live = error instanceof Error ? error.message : 'Could not save the memory.';
    }
  }

  async function updateMemory(memory: Memory, patch: Partial<Memory>) {
    if (memoryLocked) return;
    const next: Memory = { ...memory, ...patch, updatedAt: Date.now() };
    try {
      await saveMemory(next);
      memories = memories.map((item) => item.id === memory.id ? next : item);
    } catch (error) {
      live = error instanceof Error ? error.message : 'Could not update the memory.';
    }
  }

  async function confirmDeleteMemory(memory: Memory) {
    if (memoryLocked) return;
    memoryDeleteId = null;
    const snapshot = plain(memory);
    try { await deleteMemory(memory.id); }
    catch (error) {
      live = error instanceof Error ? error.message : 'Could not delete the memory.';
      return;
    }
    memories = memories.filter((item) => item.id !== memory.id);
    showToast('Memory deleted.', async () => {
      await saveMemory(snapshot);
      memories = [snapshot, ...memories];
    });
  }

  function onComposerKey(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if (!sendsOnEnter(event, settings.sendKey)) return;
    event.preventDefault();
    if (!activeRunId) void submit();
  }

  function onTabs(event: KeyboardEvent) {
    const names: Tab[] = ['memory', 'context', 'tools'];
    if (!['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const focused = document.activeElement?.id.replace(/^tab-/, '') as Tab | undefined;
    const current = focused && names.includes(focused) ? focused : tab;
    const index = names.indexOf(current);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? names.length - 1 : event.key === 'ArrowRight' ? (index + 1) % names.length : (index - 1 + names.length) % names.length;
    tab = names[next];
    queueMicrotask(() => document.getElementById(`tab-${tab}`)?.focus());
  }

  function onWindowKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      if (settingsOpen) closeSettings();
      else if (paletteOpen) paletteOpen = false;
      else if (pendingDeleteId) pendingDeleteId = null;
      else if (memoryDeleteId) memoryDeleteId = null;
      else if (memoryEditId) memoryEditId = null;
      else if (editingIndex !== null) { editingIndex = null; draft = ''; }
      else return;
      event.preventDefault();
      return;
    }
    if (!(event.metaKey || event.ctrlKey)) return;
    const key = event.key.toLowerCase();
    if (key === 'k') {
      event.preventDefault();
      paletteOpen = !paletteOpen;
      if (paletteOpen) { settingsOpen = false; paletteQuery = ''; paletteIndex = 0; }
      return;
    }
    if (key === ',') {
      event.preventDefault();
      if (settingsOpen) closeSettings(); else openSettings();
      return;
    }
    if (settingsOpen || paletteOpen) return;
    if (key === 'n') { event.preventDefault(); void createChat(false); }
    else if (key === '.') { event.preventDefault(); void stop(); }
  }

  function bindMarkdown(node: HTMLElement) {
    const onClick = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const copy = target.closest('button.copy-code');
      if (copy && node.contains(copy)) {
        event.preventDefault();
        const code = copy.closest('.code-block')?.querySelector('code')?.textContent ?? '';
        void navigator.clipboard.writeText(code).then(() => { copy.textContent = 'Copied'; }).catch(() => { copy.textContent = 'Copy failed'; });
        return;
      }
      const anchor = target.closest('a');
      if (!anchor || !node.contains(anchor)) return;
      event.preventDefault();
      const href = anchor.getAttribute('href') ?? '';
      if (/^https?:\/\//i.test(href)) {
        void openExternal(href).catch((error: unknown) => { live = error instanceof Error ? error.message : 'Could not open that link.'; });
      }
    };
    node.addEventListener('click', onClick);
    return { destroy() { node.removeEventListener('click', onClick); } };
  }

  function trapTab(node: HTMLElement) {
    const items = () => [...node.querySelectorAll<HTMLElement>('button, input, select, textarea')]
      .filter((element) => !element.hasAttribute('disabled'));
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const list = items();
      if (list.length === 0) return;
      const first = list[0];
      const last = list[list.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    node.addEventListener('keydown', onKey);
    (items().find((element) => element.tagName !== 'BUTTON') ?? items()[0])?.focus();
    return { destroy() { node.removeEventListener('keydown', onKey); } };
  }

  function toolClass(step: ToolStep): string {
    if (step.status === 'denied' || step.status === 'error') return 'denied';
    if (step.status === 'complete') return 'complete';
    return '';
  }

  const commands = $derived.by(() => {
    const items: Command[] = [
      { id: 'new', label: 'New conversation', hint: '⌘N', disabled: Boolean(activeRunId), run: () => void createChat(false) },
      { id: 'incognito', label: 'New incognito conversation', hint: '', disabled: Boolean(activeRunId), run: () => void createChat(true) },
      { id: 'settings', label: 'Settings', hint: '⌘,', disabled: false, run: openSettings },
      { id: 'stop', label: 'Stop reply', hint: '⌘.', disabled: !activeRunId, run: () => void stop() },
      { id: 'discover', label: 'Rediscover servers', hint: '', disabled: false, run: () => void rediscover() },
      { id: 'md', label: 'Export Markdown', hint: '', disabled: !selected || Boolean(selected.incognito), run: () => void exportConversation('md') },
      { id: 'json', label: 'Export JSON', hint: '', disabled: !selected || Boolean(selected.incognito), run: () => void exportConversation('json') },
      ...conversations.map((conversation) => ({
        id: conversation.id,
        label: conversation.title,
        hint: conversation.incognito ? 'Incognito' : 'Open chat',
        disabled: Boolean(activeRunId) && conversation.id !== selectedId,
        run: () => selectChat(conversation.id),
      })),
    ];
    const needle = paletteQuery.trim().toLowerCase();
    return needle ? items.filter((item) => item.label.toLowerCase().includes(needle)) : items;
  });

  $effect(() => {
    const choice = settingsOpen && settingsDraft ? settingsDraft : settings;
    document.documentElement.dataset.theme = choice.theme;
    document.documentElement.style.setProperty('--accent', safeAccent(choice.accent));
  });

  $effect(() => {
    const query = searchQuery;
    const revision = conversations.map((item) => `${item.id}:${item.updatedAt}:${item.messages.length}`).join('|');
    void revision;
    const timer = setTimeout(() => void runSearch(query), 120);
    return () => clearTimeout(timer);
  });

  async function runSearch(query: string) {
    const seq = ++searchSeq;
    const trimmed = query.trim();
    if (!trimmed) { matchIds = null; return; }
    try {
      const ids = await searchConversations(trimmed);
      if (seq !== searchSeq) return;
      const needle = trimmed.toLowerCase();
      const local = conversations.filter((item) => item.incognito && (
        item.title.toLowerCase().includes(needle) || item.messages.some((message) => message.content.toLowerCase().includes(needle))
      )).map((item) => item.id);
      matchIds = [...new Set([...ids, ...local])];
    } catch (error) {
      if (seq !== searchSeq) return;
      matchIds = [];
      live = error instanceof Error ? error.message : 'Search failed.';
    }
  }

  $effect(() => {
    if (!scroller || !selected) return;
    void selected.messages.length;
    void selected.messages[selected.messages.length - 1]?.content;
    scroller.scrollTop = scroller.scrollHeight;
  });

  $effect(() => { if (paletteOpen) paletteInput?.focus(); });
  $effect(() => { if (paletteIndex >= commands.length) paletteIndex = 0; });

  onMount(() => {
    void load();
    window.addEventListener('keydown', onWindowKey);
    return () => {
      window.removeEventListener('keydown', onWindowKey);
      saver.flush();
    };
  });
</script>

{#snippet chatRow(conversation: Conversation)}
  <li class="chat-item" class:active={conversation.id === selectedId}>
    {#if renamingId === conversation.id}
      <form onsubmit={(event) => { event.preventDefault(); void commitRename(conversation); }}>
        <label class="sr-only" for={"rename-" + conversation.id}>Conversation title</label>
        <input id={"rename-" + conversation.id} class="field" bind:value={renameValue} onkeydown={(event) => {
          if (event.key === 'Escape') { event.stopPropagation(); renamingId = null; }
        }} />
      </form>
    {:else}
      <button type="button" class="title" disabled={Boolean(activeRunId) && conversation.id !== selectedId} onclick={() => selectChat(conversation.id)}>
        {conversation.title}
      </button>
      <p class="meta">{conversation.incognito ? 'Incognito · not saved' : `${conversation.messages.length} messages`}</p>
    {/if}
    {#if pendingDeleteId === conversation.id}
      <div class="confirm">
        <span>Delete this chat?</span>
        <button type="button" class="btn warn" disabled={Boolean(activeRunId)} onclick={() => void confirmDelete(conversation)}>Delete</button>
        <button type="button" class="btn" onclick={() => pendingDeleteId = null}>Keep</button>
      </div>
    {:else}
      <div class="ops actions">
        <button type="button" class="icon-btn" disabled={Boolean(activeRunId)} aria-pressed={conversation.pinned} onclick={() => void togglePin(conversation)}>{conversation.pinned ? 'Unpin' : 'Pin'}</button>
        <button type="button" class="icon-btn" disabled={Boolean(activeRunId)} onclick={() => { renamingId = conversation.id; renameValue = conversation.title; }}>Rename</button>
        <button type="button" class="icon-btn" disabled={Boolean(activeRunId)} onclick={() => pendingDeleteId = conversation.id}>Delete</button>
      </div>
    {/if}
  </li>
{/snippet}

<h1 class="sr-only">Hearth</h1>
<div class="sr-only" aria-live="polite">{live}</div>

{#if !ready}
  <div class="app"><main class="stage"><p class="banner">Opening the library…</p></main></div>
{:else if loadError}
  <div class="app"><main class="stage"><p class="alert" role="alert">{loadError}</p><button type="button" class="btn" onclick={() => { loadError = ''; ready = false; void load(); }}>Try again</button></main></div>
{:else}
  <div class="app" class:side-open={sideOpen} class:inspector-open={inspectorOpen}>
    <aside class="sidebar" aria-label="Conversations">
      <div class="brand">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 3c2 3 2 5 0 8-3-1-5 1-5 4a5 5 0 0 0 10 0c0-3-2-5-5-4 2-3 2-5 0-8z"/></svg>
        <div><strong>Hearth</strong><div class="hint">Local only</div></div>
      </div>
      <label class="search">
        <span class="sr-only">Search messages</span>
        <input placeholder="Search messages" bind:value={searchQuery} />
      </label>
      <div class="stack">
        <button type="button" class="btn primary" onclick={() => void createChat(false)}>New conversation</button>
        <button type="button" class="btn" onclick={() => void createChat(true)}>New incognito</button>
      </div>
      <div class="chat-list grow">
        {#if pinnedChats.length}
          <p class="kicker">Pinned</p>
          <ul>{#each pinnedChats as conversation (conversation.id)}{@render chatRow(conversation)}{/each}</ul>
        {/if}
        {#if recentChats.length}
          <p class="kicker">Recent</p>
          <ul>{#each recentChats as conversation (conversation.id)}{@render chatRow(conversation)}{/each}</ul>
        {/if}
        {#if visibleChats.length === 0}
          <p class="empty-copy">{searchQuery.trim() ? 'No conversations contain that text.' : 'No conversations yet.'}</p>
        {/if}
      </div>
      <div class="pills" aria-label="Backends">
        {#each settings.backends as endpoint (endpoint.id)}
          {@const pill = pillFor(endpoint)}
          <div class="pill" class:on={pill.on}>
            <span><i></i>{endpoint.name}</span>
            <small>{pill.detail}</small>
          </div>
        {/each}
      </div>
      {#if discoverError}<p class="alert" role="alert">{discoverError}</p>{/if}
      <button type="button" class="btn" onclick={() => void rediscover()}>{discovery === 'checking' ? 'Checking servers…' : 'Rediscover servers'}</button>
      <p class="hardware">{hardwareLine(system.chip, system.totalMemoryBytes, system.gpuLimitBytes)}</p>
      <button type="button" class="btn" onclick={openSettings}>Settings</button>
    </aside>

    <main class="stage">
      <header class="topbar">
        <button type="button" class="btn narrow-only chats-toggle" aria-pressed={sideOpen} onclick={() => { sideOpen = !sideOpen; inspectorOpen = false; }}>Chats</button>
        <h2>{selected?.title ?? 'Hearth'}</h2>
        <span class="spacer"></span>
        {#if selected}
          <div class="row" aria-label="Mode">
            <button type="button" class="btn" aria-pressed={selected.mode === 'chat'} disabled={Boolean(activeRunId)} onclick={() => void patchSelected((conversation) => { conversation.mode = 'chat'; })}>Chat</button>
            <button type="button" class="btn" aria-pressed={selected.mode === 'agent'} disabled={Boolean(activeRunId)} onclick={() => void patchSelected((conversation) => { conversation.mode = 'agent'; conversation.thinking = false; })}>Agent</button>
          </div>
          {#if models.length === 0}
            <button type="button" class="btn" onclick={openSettings}>{discovery === 'checking' ? 'Checking servers…' : 'Configure a server'}</button>
          {:else}
            <label>
              <span class="sr-only">Model</span>
              <select class="select" disabled={Boolean(activeRunId)} value={currentModel ? String(models.indexOf(currentModel)) : ''} onchange={onModelChange}>
                <option value="">{selected.modelId && !currentModel ? `${selected.modelId} · not in the last check` : 'Select a reported model'}</option>
                {#each models as model, index (model.backendId + ':' + model.id)}
                  <option value={index}>{model.name} · {model.backendName} · {loadedLabel(model)}</option>
                {/each}
              </select>
            </label>
          {/if}
          <div class="row" aria-label="Thinking" aria-describedby="thinking-note">
            <button type="button" class="btn" aria-pressed={selected.mode === 'agent' || !selected.thinking} disabled={Boolean(activeRunId) || selected.mode === 'agent'} onclick={() => void patchSelected((conversation) => { conversation.thinking = false; })}>Fast</button>
            <button type="button" class="btn" aria-pressed={selected.mode === 'chat' && selected.thinking} disabled={Boolean(activeRunId) || selected.mode === 'agent'} onclick={() => void patchSelected((conversation) => { conversation.thinking = true; })}>Deep</button>
          </div>
          <button type="button" class="btn" aria-pressed={selected.incognito} disabled={Boolean(activeRunId)} onclick={() => void setIncognito(!selected.incognito)}>Incognito</button>
          <button type="button" class="btn" disabled={selected.incognito} onclick={() => void exportConversation('md')}>Export Markdown</button>
          <button type="button" class="btn" disabled={selected.incognito} onclick={() => void exportConversation('json')}>Export JSON</button>
        {/if}
        <button type="button" class="btn narrow-only inspector-toggle" aria-pressed={inspectorOpen} onclick={() => { inspectorOpen = !inspectorOpen; sideOpen = false; }}>Inspector</button>
      </header>

      {#if !isDesktop}
        <p class="banner">Browser preview stores chats, memory and settings in this browser. Generating needs the Hearth desktop app and a local server.</p>
      {/if}
      {#if runError}<p class="alert" role="alert">{runError}</p>{/if}
      {#if selected?.incognito}
        <p class="banner">Incognito. This chat is not saved, memory is off, and export is off.</p>
      {/if}
      {#if activeRunId}
        <p class="banner">A reply is in progress. Stop it before switching chats.</p>
      {/if}
      {#if selected?.mode === 'agent'}
        <p class="hint topbar">Agent tools are the calculator, directory listing, and file read. Writes ask every time. {settings.workspace.trim() ? 'Workspace is set.' : 'File tools stay off until Settings has an absolute path to an existing folder.'}</p>
      {/if}

      <div class="transcript" bind:this={scroller} use:bindMarkdown>
        <div class="sheet">
          {#if !selected}
            <div class="welcome">
              <p class="kicker">Local models</p>
              <h2>A quiet hearth for the model already on this Mac.</h2>
              <p>No sample chats. A reply appears only after a server streams it.</p>
              {#if !isDesktop}<p>This window is a browser preview. Generating needs the Hearth desktop app.</p>{/if}
              <button type="button" class="btn primary" onclick={() => void createChat(false)}>New conversation</button>
            </div>
          {:else if selected.messages.length === 0}
            <div class="welcome">
              <p class="kicker">{selected.incognito ? 'Incognito' : selected.mode}</p>
              <h2>{selected.title}</h2>
              <p>Write when you are ready. Hearth shows only text the server streams back.</p>
            </div>
          {:else}
            {#each selected.messages as message, messageIndex (message.id)}
              {#if message.role === 'user'}
                <div class="user-row">
                  <div>
                    <div class="user">{message.content}</div>
                    <div class="actions">
                      {#if messageIndex === lastUserIndex(selected.messages)}
                        <button type="button" class="icon-btn" disabled={Boolean(activeRunId)} onclick={beginEdit}>Edit</button>
                      {/if}
                      <button type="button" class="icon-btn" onclick={() => void copyText(message.content)}>Copy</button>
                      {#if !memoryLocked && settings.memoryEnabled && selected.memoryEnabled}
                        <button type="button" class="icon-btn" onclick={() => void remember(message)}>Remember</button>
                      {/if}
                    </div>
                  </div>
                </div>
              {:else}
                <article class="assistant" aria-busy={message.status === 'streaming'}>
                  <p class="kicker">{message.content || message.thinking || message.tools.length ? 'Hearth' : 'Not sent'}</p>
                  {#if message.thinking}
                    <details class="think"><summary>Thinking{message.status === 'streaming' ? '…' : ''}</summary><pre>{message.thinking}</pre></details>
                  {/if}
                  {#if message.content}
                    <div class="prose">{@html renderMarkdown(message.content)}</div>
                  {:else if message.status === 'streaming'}
                    <p class="hint">Waiting for the server…</p>
                  {/if}
                  {#each message.tools as step (step.id)}
                    <section class="tool">
                      <header><strong>{step.name}</strong><span class={toolClass(step)}>{step.status}{step.durationMs > 0 ? ` · ${step.durationMs} ms` : ''}</span></header>
                      {#if step.preview}<pre>{step.preview}</pre>{/if}
                      {#if step.output}<pre>{step.output}</pre>{/if}
                    </section>
                  {/each}
                  {#if pending && message.status === 'streaming'}
                    <section class="tool" aria-live="polite">
                      <header><strong>Allow {pending.toolName}?</strong><span>This ask is only for this call</span></header>
                      {#if pending.preview}<pre>{pending.preview}</pre>{/if}
                      <pre>{JSON.stringify(pending.args, null, 2)}</pre>
                      <div class="actions">
                        <button type="button" class="btn primary" disabled={approving} onclick={() => void resolveApproval(true)}>Allow once</button>
                        <button type="button" class="btn warn" disabled={approving} onclick={() => void resolveApproval(false)}>Deny</button>
                      </div>
                    </section>
                  {/if}
                  {#if failures[message.id]}
                    <p class="alert" role="alert"><strong>Not a model reply. </strong>{failures[message.id]}</p>
                  {:else if message.status === 'error'}
                    <p class="alert" role="alert">This reply ended with an error. Text above, if any, is partial output.</p>
                  {:else if message.status === 'stopped'}
                    <p class="hint">Stopped. Text above is partial output.</p>
                  {/if}
                  {#if message.stats}
                    <p class="stat">
                      {message.stats.estimated ? `≈ ${message.stats.promptTokens} prompt tokens and ${message.stats.completionTokens} completion tokens (estimate).` : `${message.stats.promptTokens} prompt tokens, ${message.stats.completionTokens} completion tokens.`}
                      {#if message.stats.ttftMs > 0} First token {(message.stats.ttftMs / 1000).toFixed(1)}s.{/if}
                      {#if !message.stats.estimated && message.stats.tokensPerSecond > 0} {message.stats.tokensPerSecond.toFixed(1)} tok/s.{/if}
                    </p>
                  {/if}
                  {#if message.memoriesUsed.length}
                    <p class="hint">Memories used: {message.memoriesUsed.map((memoryId) => memories.find((item) => item.id === memoryId)?.text ?? memoryId).join(' · ')}</p>
                  {/if}
                  <div class="actions">
                    {#if messageIndex === selected.messages.length - 1 && message.role === 'assistant'}
                      <button type="button" class="icon-btn" disabled={Boolean(activeRunId)} onclick={() => void regenerate()}>Regenerate</button>
                    {/if}
                    <button type="button" class="icon-btn" onclick={() => void copyText(message.content)}>Copy</button>
                    {#if !memoryLocked && settings.memoryEnabled && selected.memoryEnabled && message.content}
                      <button type="button" class="icon-btn" onclick={() => void remember(message)}>Remember</button>
                    {/if}
                  </div>
                </article>
              {/if}
            {/each}
          {/if}
        </div>
      </div>

      <form class="composer" onsubmit={(event) => { event.preventDefault(); if (!activeRunId) void submit(); }}>
        <div class="box">
          {#if selected?.mode === 'agent'}
            <p id="thinking-note" class="hint">Agent prioritizes valid JSON actions: Qwen thinking is requested off and gpt-oss reasoning low, where supported.</p>
          {:else if selected}
            <p id="thinking-note" class="hint">Deep requests Qwen3 thinking or gpt-oss high reasoning; Fast requests low reasoning for gpt-oss. Server support varies, and LM Studio support needs live verification.</p>
          {/if}
          {#if editingIndex !== null}<p class="hint">Editing your last message. Sending replaces it and removes the replies after it. <button type="button" class="icon-btn" onclick={() => { editingIndex = null; draft = ''; }}>Cancel edit</button></p>{/if}
          <label class="sr-only" for="composer">Message</label>
          <textarea id="composer" bind:value={draft} placeholder={selected ? 'Message' : 'Start a conversation first'} disabled={!selected} onkeydown={onComposerKey}></textarea>
          <div class="bar">
            <span class="hint">{settings.sendKey === 'cmd_enter' ? '⌘ Enter sends' : 'Enter sends'} · ⌘. stops</span>
            {#if activeRunId}
              <button type="button" class="btn warn" onclick={() => void stop()}>{stopRequested ? 'Stopping…' : 'Stop'}</button>
            {:else}
              <button type="submit" class="btn primary" disabled={!selected || !draft.trim()}>Send</button>
            {/if}
          </div>
        </div>
      </form>
    </main>

    <aside class="inspector" aria-label="Inspector">
      <div class="tabs" role="tablist" aria-label="Inspector" tabindex="-1" onkeydown={onTabs}>
        {#each ['memory', 'context', 'tools'] as name (name)}
          <button id={"tab-" + name} type="button" class="tab" role="tab" aria-selected={tab === name} aria-controls="inspector-panel" tabindex={tab === name ? 0 : -1} onclick={() => tab = name as Tab}>{name[0].toUpperCase() + name.slice(1)}</button>
        {/each}
      </div>
      <div id="inspector-panel" class="list grow" role="tabpanel" aria-labelledby={"tab-" + tab}>
        {#if tab === 'memory'}
          {#if memoryLocked}
            <p class="note-card">Memory is off for this incognito chat. Notes are not read, saved, or exported.</p>
          {:else}
            {#if !settings.memoryEnabled}<p class="hint">Memory injection is off in Settings. Notes you save stay in the library.</p>{/if}
            <form class="stack" onsubmit={(event) => { event.preventDefault(); void saveManualMemory(); }}>
              <label for="new-memory">New memory</label>
              <textarea id="new-memory" class="field" bind:value={memoryText}></textarea>
              <label for="memory-category">Category</label>
              <select id="memory-category" class="field" bind:value={memoryCategory}>
                {#each categories as category (category)}<option value={category}>{category}</option>{/each}
              </select>
              <button type="submit" class="btn primary" disabled={!memoryText.trim()}>Save memory</button>
            </form>
            {#each memories.filter((memory) => memory.status === 'candidate') as memory (memory.id)}
              <article class="candidate">
                <p>{memory.text}</p>
                <div class="actions">
                  <button type="button" class="btn primary" onclick={() => void updateMemory(memory, { status: 'active', enabled: true })}>Approve</button>
                  <button type="button" class="btn warn" onclick={() => void confirmDeleteMemory(memory)}>Reject</button>
                </div>
              </article>
            {/each}
            {#each memories.filter((memory) => memory.status !== 'candidate') as memory (memory.id)}
              <article class="memory" class:off={!memory.enabled || memory.status !== 'active'}>
                {#if memoryEditId === memory.id}
                  <label class="sr-only" for={"memory-" + memory.id}>Memory text</label>
                  <textarea id={"memory-" + memory.id} class="field" bind:value={memoryEditText}></textarea>
                  <div class="actions">
                    <button type="button" class="btn primary" onclick={() => { const text = memoryEditText.trim(); memoryEditId = null; if (text) void updateMemory(memory, { text }); }}>Save</button>
                    <button type="button" class="btn" onclick={() => memoryEditId = null}>Cancel</button>
                  </div>
                {:else}
                  <p>{memory.text}</p>
                  <p class="meta">{memory.category} · {memory.status}{memory.pinned ? ' · pinned' : ''}{memory.enabled ? '' : ' · disabled'}</p>
                  {#if memoryDeleteId === memory.id}
                    <div class="confirm"><span>Delete this memory?</span><button type="button" class="btn warn" onclick={() => void confirmDeleteMemory(memory)}>Delete</button><button type="button" class="btn" onclick={() => memoryDeleteId = null}>Keep</button></div>
                  {:else}
                    <div class="actions">
                      <button type="button" class="icon-btn" onclick={() => { memoryEditId = memory.id; memoryEditText = memory.text; }}>Edit</button>
                      <button type="button" class="icon-btn" aria-pressed={memory.pinned} onclick={() => void updateMemory(memory, { pinned: !memory.pinned })}>{memory.pinned ? 'Unpin' : 'Pin'}</button>
                      <button type="button" class="icon-btn" aria-pressed={!memory.enabled} onclick={() => void updateMemory(memory, { enabled: !memory.enabled })}>{memory.enabled ? 'Disable' : 'Enable'}</button>
                      {#if memory.status !== 'active'}<button type="button" class="icon-btn" onclick={() => void updateMemory(memory, { status: 'active', enabled: true })}>Restore</button>{/if}
                      <button type="button" class="icon-btn" onclick={() => memoryDeleteId = memory.id}>Delete</button>
                    </div>
                  {/if}
                {/if}
              </article>
            {/each}
            {#if memories.length === 0}<p class="empty-copy">No memories yet.</p>{/if}
          {/if}
        {:else if tab === 'context'}
          <p class="hardware">{hardwareLine(system.chip, system.totalMemoryBytes, system.gpuLimitBytes)}</p>
          {#if fit}<p class="fit {fit.level}">{fit.text}</p>{/if}
          {#if currentModel}<p class="hint">{currentModel.name} · {currentModel.backendName} · {loadedLabel(currentModel)}{currentModel.contextWindow ? ` · context ${currentModel.contextWindow}` : ''}</p>{/if}
          {#if inspection}
            <p class="hint">{inspection.estimated ? 'Heuristic estimate, not a tokenizer count.' : 'Counts reported for this turn.'}</p>
            <div class="budget">
              {#each [['System', inspection.systemTokens], ['Memory', inspection.memoryTokens], ['History', inspection.historyTokens], ['Output', inspection.outputTokens]] as row (row[0])}
                <div><span>{row[0]}</span><div class="track"><span style:width={inspection.contextWindow ? `${Math.min(100, Math.round(Number(row[1]) / inspection.contextWindow * 100))}%` : '0%'}></span></div><span>{row[1]}</span></div>
              {/each}
            </div>
            <p class="meta">{inspection.totalTokens} / {inspection.contextWindow} tokens</p>
            {#if inspection.memoriesUsed.length}<p class="hint">Included notes: {inspection.memoriesUsed.map((memoryId) => memories.find((item) => item.id === memoryId)?.text ?? memoryId).join(' · ')}</p>{/if}
            <details class="prompt"><summary>Prompt sent</summary><pre>{inspection.messages.map((message) => `${message.role}\n${message.content}`).join('\n\n')}</pre></details>
          {:else}
            <p class="empty-copy">Context appears after you send. Counts stay empty until then.</p>
          {/if}
        {:else}
          <p class="hint">Tool steps from the latest reply. Shell, network, and unattended actions are not in this release.</p>
          {#if pending}<p class="note-card">Allow or deny {pending.toolName} on the card in the conversation.</p>{/if}
          {#each selected?.messages.filter((message) => message.tools.length) ?? [] as message (message.id)}
            {#each message.tools as step (step.id)}
              <section class="tool">
                <header><strong>{step.name}</strong><span class={toolClass(step)}>{step.status}</span></header>
                {#if step.preview}<pre>{step.preview}</pre>{/if}
                {#if step.output}<pre>{step.output}</pre>{/if}
              </section>
            {/each}
          {:else}
            <p class="empty-copy">No tool steps yet.</p>
          {/each}
        {/if}
      </div>
    </aside>
  </div>
{/if}

{#if settingsOpen && settingsDraft}
  <div class="backdrop">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" use:trapTab>
      <div class="row"><h2 id="settings-title">Settings</h2><button type="button" class="btn" onclick={closeSettings}>Close</button></div>
      {#if settingsError}<p class="alert" role="alert">{settingsError}</p>{/if}
      <form class="stack" onsubmit={(event) => { event.preventDefault(); void commitSettings(); }}>
        <div class="grid-2">
          <div class="field-block">
            <label for="settings-theme">Theme</label>
            <select id="settings-theme" bind:value={settingsDraft.theme}>
              <option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option>
            </select>
          </div>
          <div class="field-block">
            <label for="settings-accent">Accent</label>
            <input id="settings-accent" type="text" bind:value={settingsDraft.accent} spellcheck="false" />
          </div>
          <div class="field-block">
            <label for="settings-context">Context window</label>
            <input id="settings-context" type="number" min="512" max="131072" step="1" bind:value={settingsDraft.contextWindow} />
          </div>
          <div class="field-block">
            <label for="settings-output">Max output tokens</label>
            <input id="settings-output" type="number" min="16" step="1" bind:value={settingsDraft.maxOutputTokens} />
          </div>
          <div class="field-block">
            <label for="settings-temperature">Temperature</label>
            <input id="settings-temperature" type="number" min="0" max="2" step="0.05" bind:value={settingsDraft.temperature} />
          </div>
          <div class="field-block">
            <label for="settings-steps">Max agent steps</label>
            <input id="settings-steps" type="number" min="1" max="20" step="1" bind:value={settingsDraft.maxSteps} />
          </div>
        </div>
        <div class="field-block">
          <label for="settings-send-key">Send key</label>
          <select id="settings-send-key" bind:value={settingsDraft.sendKey}><option value="enter">Enter</option><option value="cmd_enter">⌘ Enter</option></select>
        </div>
        <label class="row"><input type="checkbox" bind:checked={settingsDraft.memoryEnabled} /> Use saved memory in normal chats</label>
        <label>System prompt<textarea rows="4" bind:value={settingsDraft.systemPrompt}></textarea></label>
        <label>Workspace absolute path
          <input type="text" bind:value={settingsDraft.workspace} spellcheck="false" placeholder="/Users/name/existing-folder" />
        </label>
        <p class="hint">File tools use this folder only. Hearth does not create it, and this screen cannot see whether it exists.</p>
        <p class="kicker">Loopback servers</p>
        {#each settingsDraft.backends as endpoint, index (endpoint.id)}
          <div class="field-block">
            <label for={"endpoint-url-" + endpoint.id}>{endpoint.name} URL</label>
            <input id={"endpoint-url-" + endpoint.id} type="url" value={endpoint.url} spellcheck="false" oninput={(event) => { if (settingsDraft) settingsDraft.backends[index].url = event.currentTarget.value; }} />
          </div>
          <label class="row"><input type="checkbox" checked={endpoint.enabled} onchange={(event) => { if (settingsDraft) settingsDraft.backends[index].enabled = event.currentTarget.checked; }} /> Enabled</label>
        {/each}
        <label>Default model
          <select value={JSON.stringify([settingsDraft.defaultBackendId, settingsDraft.defaultModelId])} onchange={(event) => {
            if (!settingsDraft) return;
            const parsed = JSON.parse(event.currentTarget.value) as [string, string];
            settingsDraft.defaultBackendId = parsed[0] || settingsDraft.defaultBackendId;
            settingsDraft.defaultModelId = parsed[1] ?? '';
          }}>
            <option value={JSON.stringify([settingsDraft.defaultBackendId, ''])}>None selected</option>
            {#each models as model (model.backendId + ':' + model.id)}
              <option value={JSON.stringify([model.backendId, model.id])}>{model.name} · {model.backendName} · {loadedLabel(model)}</option>
            {/each}
          </select>
        </label>
        {#if models.length === 0}<p class="hint">No model was reported. Save a loopback URL, then rediscover. An empty list is not a loaded model.</p>{/if}
        <div class="actions">
          <button type="button" class="btn" onclick={() => settingsDraft && void rediscover(settingsDraft)}>Rediscover servers</button>
          <button type="submit" class="btn primary">Save settings</button>
        </div>
      </form>
    </div>
  </div>
{/if}

{#if paletteOpen}
  <div class="backdrop">
    <div class="dialog palette" role="dialog" aria-modal="true" aria-labelledby="palette-title" use:trapTab>
      <h2 id="palette-title" class="sr-only">Commands</h2>
      <label class="sr-only" for="palette-search">Search commands and chats</label>
      <input id="palette-search" bind:this={paletteInput} bind:value={paletteQuery} placeholder="Search commands and chats" onkeydown={(event) => {
        if (event.key === 'ArrowDown') { event.preventDefault(); paletteIndex = Math.min(commands.length - 1, paletteIndex + 1); }
        else if (event.key === 'ArrowUp') { event.preventDefault(); paletteIndex = Math.max(0, paletteIndex - 1); }
        else if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); const command = commands[paletteIndex]; if (command && !command.disabled) { paletteOpen = false; command.run(); } }
      }} />
      <ul>
        {#each commands as command, index (command.id)}
          <li><button type="button" class:active={index === paletteIndex} disabled={command.disabled} onclick={() => { if (!command.disabled) { paletteOpen = false; command.run(); } }}>{command.label}<span class="hint">{command.hint}</span></button></li>
        {/each}
      </ul>
      {#if commands.length === 0}<p class="empty-copy">No matching commands.</p>{/if}
    </div>
  </div>
{/if}

{#if toast}
  <div class="toast" role="status">
    <span>{toast.text}</span>
    {#if toast.undo}<button type="button" onclick={() => { const undo = toast?.undo; toast = null; if (undo) void undo().catch((error: unknown) => { live = error instanceof Error ? error.message : 'Could not undo.'; }); }}>Undo</button>{/if}
  </div>
{/if}
