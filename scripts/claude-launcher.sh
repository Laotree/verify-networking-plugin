#!/usr/bin/env bash
# claude-launcher.sh — macOS .app launcher for verify-networking + Claude Desktop
#
# This script is the executable inside "Verify & Launch Claude.app/Contents/MacOS/".
# It runs the verify-networking binary in non-interactive mode, then shows a native
# macOS alert dialog for warnings/failures (Quit / Retry / Continue).
# On green (or Continue), it opens the Claude desktop app.

CHECKER="$HOME/.claude/plugins/verify-networking"
APP_NAME="Claude"

# If the checker binary is missing, open Claude directly and exit.
if [[ ! -x "$CHECKER" ]]; then
    open -a "$APP_NAME"
    exit 0
fi

while true; do
    # Run checks in non-interactive mode.
    # Exit codes: 0 = green, 2 = yellow, 1 = red.
    output=$("$CHECKER" claude --non-interactive 2>&1)
    code=$?

    if [[ $code -eq 0 ]]; then
        # All checks passed — open Claude immediately.
        open -a "$APP_NAME"
        exit 0
    fi

    # Strip ANSI escape codes so the text is clean for the dialog.
    clean=$(printf '%s' "$output" | sed $'s/\033\\[[0-9;]*[mGKHF]//g')

    # Escape characters that would break AppleScript string literals.
    safe=$(printf '%s' "$clean" | sed 's/\\/\\\\/g; s/"/\\"/g')

    if [[ $code -eq 1 ]]; then
        kind="critical"
        title="Network issues detected — Claude"
    else
        kind="caution"
        title="Network concerns detected — Claude"
    fi

    choice=$(osascript \
        -e "tell application \"System Events\"" \
        -e "  set r to display alert \"$title\" message \"$safe\" buttons {\"Quit\", \"Retry\", \"Continue\"} default button \"Continue\" as $kind" \
        -e "  return button returned of r" \
        -e "end tell" 2>/dev/null)

    case "$choice" in
        "Continue") open -a "$APP_NAME"; exit 0 ;;
        "Retry")    continue ;;
        *)          exit 1 ;;   # Quit or dismissed
    esac
done
