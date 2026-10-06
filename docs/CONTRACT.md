# Hearth — Integration Contract (v0.1) — SUPERSEDED / HISTORICAL

> **Status: SUPERSEDED.** This was mimo's Round-1 proposal, written before the
> build was underway. The operative contracts are **BUILD-PLAN.md** (ownership,
> lanes, gates) and the actual shared types in **src-tauri/src/models.rs** and
> **src/lib/types.ts**. Where this file disagrees with those — wire shapes,
> command names, lane ownership — this file is wrong and kept only as discussion
> history. Do not code against it.

---

## 0. Rules of engagement

1. **Lane ownership is strict.** You edit only the paths in §1. To change a file
   another lane owns: write a request in `agent-comms.md` (via your drop file);
   mimo applies it or brokers a swap.
2. **Shared types (§2) are frozen after sign-off.** If you need a field added,
   ask early — the cost of a shape change doubles with every consumer.
3. **No new runtime dependencies** without a line in agent-comms.md justifying
   it against the bundle/RAM budgets (§15 of project.md). Dev-dependencies for
   tests are a lower bar but still announce them.
4. **Every lane ships unit tests with its code.** Integration tests are mimo's.
5. Format: Rust = `rustfmt` default, `cargo clippy` clean. TS = strict mode,
   prettier-compatible (2-space, semicolons, single quotes).

## 1. Directory & ownership map

```
hearth/  (repo root = the "Project Heath" directory)
├── project.md                        [user-owned — DO NOT EDIT]
├── agent-comms.md                    [shared log — append via drop files, mimo merges]
├── docs/CONTRACT.md                  [mimo]
├── package.json, vite.config.ts, svelte.config.js, tsconfig.json   [mimo scaffold; grok may extend, announce]
├── src/                              [GROK — entire frontend]
│   ├── app.html, app.css
│   ├── lib/api/types.ts              [grok writes, mimo audits vs §2]
│   ├── lib/api/ipc.ts                [grok: typed invoke() wrappers]
│   ├── lib/stores/                   [grok]
│   ├── lib/components/               [grok]
│   ├── lib/markdown/                 [grok]
│   └── routes/                       [grok]
├── src-tauri/
│   ├── Cargo.toml, tauri.conf.json, capabilities/   [mimo scaffold; sol may extend, announce]
│   ├── profiles/*.toml               [mimo — shipped model profiles, §7.5]
│   └── src/
│       ├── main.rs, lib.rs           [mimo — Tauri builder + command registration]
│       ├── types.rs                  [mimo — the §2 shared domain types]
│       ├── commands/*.rs             [mimo — Tauri command handlers]
│       ├── backends/                 [SOL — trait + openai_compat + lmstudio + omlx + ollama + probe + identity + router]
│       ├── system/                   [SOL — wired_limit.rs, pressure.rs]
│       ├── context/                  [mimo — builder.rs, budget.rs, tokenizer.rs, kv_estimate.rs]
│       ├── profiles/                 [mimo — loader.rs, matcher.rs]
│       ├── agent/parser.rs, repair.rs[mimo — tolerant tool-call parse + JSON repair]
│       ├── db/                       [mimo — schema.sql, migrations, repo.rs]
│       └── testing/mock_server.rs    [mimo — std-only OpenAI-compat mock, used by all lanes' tests]
└── README.md                         [mimo, final round]
```

**Deferred (must NOT appear in v0.1):** memory system, agent loop/tool execution,
quick-ask, menu bar, branching, attachments, personas, evals suite, benchmark
screen, MCP, voice, speculative decoding, compare mode, LAN mode.

## 2. Shared domain types

Rust (`src-tauri/src/types.rs`, serde) ↔ TypeScript (`src/lib/api/types.ts`).
All enums serialize as **lowercase strings**. All timestamps are **unix millis (u64/i64)**.
IDs are **ULID strings** (26 chars) generated in Rust.

### 2.1 Core

```ts
type BackendId = 'lmstudio' | 'omlx' | 'ollama' | `custom-${string}`;
type Role = 'chat' | 'agent' | 'embeddings' | 'rerank' | 'background' | 'quick_ask';
type FitStatus = 'green' | 'amber' | 'red';
type ToolFormat = 'hermes' | 'llama3' | 'mistral' | 'harmony' | 'json';

interface SamplingParams {
  temperature: number;   // explicit always — never rely on backend defaults
  top_p: number;
  top_k: number;
  min_p: number;
  max_tokens: number;    // = reserved_output slice
}

interface Msg { role: 'system' | 'user' | 'assistant' | 'tool'; content: string }
```

### 2.2 Models & backends

```ts
interface BackendStatus {
  id: BackendId; kind: BackendId; url: string;
  state: 'running' | 'off' | 'error';
  version: string | null;
  loaded: LoadedModel[];         // from /api/v0/models (lmstudio), /api/ps (ollama), probe (omlx)
  error: string | null;
}

interface LoadedModel { backend_model_id: string; ram_bytes: number | null; ctx: number | null }

interface ModelInfo {
  model_key: string;             // canonical key, §2.4 grammar
  display: string;               // human name for UI
  family: string; size: string | null; variant: string | null;
  quant: string | null; format: 'mlx' | 'gguf' | 'unknown';
  size_bytes: number | null; max_ctx: number | null;
  backends: { backend_id: BackendId; backend_model_id: string }[];
  profile_key: string;           // matched profile (e.g. "qwen3", "default")
  fits: FitVerdict;
}

interface FitVerdict {
  status: FitStatus;
  breakdown: {
    gpu_wired_limit_bytes: number;   // iogpu.wired_limit_mb × 1MB
    already_loaded_bytes: number;
    weights_bytes: number | null;    // null = unknown → amber at best
    kv_bytes: number;                // kv_estimate(ctx), §2.5
  };
  reason: string;                // one line, user-facing
}

interface Capabilities {
  backend_id: BackendId;
  json_schema: boolean; native_tools: boolean; vision: boolean;
  batching: boolean; persistent_kv: boolean; rerank: boolean; draft: boolean;
  probed_at: number | null;      // null = not yet probed
}
```

### 2.3 Chat

```ts
interface ConversationSummary {
  id: string; title: string | null; mode: 'chat' | 'agent';
  model_key: string | null; backend_id: BackendId | null;
  pinned: boolean; incognito: boolean;
  created_at: number; updated_at: number;
}

interface Message {
  id: string; conversation_id: string; parent_id: string | null;
  role: Msg['role']; content: string;
  thinking: string | null;           // stored separately, NEVER re-sent to model
  tool_calls: unknown | null;        // v0.1: parsed tool-call JSON if the model emitted one
  stats: MessageStats | null;        // assistant turns only
  created_at: number;
}

interface MessageStats {
  ttft_ms: number;                   // send → first text delta
  prefill_ms: number | null;         // backend-reported if available
  decode_tps: number;                // text tokens / (end - ttft)
  prompt_tokens: number | null;      // backend `usage`
  completion_tokens: number | null;
  cache: 'hit' | 'miss' | 'unknown'; // estimated from prefill speed vs profile baseline
  model_key: string; backend_id: BackendId;
}
```

### 2.4 Canonical model key grammar (owned by sol's identity.rs; pinned here)

```
{family}/{size}/{variant}/{quant}/{format}
 family  lowercase, org prefix stripped: "qwen/qwen3-30b-a3b" → "qwen3"
 size    lowercase: "30b-a3b", "14b", "0.6b", null if unparseable
 variant "instruct" | "base" | null
 quant   normalised: "q4_k_m"→"q4", "mxfp4"→"mxfp4", "4bit"→"q4", null if unknown
 format  "mlx" | "gguf" | "unknown"
Example: qwen/qwen3-30b-a3b-instruct-4bit (MLX) → "qwen3/30b-a3b/instruct/q4/mlx"
```

Same-key models across backends collapse into ONE `ModelInfo` with multiple
`backends[]` entries. Unknown shapes must not panic — fall back to
`{raw}/{null...}` with `format: "unknown"`.

### 2.5 KV estimate (owned by mimo's kv_estimate.rs; pinned here)

```
kv_bytes = layers × kv_heads × head_dim × 2 (K+V) × bytes_per_elem × ctx_tokens
```
Per-model config values live in the profile TOML (`kv = { layers=48, kv_heads=4, head_dim=128, bytes=2 }`).
Unknown model → `kv_bytes = null` → FitVerdict caps at `amber`.

## 3. Tauri command surface

All commands live in `src-tauri/src/commands/*`, registered in `lib.rs`.
TS calls them through `src/lib/api/ipc.ts`. Failures return the §4 error envelope.

```ts
// backends & models
backends_status(): Promise<BackendStatus[]>
models_refresh(): Promise<ModelInfo[]>                 // re-list + re-fit all backends
probe_capabilities(backend_id: BackendId): Promise<Capabilities>
fits_check(backend_id: BackendId, backend_model_id: string, ctx: number): Promise<FitVerdict>
model_load(backend_id: BackendId, backend_model_id: string, ctx: number, ttl_minutes: number | null): Promise<void>
model_unload(backend_id: BackendId, backend_model_id: string): Promise<void>
system_status(): Promise<SystemStatus>                 // §2 ext: mem_total, gpu_wired_limit_bytes, mem_pressure: 'normal'|'warn'|'critical', loaded_bytes

// conversations
conversation_list(): Promise<ConversationSummary[]>
conversation_create(mode: 'chat'|'agent', model_key: string | null, backend_id: BackendId | null): Promise<ConversationSummary>
conversation_update(id: string, patch: { title?, pinned?, model_key?, backend_id? }): Promise<ConversationSummary>
conversation_delete(id: string): Promise<void>
message_history(conversation_id: string): Promise<Message[]>
search_messages(query: string): Promise<{ message_id: string; conversation_id: string; snippet: string }[]>

// generation  (see §5 for the stream protocol)
send_message(conversation_id: string, text: string, ch: Channel<StreamEvent>): Promise<Message>   // resolves with the final assistant Message when the stream completes or errors
stop_generation(conversation_id: string): Promise<void>
regenerate(conversation_id: string, ch: Channel<StreamEvent>): Promise<Message>
edit_resend(message_id: string, new_text: string, ch: Channel<StreamEvent>): Promise<Message>

// settings  (config.toml is the backing store; key = dotted TOML path)
settings_get(): Promise<Record<string, unknown>>      // full merged config
settings_set(key: string, value: unknown): Promise<void>
```

## 4. Error envelope

```ts
interface HError { code: string; message: string; detail?: string }
// codes: 'backend_unreachable' | 'backend_http' | 'model_not_found' | 'no_backend_for_role'
//        | 'cancelled' | 'invalid_request' | 'db' | 'internal'
```
Rust side: `impl From<HError> for tauri::Error`-style serialization so
`invoke()` rejections in TS always carry `{code, message}`.

## 5. Streaming protocol (Tauri 2 `Channel<StreamEvent>`)

```ts
type StreamEvent =
  | { type: 'phase';  phase: 'prefill' | 'decode' }
  | { type: 'text';   delta: string }                 // incremental assistant text
  | { type: 'thinking'; delta: string }               // incremental thinking (render dimmed; never stored in content)
  | { type: 'tool_call'; call: { tool: string; args: unknown } }   // v0.1: surfaced, NOT executed
  | { type: 'stats';  stats: MessageStats }
  | { type: 'error';  error: HError }
  | { type: 'done' };
```

Lifecycle rules:
1. `send_message` persists the user message + a placeholder assistant row FIRST,
   then streams. The channel receives `phase` → `text`/`thinking`* → `stats` → `done`.
2. `stop_generation` cancels via CancellationToken in the adapter; the channel
   gets `stats` + `done` (not `error`) and the partial text is kept.
3. Thinking deltas are stripped before persisting `content`; `Message.thinking`
   keeps them. On resend, `thinking` is never included.
4. The Rust side caps delta batching at ~20 Hz per channel (coalesce small deltas)
   so the WebView isn't flooded at 80 tok/s.

## 6. Prompt assembly contract (context/builder.rs — mimo)

The ONLY function that builds prompts. Stable prefix order is binding:

```
[system_prompt]  [tool_schemas]     ← stable prefix (byte-identical across turns)
[memory_injection] [conversation_summary] [recent_turns] [dynamic tail: date, user msg]
```

v0.1 budgets (profile-overridable): `reserved_output=2048, system≤300, tools≤600,
memory≤400 (unused in v0.1 but reserved), summary≤600, recent_turns=remainder`.
`BudgetSlices` mirrors these numbers 1:1 in TS for the context meter.

## 7. Model profiles (src-tauri/profiles/*.toml — mimo)

Shipped: `qwen3.toml`, `gpt_oss.toml`, `gemma3.toml`, `mistral_small.toml`, `default.toml`.
Fields exactly as project.md §7.5 plus `kv = {...}` (§2.5). Matching: glob list in
`match = [...]`, first-match by file order most-specific-first; unknown models get
`default.toml` (conservative: json tool format, 8k ctx, few-shot on, max_tools=5).

## 8. Mock OpenAI server (testing/mock_server.rs — mimo, used by everyone)

- std-only (`std::net::TcpListener` + threads). No extra crates.
- Endpoints: `GET /v1/models`, `POST /v1/chat/completions` (SSE `text/event-stream`
  streaming + non-stream JSON), `POST /v1/embeddings` (fixed random-by-seed vectors).
- Scriptable per-test: responses are queued at construction
  (`MockServer::with_script(vec![...])`); each script entry = stream of deltas +
  optional tool-call chunk + `usage` block. Records every request body for
  assertions (this is how adapter tests verify prompt layout byte-for-byte).
- sol's adapter tests MUST use it; grok never needs it (grok's contract is the TS
  types + Storybook-free visual sanity).

## 9. Definition of done (per lane)

- [ ] All owned files compile / typecheck (`cargo clippy -- -D warnings`, `svelte-check`).
- [ ] Unit tests pass for owned logic (mimo integrates: `cargo test`, `npm test`).
- [ ] No cross-lane file touched (or the change is brokered in agent-comms.md).
- [ ] Deferred features have zero dead scaffolding — absence is documented in README.
- [ ] Every public Rust fn and TS store has a doc comment saying WHY, not WHAT.
