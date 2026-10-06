import { expect, test, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { defaultSettings } from '../src/lib/api';
import type { Conversation, Memory, Settings, StreamEvent } from '../src/lib/types';

interface Seed { conversations: Conversation[]; memories: Memory[]; settings: Settings }
function seed(overrides: Partial<Seed> = {}): Seed {
  return { conversations: [], memories: [], settings: structuredClone(defaultSettings), ...overrides };
}
function conversation(title = 'Research notebook', content = 'The moonflower blooms at night.'): Conversation {
  return {
    id: '00000000-0000-4000-8000-000000000001', title, mode: 'chat', backendId: 'lmstudio', modelId: '',
    pinned: false, incognito: false, memoryEnabled: true, thinking: false, createdAt: 1, updatedAt: 1,
    messages: [
      { id: '00000000-0000-4000-8000-000000000002', role: 'user', content: 'Explain this plant.', thinking: '', createdAt: 1, status: 'complete', tools: [], memoriesUsed: [] },
      { id: '00000000-0000-4000-8000-000000000003', role: 'assistant', content, thinking: '', createdAt: 2, status: 'complete', tools: [], memoriesUsed: [] },
    ],
  };
}
async function preview(page: Page, initial = seed()) {
  await page.addInitScript(data => {
    if (localStorage.getItem('hearth-preview-v1') === null) localStorage.setItem('hearth-preview-v1', JSON.stringify(data));
  }, initial);
  await page.goto('/');
}
async function stored(page: Page): Promise<Seed> {
  return page.evaluate(() => JSON.parse(localStorage.getItem('hearth-preview-v1') ?? '{}'));
}

// This fake exists only in the test browser. It exercises the shipped Tauri
// bridge and channel protocol without adding a simulated model to the product.
async function native(page: Page, initial = seed()) {
  initial.settings.defaultModelId = 'test-model';
  await page.addInitScript(data => {
    type TestRun = { request: { runId: string }; callbackId: number; index: number; content: string; thinking: string; resolve: () => void };
    const callbacks = new Map<number, (value: unknown) => void>();
    const runs: TestRun[] = [];
    const calls: { command: string; args: any }[] = [];
    const ipcCopy = <T>(value: T): T => JSON.parse(JSON.stringify(value));
    let callbackId = 0;
    const stats = { promptTokens: 10, completionTokens: 4, ttftMs: 100, durationMs: 500, tokensPerSecond: 10, estimated: false, backendId: 'lmstudio', modelId: 'test-model' };
    const emit = (payload: any, runIndex = runs.length - 1) => {
      const run = runs[runIndex];
      if (!run) throw new Error('No generation request has arrived.');
      if (payload.type === 'delta') { run.content += payload.text ?? ''; run.thinking += payload.thinking ?? ''; }
      callbacks.get(run.callbackId)?.({ index: run.index++, message: { runId: run.request.runId, ...payload } });
      if (payload.type === 'done' || payload.type === 'error') run.resolve();
    };
    const api = {
      data, calls, runs, stats, emit,
      transformCallback(callback: (value: unknown) => void) { const id = ++callbackId; callbacks.set(id, callback); return id; },
      unregisterCallback(id: number) { callbacks.delete(id); },
      async invoke(command: string, args: any = {}) {
        calls.push({ command, args });
        if (command === 'bootstrap') return structuredClone({ ...data, system: { totalMemoryBytes: 8 * 1024 ** 3, gpuLimitBytes: null, chip: 'Test Mac', platform: 'macos' } });
        if (command === 'discover_backends') return [{ id: 'lmstudio', name: 'LM Studio', url: 'http://127.0.0.1:1234/v1', online: true, error: null, models: [{ id: 'test-model', name: 'Test model', backendId: 'lmstudio', canonicalKey: 'test-model', loaded: true, sizeBytes: null, contextWindow: 8192, format: null }] }];
        if (command === 'save_conversation') { data.conversations = [ipcCopy(args.conversation), ...data.conversations.filter((item: Conversation) => item.id !== args.conversation.id)]; return; }
        if (command === 'delete_conversation') { data.conversations = data.conversations.filter((item: Conversation) => item.id !== args.conversationId); return; }
        if (command === 'save_memory') { data.memories = [ipcCopy(args.memory), ...data.memories.filter((item: Memory) => item.id !== args.memory.id)]; return; }
        if (command === 'delete_memory') { data.memories = data.memories.filter((item: Memory) => item.id !== args.memoryId); return; }
        if (command === 'save_settings') { data.settings = ipcCopy(args.settings); return; }
        if (command === 'search_conversations') return data.conversations.filter((item: Conversation) => item.title.toLowerCase().includes(args.query.toLowerCase()) || item.messages.some(message => message.content.toLowerCase().includes(args.query.toLowerCase()))).map((item: Conversation) => item.id);
        if (command === 'generate') return new Promise<void>(resolve => runs.push({ request: ipcCopy(args.request), callbackId: args.onEvent.id, index: 0, content: '', thinking: '', resolve }));
        if (command === 'stop_generation') { const index = runs.findIndex(run => run.request.runId === args.runId); const run = runs[index]; emit({ type: 'done', status: 'stopped', content: run.content, thinking: run.thinking, stats }, index); return; }
        if (command === 'approve_tool' || command === 'export_text' || command === 'open_external') return;
        throw new Error(`Unexpected native test command: ${command}`);
      },
    };
    (window as any).__TAURI_INTERNALS__ = api;
    (window as any).__hearthTest = api;
  }, initial);
  await page.goto('/');
}
async function event(page: Page, payload: Omit<StreamEvent, 'runId'> | Record<string, unknown>, runIndex?: number) {
  await page.evaluate(({ payload, runIndex }) => (window as any).__hearthTest.emit(payload, runIndex), { payload, runIndex });
}

test('preview identifies itself and does not advertise a working local model', async ({ page }) => {
  await preview(page);
  await expect(page.getByText(/browser preview/i).first()).toBeVisible();
  await expect(page.getByText(/desktop app/i).first()).toBeVisible();
});

test('model markdown cannot execute HTML or active-content links', async ({ page }) => {
  const content = '<script>window.__injected = true</script>\n<img src=x onerror="window.__injected = true">\n[unsafe](javascript:alert(1))\n\n```html\n<img src=x onerror=alert(1)>\n```';
  await preview(page, seed({ conversations: [conversation('Rendering audit', content)] }));
  await expect(page.getByText('<script>window.__injected = true</script>', { exact: false })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Copy code', exact: true })).toBeVisible();
  await expect(page.locator('a[href^="javascript:"]')).toHaveCount(0);
  await expect(page.locator('img[onerror]')).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__injected)).toBeUndefined();
});

test('preview sending preserves the user message and reports the desktop requirement', async ({ page }) => {
  await preview(page);
  await page.getByRole('button', { name: 'New conversation', exact: true }).first().click();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('A browser-only question');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('desktop app');
  await expect.poll(async () => (await stored(page)).conversations[0]?.messages.at(-1)?.status).toBe('error');
  expect((await stored(page)).conversations[0].messages[0]).toMatchObject({ role: 'user', content: 'A browser-only question' });
  expect((await stored(page)).conversations[0].messages.at(-1)).toMatchObject({ role: 'assistant', status: 'error', content: '' });
  await expect(page.getByText('Not a model reply.', { exact: true })).toBeVisible();
});

test('chat creation, rename, pin and confirmed deletion survive a reload', async ({ page }) => {
  await preview(page);
  await page.getByRole('button', { name: 'New conversation', exact: true }).first().click();
  const original = page.getByRole('listitem').filter({ has: page.getByRole('button', { name: 'New conversation', exact: true }) });
  await original.getByRole('button', { name: 'Rename', exact: true }).click();
  await page.getByRole('textbox', { name: 'Conversation title' }).fill('Trip planning');
  await page.getByRole('textbox', { name: 'Conversation title' }).press('Enter');
  const row = page.getByRole('listitem').filter({ has: page.getByRole('button', { name: 'Trip planning', exact: true }) });
  await row.getByRole('button', { name: 'Pin', exact: true }).click();
  await expect.poll(async () => (await stored(page)).conversations[0]?.pinned).toBe(true);
  await page.reload();
  await expect(page.getByRole('button', { name: 'Trip planning', exact: true })).toBeVisible();
  await expect(row.getByRole('button', { name: 'Unpin', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await row.getByRole('button', { name: 'Delete', exact: true }).click();
  await row.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Trip planning', exact: true })).toHaveCount(0);
  await page.reload();
  expect((await stored(page)).conversations).toEqual([]);
});

test('chat search includes message content and clears its filter', async ({ page }) => {
  const unrelated = { ...conversation('Work log', 'A different subject.'), id: '00000000-0000-4000-8000-000000000004' };
  await preview(page, seed({ conversations: [conversation(), unrelated] }));
  await page.getByRole('textbox', { name: 'Search messages', exact: true }).fill('MOONFLOWER');
  await expect(page.getByRole('button', { name: 'Research notebook', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Work log', exact: true })).toHaveCount(0);
  await page.getByRole('textbox', { name: 'Search messages', exact: true }).fill('no-such-phrase');
  await expect(page.getByText('No conversations contain that text.', { exact: true })).toBeVisible();
  await page.getByRole('textbox', { name: 'Search messages', exact: true }).fill('');
  await expect(page.getByRole('button', { name: 'Work log', exact: true })).toBeVisible();
});

test('manual notes can be created without a chat and edited, pinned, hidden and deleted', async ({ page }) => {
  await preview(page);
  await page.getByRole('textbox', { name: 'New memory', exact: true }).fill('Prefer practical examples');
  await page.getByRole('combobox', { name: 'Category', exact: true }).selectOption('preference');
  await page.getByRole('button', { name: 'Save memory', exact: true }).click();
  const note = page.getByRole('article').filter({ has: page.getByText('Prefer practical examples', { exact: true }) });
  await expect(note).toBeVisible();
  await note.getByRole('button', { name: 'Edit', exact: true }).click();
  await page.getByRole('textbox', { name: 'Memory text', exact: true }).fill('Prefer concise practical examples');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  const updated = page.getByRole('article').filter({ has: page.getByText('Prefer concise practical examples', { exact: true }) });
  await updated.getByRole('button', { name: 'Pin', exact: true }).click();
  await updated.getByRole('button', { name: 'Disable', exact: true }).click();
  await expect.poll(async () => (await stored(page)).memories[0]).toMatchObject({ text: 'Prefer concise practical examples', category: 'preference', pinned: true, enabled: false });
  await page.reload();
  await expect(updated.getByRole('button', { name: 'Enable', exact: true })).toBeVisible();
  await updated.getByRole('button', { name: 'Delete', exact: true }).click();
  await updated.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect.poll(async () => (await stored(page)).memories.length).toBe(0);
});

test('candidate notes require explicit approval or rejection', async ({ page }) => {
  const note: Memory = { id: 'candidate', text: 'A suggested note', category: 'other', status: 'candidate', pinned: false, enabled: true, createdAt: 1, updatedAt: 1 };
  await preview(page, seed({ memories: [note, { ...note, id: 'reject', text: 'A rejected suggestion' }] }));
  const candidate = page.getByRole('article').filter({ has: page.getByText('A suggested note', { exact: true }) });
  await candidate.getByRole('button', { name: 'Approve', exact: true }).click();
  await expect.poll(async () => (await stored(page)).memories.find(memory => memory.id === 'candidate')?.status).toBe('active');
  await page.getByRole('article').filter({ has: page.getByText('A rejected suggestion', { exact: true }) }).getByRole('button', { name: 'Reject', exact: true }).click();
  await expect.poll(async () => (await stored(page)).memories.length).toBe(1);
});

test('settings validate budgets and local endpoints, then persist theme and send key', async ({ page }) => {
  await preview(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await dialog.getByRole('combobox', { name: 'Theme', exact: true }).selectOption('dark');
  await dialog.getByRole('combobox', { name: 'Send key', exact: true }).selectOption('cmd_enter');
  await dialog.getByLabel('Context window', { exact: true }).fill('512');
  await dialog.getByLabel('Max output tokens', { exact: true }).fill('1024');
  await dialog.getByRole('button', { name: 'Save settings', exact: true }).click();
  await expect(dialog.getByRole('alert')).toContainText('context window');
  await dialog.getByLabel('Max output tokens', { exact: true }).fill('128');
  await dialog.getByLabel('LM Studio URL', { exact: true }).fill('https://example.com/v1');
  await dialog.getByRole('button', { name: 'Save settings', exact: true }).click();
  await expect(dialog.getByRole('alert')).toContainText('loopback');
  await dialog.getByLabel('LM Studio URL', { exact: true }).fill('http://127.0.0.1:1234/v1');
  await dialog.getByLabel('Temperature', { exact: true }).fill('0.25');
  await dialog.getByRole('button', { name: 'Save settings', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect((await stored(page)).settings).toMatchObject({ theme: 'dark', sendKey: 'cmd_enter', contextWindow: 512, maxOutputTokens: 128, temperature: 0.25, networkTools: false, autoExtract: false });
});

test('incognito does not persist messages, enable memory or remember an answer', async ({ page }) => {
  await preview(page, seed({ conversations: [conversation()] }));
  await page.getByRole('button', { name: 'New incognito', exact: true }).click();
  await expect(page.getByText('Memory is off for this incognito chat. Notes are not read, saved, or exported.', { exact: true })).toBeVisible();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('Sensitive private question');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('desktop app');
  await expect(page.getByRole('button', { name: 'Remember', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Save memory', exact: true })).toHaveCount(0);
  expect(JSON.stringify(await stored(page))).not.toContain('Sensitive private question');
  await page.reload();
  await expect(page.getByRole('button', { name: 'Sensitive private question', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Research notebook', exact: true })).toBeVisible();
});

test('keyboard shortcuts create chats, open settings and filter the command palette', async ({ page }) => {
  await preview(page);
  await page.keyboard.press('Meta+n');
  await expect(page.getByRole('listitem').filter({ has: page.getByRole('button', { name: 'New conversation', exact: true }) })).toBeVisible();
  await page.keyboard.press('Meta+,');
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Meta+k');
  const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
  await palette.getByRole('textbox', { name: 'Search commands and chats', exact: true }).fill('incognito');
  await palette.getByRole('button', { name: 'New incognito conversation', exact: true }).click();
  await expect(palette).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Incognito', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

test('settings takes and traps keyboard focus while its modal is open', async ({ page }) => {
  await preview(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await expect.poll(() => dialog.evaluate(element => element.contains(document.activeElement))).toBe(true);
  await dialog.getByRole('button', { name: 'Close', exact: true }).focus();
  await page.keyboard.press('Shift+Tab');
  expect(await dialog.evaluate(element => element.contains(document.activeElement))).toBe(true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('conversation exports produce usable Markdown and JSON downloads', async ({ page }) => {
  await preview(page, seed({ conversations: [conversation()] }));
  const markdownDownload = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export Markdown', exact: true }).click();
  const markdown = await markdownDownload;
  const markdownPath = await markdown.path();
  expect(markdown.suggestedFilename()).toMatch(/\.md$/);
  expect(await readFile(markdownPath!, 'utf8')).toContain('The moonflower blooms at night.');
  const jsonDownload = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export JSON', exact: true }).click();
  const json = await jsonDownload;
  const jsonPath = await json.path();
  expect(json.suggestedFilename()).toMatch(/\.json$/);
  expect(JSON.parse(await readFile(jsonPath!, 'utf8'))).toMatchObject({ title: 'Research notebook', incognito: false, messages: [{ role: 'user' }, { role: 'assistant' }] });
});

test('Enter during IME composition leaves the draft unsent', async ({ page }) => {
  await preview(page);
  await page.getByRole('button', { name: 'New conversation', exact: true }).first().click();
  const message = page.getByRole('textbox', { name: 'Message', exact: true });
  await message.fill('日本語の入力');
  await message.dispatchEvent('keydown', { key: 'Enter', code: 'Enter', isComposing: true, bubbles: true });
  await expect(message).toHaveValue('日本語の入力');
  expect((await stored(page)).conversations[0].messages).toEqual([]);
});

test('the configured Command-Enter send key leaves plain Enter as a newline', async ({ page }) => {
  const settings = structuredClone(defaultSettings);
  settings.sendKey = 'cmd_enter';
  await preview(page, seed({ settings, conversations: [{ ...conversation(), messages: [] }] }));
  const message = page.getByRole('textbox', { name: 'Message', exact: true });
  await message.fill('A two-line request');
  await message.press('Enter');
  await expect(message).toHaveValue('A two-line request\n');
  expect((await stored(page)).conversations[0].messages).toEqual([]);
  await message.press('Meta+Enter');
  await expect(page.getByRole('alert')).toContainText('desktop app');
});

test('editing a prompt replaces its reply and Regenerate replaces only the assistant turn', async ({ page }) => {
  await native(page, seed({ conversations: [conversation()] }));
  await page.getByRole('button', { name: 'Edit', exact: true }).click();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('A revised question');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(1);
  const stats = await page.evaluate(() => (window as any).__hearthTest.stats);
  await event(page, { type: 'done', status: 'complete', content: 'A revised answer', thinking: '', stats });
  await expect(page.getByText('The moonflower blooms at night.', { exact: true })).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.data.conversations[0].messages)).toMatchObject([{ role: 'user', content: 'A revised question' }, { role: 'assistant', content: 'A revised answer' }]);
  await page.getByRole('button', { name: 'Regenerate', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(2);
  await event(page, { type: 'done', status: 'complete', content: 'A regenerated answer', thinking: '', stats });
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.data.conversations[0].messages)).toMatchObject([{ role: 'user', content: 'A revised question' }, { role: 'assistant', content: 'A regenerated answer' }]);
  expect(await page.evaluate(() => (window as any).__hearthTest.data.conversations[0].messages.length)).toBe(2);
});

test('native streams keep partial replies on Stop and reject stale events', async ({ page }) => {
  await native(page);
  await page.getByRole('button', { name: 'New conversation', exact: true }).first().click();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('First request');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(1);
  await event(page, { type: 'delta', text: 'A useful partial answer', thinking: 'Private model reasoning' });
  await expect(page.getByText('A useful partial answer', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(page.getByText('Stopped. Text above is partial output.', { exact: true })).toBeVisible();
  await expect(page.getByText('Stopping…', { exact: true })).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.data.conversations[0].messages.at(-1).status)).toBe('stopped');
  await event(page, { type: 'delta', text: ' stale content', thinking: '' }, 0);
  await expect(page.getByText(/stale content/)).toHaveCount(0);
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('Second request');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(2);
  await event(page, { type: 'error', message: 'A stale error must stay hidden' }, 0);
  await event(page, { type: 'delta', text: 'A fresh answer', thinking: '' }, 1);
  const stats = await page.evaluate(() => (window as any).__hearthTest.stats);
  await page.evaluate(stats => {
    const native = (window as any).__hearthTest;
    native.emit({ type: 'done', status: 'complete', content: 'A fresh answer', thinking: '', stats }, 1);
    native.emit({ type: 'delta', text: ' stale terminal content', thinking: '' }, 1);
  }, stats);
  await expect(page.getByText('A fresh answer', { exact: true })).toBeVisible();
  await expect(page.getByText(/stale terminal content/)).toHaveCount(0);
  await expect(page.getByText('A stale error must stay hidden', { exact: true })).toHaveCount(0);
});

test('write approvals show the exact preview and require an explicit decision', async ({ page }) => {
  await native(page);
  await page.getByRole('button', { name: 'New conversation', exact: true }).first().click();
  await page.getByRole('button', { name: 'Agent', exact: true }).click();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('Write a note');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(1);
  await event(page, { type: 'approval', approvalId: 'write-1', toolName: 'write_file', args: { path: 'note.md', content: 'New content' }, preview: 'note.md\nBefore: empty\nAfter: New content' });
  await expect(page.getByText('note.md\nBefore: empty\nAfter: New content', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__hearthTest.calls.filter((call: any) => call.command === 'approve_tool').length)).toBe(0);
  await page.getByRole('button', { name: 'Deny', exact: true }).click();
  expect(await page.evaluate(() => (window as any).__hearthTest.calls.find((call: any) => call.command === 'approve_tool').args)).toMatchObject({ approvalId: 'write-1', allow: false });
  await event(page, { type: 'approval', approvalId: 'write-2', toolName: 'write_file', args: { path: 'other.md', content: 'Approved content' }, preview: 'other.md\nAfter: Approved content' });
  await page.getByRole('button', { name: 'Allow once', exact: true }).click();
  expect(await page.evaluate(() => (window as any).__hearthTest.calls.filter((call: any) => call.command === 'approve_tool').at(-1).args)).toMatchObject({ approvalId: 'write-2', allow: true });
  await event(page, { type: 'approval', approvalId: 'write-3', toolName: 'write_file', args: { path: 'stop.md', content: 'Never approve' }, preview: 'stop.md\nAfter: Never approve' });
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Allow once', exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__hearthTest.calls.filter((call: any) => call.command === 'approve_tool').length)).toBe(2);
});

test('native generation errors preserve partial content with an explicit failure status', async ({ page }) => {
  await native(page);
  await page.getByRole('button', { name: 'New conversation', exact: true }).first().click();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('A request interrupted by failure');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(1);
  await event(page, { type: 'delta', text: 'The connection failed after this text', thinking: '' });
  await event(page, { type: 'error', message: 'Backend connection lost' });
  await expect(page.getByText('Not a model reply.', { exact: true })).toBeVisible();
  await expect(page.getByText('The connection failed after this text', { exact: true })).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('Backend connection lost');
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.data.conversations[0].messages.at(-1).status)).toBe('error');
});

test('native incognito replies stay in memory and cannot be remembered or exported', async ({ page }) => {
  await native(page);
  await page.getByRole('button', { name: 'New incognito', exact: true }).click();
  await page.getByRole('textbox', { name: 'Message', exact: true }).fill('A private native request');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__hearthTest.runs.length)).toBe(1);
  const stats = await page.evaluate(() => (window as any).__hearthTest.stats);
  await event(page, { type: 'done', status: 'complete', content: 'A private answer', thinking: '', stats });
  await expect(page.getByText('A private answer', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Remember', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Export Markdown', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Export JSON', exact: true })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__hearthTest.data.conversations)).toEqual([]);
  expect(await page.evaluate(() => (window as any).__hearthTest.calls.some((call: any) => call.command === 'save_conversation'))).toBe(false);
});

test('desktop and narrow layouts keep controls and modal content inside the viewport', async ({ page }) => {
  const content = 'A short explanation.\n\n```typescript\nconst message = "' + 'longvalue'.repeat(40) + '";\n```';
  await preview(page, seed({ conversations: [conversation('Layout audit', content)] }));
  await page.screenshot({ path: '.agent-work/ui-desktop.png', fullPage: true });
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.screenshot({ path: '.agent-work/ui-dark.png', fullPage: true });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 760, height: 650 });
  const overflow = () => page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(await overflow()).toBe(false);
  await expect(page.getByRole('textbox', { name: 'Message', exact: true })).toBeVisible();
  await page.screenshot({ path: '.agent-work/ui-narrow.png', fullPage: true });
  expect(await page.locator('.code-block pre').evaluate(element => {
    const style = getComputedStyle(element);
    return element.scrollWidth <= element.clientWidth || ['auto', 'scroll'].includes(style.overflowX);
  })).toBe(true);
  await page.keyboard.press('Meta+,');
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible();
  expect(await overflow()).toBe(false);
  await page.screenshot({ path: '.agent-work/ui-settings.png', fullPage: true });
});
