#!/usr/bin/env bash
# Build this fork's engine and skill and install them over the owner's user-level Impeccable install.
# fork/sync-upstream.sh runs it whenever main moved; it is safe to run by hand from any clone of the fork.
# Revert to upstream: npx -y impeccable@latest update --global --no-hooks -y --force && depager skill install --force
set -euo pipefail
export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$HOME/.local/bin:$HOME/.local/share/fnm/aliases/default/bin:$PATH"
cd "$(dirname "$0")/.."
log() { printf '%s %s\n' "$(date -Is)" "$*"; }

# The engine builds on an Instabox box, so the PC stays free; the boxes carry Rust 1.99.0 only.
# A box binary runs here (box glibc 2.41, PC 2.39, checked 2026-10-27), and the schema proves it is the fork's.
engine="$PWD/target/release/impeccable"
if [ "${IMPECCABLE_DEPLOY_LOCAL_ENGINE:-}" != 1 ]; then
  offload --clean --pull target/release/impeccable -- \
    'export RUSTUP_TOOLCHAIN=1.99.0-x86_64-unknown-linux-gnu; cargo build -q --release -p impeccable'
fi
"$engine" serve-question --schema | grep -q '"palette"' || { log "the built engine lacks the fork's decision page"; exit 1; }

# The skill for every harness, with this engine's digest in place of upstream's for the same version:
# the launcher in each installed tree refuses an engine its engine-checksums.txt does not list.
bun run build >/dev/null
version="$(tr -d '[:space:]' < ENGINE_VERSION)"
sum="$(sha256sum "$engine" | cut -d' ' -f1)"
for f in dist/universal/.*/skills/impeccable/scripts/engine-checksums.txt; do
  sed -i "s|^$version impeccable-linux-x64 .*|$version impeccable-linux-x64 $sum|" "$f"
done
IMPECCABLE_BUNDLE_PATH="$PWD/dist/universal" "$engine" update --global --no-hooks -y --force

# The installer never ships an engine in a bundle and keeps one it finds, so put this one in every tree.
for skill in "$HOME/.claude/skills/impeccable" "$HOME/.agents/skills/impeccable" "$HOME/.github/skills/impeccable" \
             "$HOME/.local/share/hermes/skills/impeccable" "$HOME/.config/opencode/skills/impeccable"; do
  [ -d "$skill" ] || continue
  install -D -m 0755 "$engine" "$skill/scripts/bin/linux-x64/impeccable"
  "$skill/scripts/impeccable" serve-question --schema | grep -q '"palette"' \
    || { log "$skill does not run the fork's engine"; exit 1; }
done
# An update rewrites impeccable's SKILL.md, which drops depager's blocks; this puts them back.
command -v depager >/dev/null && depager skill install --force >/dev/null
log "deployed $(git rev-parse --short HEAD): engine $version sha256 ${sum:0:12}"
