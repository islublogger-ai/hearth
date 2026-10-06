# Hearth v0.1 — Release Notes (personal release)

Scoped v0.1 agreed in [BUILD-PLAN.md](BUILD-PLAN.md) and critiqued in
[agent-comms.md](agent-comms.md). A native macOS chat/agent harness for local
inference servers (LM Studio, oMLX, Ollama). It does not host model weights.

**Run it:** open `release/Hearth.app` (Apple Silicon, unsigned/ad-hoc). Build
from source with `npm ci && npm run package` (or `npm run desktop` for a dev
launch). The workspace-local Rust toolchain under `.tools/` is recognised by the
scripts when present.

---

## Observed results — all runs on this build Mac (MacBook Air M1, 8 GB), 2026-10-06

| Check | Command | Result |
| --- | --- | --- |
| Rust unit + integration tests | `cargo test` (src-tauri) | **52 passed / 0 failed** (44 lib incl. 12 storage tests, 8 streaming integration) — log: `.agent-work/final-suite.log` |
| Rust lints | `cargo clippy --all-targets -- -D warnings` | **clean** (0 code warnings) |
| Svelte/TS strict check | `npm run check` | **0 errors, 0 warnings** |
| Frontend unit tests | `npm test` | **21 passed** (6 files) |
| Frontend production build | `npm run build` | clean — 202.34 kB JS / 81.06 kB gzip, 10.93 kB CSS |
| Browser flows (Playwright) | `npm run test:e2e` | **20 passed** in 18.6 s |
| Dependency audit | `npm audit` | **0 vulnerabilities** |

## Native measurements (this Mac)

- **Bundle:** `release/Hearth.app` — **9.82 MiB**, Mach-O thin **arm64**,
  `CFBundleIdentifier app.hearth.local`, version 0.1.0,
  `LSMinimumSystemVersion 13.1`.
- **Signing:** ad-hoc only (`codesign` identifier `hearth-4f70091b65a4e1df`).
  No Developer ID, no notarization. Gatekeeper will warn on other machines.
- **Launch:** verified multiple cold launches; **~100 MB RSS for the main
  process at 6 s idle** (102,528 KB measured). macOS WebKit helper processes
  accompany the app and were **not separately metered** — treat the figure as
  the harness process only.
- **First-run persistence:** a real launch created
  `~/Library/Application Support/app.hearth.local/hearth.db` with
  `PRAGMA user_version = 1` and the full schema (16 tables including FTS5
  shadow tables) — migrations executed correctly outside of tests.

## Review decisions (three-agent review record)

- **sol (verification subagent):** *approve* the frontend for the scoped
  personal v0.1 — observed browser rendering + the test IPC fixture only;
  explicitly excluded native IPC and live inference from its approval scope.
- **sol (core pass):** native cross-review fixes landed (bounded SSE events,
  UTF-8 observation budgets, one-use run-bound approvals, literal thinking tags
  in JSON actions, output-limit partial preservation, resolved backend/model in
  stats, Ollama Qwen3 `reasoning_effort` handling). Set the reasoning-wording
  requirement used below and in README.
- **grok (final release review, `grok-final-review.md`):** *approve-with-fixes.*
  Every finding dispositioned: the two over-claiming reasoning sentences in
  README/BUILD-PLAN were corrected to the required precision; the stale 23:36
  bundle was rebuilt from the corrected tree (its `12.0` minimum became `13.1`);
  this file and `release/Hearth.app` were created; the final suite log was
  persisted. No frontend change was blocking.
- **mimo (pro, storage lane + release handover):** storage contract fully
  implemented and re-verified by execution after the core-pass corrections
  (transactional delete/audit, blank-search contract, genuine-error propagation,
  load-time settings hygiene, stats-rate validation). **Final assessment:
  APPROVE for the personal v0.1 release.**

## Reasoning controls — precise behavior

Hearth **requests** model-family reasoning controls where supported. GPT-OSS
Fast requests **low** reasoning (`reasoning_effort: "low"`), **not zero**;
servers may still perform internal reasoning. Agent mode requests the fastest
reasoning settings for valid JSON actions with the same caveat. LM Studio's
OpenAI-compatible reasoning controls **require live verification** and are
unverified. Stable prompt prefixes are Hearth's request bytes only — server-side
chat templates and caches are outside that guarantee.

## Unverified (explicitly, until the systems exist here)

- **Live inference quality** against real LM Studio/oMLX/Ollama models — no
  inference backend or model exists on this build Mac. The streaming, approval
  and tool paths were exercised against explicitly named test-only HTTP fixtures
  (`src-tauri/tests/streaming.rs`, service tests), never shipped as backends.
- **M2 Pro 32 GB performance targets** (project.md §15 speed budgets).
- LM Studio reasoning-control support, and interop with any installed
  inference-server version.
- Behavior under Gatekeeper on a machine other than the build Mac.

## Known limitations (documented, by design or disclosure)

- File writes are atomic with an approved-snapshot check; an external process
  changing a file inside the final compare-to-rename interval remains a
  documented race (macOS has no atomic content CAS). New files are created with
  exclusive no-replace publication.
- Token counts are conservative byte-based heuristics until a backend reports
  usage; estimate values are `≈`-labelled and unavailable speed is hidden.
- `thinking` text is stored with the message but never searched (deliberate
  privacy narrowing of the FTS index).
- Search is word-level (FTS) on message content plus case-insensitive substring
  on titles; substrings inside words are not matched.
- `cargo` reports a future-Rust-compat advisory for `nom v1.2.4` via `meval`'s
  dependency tree — cosmetic, no action possible from this repo.

## Deferred (not in this release, not simulated)

Embeddings/reranking/vector retrieval, automatic memory extraction and
summaries, model profiles/tokenizer budgeting, raw Harmony streaming, automated
model load/unload, shell/Python/web/MCP tools, attachments, branching,
quick-ask, background jobs, evals, signing/notarization. See README's deferred
list and project.md for the roadmap.

## Reproduce

```sh
npm ci
npm run check        # svelte-check strict
npm test             # vitest
npm run build        # production frontend
npm run test:e2e     # Playwright flows
npm run test:core    # Rust suite (uses .tools/ toolchain when present)
npm run lint:core    # clippy -D warnings
npm run package      # → src-tauri/target/release/bundle/macos/Hearth.app
```
