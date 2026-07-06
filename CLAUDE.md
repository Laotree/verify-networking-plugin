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

# Test daemon mode (starts proxy, Ctrl+C to stop)
./target/release/verify-networking --daemon

# Test daemon mode with custom port
./target/release/verify-networking --daemon --port 9999
```

## Makefile targets

```bash
make          # debug build
make release  # release build
make test     # run tests
make lint     # clippy
make fmt      # format source
make clean    # remove build artifacts
make install  # build + install + patch shell RC
make hooks    # install git pre-push hook
```

## Architecture

The plugin is a Rust binary installed at `~/.claude/plugins/verify-networking`. `install.sh` adds shell wrapper functions to the user's RC file that intercept every `claude` (and `codex`, if installed) invocation before the process starts:

```bash
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" claude || return 1; }
    command claude "$@"
}

codex() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" codex || return 1; }
    command codex "$@"
}
```

The tool name (`claude` / `codex`) is passed as the first argument so the binary knows which API endpoint to probe. This ensures the network check runs before either tool makes any outbound requests.

**Checks** (`src/checks.rs`): All three run concurrently via `tokio::join!`. Targets differ by tool:

| Check | Claude target | Codex target |
|-------|--------------|--------------|
| DNS | `api.anthropic.com` | `api.openai.com` |
| Exit IP | `ipinfo.io/json` — blocks CN/HK/KP/CU/IR/SY/RU/BY | same |
| Connectivity | 3 × TCP to `api.anthropic.com:443`, 5 s timeout | 3 × TCP to `api.openai.com:443`, 5 s timeout |

**Status thresholds**: Fail = DNS error / blocked country / 100% TCP loss. Warn = partial TCP loss or avg latency >500ms. Overall: Red if any Fail, Yellow if any Warn, Green if all Ok.

**Trace** (`src/trace.rs`): On any non-green result, runs `mtr --report --no-dns -c 3` (preferred) or `traceroute -n` toward the API host. If [nali](https://github.com/zu1k/nali) is on `$PATH`, output is piped through it to annotate each hop IP with geolocation data; the trace header then reads `mtr + nali` or `traceroute + nali`. Falls back silently to plain output if nali is absent or times out (5 s guard). After printing the hops, if the exit IP from the `ipinfo.io` check is absent from the trace output, a warning is shown — this indicates the route to the API host differs from the route ipinfo.io observed (common with transparent proxies or split-tunnel VPNs).

**UI** (`src/ui.rs`): All output goes to stderr. While the trace runs, an animated braille spinner is shown so the user knows a slow probe (up to 30 s) is in progress; the spinner line is cleared before the hop list prints. Interactive prompts (`[C]ontinue [R]etry [Q]uit`) read from `/dev/tty` so they work even when stdin is redirected.

**Exit codes**: 0 = proceed (shell function runs `command claude "$@"`), 1 = abort (user quit, shell function returns 1).

### Daemon / Proxy Mode (`src/proxy.rs`) — v0.2.0

The daemon runs as an HTTP CONNECT proxy on `127.0.0.1:<port>` (default 8443). It accepts CONNECT requests from any client and proxies TCP connections to the real target.

**Monitored hosts**: `api.anthropic.com:443` and `api.openai.com:443`. All other CONNECT targets are proxied transparently without network checks.

**Connection flow**:
1. Client sends `CONNECT api.anthropic.com:443 HTTP/1.1`
2. Daemon checks session block list (per-session, persisted in `Arc<Mutex<SessionState>>`)
3. If not blocked → runs `checks::run_all(target)`
4. If all green → `HTTP/1.1 200 Connection Established` → `tokio::io::copy_bidirectional`
5. If risk detected → holds the connection → prompts via `/dev/tty`:
   - `Continue` → proxy connection
   - `Retry` → `HTTP 503 Service Unavailable` (client retries)
   - `BlockSession` → add host to session block list → `HTTP 403 Forbidden`
   - `Quit` → `HTTP 403 Forbidden`

**`Choice::BlockSession`** is consumed only in daemon mode; in startup mode it exits 0 (proceed).

**Launch**: `verify-networking --daemon [--port PORT] [codex]`. Parsed in `main.rs` before the startup check logic.

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
