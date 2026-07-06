#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="verify-networking"
INSTALL_DIR="$HOME/.claude/plugins"
SETTINGS="$HOME/.claude/settings.json"
BINARY_PATH="$INSTALL_DIR/$BINARY"

# ---------------------------------------------------------------------------
# Cargo version guard — Cargo.lock v4 requires Cargo ≥ 1.78.0
# ---------------------------------------------------------------------------
MIN_CARGO="1.78.0"

check_cargo_version() {
    if ! command -v cargo &>/dev/null; then
        echo ""
        echo "✗ cargo not found. Install Rust first:"
        echo ""
        echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo ""
        exit 1
    fi

    local cargo_ver
    cargo_ver=$(cargo --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)

    # Use sort -V to compare: if MIN_CARGO > cargo_ver, sort -V -C will fail
    # because the two lines would not be in non-descending order.
    if ! printf '%s\n%s\n' "$MIN_CARGO" "$cargo_ver" | sort -V -C 2>/dev/null; then
        echo ""
        echo "✗ Cargo $cargo_ver is too old (need ≥ $MIN_CARGO)."
        echo "  The project's Cargo.lock uses format v4, which your Cargo cannot read."
        echo ""
        echo "  Fix — update your Rust toolchain:"
        echo ""
        echo "    rustup update stable       # if you installed via rustup (recommended)"
        echo "    brew upgrade rust          # if you installed via Homebrew"
        echo ""
        echo "  Then re-run: ./install.sh"
        echo ""
        exit 1
    fi

    echo "→ Cargo $cargo_ver  ✓"
}

check_cargo_version

echo "→ Building $BINARY (release)..."
cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"

mkdir -p "$INSTALL_DIR"
cp "$SCRIPT_DIR/target/release/$BINARY" "$BINARY_PATH"
echo "→ Installed to $BINARY_PATH"

# Remove old UserPromptSubmit hook entry if a previous install added it
if [[ -f "$SETTINGS" ]]; then
    python3 - <<PYEOF
import json, os, sys

settings_path = "$SETTINGS"
binary_path = "$BINARY_PATH"

with open(settings_path) as f:
    settings = json.load(f)

hooks = settings.get("hooks", {})
ups = hooks.get("UserPromptSubmit", [])
cleaned = [
    e for e in ups
    if not any(h.get("command") == binary_path for h in e.get("hooks", []))
]
if len(cleaned) != len(ups):
    if cleaned:
        hooks["UserPromptSubmit"] = cleaned
    else:
        hooks.pop("UserPromptSubmit", None)
    if not hooks:
        settings.pop("hooks", None)
    with open(settings_path, "w") as f:
        json.dump(settings, f, indent=2)
    print("→ Removed old UserPromptSubmit hook entry from settings.json")
PYEOF
fi

# ---------------------------------------------------------------------------
# Shell wrapper snippets
# ---------------------------------------------------------------------------

# Claude wrapper — passes 'claude' so the binary targets api.anthropic.com
CLAUDE_MARKER="# verify-networking-plugin"
read -r -d '' CLAUDE_SNIPPET <<'EOF' || true
# verify-networking-plugin
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" claude || return 1; }
    command claude "$@"
}
EOF

# Codex wrapper — passes 'codex' so the binary targets api.openai.com
CODEX_MARKER="# verify-networking-plugin-codex"
read -r -d '' CODEX_SNIPPET <<'EOF' || true
# verify-networking-plugin-codex
codex() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" codex || return 1; }
    command codex "$@"
}
EOF

# ---------------------------------------------------------------------------
# Helper: add a snippet to an RC file if its marker is absent
# ---------------------------------------------------------------------------
add_snippet_to_rc() {
    local rc="$1"
    local marker="$2"
    local snippet="$3"
    local label="$4"
    if grep -qF "$marker" "$rc" 2>/dev/null; then
        echo "→ ${label} wrapper already present in $rc"
    else
        printf '\n%s\n' "$snippet" >> "$rc"
        echo "→ Added ${label}() wrapper to $rc"
        echo "  Run: source $rc"
    fi
}

add_to_rc() {
    local rc="$1"
    add_snippet_to_rc "$rc" "$CLAUDE_MARKER" "$CLAUDE_SNIPPET" "claude"
}

# ---------------------------------------------------------------------------
# Wrap aliases whose RHS invokes 'claude' (e.g. alias a1m='aivo claude ...')
# Replaces the alias with a shell function that runs the checker first.
# ---------------------------------------------------------------------------
patch_claude_aliases() {
    local rc="$1"
    [[ -f "$rc" ]] || return 0

    local aliases
    aliases=$(grep -E "^[[:space:]]*alias [^=]+=['\"].*claude.*['\"]" "$rc" 2>/dev/null || true)
    [[ -z "$aliases" ]] && return 0

    while IFS= read -r line; do
        local alias_name alias_cmd
        if [[ "$line" =~ ^[[:space:]]*alias[[:space:]]+([^=[:space:]]+)=\'(.+)\'[[:space:]]*$ ]]; then
            alias_name="${BASH_REMATCH[1]}"
            alias_cmd="${BASH_REMATCH[2]}"
        elif [[ "$line" =~ ^[[:space:]]*alias[[:space:]]+([^=[:space:]]+)=\"(.+)\"[[:space:]]*$ ]]; then
            alias_name="${BASH_REMATCH[1]}"
            alias_cmd="${BASH_REMATCH[2]}"
        else
            continue
        fi

        [[ "$alias_name" == "claude" ]] && continue

        local fn_marker="# verify-networking-alias:${alias_name}"
        if grep -qF "$fn_marker" "$rc" 2>/dev/null; then
            echo "→ Alias wrapper for '${alias_name}' already present in $rc"
        else
            cat >> "$rc" << FNSNIPPET

${fn_marker}
unalias ${alias_name} 2>/dev/null || true
${alias_name}() {
    local checker="\$HOME/.claude/plugins/verify-networking"
    [[ -x "\$checker" ]] && { "\$checker" claude || return 1; }
    ${alias_cmd} "\$@"
}
FNSNIPPET
            echo "→ Added wrapper function for alias '${alias_name}' in $rc"
        fi
    done <<< "$aliases"
}

# ---------------------------------------------------------------------------
# Add codex wrapper to an RC file (only if `codex` is available on PATH)
# ---------------------------------------------------------------------------
add_codex_to_rc() {
    local rc="$1"
    if command -v codex &>/dev/null; then
        add_snippet_to_rc "$rc" "$CODEX_MARKER" "$CODEX_SNIPPET" "codex"
    fi
}

# ---------------------------------------------------------------------------
# Apply wrappers to detected RC files
# ---------------------------------------------------------------------------
added=false
if [[ "$SHELL" == *zsh* && -f "$HOME/.zshrc" ]]; then
    add_to_rc "$HOME/.zshrc"
    patch_claude_aliases "$HOME/.zshrc"
    add_codex_to_rc "$HOME/.zshrc"
    added=true
fi
if [[ "$SHELL" == *bash* && -f "$HOME/.bashrc" ]]; then
    add_to_rc "$HOME/.bashrc"
    patch_claude_aliases "$HOME/.bashrc"
    add_codex_to_rc "$HOME/.bashrc"
    added=true
fi
if [[ "$SHELL" == *bash* && -f "$HOME/.bash_profile" && "$added" == false ]]; then
    add_to_rc "$HOME/.bash_profile"
    patch_claude_aliases "$HOME/.bash_profile"
    add_codex_to_rc "$HOME/.bash_profile"
fi
if [[ -f "$HOME/.profile" ]]; then
    [[ "$added" == false ]] && { add_to_rc "$HOME/.profile"; added=true; }
    patch_claude_aliases "$HOME/.profile"
    add_codex_to_rc "$HOME/.profile"
fi

if [[ "$added" == false ]]; then
    echo ""
    echo "→ Could not detect shell RC. Add this to your shell config manually:"
    echo ""
    echo "$CLAUDE_SNIPPET"
    if command -v codex &>/dev/null; then
        echo ""
        echo "$CODEX_SNIPPET"
    fi
fi

# ---------------------------------------------------------------------------
# Codex Desktop App wrapper (.app bundle)
# ---------------------------------------------------------------------------
# Searches common install locations for Codex.app and, if found, creates
# ~/Applications/Verify & Launch Codex.app — a thin shell-script app that
# runs the network check before opening the real Codex desktop app.
# ---------------------------------------------------------------------------
create_codex_app_wrapper() {
    local codex_app=""
    for candidate in \
        "/Applications/Codex.app" \
        "$HOME/Applications/Codex.app"
    do
        if [[ -d "$candidate" ]]; then
            codex_app="$candidate"
            break
        fi
    done

    if [[ -z "$codex_app" ]]; then
        echo "→ Codex desktop app not found — skipping .app wrapper"
        return 0
    fi

    echo "→ Codex desktop app found at $codex_app"

    local bundle_root="$HOME/Applications/Verify & Launch Codex.app"
    local wrapper_dir="$bundle_root/Contents/MacOS"
    local plist_dir="$bundle_root/Contents"
    local resources_dir="$bundle_root/Contents/Resources"
    mkdir -p "$wrapper_dir" "$resources_dir"

    # Info.plist — references AppIcon so macOS picks up our custom .icns
    cat > "$plist_dir/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Verify &amp; Launch Codex</string>
    <key>CFBundleDisplayName</key>
    <string>Verify &amp; Launch Codex</string>
    <key>CFBundleExecutable</key>
    <string>launcher</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.verify-networking.codex-launcher</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

    # Launcher script (the actual executable)
    cp "$SCRIPT_DIR/scripts/codex-launcher.sh" "$wrapper_dir/launcher"
    chmod +x "$wrapper_dir/launcher"

    # App icon — Codex logo + shield badge (requires ImageMagick 7 / magick)
    local icon_script="$SCRIPT_DIR/scripts/make-icon.sh"
    local icon_out="$resources_dir/AppIcon.icns"
    _install_badged_icon "$icon_script" "$icon_out" "$codex_app" "Codex"

    # Tell macOS to refresh the icon cache for the new bundle
    touch "$bundle_root" 2>/dev/null || true

    echo "→ Created ~/Applications/Verify & Launch Codex.app"
    echo ""
    echo "  ┌─────────────────────────────────────────────────────────────────────┐"
    echo "  │  One-time Dock setup:                                               │"
    echo "  │  1. Open Finder → Go → Applications (or ~/Applications)            │"
    echo "  │  2. Drag 'Verify & Launch Codex' to your Dock                      │"
    echo "  │  3. Right-click the old Codex icon → Remove from Dock              │"
    echo "  │  Now every Dock launch runs the network check first.               │"
    echo "  └─────────────────────────────────────────────────────────────────────┘"
}

# ---------------------------------------------------------------------------
# Claude Desktop App wrapper (.app bundle)
# ---------------------------------------------------------------------------
create_claude_app_wrapper() {
    local claude_app=""
    for candidate in \
        "/Applications/Claude.app" \
        "$HOME/Applications/Claude.app"
    do
        if [[ -d "$candidate" ]]; then
            claude_app="$candidate"
            break
        fi
    done

    if [[ -z "$claude_app" ]]; then
        echo "→ Claude desktop app not found — skipping .app wrapper"
        return 0
    fi

    echo "→ Claude desktop app found at $claude_app"

    local bundle_root="$HOME/Applications/Verify & Launch Claude.app"
    local wrapper_dir="$bundle_root/Contents/MacOS"
    local plist_dir="$bundle_root/Contents"
    local resources_dir="$bundle_root/Contents/Resources"
    mkdir -p "$wrapper_dir" "$resources_dir"

    # Info.plist
    cat > "$plist_dir/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Verify &amp; Launch Claude</string>
    <key>CFBundleDisplayName</key>
    <string>Verify &amp; Launch Claude</string>
    <key>CFBundleExecutable</key>
    <string>launcher</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.verify-networking.claude-launcher</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

    # Launcher script
    cp "$SCRIPT_DIR/scripts/claude-launcher.sh" "$wrapper_dir/launcher"
    chmod +x "$wrapper_dir/launcher"

    # App icon — Claude logo + shield badge
    local icon_script="$SCRIPT_DIR/scripts/make-icon.sh"
    local icon_out="$resources_dir/AppIcon.icns"
    _install_badged_icon "$icon_script" "$icon_out" "$claude_app" "Claude"

    # Nudge macOS icon cache
    touch "$bundle_root" 2>/dev/null || true

    echo "→ Created ~/Applications/Verify & Launch Claude.app"
    echo ""
    echo "  ┌─────────────────────────────────────────────────────────────────────┐"
    echo "  │  One-time Dock setup:                                               │"
    echo "  │  1. Open Finder → Go → Applications (or ~/Applications)            │"
    echo "  │  2. Drag 'Verify & Launch Claude' to your Dock                     │"
    echo "  │  3. Right-click the old Claude icon → Remove from Dock             │"
    echo "  │  Now every Dock launch runs the network check first.               │"
    echo "  └─────────────────────────────────────────────────────────────────────┘"
}

# ---------------------------------------------------------------------------
# Shared helper: generate badged icon or fall back to plain copy
# Usage: _install_badged_icon <make-icon.sh> <output.icns> <source.app> <label>
# ---------------------------------------------------------------------------
_install_badged_icon() {
    local icon_script="$1" icon_out="$2" source_app="$3" label="$4"
    if command -v magick &>/dev/null && [[ -x "$icon_script" ]]; then
        "$icon_script" "$icon_out" "$source_app" \
            && echo "→ App icon generated ($label logo + shield badge)" \
            || {
                echo "→ Icon generation failed — falling back to plain $label icon"
                find "$source_app/Contents/Resources" -maxdepth 1 -name "*.icns" \
                    | head -1 | xargs -I{} cp {} "$icon_out" 2>/dev/null || true
            }
    else
        find "$source_app/Contents/Resources" -maxdepth 1 -name "*.icns" \
            | head -1 | xargs -I{} cp {} "$icon_out" 2>/dev/null \
            && echo "→ Copied $label icon (install ImageMagick for the shield-badge variant)" \
            || echo "→ Could not copy $label icon — app will use default macOS icon"
    fi
}

create_codex_app_wrapper
create_claude_app_wrapper

# ---------------------------------------------------------------------------
# Daemon / per-call proxy setup (optional)
# ---------------------------------------------------------------------------
DAEMON_PORT=8443
setup_daemon_proxy() {
    local rc="$1"
    local daemon_marker="# verify-networking-daemon"
    if grep -qF "$daemon_marker" "$rc" 2>/dev/null; then
        echo "→ Daemon proxy env vars already present in $rc"
    else
        cat >> "$rc" << DAEOS

# verify-networking-daemon
# Export proxy env vars for per-call network checking via the daemon.
# To enable: uncomment the lines below.
# To start the daemon:
#   verify-networking --daemon [--port ${DAEMON_PORT}]
#
# export https_proxy=http://127.0.0.1:${DAEMON_PORT}
# export all_proxy=http://127.0.0.1:${DAEMON_PORT}
DAEOS
        echo "→ Added daemon proxy config (commented out) to $rc"
    fi
}

# Add daemon proxy env vars to RC files (commented out by default)
for rc_file in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile"; do
    [[ -f "$rc_file" ]] && setup_daemon_proxy "$rc_file" || true
done

# Git pre-push hook — blocks direct pushes to main/master
GIT_DIR="$(git -C "$SCRIPT_DIR" rev-parse --git-dir 2>/dev/null || true)"
if [[ -n "$GIT_DIR" ]]; then
    ln -sf "$SCRIPT_DIR/hooks/pre-push" "$GIT_DIR/hooks/pre-push"
    echo "→ Installed git pre-push hook"
else
    echo "→ Not a git repo — skipping git hook install"
fi

echo ""
echo "✓ Done. Network check runs before every Claude and Codex session"
echo "  (CLI wrappers + detected alias wrappers + desktop app wrappers)."
echo ""
echo "  🔷 New: Daemon mode for per-call network checking"
echo "  Start the daemon:"
echo "    verify-networking --daemon [--port 8443]"
echo ""
echo "  Then configure your tools to use the proxy:" 
echo "    export https_proxy=http://127.0.0.1:8443"
echo "    claude --proxy http://127.0.0.1:8443"
echo ""
echo "  The daemon intercepts each API request, checks the network,"
echo "  and holds the connection if a risk is detected — awaiting your confirmation."
echo ""
echo "  To remove:"
echo "    • Delete the claude() / codex() functions from your shell RC"
echo "    • rm $BINARY_PATH"
echo "    • Delete ~/Applications/Verify\\ \\&\\ Launch\\ Claude.app"
echo "    • Delete ~/Applications/Verify\\ \\&\\ Launch\\ Codex.app"
