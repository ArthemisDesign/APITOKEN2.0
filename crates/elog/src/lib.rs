//! Unified error logging for the engine.
//!
//! Every runtime diagnostic line in the engine flows through this crate instead of raw
//! `eprintln!` so that:
//!
//! - the output format is uniform and greppable: `[LEVEL][category] message`;
//! - levels separate failures (`error`) from degradations (`warn`) and lifecycle noise
//!   (`info`);
//! - every line passes through one secret scrubber — a token that reaches `elog` cannot
//!   reach the log, regardless of which layer emitted it (engine invariant: never print
//!   tokens);
//! - switching the sink later (journald fields, structured tracing) touches one crate,
//!   not hundreds of call sites.
//!
//! The crate is a leaf: it has no dependencies and must not grow any. Layers that log
//! (`forward`, `server`, `router`, `authbot`, `registry`, `pool`) depend on it directly;
//! `metering` stays pure and does not.
//!
//! ## Usage
//!
//! ```ignore
//! elog::error("forward", format!("subs admin query failed: {e:#}"));
//! elog::warn("billing", "billing reserve cancellation did not produce a balance");
//! elog::info("server", "listening on 127.0.0.1:8080");
//! ```
//!
//! The message is `impl Display`: compose with `format!` at the call site. Anyhow errors
//! keep their chain via the alternate `{e:#}` form. Do not pass untrusted client content
//! as the message itself when a static description is available — prefer a static reason
//! plus sanitized details; the scrubber is the last line of defense, not a license.
//!
//! ## Levels
//!
//! - `error` — an operation failed or an invariant was violated; an operator should
//!   eventually look at it.
//! - `warn` — degradation, retry, rotation, best-effort fallback that succeeded; the
//!   system still serves.
//! - `info` — lifecycle and state transitions (startup, shutdown, refresh success).

use std::fmt::Display;

/// Severity of a diagnostic line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// A failure or invariant violation — an operator should look at it.
    Error,
    /// Degradation, retry, rotation, best-effort fallback — the system still serves.
    Warn,
    /// Lifecycle and state transitions.
    Info,
}

impl Level {
    /// Upper-case tag used in the emitted line, e.g. `ERROR`.
    pub fn tag(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
        }
    }
}

/// Log a failure. See the crate docs for the message contract.
pub fn error(category: &str, msg: impl Display) {
    emit(Level::Error, category, msg);
}

/// Log a degradation or retry. See the crate docs for the message contract.
pub fn warn(category: &str, msg: impl Display) {
    emit(Level::Warn, category, msg);
}

/// Log a lifecycle or state transition. See the crate docs for the message contract.
pub fn info(category: &str, msg: impl Display) {
    emit(Level::Info, category, msg);
}

fn emit(level: Level, category: &str, msg: impl Display) {
    let line = format!("[{}][{}] {}", level.tag(), category, msg);
    eprintln!("{}", scrub_secrets(&line));
}

/// Mask secrets (bearer tokens, `sk-*` keys, Google OAuth tokens, key headers) in a line
/// before it is printed. Purely lexical: a token whose prefix is present is masked
/// together with the following non-space run. Applied to every emitted line, so a secret
/// embedded in any message — even one composed by another layer — cannot reach the log.
pub fn scrub_secrets(line: &str) -> String {
    const TOKEN_RUN: &[u8] = b" \t\r\n\"',:]}=]";
    // (prefix, keep_prefix) — keep_prefix retains the label ("Bearer ") and masks only
    // the token run; otherwise the whole match including the prefix is masked.
    const PREFIXES: &[(&str, bool)] = &[
        ("Bearer ", true),
        ("bearer ", true),
        ("authorization: ", true),
        ("Authorization: ", true),
        ("x-api-key: ", true),
        ("X-Api-Key: ", true),
        ("x-goog-api-key: ", true),
        ("X-Goog-Api-Key: ", true),
        ("sk-ant-", false),
        ("sk-pool-", false),
        ("sk-openkeys-", false),
        ("sk-proj-", false),
        ("sk-svcacct-", false),
        ("ya29.", false),
    ];
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < bytes.len() {
        let mut masked = false;
        for (prefix, keep) in PREFIXES {
            let start_is_word = i > 0
                && !matches!(
                    bytes[i - 1],
                    b' ' | b'\t'
                        | b'\n'
                        | b'\r'
                        | b'"'
                        | b'\''
                        | b','
                        | b'['
                        | b'('
                        | b'{'
                        | b':'
                        | b'='
                );
            if start_is_word {
                break;
            }
            if bytes[i..].starts_with(prefix.as_bytes()) {
                if *keep {
                    out.push_str(prefix);
                }
                out.push_str("***");
                let mut j = i + prefix.len();
                while j < bytes.len() && !TOKEN_RUN.contains(&bytes[j]) {
                    j += 1;
                }
                i = j;
                masked = true;
                break;
            }
        }
        if !masked {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests;
