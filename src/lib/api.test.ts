import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  bootstrap, defaultSettings, deleteConversation, deleteMemory, discoverBackends,
  generate, newConversation, openExternal, saveConversation, saveMemory,
  saveSettings, searchConversations,
} from './api';
import type { Memory } from './types';

const storage = new Map<string, string>();
beforeEach(() => {
  storage.clear();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
});
afterEach(() => vi.unstubAllGlobals());

describe('truthful browser preview', () => {
  it('never pretends to generate or discover a resident model', async () => {
    const settings = structuredClone(defaultSettings);
    settings.defaultModelId = 'user-specified-model';
    const conversation = newConversation(settings);
    const onEvent = vi.fn();
    await expect(generate({ runId: 'test-run', conversation, settings }, onEvent)).rejects.toThrow(/desktop app/);
    expect(onEvent).not.toHaveBeenCalled();
    const backends = await discoverBackends(settings);
    expect(backends.length).toBeGreaterThan(0);
    expect(backends.every(backend => !backend.online && backend.models.length === 0)).toBe(true);
    expect((await bootstrap()).system.chip).toBe('Browser preview');
  });

  it('does not persist incognito conversations or enable their memories', async () => {
    const conversation = newConversation(defaultSettings, true);
    conversation.messages.push({ id: 'private-message', role: 'user', content: 'private material', thinking: '', createdAt: Date.now(), status: 'complete', tools: [], memoriesUsed: [] });
    expect(conversation.memoryEnabled).toBe(false);
    await saveConversation(conversation);
    expect((await bootstrap()).conversations).toEqual([]);
    expect([...storage.values()].join('')).not.toContain('private material');
  });

  it('keeps message search and deletion consistent after edits', async () => {
    const conversation = newConversation(defaultSettings);
    conversation.title = 'Garden notes';
    conversation.messages.push({ id: 'message', role: 'user', content: 'Rare moonflower blossoms', thinking: '', createdAt: Date.now(), status: 'complete', tools: [], memoriesUsed: [] });
    await saveConversation(conversation);
    expect(await searchConversations('MOONFLOWER')).toEqual([conversation.id]);
    conversation.title = 'Renamed notebook';
    conversation.messages[0].content = 'Orchid care';
    await saveConversation(conversation);
    expect(await searchConversations('moonflower')).toEqual([]);
    expect(await searchConversations('renamed')).toEqual([conversation.id]);
    await deleteConversation(conversation.id);
    expect(await searchConversations('orchid')).toEqual([]);
    expect((await bootstrap()).conversations).toEqual([]);
  });

  it('persists memory edits, pins, disabled status and removal', async () => {
    const memory: Memory = { id: 'memory', text: 'Prefer plain English', category: 'preference', status: 'active', pinned: false, enabled: true, createdAt: Date.now(), updatedAt: Date.now() };
    await saveMemory(memory);
    await saveMemory({ ...memory, text: 'Prefer concise plain English', pinned: true, enabled: false });
    expect((await bootstrap()).memories).toMatchObject([{ text: 'Prefer concise plain English', pinned: true, enabled: false }]);
    await deleteMemory(memory.id);
    expect((await bootstrap()).memories).toEqual([]);
  });

  it('retains explicit settings and supplies independent defaults after corrupt storage', async () => {
    await saveSettings({ ...structuredClone(defaultSettings), theme: 'dark', temperature: 0.25 });
    expect((await bootstrap()).settings).toMatchObject({ theme: 'dark', temperature: 0.25 });
    storage.set('hearth-preview-v1', 'incomplete JSON');
    const first = await bootstrap();
    first.settings.backends[0].url = 'changed in memory';
    expect((await bootstrap()).settings.backends[0].url).toBe(defaultSettings.backends[0].url);
  });

  it('rejects active-content links before opening another window', async () => {
    const open = vi.fn();
    vi.stubGlobal('window', { open });
    await expect(openExternal('javascript:alert(1)')).rejects.toThrow(/Only web links/);
    await expect(openExternal('file:///etc/passwd')).rejects.toThrow(/Only web links/);
    expect(open).not.toHaveBeenCalled();
    await openExternal('https://example.com/docs');
    expect(open).toHaveBeenCalledWith('https://example.com/docs', '_blank', 'noopener,noreferrer');
  });
});
