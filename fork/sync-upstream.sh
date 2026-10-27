#!/usr/bin/env bash
# Merge pbakaus/impeccable main into 0xCydral/impeccable main, push it, and deploy it (fork/deploy.sh).
# Runs in its own clone so a dirty working checkout never blocks or loses work. Same pattern as the rea fork.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$HOME/.local/share/fnm/aliases/default/bin:$HOME/.local/bin:$PATH"

dir="${IMPECCABLE_SYNC_DIR:-$HOME/.local/share/impeccable-sync}"
log() { printf '%s %s\n' "$(date -Is)" "$*"; }

give_up() {
  git merge --abort 2>/dev/null || true
  git reset -q --hard origin/main
  log "CONFLICT merging upstream $(git rev-parse --short upstream/main): $1; resolve by hand"
  if command -v notify-send >/dev/null; then
    notify-send "impeccable sync" "Upstream merge needs a hand: $1"
  fi
  exit 1
}

# Hand a failed merge to a headless Claude Code run, then check its work here instead of trusting its
# report. One attempt per upstream commit, so a merge it cannot land costs one run.
agent_fix() {
  local up prompt
  up="$(git rev-parse upstream/main)"
  command -v claude >/dev/null || give_up "claude CLI not found"
  [ "$(cat .agent-failed-upstream 2>/dev/null)" != "$up" ] || give_up "claude already failed on ${up:0:9}"
  log "$1 Handing it to claude."
  prompt="You are in $dir, a clone of the fork 0xCydral/impeccable, in the middle of merging pbakaus/impeccable upstream/main into main.
$1
Finish the merge:
1. Resolve every conflict keeping both sides: upstream's changes and the fork's own additions (fork/, the decision page's Save, Re-roll palette and Notes in crates/context/src/serve_question.rs and question_page.rs, and their lines in skill/reference/new-work.md and docs/CLI-CONTRACT.md). Never drop a fork feature to make a conflict go away.
2. Edit skill/, never the generated harness mirrors; regenerate them with bun run build:release.
3. Stage the result and commit. Keep the message 'chore(sync): merge upstream main' for the merge, and use a separate commit for any follow-up fix.
Do not push, do not touch files outside $dir, and do not add AI attribution to commits."
  # The agent reads untrusted upstream diffs, so it gets its own login and nothing else: no keys, tokens or
  # SSH agent from this environment, and no tool beyond reading, editing, git and bun.
  timeout 45m env -i HOME="$HOME" USER="$USER" PATH="$PATH" LANG="${LANG:-C.UTF-8}" TERM=dumb \
    CLAUDE_CONFIG_DIR="${CLAUDE_CONFIG_DIR:-$HOME/.claude}" \
    claude -p "$prompt" --model sonnet --permission-mode dontAsk \
    --allowedTools "Read Edit Write Glob Grep Bash(git *) Bash(bun *) Bash(rtk git *)" \
    < /dev/null >> .agent.log 2>&1 || true # --allowedTools is variadic: the prompt must come first
  if [ -z "$(git ls-files -u)" ] && ! git rev-parse -q --verify MERGE_HEAD >/dev/null \
    && git merge-base --is-ancestor "$up" HEAD \
    && [ -z "$(git status --porcelain --untracked-files=no)" ] \
    && ! git diff origin/main HEAD | grep -Eq '^\+(<<<<<<<|>>>>>>>)( |$)' \
    && git push -q origin main; then
    log "claude merged upstream ${up:0:9}; pushed $(git rev-parse --short HEAD)"
  else
    printf '%s\n' "$up" > .agent-failed-upstream
    give_up "claude could not land ${up:0:9}, see $dir/.agent.log"
  fi
}

if [ ! -d "$dir/.git" ]; then
  git clone -q https://github.com/0xCydral/impeccable.git "$dir"
  git -C "$dir" remote add upstream https://github.com/pbakaus/impeccable.git
  git -C "$dir" remote set-url --push upstream DISABLED
fi

cd "$dir"
git fetch -q origin main
git fetch -q upstream main
git checkout -q -B main origin/main

if git merge-base --is-ancestor upstream/main HEAD; then
  log "up to date at $(git rev-parse --short HEAD)"
elif git merge -q --no-edit -m "chore(sync): merge upstream main" upstream/main; then
  # A clean merge still needs the mirrors regenerated when upstream or the fork touched skill/.
  bun run build:release >/dev/null
  if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    git add -u && git commit -q -m "chore(sync): regenerate the harness mirrors"
  fi
  git push -q origin main
  log "merged upstream $(git rev-parse --short upstream/main) into main"
else
  agent_fix "The merge stopped on conflicts in: $(git diff --name-only --diff-filter=U | tr '\n' ' ')"
fi

# Deploy whenever main moved since the last deploy, whether upstream or our own merge moved it.
head="$(git rev-parse HEAD)"
if [ "$(cat .deployed-head 2>/dev/null)" != "$head" ]; then
  "$dir/fork/deploy.sh"
  printf '%s\n' "$head" > .deployed-head
fi
