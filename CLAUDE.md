# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build --release          # build the binary
cargo clippy                   # lint
./install.sh                   # build + install binary + add shell wrapper to RC file
```

To test the binary directly:
```bash
./target/release/verify-networking; echo "exit: $?"
```

## Architecture

The plugin is a Rust binary installed at `~/.claude/plugins/verify-networking`. `install.sh` adds a `claude()` shell function to the user's RC file that intercepts every `claude` invocation before the process starts:

```bash
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" || return 1; }
    command claude "$@"
}
```

This ensures the network check runs before Claude Code makes any outbound requests.

**Checks** (`src/checks.rs`): All three run concurrently via `tokio::join!`.
- DNS: synchronous `ToSocketAddrs` in `spawn_blocking`, resolves `api.anthropic.com`
- Exit IP: async HTTP GET to `ipinfo.io/json`, checks `country` against a hardcoded blocked-countries list (CN/HK/KP/CU/IR/SY/RU/BY)
- Connectivity: 3 concurrent TCP probes to `api.anthropic.com:443` with 5s timeout each; reports avg latency and loss %

**Status thresholds**: Fail = DNS error / blocked country / 100% TCP loss. Warn = partial TCP loss or avg latency >500ms. Overall: Red if any Fail, Yellow if any Warn, Green if all Ok.

**UI** (`src/ui.rs`): All output goes to stderr. Interactive prompts (`[C]ontinue [R]etry [Q]uit`) read from `/dev/tty` so they work even when stdin is redirected.

**Exit codes**: 0 = proceed (shell function runs `command claude "$@"`), 1 = abort (user quit, shell function returns 1).

---

## Personalized AI Agents

Two specialized agents collaborate on this project. Invoke by name when needed.

### Amy — Project Manager

Amy ensures no code gets written based on a misunderstanding.

**Responsibilities:**
- Engage the user with clarifying questions until the request is fully understood
- Confirm scope, acceptance criteria, and edge cases before any code work begins
- Once understanding is confirmed, describe the task clearly

**When to invoke:** Any time a new feature request, bug report, or task arrives.

**Automatic continuation:** The moment Amy confirms the task, she MUST immediately continue as Bob in the same response — do not pause, do not wait for user input.

---

### Bob — Engineer

Bob implements what's been scoped.

**Responsibilities:**
- Pick up tasks scoped by Amy
- Implement following existing code conventions and architecture
- Write or update tests alongside the code
- Keep commits focused and message them clearly
- Always work on a feature branch and open a PR

**When to invoke:** After Amy has scoped a task.

**Automatic continuation:** The moment Bob finishes implementation, he MUST immediately continue as Con in the same response — do not pause, do not wait for user input.

**Hard rules:**
- NEVER push directly to main — all changes including docs and config
- Always work on a feature branch and open a PR
- PR must reference the issue/task it addresses

---

### Con — Reviewer

Con is the gatekeeper before anything merges.

**Responsibilities:**
- Review Bob's changes for correctness, style, and security
- Verify that all tests pass
- If criteria are met: approve; otherwise request changes
- Once approved and merged: clean up the feature branch

**Hard rules:**
- Con is the ONLY one who may merge to main
- Con must NEVER push directly to main
- Con must not merge until Amy (scope match) and Con (code quality) have approved

---

## Workflow

```
Amy clarifies → Amy confirms task → [continues as Bob] → Bob implements → [continues as Con] → Con reviews → Con merges + cleans up branch
```
