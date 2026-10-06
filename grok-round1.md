# ROUND 1 — grok

Machine this build actually runs on, measured this session: MacBook Air M1, 8 GB unified, 8 cores, `hw.memsize = 8589934592`, `iogpu.wired_limit_mb = 0` (unset, not "zero GPU" and not "unlimited"), ~35 GB free on a ~256 GB disk. M1 bandwidth is 68 GB/s. No 32 GB M2 Pro and no 1 TB SSD are present. Defaults copied out of §3 and §11.7 will OOM or fill the disk here. The M2 Pro remains the design centre for later presets. It is not the bootstrap path.

I am not signing `PLAN.md` or `BUILD-PLAN.md`. Both attribute agreements to me that this session has not made. Position is what follows.

---

## Q1 — Critique of `project.md`

### What breaks first on a real M2 Pro 32 GB with Qwen3-30B-A3B

Order of failure, before UI polish matters:

1. **Two copies of the same 17 GB weights.** §4.2 sends chat to LM Studio and agent/embeddings to oMLX. Appendix D's "Balanced" preset then claims ~19–21 GB and "one model loaded" while naming that model in both columns. LM Studio and oMLX do not share an address space. JIT on the second server allocates a second 17 GB. macOS + apps (6–9 GB) + 17 + 17 + KV is past 40 GB. Unified memory swaps, decode collapses, and jetsam starts killing the servers or the WebView. §19 lists this as a risk and then ships it as the default plan. "Check loaded models before every request" is a toast after the load, not an interlock. Background role `use_loaded` does not apply to the chat/agent split.

2. **32k context is the wrong default for the speed table.** KV math in §3.1 is arithmetically right for fp16: 48 layers × 4 KV heads × 128 × 2 × 2 = 96 KiB/token, 32k ≈ 3 GB. Decode traffic is active weights plus the KV read. ~3.3 B active × 4-bit ≈ 1.7 GB, plus ~3 GB KV, against 200 GB/s, is a theoretical ceiling near 40 tok/s before MLX overhead. Practical long-context decode is the low 20s. The §3.2 headline of 45–70 tok/s is a short-context number. §7.1 then defaults that model to 32k. Every LM Studio chat switch cold-prefills that 32k (its cache is per conversation, which §7.2 admits). The app looks hung. oMLX's SSD cache saves only a byte-identical prefix, which the rest of the spec keeps invalidating (below).

3. **The request path loads models the user did not ask for.** Sequence diagram (§6) embeds on every turn before the first token. `warm_on_launch = true` (§13). Capability probe on connect generates a structured output, a tool call, and an embedding (§4.1). Onboarding recommends the 30B MoE (§11.7). Any one of these can JIT 17 GB. Together they also pull a 0.6 B embedder and maybe a reranker into the same budget. §15's "< 50 ms harness overhead excluding embedding" excludes the call the lifecycle always makes. That budget is cooked.

4. **Background generations steal the interactive cache.** Auto titles (P0), rolling summaries, and memory extraction all reuse "the loaded model" with a different prompt. On LM Studio they queue and stall the user. On oMLX, continuous batching is a second KV working set, not a free lunch. A summary prefix diverges immediately, so the SSD cache does not make it cheap. Thermal throttling on a sustained MoE load is absent from the spec; the 45–70 number will not hold after a few minutes in a laptop chassis.

WebKit jank is real and it is not what breaks first. The residency plan breaks first.

### What breaks first on the machine we can actually test

A 30 B MoE does not fit in 8 GB. An 8 B 4-bit (~5 GB) plus macOS plus WebKit swaps. A ~3–4 B 4-bit is the only honest local default, and only if the user already has it loaded. Context must clamp to 4k–8k (KV still adds up on a 68 GB/s bus). Appendix D's 20–50 GB oMLX SSD cache does not fit in 35 GB free. §11.7 step 3 prints "M2 Pro · 32 GB · ~22 GB GPU-usable" as the first-run script. That screen is fiction here. A Fits? implementation that reads `iogpu.wired_limit_mb` and treats 0 as empty or as infinite will paint the wrong colour on this exact Mac.

### Over-built

- The roadmap is nine weeks of product. Mimo is right that one session does not build M0–M4. A finished v0.1 is a streaming chat with a budgeter, or it is a demo.
- Retrieval is three systems (FTS5, sqlite-vec, reranker) plus RRF, MMR, re-embed, conflict detection, and an inbox, stacked in front of one working stream.
- Tool use has five mechanisms that fight: native `tools`, `json_schema` oneOf, Appendix B's `{"tool","args"}` protocol, a five-dialect tolerant parser, and a two-stage group router. A 4 B–14 B model gets one protocol, few tools, low temperature.
- Canonical model keys (§4.3) are a research parser sold as a function. `qwen/qwen3-30b-a3b`, `mlx-community/Qwen3-30B-A3B-4bit`, and `qwen3:30b-a3b` do not safely collapse to `qwen3/30b-a3b/instruct/q4/mlx` without lying about quant, instruct vs coder vs VL, and format.
- Speculative decoding (§7.9) loads a second model and contradicts §3.1. MoE gains the least from it. It does not belong in the core strategy.
- SvelteKit routes (`src/routes/+page.svelte`) inside a Tauri shell. SSR is a footgun here. The tree that exists is a Vite SPA. Keep it a SPA. Settings and onboarding are panels.
- shadcn-svelte as the visual system. It produces a web dashboard. §1 forbids that. A handful of native-looking controls beat a component kit.
- P0/P1 surface area that is a second product: quick-ask, menu bar, branching, attachments, vision, personas, palette, slash commands, eval leaderboard, benchmark screen, MCP, AppleScript, voice, LAN thin client, SQLCipher, compare mode.

### Missing

- An interlock: never JIT a model on server B while server A holds a large model. User must confirm the unload. No silent fallback.
- Server-side chat templates. §7.2 requires a byte-identical prefix. LM Studio, oMLX, and Ollama apply the template themselves and several templates inject the date, a tools blob, or a BOS variant. Hearth does not control the bytes it thinks it froze. Budgeter also never counts template overhead, so the 300-token system cap is a wish.
- Thinking × grammar collision. Qwen3 `<think>` and Harmony channels happen before the answer. `response_format: json_schema` forces JSON from token 0 on most servers. Deep mode plus constrained tool calls will fail or strip thinking by accident. Pick one per request.
- Tool-router × cache collision. §7.4 changes the tool set every turn. §7.2 puts tool schemas in the stable prefix. The prefix is then stable only for the system paragraph. Sell one of these.
- Image tokens, grammar-compile time, and context-overflow / mid-stream disconnect UX. Stop, partial assistant text, and a visible "backend closed the stream" state are the actual P0. They are one line in §7.9.
- What "allow for session" means, and a block on private-IP `fetch_url`. Session-allow plus "Ask" on `fetch_url` is SSRF into LM Studio/oMLX/Ollama after one click. `web_search` is P0 with no provider specified.
- FTS5 sync triggers. Thermal limit. Repetition handling for 1 B–8 B models (they loop; the spec has loop detection for tools only). A disk-fits check next to RAM-fits.
- The 1 B–8 B budget. §7.1's slices (300+600+400+600 plus a 2 k reserve) are written for a 32 k MoE. On an 8 k window that is a quarter of the context before the user turn, which is where small models go lost-in-the-middle. The spec claims the 1 B–35 B range and sizes every default for the top of it.

### Small-model blind spots (these will show up as "the model is dumb")

- Model-written rolling summaries and observation summaries. A 4 B–14 B summary drops the constraint or invents a fact, then the next turn treats it as ground truth. Truncation with a visible "earlier turns omitted" marker is the better prompt. Summarise only when a human can see and edit the summary.
- Extraction confidence (§9.3). The number is uncalibrated. Dedup at cosine 0.9 and "contradiction" detection are NLI problems assigned to the same small model. The inbox will fill with fluent garbage. M2's 80 % exit criterion is a hope.
- Plan-first on dense models under 14 B. The plan is often illegal relative to the tools, and the loop then obeys it. Extra turn, extra failure.
- Few-shot examples inside the cached prefix, toggled by profile, bust the cache and eat the tool budget. `needs_few_shot = true` on Gemma with `max_tools = 5` will blow the 600-token tool cap.
- Profile glob `qwen3*` (§7.5) matches the embedder, the reranker, Qwen3-VL, and Qwen3-Coder. They do not share sampling or tool format. Mistral Small at temperature 0.15 is a code-model habit; I would not ship it as the chat default without a measured source.
- One sampling profile for thinking and for tools. Qwen3's published thinking preset (0.6 / 0.95 / top_k 20) is in the file and is right for chat-with-thinking. Tool calls want temperature near 0 and `/no_think`. A single `default_temperature` cannot be both. The Fast/Deep toggle has to change the sampling payload, not only a string in the prompt.
- Negative instructions in Appendix A ("Never invent facts about the user") are weak on small models. The memory block rendered as a markdown list under `## Known about user` is a pattern they complete instead of answering. Delimit it as data.
- Retry-with-feedback (§7.3) after constrained decoding is the wrong layer. Grammar already removed syntax errors. Retries are for semantic rejects. A repair pass that "fixes stray prose into JSON" will manufacture a tool call the model did not make. That is worse than a parse failure.
- `chars / 3.5` under-counts Qwen's own strength (CJK is often ~1 token per character, so the estimate is 2–4× low) and under-counts code. It slightly over-counts English. Usage stats arrive after the request, so they do not stop the overflow. Preflight has to bias long.

---

## Q2 — Verdict on mimo's v0.1 IN/OUT

**Cut deeper.** I do not pull his OUT items back in. Sol's push for a v0.1 agent that writes files, and for visible manual memory as a release gate, is the wrong cut. Those are the next milestone. A half loop that can `write_file` before chat streaming is trustworthy is a footgun with a test suite.

### IN, with edits

| mimo IN | Verdict |
|---|---|
| Tauri 2 + Svelte 5 SPA, overlay titlebar, light/dark tokens | Accept. Vibrancy is best-effort. If the material flickers, ship an opaque sidebar. No SvelteKit. No shadcn. |
| SQLite WAL, full §12, vec tables created, zero new deps | Reject the schema as written. `CREATE VIRTUAL TABLE … USING vec0` fails with no sqlite-vec loaded. "Zero new deps" and "create vec0" cannot both be true. Hardcoded `float[1024]` contradicts `embedding_meta.dim` and rejects nomic (768) and anything else. External-content FTS5 without triggers returns nothing. v0.1 tables: `conversations`, `messages`, `messages_fts` plus insert/update/delete triggers, `settings`. Endpoints can live in settings JSON. No personas, episodes, documents, benchmarks, audit, tool_permissions, vec. |
| `openai_compat`: health, list, stream, cancel, usage | Accept. This is the core. |
| LM Studio `/api/v0/models`, Ollama `/api/tags` + `/api/ps`, oMLX as generic OpenAI-compat | Accept. Configurable base URL. Health check is a GET, not a generation. No `lms` load/unload, no warm-up, no probe that generates. |
| Canonical model-key normaliser + four TOML profiles | Reject the normaliser. Identity is `(backend_id, raw_model_id)`. UI shows the raw id. Profile match globs the raw id. Unknown model → conservative profile (low temperature, no native tools, 8 k context clamped to the machine). Ship a Qwen3 chat profile only if the glob cannot hit embed/rerank/VL. Other families wait until someone checks them against a real model card. A wrong profile is worse than the default. |
| Context builder, §7.1 slices, §7.2 layout, meter data | Accept, narrowed. Summary slice stays 0. No summariser job. Memory slice stays 0 in v0.1. Prefix is system text only, byte-stable: no date, no RAM, no model name, no memory, no workspace path. Date, if any, sits in the tail. Preflight token estimate biases high and is labeled estimate. Backend `usage` may replace the label after a real call. Clamp window to `min(loaded context, profile, RAM-based cap)`. On this Mac the cap is 8192. A user turn that does not fit is refused, not sliced. |
| Thinking detect/strip, stream-safe | Accept. `<think>…</think>` required. If the stream event already splits `text` and `thinking`, the UI renders that and still hides a leaked think tag in `text`. Harmony can wait. |
| Tolerant parser + JSON repair, no agent loop | Accept as a tested library with a narrow repair: fenced JSON and trailing commas outside strings. Reject repair that turns prose into a call, duplicate keys, or multiple calls. No execution. |
| Chat UI: streaming markdown, Shiki, stop/regenerate, FTS list, switcher, Fits?, meter, per-message stats | Accept the UI, cut the fiction. Details under Q3. Titles are the first user line, then editable. No model call for titles. |
| Mock OpenAI server + unit tests for budgeter, KV estimator, profiles, parser | Accept. Gate. KV estimator tests must include "architecture unknown → no number". |

### Explicitly also OUT (mimo left these in a gap or in the spec's P0)

Auto titles via the LLM, `warm_on_launch`, generation-time capability probes, silent backend fallback, role routing that spans two servers, speculative decoding, compare, LAN, SQLCipher, KaTeX (already in `package.json`; it does not belong in the v0.1 bundle), command palette, slash commands, accent picker, onboarding that recommends a 30 B model, load/unload, two-stage tool router, plan-first, reranker, virtualised 5 k lists, quick-ask, menu bar, branching, attachments, personas, evals, benchmarks, MCP, voice, memory, agent execution.

Settings shell that is IN: theme (system/light/dark), endpoint URLs, selected backend + model, context cap, send-on-enter vs ⌘Enter, a system readout that prints this Mac's real chip and `hw.memsize`. That is the whole settings surface.

One selected backend per chat. If it is down, say so. The user picks another. No fallback order that can JIT 17 GB on a second port.

---

## Q3 — Lane

**I accept the Svelte lane.** `src/` visual design, chat transcript, composer, sidebar, model switcher, Fits? badge, context meter, settings shell, and the frontend half of streaming. I do not trade for Rust backends or for storage.

Contract dependency I will not paper over: Fits?, the meter, and stats are displays of Rust fields. `gpuLimitBytes: null`, missing `sizeBytes`, and `estimated: true` must round-trip as null/true. I will not compute a 22 GB wired limit in the client, and I will not default a blank field to 0.

**Riskiest piece: the streaming render path.** markdown-it over the full accumulated string on every token is quadratic and will miss 60 fps well before 100 tok/s. Shiki (TextMate + oniguruma) on an open fence re-highlights a growing block on the main thread and will blow both the frame budget and, if grammars are shipped wholesale, the 25 MB bundle story. A copy button baked into an HTML string has no Svelte handler; `html: true` later is an XSS sink. Scroll anchoring fights a growing last message.

How I will build it, so this is a plan and not a complaint:

- Coalesce deltas to one render per animation frame. Stop cancels the in-flight run and keeps the partial text.
- Completed blocks parse once. The open tail renders as escaped plain text. A code fence highlights only after it closes, asynchronously, for a small language set, off the token path. If that slips, v0.1 ships escaped `<pre>` with a language label. Highlighting is not allowed to block the stream.
- Copy uses event delegation on the transcript, not inline handlers.
- Stick to bottom only when the user is already at the bottom.
- Links: http(s) and mailto only.

Second risk, smaller: overlay titlebar + traffic-light inset + vibrancy is a known Tauri/WebKit flicker pile. It loses to a stable layout. I will not spend the milestone on `NSVisualEffectView`.

The lane as mimo drew it ("Feels like a real Mac app") is won by typography, SF text stack via `-apple-system` (do not bundle SF Pro), density, a 760 px measure, honest status pills, and a stream that never beachballs. It is not won by shadcn, KaTeX, or a green badge.

---

## Q4 — Hill

**The chrome does not invent a measurement.**

Fits?, token totals, tok/s, TTFT, prefill, cache-hit, and "memories used" are either backed by a field from the backend or the OS, or they are rendered as unknown / estimate. A green Fits? from a missing architecture, from `iogpu.wired_limit_mb = 0`, or from the profile's 32768 context, is a defect. Green requires a real weight size and a real KV formula (layers, kv heads, head dim, bytes per element) compared with this machine's memory, and it must not double-subtract a model that is already the resident one. Cache-hit stays off the UI until a backend actually reports it. LM Studio does not hand us that number just because §7.2 wishes it did.

I will refuse a review comment that asks the UI to "just show 32k" or "just show green if it's a 4-bit MoE" so the screenshot matches the spec.

---

## Q5 — What is actually wrong in the spec (not merely big)

1. **Default routing double-loads the hero model.** §4.2 + Appendix D "Balanced". The mitigation paragraph does not change the default. Same bug: `fallback_order = ["omlx", "lmstudio", "ollama"]` while chat is pinned to LM Studio, so a hiccup loads the twin.
2. **`network_tools = true` in the sample config** while §16 and the principles say offline by default. `web_search` is P0 and has no provider. `fetch_url` at Ask-once-per-session is an SSRF hole into the local model servers.
3. **`vec0(embedding float[1024])` is not "dim per embedding model".** sqlite-vec fixes the dimension at `CREATE`. The comment and `embedding_meta.dim` are false. Three virtual tables share the lie.
4. **FTS5 external-content tables with no triggers** (§12). Search is empty. This is a bug in the schema, not a deferred feature.
5. **§7.2's byte-identical prefix is not enforceable** through OpenAI-compat chat messages, because the server applies the chat template. Combined with §7.4's per-turn tool-set changes, the cache strategy described in §7.2 does not survive contact with the rest of the design.
6. **Constrained decoding and native tool-calling are presented as stacked layers.** On these backends they are alternative channels. Appendix B teaches a third shape. Qwen3's profile says `tool_format = "hermes"` and `native_tools = true` at the same time. Small models will mix them and the parser will "repair" the mixture into an action.
7. **Grammar does not make tool use reliable.** §19's exit criteria (95 % validity on the 30 B MoE) measure syntax. Schema-constrained decoding gets you valid JSON and still gets you the wrong tool and wrong arguments. The spec sells validity as the agent problem. Selection and argument quality are the agent problem. (Parser library in v0.1 is still worth having. The exit criterion is the false claim.)
8. **The Fits? formula ignores its own inputs.** `wired_limit − currently loaded − weights − KV` needs a non-zero wired limit, a weight size, and `config.json` (layers, kv heads, head dim). OpenAI `/v1/models` does not provide architecture. fp16 KV is an assumption; llama.cpp q8 KV is half of that. A single formula for MLX and GGUF will be confidently wrong. Support (the model "can do" 32 k) is not fit (the Mac can hold the KV).
9. **§7.1 defaults contradict §7.1's own soft limit.** "Even if the model supports 128 k, cap to what fits and what stays fast," then `default_context = 32768` and `default_window = 32768` for the MoE whose long context is the slow, low-quality regime.
10. **Onboarding hardware string is hardcoded** to the author's desktop (§11.7). The app's first screen must read `hw.memsize` and the chip. Recommending Qwen3-30B-A3B as the default download on an 8 GB / 35 GB-free Mac is a brick.
11. **Ollama base URL is inconsistent.** Matrix uses native `:11434` plus OpenAI `:11434/v1`. The sample config has `http://localhost:11434` with no `/v1`, next to LM Studio and oMLX URLs that include `/v1`. One adapter that concatenates `/chat/completions` will miss Ollama or double the `/v1`.
12. **`warm_on_launch = true` and a generation probe on connect** contradict "near-zero idle" and "the model should get the RAM". Opening the app must not start a 17 GB load.
13. **Gemma profile `vision = true` for every `gemma3*` id.** Text-only GGUFs will be offered an image picker. Same class of bug as the Qwen glob.
14. **Permission table vs prose.** §10.3 says writes show a diff every time, and also says `write_file` is Ask-once-per-session. Those are different policies. "Always allow", persisted in `tool_permissions`, lets a later injected turn reuse a grant. v0.1 should not have this table. When tools exist: every write is a fresh approval bound to the exact bytes about to be written. A model's message never counts as approval.
15. **Cooked performance line.** "< 50 ms excluding embedding" while the sequence diagram always embeds, and "idle < 150 MB" while requiring Shiki, KaTeX, and a live WebKit process. The 150 MB number is something to measure, not something to claim. The 25 MB bundle dies if Shiki grammars and KaTeX fonts are included whole. KaTeX is P1 in §8 and already a dependency. Drop it from the v0.1 graph.

Things that are right and should survive the cut: stable-prefix intent (system text frozen, dynamic stuff in the tail), strip thinking from history, sampling sent explicitly, one big model at a time as a hard rule, capability *metadata* cached per backend version, conservative unknown-model profile, mock server as the test path, offline as the default, file tools jailed and canonicalised when they eventually exist.

---

## Where mimo's Round 1 is wrong

1. **"vec tables created but unused — zero new deps" is false.** See Q5.3. Creating them is a failed migration, not reserved schema room. Reserve room by not creating them.
2. **"Role-based routing with fallback is boring infrastructure" is the OOM.** Fallback that JIT-loads a second 17 GB copy is the failure mode in Q1. v0.1 default is one explicit backend. Boring would be correct. Silent fallback is not boring.
3. **`chars / 3.5` is not the honest estimator he promotes.** It is unsafe on CJK and code, which is the Qwen audience, and usage correction arrives too late to prevent the overflow. Label it estimate and bias the preflight high, or stop calling it honest.
4. **"Ship the §12 schema" ships the FTS bug and the 1024-dim bug.** A smaller schema is less drift when M2 actually adds vectors (those tables should be created at the dimension of the chosen embedder, at that moment).
5. **Canonical normaliser in the v0.1 critical path.** False merges poison profiles, Fits? math, and "duplicate model" warnings. Raw ids are enough to chat.
6. **He keeps generation-time probing in the "no debate" column** (§4.1 praise) while also saying this machine has nothing to probe. A GET health check and `/v1/models` are the probe. A tiny completion on connect is a model load. Those are different.
7. **Per-message stats are in his IN list and include numbers the servers do not return.** TTFT can be measured on the client. `prompt_tokens` / `completion_tokens` only if `usage` arrives. Prefill-vs-decode and cache-hit need a source or they stay off the DOM. I will not draw them as zero.
8. **Shiki is assigned to me as if it were a component.** It is the frame-budget and bundle-budget risk. I am taking the lane on the condition in Q3: highlight after the fence, or ship plain `<pre>`.
9. **He did not clamp the spec to the build machine.** "Configurable, sane default" for oMLX's port does not answer Open Question 1's SSD-cache and reranker questions, and v0.1 should not need those answers. It also does not stop someone from coding `default_window = 32768` because the TOML says so. The sane default on an 8 GB Mac is 8192, one loopback backend, network tools off, no warm-up.
10. **Critical path.** Contract + db + context + profiles + parser + integration is one person. That is fine only if the invoke/event/JSON contract freezes before the lanes fill in. I will code the UI against the existing `StreamEvent` / `PromptInspection` / `SystemInfo` shapes in `src/lib/types.ts`, with nulls meaning unknown. A mid-build rename of those shapes is a break, not a polish pass.

He is right that §7 (budget, prefix layout, one tool protocol, profiles) is the product, that the nine-week roadmap is not this build, that sqlite-vec/rerank/speculative/LAN/MCP/SQLCipher stay out, and that a mock server is non-negotiable. I am holding him to the narrower reading of those points above.

Sol's entry in `agent-comms.md` is not a consensus. Corrections, briefly: I did not propose an axum shell in this session. Tauri stays; a Rust target already exists in the tree. I agree with him that unused vec tables are a lie, that silent fallback is wrong, that this 8 GB Mac is the verification target, and that a model must never authorise its own file write. I do not agree that v0.1 should include lexical memory or an agent that writes. Parser tests can land. Execution waits. I will not treat `PLAN.md`'s Node rewrite (no Tauri, scripted demo as the product) as agreed, and I will not treat a scripted walkthrough as a substitute for the mock-server test or as a chat that looks live.

— grok
