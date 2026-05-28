[![MIT License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

# verify-networking

Network pre-flight check for AI CLI tools — runs before **Claude Code** and **Codex CLI** start, verifying DNS resolution, exit IP region, and TCP connectivity with 🟢🟡🔴 status.

## How It Works

`install.sh` adds shell wrapper functions to your RC file that intercept `claude` and `codex` invocations before the process starts:

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

Each wrapper passes its tool name as an argument so the binary knows which API endpoint to probe.

Before either tool starts, three checks run concurrently:

| Check | Claude target | Codex target |
|-------|--------------|--------------|
| **DNS** | `api.anthropic.com` | `api.openai.com` |
| **Exit IP** | `ipinfo.io` — blocks CN, HK, KP, CU, IR, SY, RU, BY | same |
| **Connectivity** | 3 × TCP to `api.anthropic.com:443` | 3 × TCP to `api.openai.com:443` |

| Status | Meaning | Behaviour |
|--------|---------|-----------|
| 🟢 Green | All checks passed | Tool starts immediately |
| 🟡 Yellow | Concerns (high latency / partial loss) | Prompts `[C]ontinue [R]etry [Q]uit` |
| 🔴 Red | Hard failure (DNS / blocked region / no connectivity) | Prompts `[C]ontinue [R]etry [Q]uit` |

## Installation

### Homebrew

```bash
brew install Laotree/tap/verify-networking
```

Then add the shell wrappers to your RC file (see [Manual](#manual) below).

### Optional: richer trace output with nali

When a network issue is detected, the tool runs `mtr` or `traceroute` and annotates each hop IP with geolocation data if [nali](https://github.com/zu1k/nali) is on your `$PATH`. No configuration required — it is detected automatically at runtime.

```bash
brew install nali          # macOS
# or
cargo install nali         # cross-platform
```

Without nali the trace still runs; IPs are just shown as raw addresses.

### cargo install

```bash
cargo install verify-networking
cp "$(which verify-networking)" ~/.claude/plugins/
```

Then add the shell wrappers to your RC file (see [Manual](#manual) below).

### From source

```bash
git clone https://github.com/Laotree/verify-networking-plugin
cd verify-networking-plugin
./install.sh
source ~/.zshrc   # or ~/.bashrc
```

`install.sh` automatically detects whether `codex` is on your `$PATH` and adds its wrapper alongside the `claude` wrapper. It also creates **`Verify & Launch Claude.app`** (and `Verify & Launch Codex.app` if Codex is installed) in `~/Applications` — drag either to the Dock for one-click launch with a network pre-flight check. If you install Codex later, re-run `./install.sh`.

### Manual

```bash
cargo build --release
cp target/release/verify-networking ~/.claude/plugins/
```

Then add to your shell RC — include only the wrappers for tools you have installed:

```bash
# Claude Code
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" claude || return 1; }
    command claude "$@"
}

# Codex CLI (add if you use Codex)
codex() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" codex || return 1; }
    command codex "$@"
}
```

## Usage

Type `claude` or `codex` as usual. The check runs automatically before every session.

**All checks passed (green) — Claude:**

```
  Verifying network before Claude starts...

  Checking...

  🟢 DNS            api.anthropic.com → 18.165.56.1
  🟢 Exit IP        1.2.3.4 [US] AS12345 Example ISP
  🟢 Connectivity   api.anthropic.com avg 217ms  loss 0%

  🟢 All checks passed.
```

**All checks passed (green) — Codex:**

```
  Verifying network before Codex starts...

  Checking...

  🟢 DNS            api.openai.com → 104.18.7.192
  🟢 Exit IP        1.2.3.4 [US] AS12345 Example ISP
  🟢 Connectivity   api.openai.com avg 183ms  loss 0%

  🟢 All checks passed.
```

**Concerns detected (yellow) — prompts before continuing:**

```
  Verifying network before Claude starts...

  Checking...

  🟢 DNS            api.anthropic.com → 18.165.56.1
  🟡 Exit IP        1.2.3.4 [US] AS12345 Example ISP
  🟡 Connectivity   api.anthropic.com avg 612ms  loss 0%

  🟡 Network concerns detected.

  [C]ontinue  [R]etry  [Q]uit ›
```

**Hard failure (red) — blocked region (with nali installed):**

```
  Verifying network before Claude starts...

  Checking...

  🟢 DNS            api.anthropic.com → 18.165.56.1
  🔴 Exit IP        1.2.3.4 [HK] AS9304 Example ISP — Claude unavailable in this region
  🟢 Connectivity   api.anthropic.com avg 201ms  loss 0%

  🔴 Network issues detected.

  Running traceroute for a more precise path analysis — this may take ~30 s  ⠸

  Route to api.anthropic.com (via traceroute + nali)
   1  192.168.1.1 [局域网 IP]   2 ms  2 ms  2 ms
   2  10.0.0.1 [局域网 IP]   8 ms  9 ms  8 ms
   3  203.0.113.1 [香港 Example Carrier]   15 ms  14 ms  15 ms
   4  89.149.128.174 [欧美地区 GTT通讯公司骨干网]   183 ms  182 ms  183 ms
   5  141.101.72.19 [美国加利福尼亚州洛杉矶 CloudFlare节点]   210 ms  213 ms  211 ms
   6  18.165.56.1 [美国]   200 ms  201 ms  200 ms

  Note: exit IP 1.2.3.4 not seen in trace hops — the route to this host may differ from the route ipinfo.io observed.

  [C]ontinue  [R]etry  [Q]uit ›
```

Without nali the trace shows the same hops but with bare IPs and no geolocation annotations, and the header reads `(via traceroute)` instead of `(via traceroute + nali)`.

## Build

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

## Removal

```bash
# Remove binary
rm ~/.claude/plugins/verify-networking

# Remove shell functions — delete the claude() and codex() blocks from ~/.zshrc (or ~/.bashrc)
```
