//! One scan per edit event, however many Impeccable hooks fire for it.
//!
//! A harness runs every hook entry it loads, so the same edit can reach the
//! `hook` verb more than once: a home manifest and a project manifest
//! (`~/.cursor/hooks.json` next to `.cursor/hooks.json`, the same for Codex),
//! a plugin's hooks next to a project hook, or a sub-project's hook next to
//! its parent's. Each copy would scan the file and hand the agent the same
//! findings again.
//!
//! The harnesses that serve the per-edit hook name each tool call: Claude
//! Code, Codex and Cursor put a `tool_use_id` on every post-tool event, and
//! every hook of that event receives the same one. The first invocation
//! atomically creates a claim file named by the hash of that identity; a
//! later invocation that finds the claim exits without scanning. An event
//! without a tool-call id is never claimed, so a harness that does not name
//! its calls keeps today's behavior rather than risk dropping a real second
//! edit.
//!
//! Cost: one hash, one `create_dir` and one `create_new` per edit, plus a
//! `read_dir` of the claim root (a handful of bucket dirs) when a new
//! ten-minute bucket starts. No directory walk.
//!
//! Every failure fails open: if the claim cannot be made or checked, the
//! scan runs.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::hook_lib::Runtime;

/// Width of one claim bucket. Duplicate invocations of one event arrive within
/// the hook timeout (seconds), so a claim is checked in its own bucket and the
/// one before it, and buckets older than that are removed.
const BUCKET_SECONDS: u64 = 600;

/// The payload keys that carry a tool-call id, in the spellings the served
/// harnesses use.
const TOOL_CALL_ID_KEYS: &[&str] = &["tool_use_id", "toolUseId", "tool_call_id", "toolCallId"];

/// The session the call belongs to: Claude Code and Codex send `session_id`,
/// Cursor `conversation_id` (and `session_id`).
const SESSION_KEYS: &[&str] = &["session_id", "sessionId", "conversation_id"];

/// What happened to the claim for this invocation.
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// This invocation owns the event, or the event has no identity, or the
    /// claim failed: scan.
    Scan,
    /// Another invocation already claimed this event: skip the scan.
    Duplicate,
}

fn non_empty_str<'a>(event: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| event.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
}

/// The claim key for an event, or `None` when the payload names no tool call.
/// The project root and the scan targets are part of it, so two projects (or
/// two test fixtures) that happen to reuse an id never collide.
pub fn event_key(event: &Map<String, Value>, project_cwd: &str, files: &[String]) -> Option<String> {
    let call = non_empty_str(event, TOOL_CALL_ID_KEYS)?;
    let session = non_empty_str(event, SESSION_KEYS).unwrap_or("");
    let mut files: Vec<&str> = files.iter().map(String::as_str).collect();
    files.sort_unstable();
    let mut h = Sha256::new();
    for part in [session, call, project_cwd] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    for f in files {
        h.update(f.as_bytes());
        h.update([0u8]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `IMPECCABLE_HOOK_CLAIM_DIR`, else `<temp>/impeccable-hook-claims`, with the
/// temp dir read from the hook's own environment first.
fn claim_root(rt: &Runtime) -> PathBuf {
    if let Some(dir) = rt.env("IMPECCABLE_HOOK_CLAIM_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(dir);
    }
    let tmp = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| rt.env(k).filter(|s| !s.is_empty()).map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    tmp.join("impeccable-hook-claims")
}

fn now_bucket() -> Option<u64> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(secs / BUCKET_SECONDS)
}

/// Removes buckets older than the previous one. Runs only when this
/// invocation created the current bucket, so it is paid once per bucket.
fn sweep_old_buckets(root: &Path, bucket: u64) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(n) = name.to_str().and_then(|s| s.parse::<u64>().ok()) else { continue };
        if n + 1 < bucket {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Claims `key` for this invocation. `Duplicate` only when another
/// invocation provably holds it; every error is `Scan`.
pub fn claim_key(rt: &Runtime, key: &str) -> Claim {
    let Some(bucket) = now_bucket() else { return Claim::Scan };
    let root = claim_root(rt);
    if bucket > 0 && root.join((bucket - 1).to_string()).join(key).exists() {
        return Claim::Duplicate;
    }
    let dir = root.join(bucket.to_string());
    match std::fs::create_dir(&dir) {
        Ok(()) => sweep_old_buckets(&root, bucket),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => {
            if std::fs::create_dir_all(&dir).is_err() {
                return Claim::Scan;
            }
            sweep_old_buckets(&root, bucket);
        }
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(key)) {
        Ok(_) => Claim::Scan,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Claim::Duplicate,
        Err(_) => Claim::Scan,
    }
}

/// Claims the event this invocation is about to scan.
pub fn claim_event(rt: &Runtime, event: &Map<String, Value>, project_cwd: &str, files: &[String]) -> Claim {
    match event_key(event, project_cwd, files) {
        Some(key) => claim_key(rt, &key),
        None => Claim::Scan,
    }
}
