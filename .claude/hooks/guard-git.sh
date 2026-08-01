#!/usr/bin/env bash
# PreToolUse guard for Bash commands run by AI agents.
#
# Several agents work on this repository at the same time, each in its own git worktree. A branch
# is not an isolation boundary: one working tree has one checked-out branch, so a stray checkout,
# stash, reset or merge silently rewrites a co-resident agent's tree and makes both of them report
# work they did not do. This hook turns the rules in CLAUDE.md into a wall.
#
# Reads the PreToolUse payload on stdin, exits 2 to block with an explanation on stderr.
set -uo pipefail

payload=$(cat)

command_line=$(printf '%s' "$payload" | node -e '
let raw = "";
process.stdin.on("data", (chunk) => { raw += chunk; });
process.stdin.on("end", () => {
  try { console.log(JSON.parse(raw)?.tool_input?.command ?? ""); } catch { console.log(""); }
});
' 2>/dev/null) || command_line=''

# A guard that fails open is not a guard. If node is unavailable, fall back to a coarse extraction
# so destructive commands are still caught; a false positive here costs one blocked call, a false
# negative costs another agent's work.
if [[ -z $command_line ]] && ! command -v node >/dev/null 2>&1; then
  command_line=$(printf '%s' "$payload" \
    | sed -n 's/.*"command"[[:space:]]*:[[:space:]]*"\(.*\)".*/\1/p')
fi
[[ -n $command_line ]] || exit 0

# The sanctioned way to land work performs its own rebase and push; never second-guess it.
case "$command_line" in
  *agent-merge.sh*) exit 0 ;;
esac

deny() {
  printf 'Blocked by .claude/hooks/guard-git.sh: %s\n\n%s\n' "$1" "$2" >&2
  exit 2
}

# Only inspect segments that actually start a git invocation, so documentation, grep patterns and
# echoed text containing these words are not blocked.
while IFS= read -r segment || [[ -n $segment ]]; do
  segment=${segment#"${segment%%[![:space:]]*}"}
  # Skip leading environment assignments and common prefixes.
  while [[ $segment == *=* && ${segment%%[[:space:]]*} == *=* ]]; do
    segment=${segment#*[[:space:]]}
    segment=${segment#"${segment%%[![:space:]]*}"}
  done
  [[ $segment == git\ * || $segment == git ]] || continue
  arguments=${segment#git}

  case "$arguments" in
    *' checkout'*|*' switch'*)
      deny 'branch switching in a shared working tree' \
'You are working in your own git worktree. Switching branches here rewrites the files of every
other agent sharing this tree, and carries their uncommitted work onto your branch.
Need a different base? Create a new worktree:
  worktree=$(./deploy/agent-worktree.sh create fix/task-slug task-slug) && cd "$worktree"
Need to discard a file? Ask the human first.' ;;
    *' stash'*|*' reset '*--hard*|*' clean '*-*f*)
      deny 'a destructive tree operation' \
'stash, reset --hard and clean -f silently delete work in progress, including a neighbouring
agent'"'"'s uncommitted edits. Neither git nor the other agent will warn anyone. Ask the human.' ;;
    *' merge'*|*' rebase'*)
      deny 'a history rewrite outside the sanctioned merge path' \
'Landing work is a production deployment, and it is serialized for a reason. Run:
  git push -u origin HEAD && ./deploy/agent-merge.sh
It rebases, re-runs the full gate on the exact SHA it pushes, takes the machine-wide merge lock
and holds it until deploy/watchdog is green.' ;;
    *' worktree '*add*)
      deny 'unmanaged worktree creation' \
'Raw worktree creation has no lifecycle metadata and cannot be cleaned safely after an interrupted
agent. Use the managed path:
  worktree=$(./deploy/agent-worktree.sh create fix/task-slug task-slug) && cd "$worktree"' ;;
    *' worktree '*remove*|*' worktree '*prune*)
      deny 'unmanaged worktree cleanup' \
'A raw removal cannot prove ownership and a global prune cannot preserve lifecycle policy. Finish
one selected clean merged worktree with deploy/agent-worktree.sh finish, or inspect global residue
with deploy/agent-worktree.sh doctor / gc (gc is dry-run unless --apply is explicit).' ;;
  esac

  case "$arguments" in
    *' add '*-A*|*' add '*--all*|*' add '*' .'*|*' add .'*)
      deny 'a wildcard stage' \
'git add -A and git add . sweep other agents'"'"' files into your commit, which is how work ends up
attributed to the wrong author. Stage your own paths explicitly:
  git add crates/forward/src/...' ;;
  esac

  case "$arguments" in
    *' push'*master*|*' push'*main*)
      deny 'a direct push to the deployment branch' \
'A push to master triggers a production deployment on the host. Use the serialized path:
  ./deploy/agent-merge.sh' ;;
  esac
done < <(printf '%s\n' "$command_line" | tr ';|&\n' '\n\n\n\n')

exit 0
