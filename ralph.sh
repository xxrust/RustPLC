#!/bin/bash
# Ralph Wiggum - Long-running AI agent loop
# Usage: ./ralph.sh [--tool amp|claude|codex] [max_iterations]

set -e

# Parse arguments
TOOL="amp"  # Default to amp for backwards compatibility
MAX_ITERATIONS=10
ITERATION_TIMEOUT_SECONDS="${ITERATION_TIMEOUT_SECONDS:-3600}"

while [[ $# -gt 0 ]]; do
  case $1 in
    --tool)
      TOOL="$2"
      shift 2
      ;;
    --tool=*)
      TOOL="${1#*=}"
      shift
      ;;
    *)
      # Assume it's max_iterations if it's a number
      if [[ "$1" =~ ^[0-9]+$ ]]; then
        MAX_ITERATIONS="$1"
      fi
      shift
      ;;
  esac
done

# Validate tool choice
if [[ "$TOOL" != "amp" && "$TOOL" != "claude" && "$TOOL" != "codex" ]]; then
  echo "Error: Invalid tool '$TOOL'. Must be 'amp', 'claude', or 'codex'."
  exit 1
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PRD_FILE="$SCRIPT_DIR/prd.json"
PROGRESS_FILE="$SCRIPT_DIR/progress.txt"
ARCHIVE_DIR="$SCRIPT_DIR/archive"
LAST_BRANCH_FILE="$SCRIPT_DIR/.last-branch"

all_stories_passed() {
  if [[ ! -f "$PRD_FILE" ]] || ! command -v python3 >/dev/null 2>&1; then
    return 1
  fi

  python3 - "$PRD_FILE" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
    stories = data.get("userStories", [])
    # Empty story lists should not be treated as complete.
    ok = bool(stories) and all(bool(s.get("passes")) for s in stories)
except Exception:
    ok = False

sys.exit(0 if ok else 1)
PY
}

# Archive previous run if branch changed
if [ -f "$PRD_FILE" ] && [ -f "$LAST_BRANCH_FILE" ]; then
  CURRENT_BRANCH=$(jq -r '.branchName // empty' "$PRD_FILE" 2>/dev/null || echo "")
  LAST_BRANCH=$(cat "$LAST_BRANCH_FILE" 2>/dev/null || echo "")
  
  if [ -n "$CURRENT_BRANCH" ] && [ -n "$LAST_BRANCH" ] && [ "$CURRENT_BRANCH" != "$LAST_BRANCH" ]; then
    # Archive the previous run
    DATE=$(date +%Y-%m-%d)
    # Strip "ralph/" prefix from branch name for folder
    FOLDER_NAME=$(echo "$LAST_BRANCH" | sed 's|^ralph/||')
    ARCHIVE_FOLDER="$ARCHIVE_DIR/$DATE-$FOLDER_NAME"
    
    echo "Archiving previous run: $LAST_BRANCH"
    mkdir -p "$ARCHIVE_FOLDER"
    [ -f "$PRD_FILE" ] && cp "$PRD_FILE" "$ARCHIVE_FOLDER/"
    [ -f "$PROGRESS_FILE" ] && cp "$PROGRESS_FILE" "$ARCHIVE_FOLDER/"
    echo "   Archived to: $ARCHIVE_FOLDER"
    
    # Reset progress file for new run
    echo "# Ralph Progress Log" > "$PROGRESS_FILE"
    echo "Started: $(date)" >> "$PROGRESS_FILE"
    echo "---" >> "$PROGRESS_FILE"
  fi
fi

# Track current branch
if [ -f "$PRD_FILE" ]; then
  CURRENT_BRANCH=$(jq -r '.branchName // empty' "$PRD_FILE" 2>/dev/null || echo "")
  if [ -n "$CURRENT_BRANCH" ]; then
    echo "$CURRENT_BRANCH" > "$LAST_BRANCH_FILE"
  fi
fi

# Initialize progress file if it doesn't exist
if [ ! -f "$PROGRESS_FILE" ]; then
  echo "# Ralph Progress Log" > "$PROGRESS_FILE"
  echo "Started: $(date)" >> "$PROGRESS_FILE"
  echo "---" >> "$PROGRESS_FILE"
fi

echo "Starting Ralph - Tool: $TOOL - Max iterations: $MAX_ITERATIONS"
echo "Per-iteration timeout: ${ITERATION_TIMEOUT_SECONDS}s"

for i in $(seq 1 $MAX_ITERATIONS); do
  echo ""
  echo "==============================================================="
  echo "  Ralph Iteration $i of $MAX_ITERATIONS ($TOOL)"
  echo "==============================================================="

  # Run the selected tool with the ralph prompt.
  # Capture output in a temp file (instead of a giant shell variable) to avoid
  # memory pressure and allow simple "tail" checks for completion markers.
  RUN_LOG="$(mktemp)"
  OUTPUT_STATUS=0

  # Run the selected tool with a watchdog timeout to avoid hanging forever.
  if [[ "$TOOL" == "amp" ]]; then
    timeout "${ITERATION_TIMEOUT_SECONDS}" bash -lc "cat \"$SCRIPT_DIR/prompt.md\" | amp --dangerously-allow-all" \
      2>&1 | tee /dev/stderr | tee "$RUN_LOG" >/dev/null || OUTPUT_STATUS=$?
  elif [[ "$TOOL" == "claude" ]]; then
    # Claude Code: use --dangerously-skip-permissions for autonomous operation, --print for output
    timeout "${ITERATION_TIMEOUT_SECONDS}" claude --dangerously-skip-permissions --print < "$SCRIPT_DIR/CLAUDE.md" \
      2>&1 | tee /dev/stderr | tee "$RUN_LOG" >/dev/null || OUTPUT_STATUS=$?
  else
    # Codex CLI: use --dangerously-bypass-approvals-and-sandbox for autonomous operation
    timeout "${ITERATION_TIMEOUT_SECONDS}" codex exec --dangerously-bypass-approvals-and-sandbox - < "$SCRIPT_DIR/CODEX.md" \
      2>&1 | tee /dev/stderr | tee "$RUN_LOG" >/dev/null || OUTPUT_STATUS=$?
  fi

  if [[ "$OUTPUT_STATUS" -eq 124 ]]; then
    echo "Iteration $i timed out after ${ITERATION_TIMEOUT_SECONDS}s. Continuing..."
  fi
  
  # Completion should be based on PRD state; completion token in the prompt can be a false positive.
  if all_stories_passed; then
    echo ""
    echo "Ralph completed all tasks!"
    echo "Completed at iteration $i of $MAX_ITERATIONS"
    exit 0
  fi

  # Keep logging a completion token if it appears near the end, but do not terminate on token alone.
  if tail -n 80 "$RUN_LOG" | grep -q "<promise>COMPLETE</promise>"; then
    echo "Completion token detected, but PRD still has pending stories. Continuing..."
  fi

  rm -f "$RUN_LOG"
  
  echo "Iteration $i complete. Continuing..."
  sleep 2
done

echo ""
echo "Ralph reached max iterations ($MAX_ITERATIONS) without completing all tasks."
echo "Check $PROGRESS_FILE for status."
exit 1
