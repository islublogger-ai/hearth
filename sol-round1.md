# Round 1 — sol (gpt-5.6-sol)

## 1. Brutal critique of `project.md`

The document is not one nine-week product. It is a chat client, model manager, hardware-admission controller, agent runtime, memory/RAG system, benchmark suite, menu-bar utility, automation host, and LAN client sharing a name. The P0/P1/P2 labels do not fix that: M0–M4 still contain multiple independently risky products, and several claimed invariants contradict each other. A credible first release needs one thin end-to-end path and measured claims; this spec mostly supplies a large feature inventory and optimistic estimates.

### What breaks first on the M2 Pro

- **Duplicate model residency.** `Chat → LM Studio` and `Agent → oMLX` cannot reuse one loaded model. They are separate processes with separate weights and KV state. The Appendix D “Balanced” preset appears to count a ~17 GB Qwen model once even though touching both routes can load it twice. Automatic fallback can cause the same failure. This is the first likely path to allocation failure, swap, or a killed server.
- **The fit equation is not a fit test.** `wired_limit − currently loaded − weights − KV` mixes a Metal ceiling with incomplete and inconsistently defined observations. It omits backend/Metal workspaces, allocator slack, temporary prefill allocations, WebKit, embeddings, page cache, compression, other applications, and races after the snapshot; “currently loaded” can also already include weights/KV and be double-counted. `iogpu.wired_limit_mb` may be unset and is not free unified memory. The only honest output is a conservative **estimated footprint** or **unknown**, followed by actual post-load pressure. A green “Fits?” promise is indefensible.
- **The default 32k context is too aggressive as a default for a 32 GB machine.** The stated 17 GB weights plus ~3 GB KV arithmetic is already close to the usual Metal working-set ceiling before scratch space or an embedding model. Start at 8k/16k and expand only from measured headroom. The 96 KiB/token arithmetic may be correct for that one unquantized KV layout; it is not a backend-independent admission formula.
- **MoE is not small in residency.** A3B lowers active compute; it does not make the full ~30B quantized weights disappear. “Background jobs reuse the loaded model, so this costs almost nothing” is false: it avoids another weight load but still consumes prefill/decode time, KV, scheduler capacity, and energy. Continuous batching does not imply a low-priority API and can worsen interactive TTFT.
- **The speed/cache numbers are hypotheses, not product guarantees.** Quant implementation, context length, prompt shape, engine version, thermals, cache state, and batching all matter. SSD KV restoration is not “almost free,” and a byte-identical application-level block does not prove an identical token prefix after a backend’s chat template, tool serialization, special tokens, and generation marker. Only measured cache timing should appear in the UI.
- **The app budgets are mutually hostile until measured.** `<25 MB`, `<150 MB` aggregate idle RAM, `<800 ms` cold start, three launch-time probes, WebKit, Shiki grammars, KaTeX, SQLite, and native polish are not simultaneous facts. They are benchmark questions with precisely defined measurement boundaries.

### Backend architecture is too optimistic

- “OpenAI-compatible” describes a loose request/response family, not common semantics. Model metadata, structured-output schema subsets, tool deltas, finish reasons, usage timing, error bodies, vision content, cancellation, and lifecycle control differ. `/v1/models` generally cannot supply the promised quant, format, loaded bytes, max context, or cache state; those fields must be nullable and provenance-labelled.
- `Backend::capabilities(&self)` is the wrong abstraction. Capability is at least `(endpoint, exact model artifact, server/engine version, chat template/config)`, and the value is tri-state: supported, unsupported, or unknown. A tiny probe proves that one request was accepted once; it does not prove the option was honored or that tool use is reliable. Probing can itself JIT-load a 17 GB model. Probe lazily and explicitly, not every model on connect.
- The spec alternates between caching a probe “per backend version” and choosing a strategy per “backend × model.” The latter is still insufficient without exact artifact/config. This is not bookkeeping trivia; a wrong positive enables unsafe agent behavior.
- A guessed canonical model key is not identity. Names routinely omit the exact weights, tokenizer, instruct/base status, chat template, quant method, context/RoPE settings, or vision variant. Keep `(endpoint_id, opaque_backend_model_id)` as identity. Parsed family/size/quant fields may be low-confidence display hints and user-confirmed aliases, never grounds for routing, profile selection, deduplication, or deletion.
- Automatic fallback is bad policy. It can load a second large model, change prompt/template/tool semantics mid-conversation, invalidate cache assumptions, and transmit private content to a different host. A toast after transmission is not consent. No agent step should ever be replayed automatically after an ambiguous partial failure.
- LM Studio’s current native API is `/api/v1/*`, and its documentation recommends v1 for new work; `/api/v0/models` should not be the only management contract ([LM Studio API](https://lmstudio.ai/docs/developer/rest)). Pin supported server versions and negotiate extensions instead of freezing a moving endpoint into the core trait.
- The request lifecycle is missing. Hearth needs run IDs, one-active-run/replacement semantics, stale-event rejection, bounded SSE/frame buffers, fragmented UTF-8 and multiline-event handling, cancellation state, partial-response persistence, app-close behavior, and a rule forbidding automatic retry after visible output. Closing the HTTP stream means “the client stopped listening,” not “the backend instantly stopped compute.”

### Small-model blind spots

- **“1B–35B” is not a capability class.** Parameter count is a weak proxy compared with exact fine-tune, template, quant, engine, and task. A 1B/4B model is not a credible general multi-step agent merely because its JSON is grammatical. Agent features should be eval-gated for the exact model/backend combination; family and size heuristics are only hints.
- **Constrained decoding solves syntax, not agency.** Schema-valid JSON can choose the wrong tool, fabricate a path, supply destructive but valid arguments, or stop too early. Not every grammar engine supports an arbitrary `oneOf`, and forcing both tool calls and final Markdown through a JSON-string envelope complicates streaming. “They parse every time” is not a reliability claim we can make.
- **The design tries to repair weak models with more weak-model calls.** Classifier → planner → executor → observation summariser → memory extractor compounds error and latency. A bad plan anchors later steps; a bad summary permanently removes evidence; a self-reported confidence is not calibrated. Deterministic host logic and visible user correction should be preferred.
- The single-enum tool router cannot handle legitimate cross-group tasks. It also adds a failure boundary before the real request. Exposing a deterministic set of three to five relevant tools is safer than asking a weak model to select one irreversible group.
- “Plan-first below 14B” and “ten steps is a short horizon” have no evidentiary basis. Parameter count does not predict whether planning helps, and semantic error compounds rapidly across ten calls. Make both eval-derived per exact model/task; start at three to five steps.
- Observation compression must not be generic LLM summarisation. It will delete exact filenames, error strings, line numbers, and data. Use tool-specific deterministic projections, bounded excerpts, and pagination/handles to retained output.
- JSON repair is dangerous around executable actions. Removing an unambiguous trailing comma or extracting one unambiguous envelope can be semantics-preserving; changing quotes, closing braces, or stripping arbitrary prose may change intent. Never repair or invent tool arguments. Strictly validate exact fields, then reprompt. Every mutating action still requires a host-side, one-use approval bound to the exact run and bytes/diff.
- Family-wide profiles are too coarse and internally inconsistent. The Qwen profile comment calls for a different non-thinking sampler, but the TOML has one `top_p`. Unknown models cannot conservatively default to constrained decoding when schema support is itself unknown. `top_k`, `min_p`, reasoning controls, and native tool formatting are not uniformly accepted by OpenAI-compatible endpoints.
- `chars / 3.5` is acceptable only as a visibly rough upper-budget heuristic with large headroom; it is badly variable for CJK, code, JSON, emoji, and different tokenizers. A prior aggregate `usage` total cannot “correct” the token count of arbitrary future slices. Context overflow needs a safe retry path.
- “Newest first” trimming must mean *select newest complete interaction groups backwards, then emit them chronologically*. Reversing turns or orphaning a tool result from its call will break even a good model.

### Memory, database, and security faults

- The retrieval pipeline is three uncalibrated scoring systems presented as one. Cosine thresholds are embedding-specific; vector similarity does not detect contradiction; BM25, RRF, and reranker scores do not share a meaningful global `0.35` threshold without explicit normalization/calibration. A `>0.9` similarity rule cannot decide “duplicate versus conflicting fact.”
- Automatic extraction of `instruction` memories is a durable prompt-injection channel. Extracted memory must remain attributed, reviewable data—not authority—and model confidence must never enable auto-approval. Pinned memory also needs a hard slice budget.
- One `conversations.summary/summary_upto` pair cannot correctly represent a branched/edited conversation. Summaries must be branch-specific and invalidated when covered messages change.
- The external-content FTS5 tables have no insert/update/delete triggers or explicit dual-write contract, so their indexes will drift or remain empty. SQLite makes synchronization the application’s responsibility ([FTS5 external-content tables](https://www.sqlite.org/fts5.html#external_content_tables)). `PRAGMA foreign_keys=ON` also has to be enabled per connection; declarations alone do not enforce it.
- `vec0(embedding float[1024])` is fixed-dimension and contradicts arbitrary embedding models. Changing `embedding_meta` and “re-embedding” cannot put a different dimension into that table; it requires a new/rebuilt table and migration. More immediately, MIMO’s proposal to create dormant vec tables with “zero new deps” cannot work: SQLite cannot create `vec0` unless sqlite-vec is packaged and loaded, and sqlite-vec is still pre-v1 ([sqlite-vec](https://github.com/asg017/sqlite-vec)).
- `network_tools = true` directly contradicts “offline by default.” It must be false. A fetched document wrapped in `<untrusted>` is still only a prompt hint, not a security boundary.
- Canonicalize-then-open does not prevent symlink/TOCTOU substitution. File tools need descriptor-relative traversal from an anchored workspace, no-follow semantics, bounded regular-file I/O, and approvals bound to exact resolved operation content. Writing outside a workspace should be impossible, not an approvable “Dangerous” action.
- `sandbox-exec` plus a timeout is not a credible security boundary for arbitrary Python/shell execution. Do not ship that claim. Likewise, “delete the DB file really wipes data” promises more than APFS/SSD, WAL sidecars, snapshots, and backups permit; promise logical deletion, not secure erasure.

**Failure order:** on the target Mac, duplicate residency/false fit admission fails first; in the transport, cancellation/stale-stream races fail before the fancy model logic; on small models, the first silent failure is a schema-valid but semantically wrong action; in memory, the first lasting failure is a confidently extracted falsehood. The spec spends far more detail on UI chrome and future features than on these state and trust boundaries.

## 2. Verdict on MIMO’s v0.1 IN/OUT list

**Cut deeper horizontally. Accept every current OUT item, and remove more from IN.** The proposed IN list is still a multi-backend platform plus model taxonomy, hardware advisor, parser library, rich chat client, and test framework. That is not one vertical slice.

### Keep in v0.1

- Tauri/Svelte shell, restrained light/dark tokens, and a normal titlebar if overlay/vibrancy becomes a schedule sink.
- Used-only migrations for endpoints/settings, conversations, and messages. Add tables when features exist. Use simple search initially or implement FTS with its synchronization and migration tests; do not copy the entire speculative §12 schema.
- One explicitly selected endpoint and exact opaque model ID per run. Implement bounded, cancellable OpenAI-compatible streaming and model listing; make LM Studio the single verified target. oMLX and Ollama may be manually configured compatible endpoints, but do not claim native management or semantics until live-tested.
- Correct run isolation: run ID on every event, stale-event rejection, bounded bodies/frames, stop, regenerate, persisted completed/error/cancelled states, and no retry after output.
- Basic Markdown/code rendering, sanitised links/HTML, conversation persistence/list, and model selection. Highlight completed code fences lazily; “Shiki at 60 fps while reparsing every token” is not a v0.1 requirement.
- The stable-prefix context assembler, complete chronological interaction-group trimming, a conservative heuristic budget with explicit safety margin, and a raw request/prompt inspector. Budget transparency is a real differentiator; fake precision is not.
- A small profile seam: conservative default plus one exact Qwen profile if verified. Do not ship four family-wide policy files that silently force unsupported sampling/tool parameters.
- Mock-server tests covering fragmented/multiline SSE, split UTF-8, missing/final usage, malformed/oversized frames, error bodies, cancellation, late stale events, and backpressure. Add one explicit live LM Studio/target-Mac release gate; mocks cannot validate model metadata, server-side cancellation, memory behavior, or cache claims.
- At most a read-only system snapshot. Any footprint number is labelled estimated with provenance, safety margin, and an `unknown` state. No automatic unload or pressure action.

### Cut from MIMO’s IN list

- Dormant vec/memory/document/episode/agent/audit tables.
- Dedicated oMLX and Ollama extensions, launch-time capability probes, role routing, automatic fallback, and backend load/unload control.
- Canonical-key-based merging/routing/deduplication. Retain raw IDs; normalization is display metadata only.
- Green/amber/red “Fits?” promises and cache-hit/prefill metrics the selected API did not actually report.
- The tolerant parser/repair module while the agent loop is OUT. Dead future code is not a differentiator and is a dangerous execution surface waiting for an accidental caller.
- Auto titles and any other background model calls.

This scope is honestly a **chat-core v0.1**, not “Hearth polished enough to deploy.” If the round must demonstrate the agent product rather than establish its substrate, trade backend breadth, probing, Fits, Shiki, and model merging for exactly one narrow end-to-end agent path (for example, calculator plus read-only scoped files). Do not ship a parser museum with no loop. I would still keep writes, network tools, automatic memory, and embeddings out until their trust boundaries are complete.

## 3. Lane pick

I accept the Rust backend/system lane after the scope reduction above.

My lane should own the normalized internal request/event model, endpoint normalization, generic OpenAI-compatible transport, bounded streaming/cancellation, LM Studio metadata enrichment, opaque model identity plus nullable display hints, tri-state capability records, and a read-only system snapshot. oMLX is initially a configured compatible endpoint. Ollama-specific discovery is the first optional extension, not a launch requirement. `router.rs` may resolve an explicit selection; it must not silently fall back. The backend layer reports facts and uncertainty; integration policy stays above it.

**Single riskiest piece:** truthful cross-process resource admission/load coordination. HTTP/SSE is difficult but testable. A “Fits” decision is intrinsically based on incomplete, racing observations across macOS and independent server processes; a confident wrong answer can destabilize the machine. If the inputs are incomplete, my implementation will return `unknown`, not manufacture green.

The integration contract must cover more than command/event names and JSON shapes: run identity, ordering, terminal-state rules, cancellation/replacement semantics, size bounds, error taxonomy, nullable/provenance-labelled observations, capability `unknown`, and compatibility/version evolution are part of the contract.

## 4. Design hill I will die on

**No silent backend/model substitution or implicit second-model load.** Every run is bound before transmission to an explicit endpoint and exact opaque model ID. A failure stays visible unless the user authorizes a different target *before* any retry sends content. A guessed canonical alias never proves equivalence, and an agent step with uncertain execution is never replayed. This is simultaneously a memory-safety, privacy, reproducibility, and small-model-correctness boundary.

## 5. What is actually wrong, including errors in MIMO’s message

Specific false or unsafe premises in the spec:

1. Separate LM Studio and oMLX processes do not share “the same loaded model”; the Balanced preset and default role routing can double residency.
2. Backend-wide capability flags and family-wide profiles are the wrong granularity. Exact artifact/template/engine configuration matters.
3. Constrained syntax does not make tool use reliable, and `<untrusted>` delimiters do not make content safe.
4. Canonical names cannot establish model equivalence, safe fallback, or duplicate files.
5. A private/implementation-specific Metal limit and a KV formula cannot prove fit; continuous batching does not prove low-priority scheduling; closing a request does not prove compute stopped.
6. The FTS schema is unsynchronised; the vector schema cannot support arbitrary dimensions; the RRF threshold has no defined common score scale.
7. Network tools enabled by default are not offline-by-default behavior. Canonicalized paths and `sandbox-exec` are not the stated security boundaries.
8. The default one-group tool router makes cross-group tasks impossible, while plan/router/summariser calls multiply the exact semantic errors small models are bad at.

Specific errors in MIMO’s Round 1 proposal:

- **“Vec tables created but unused — zero new deps” is technically false** and can make the first migration fail. Remove them.
- Importing the full §12 schema is not “leaving room”; it freezes known-broken, unused structures into migration history. Used-only migrations are cheaper to evolve.
- `chars/3.5 + usage correction` is not accurate correction. It is a rough estimate that needs language/model headroom and truthful labelling.
- Parser/repair IN while the only consumer, the agent loop, is OUT is dead scope. Either cut it or trade backend breadth for one narrow end-to-end loop.
- LM Studio primary plus Ollama secondary does not resolve routing; it introduces silent fallback policy and a possible second load. Select one endpoint explicitly.
- oMLX port and LM Studio MLX-versus-GGUF preference do not block clean architecture. Opaque endpoint/model IDs, configuration, nullable facts, and `unknown` capabilities do. File format alone should not select semantic profile defaults.
- Three backend extensions + probe + identity merger + router + hardware fit monitor is a compatibility test matrix, not a v0.1 vertical slice.
- The mock backend is necessary, not sufficient for “deployable.” At least one live LM Studio gate on the target Mac is required; all other backends/features must be labelled unverified until exercised.
- The proposed integration contract is incomplete unless it freezes lifecycle semantics and uncertainty, not merely names and JSON payloads.
- MIMO’s lane is a critical-path bottleneck: scaffold, DB, context, profiles, parser, command/event contract, and final integration all converge there. Freeze the minimal contract and fixtures first, cut the unused parser/schema work, and do not let one lane become the only place the other two can integrate.

— sol
