[![MIT License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

# verify-networking

Network check before Claude Code starts — DNS resolution, exit IP region, and TCP connectivity, with 🟢🟡🔴 status.

## How It Works

`install.sh` adds a `claude()` shell function to your RC file that wraps every `claude` invocation:

```bash
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" || return 1; }
    command claude "$@"
}
```

Before Claude Code starts, it runs three checks concurrently:

1. **DNS** — resolves `api.anthropic.com` via the system resolver
2. **Exit IP** — queries `ipinfo.io` and blocks if the country is CN, HK, KP, CU, IR, SY, RU, or BY (regions where Claude is unavailable)
3. **Connectivity** — 3 concurrent TCP probes to `api.anthropic.com:443`, reports avg latency and loss %

| Status | Meaning | Behaviour |
|--------|---------|-----------|
| 🟢 Green | All checks passed | Claude starts immediately |
| 🟡 Yellow | Concerns (high latency / partial loss) | Prompts `[C]ontinue [R]etry [Q]uit` |
| 🔴 Red | Hard failure (DNS / blocked region / no connectivity) | Prompts `[C]ontinue [R]etry [Q]uit` |

## Installation

### From source

```bash
git clone https://github.com/Laotree/verify-networking-plugin
cd verify-networking-plugin
./install.sh
source ~/.zshrc   # or ~/.bashrc
```

### Manual

```bash
cargo build --release
cp target/release/verify-networking ~/.claude/plugins/
```

Then add to your shell RC:

```bash
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" || return 1; }
    command claude "$@"
}
```

## Usage

Type `claude` as usual. The check runs automatically before every session.

**All checks passed (green):**

```
  Verifying network before Claude starts...

  Checking...

  🟢 DNS            api.anthropic.com → 18.165.56.1
  🟢 Exit IP        1.2.3.4 [US] AS12345 Example ISP
  🟢 Connectivity   api.anthropic.com avg 217ms  loss 0%

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

**Hard failure (red) — prompts before continuing:**

```
  Verifying network before Claude starts...

  Checking...

  🟢 DNS            api.anthropic.com → 18.165.56.1
  🟢 Exit IP        1.2.3.4 [US] AS12345 Example ISP
  🔴 Connectivity   api.anthropic.com avg —  loss 100%

  🔴 Hard failure detected.

  [C]ontinue  [R]etry  [Q]uit ›
```

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

# Remove shell function — delete the claude() block from ~/.zshrc (or ~/.bashrc)
```
