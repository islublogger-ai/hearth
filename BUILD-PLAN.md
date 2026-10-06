# Hearth collaboration and build plan

Status: sol has incorporated Grok and MiMo critiques; build ownership assigned for the initial personal release.

The source specification is preserved in project.md. The first release is a native, unsigned personal macOS application. It must function with real configured local inference servers and must also open cleanly with no server. The complete multi-week roadmap remains the direction; unavailable extensions will be documented explicitly rather than rendered as working features.

## Ownership

- **sol**: shared types and API bridge, native shell, backend adapters, streaming and cancellation, integration, release packaging and verification.
- **grok**: Svelte interface, visual design, accessible interactions and frontend review after plan critique.
- **mimo**: SQLite persistence, migrations, FTS and storage/privacy tests after plan critique.
- **sol sub-agent**: context budgets, thinking stripping, constrained action parsing, safe scoped file tools and associated tests.

Only the owner edits an assigned implementation area. Other agents report review findings through agent-comms.md. Each reviewer will examine another agent's work. File discussion messages carry actual model names, and appenders use a file lock when practical. The final release decision must explicitly distinguish observed test evidence, unavailable live evidence and deferred roadmap work.

## Shared implementation contract

Frontend data structures are in src/lib/types.ts; identical camelCase Rust payloads are in src-tauri/src/models.rs. IDs are UUID strings and timestamps are Unix milliseconds. Svelte uses a narrow API from src/lib/api.ts; Rust owns native network, database and tool execution. A generation has one runId, cancellable HTTP and approval waits, isolated events and one terminal outcome. The UI saves partial and completed assistant messages. Incognito is held only in frontend memory; native persistence rejects incognito writes.

Store API (src-tauri/src/db.rs): `Store::open(path: &Path) -> Result<Store, String>`, `conversations()`, `memories()`, `settings()`, `save_conversation(&Conversation)`, `delete_conversation(&str)`, `save_memory(&Memory)`, `delete_memory(&str)`, `save_settings(&Settings)`, `search_conversations(&str) -> Result<Vec<String>, String>`. Store internally serializes its SQLite connection with a Mutex. All methods return Result with String errors. Foreign keys, WAL and external-content FTS CRUD triggers are required. Settings and conversation validation must reject unreasonable input. A memory query returns relevant active+enabled entries plus pins; candidates are never injected.

Core helpers (context.rs, actions.rs, tools.rs) are pure or isolated from Tauri: context builder uses Settings/Conversation/Memory and returns PromptInspection; action parser returns a tool call or final answer with strict shapes; tool preparation produces normalized arguments and write preview, execution is scoped to an explicitly configured workspace and a pending approval binds to that prepared operation. Unknown or unsupported actions fail closed.

## Proposed release functionality

- Conversation creation, pin/rename/delete, full message search, persistent settings and JSON/Markdown export.
- Real OpenAI-compatible SSE streaming from LM Studio, oMLX and Ollama, truthful model discovery, per-chat selection, stop/retry/edit and separate thinking display.
- Model-aware conservative context defaults, explicit sampling, budget inspection and genuine timing/usage stats with estimate labels.
- Visible manual memory with CRUD, category/pin/disable and candidate approval; relevant bounded lexical retrieval. No pretend vector index.
- Agent mode with bounded JSON actions, calculator, scoped file listing/reading, approval-backed atomic writes, tool timeline, loop detection and cancellation.
- Warm Mac-inspired light/dark/system interface, keyboard shortcuts, palette, disconnected onboarding and native menu.
- Compiled macOS .app plus source, reproducible commands, tests and honest release notes.

External web search/fetch, shell/Python/MCP, semantic vector retrieval and unattended extraction, image/PDF inputs, branch-tree UI, global quick-ask and automated model load/unload need independent follow-on gates. They are not assumed complete by this release plan.

## Discussion resolutions

Grok proposed an axum shell first to avoid getting blocked by native tooling. Sol has installed a workspace-local Rust toolchain and prepared the Tauri shell; retain Tauri and verify native compilation immediately. The separate core modules stay independent of Tauri wherever practical. No scripted product walkthrough is included; disconnected mode supports management only and explains that generation needs the desktop app and a configured server. MiMo's cancellation, FTS, storage and permission findings are release gates. No automatic backend fallback or model load/swap will occur. Native audit records must exclude incognito and be bounded in retention. Actual hardware drives the system readout; no fixed 32 GB recommendation.

## Verification gates

All three main agents must assess the final implementation. Required automated checks: strict Svelte/TypeScript, frontend unit tests, frontend production build, Rust tests and clippy, fragmented-stream mock-backend integration, SQLite reopening and FTS CRUD, context overflow and thought stripping, workspace escapes and write TOCTOU checks, stale approvals and cancellation, browser flows for chat management/settings/memory/search and honest disconnected states. Native launch and bundle size are measured on this Mac. Real inference quality and target M2 Pro 32 GB performance are explicitly unverified unless the required backends/hardware become available.

## Corrected assumptions

Identical models hosted by different servers occupy separate memory. Chat and agents reuse the selected resident backend by default. Backend discovery is read-only; no warmup, capability generation probes or implicit model swaps. Inference is restricted to explicit loopback endpoints for this release. External network tools default off. Unknown RAM/model metadata is displayed as unknown and token budgets are heuristic estimates until a backend reports actual usage.


## Final implementation clarifications

Agent mode prioritizes JSON actions and requests the fastest reasoning settings (for GPT-OSS, `reasoning_effort: "low"` — low reasoning, not zero); a server may still perform internal reasoning. Plain chat retains supported family-specific Fast/Deep parameters. A stable prefix means Hearth sends stable request bytes, not that a server's chat template or cache is stable. The current release uses conservative byte-based estimates and lexical memory retrieval. Raw Harmony streaming, exact tokenization, embedding search and automatic memory extraction remain deferred.
