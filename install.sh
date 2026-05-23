#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="verify-networking"
INSTALL_DIR="$HOME/.claude/plugins"
SETTINGS="$HOME/.claude/settings.json"
BINARY_PATH="$INSTALL_DIR/$BINARY"

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

# Shell wrapper function — intercepts 'claude' before the process starts
MARKER="# verify-networking-plugin"
read -r -d '' SNIPPET <<'EOF' || true
# verify-networking-plugin
claude() {
    local checker="$HOME/.claude/plugins/verify-networking"
    [[ -x "$checker" ]] && { "$checker" || return 1; }
    command claude "$@"
}
EOF

add_to_rc() {
    local rc="$1"
    if grep -qF "$MARKER" "$rc" 2>/dev/null; then
        echo "→ Shell wrapper already present in $rc"
    else
        printf '\n%s\n' "$SNIPPET" >> "$rc"
        echo "→ Added claude() wrapper to $rc"
        echo "  Run: source $rc"
    fi
}

# Wrap aliases whose RHS invokes 'claude' (e.g. alias a1m='aivo claude ...')
# Replaces the alias with a shell function that runs the checker first.
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
    [[ -x "\$checker" ]] && { "\$checker" || return 1; }
    ${alias_cmd} "\$@"
}
FNSNIPPET
            echo "→ Added wrapper function for alias '${alias_name}' in $rc"
        fi
    done <<< "$aliases"
}

added=false
[[ "$SHELL" == *zsh*  && -f "$HOME/.zshrc"  ]] && { add_to_rc "$HOME/.zshrc";  patch_claude_aliases "$HOME/.zshrc";  added=true; }
[[ "$SHELL" == *bash* && -f "$HOME/.bashrc" ]] && { add_to_rc "$HOME/.bashrc"; patch_claude_aliases "$HOME/.bashrc"; added=true; }
[[ "$SHELL" == *bash* && -f "$HOME/.bash_profile" && "$added" == false ]] && { add_to_rc "$HOME/.bash_profile"; patch_claude_aliases "$HOME/.bash_profile"; }
if [[ -f "$HOME/.profile" ]]; then
    [[ "$added" == false ]] && { add_to_rc "$HOME/.profile"; added=true; }
    patch_claude_aliases "$HOME/.profile"
fi

if [[ "$added" == false ]]; then
    echo ""
    echo "→ Could not detect shell RC. Add this to your shell config manually:"
    echo ""
    echo "$SNIPPET"
fi

# Git pre-push hook — blocks direct pushes to main/master
GIT_DIR="$(git -C "$SCRIPT_DIR" rev-parse --git-dir 2>/dev/null || true)"
if [[ -n "$GIT_DIR" ]]; then
    ln -sf "$SCRIPT_DIR/hooks/pre-push" "$GIT_DIR/hooks/pre-push"
    echo "→ Installed git pre-push hook"
else
    echo "→ Not a git repo — skipping git hook install"
fi

echo ""
echo "✓ Done. Network check runs before every Claude session (claude + detected alias wrappers)."
echo ""
echo "  To remove: delete the claude() function and any alias wrapper functions from your shell RC"
echo "             and rm $BINARY_PATH"
