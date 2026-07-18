//! Nightly CI ledger writer (foundations CI/CD bead; golden-ledger
//! convention, plan §16.2): records a nightly gate run as an op + metrics in
//! an fs-ledger database that CI retains as an artifact — the nightly run is
//! itself provenance, queryable like any other study.
//!
//! Usage:
//!   nightly_ledger <db-path> <outcome: ok|error> <suite> <value>
//!
//! The Five Explicits come from the CI environment (deterministic-friendly:
//! all inputs are explicit argv/env, no hidden state):
//! - seed: not applicable to a gate run; the fixed literal `b"nightly"`
//! - versions: `{"frankensim": $GITHUB_SHA or "local"}`
//! - budget: `{"wall_s": 5400}` (the job timeout)
//! - capability: `{"ops": ["ci.nightly"], "runner": $RUNNER_OS or "local"}`
//!
//! Exit code 0 on success; 1 with a structured JSON error line on failure.

use core::fmt::Write as _;
use fs_ledger::{EventRow, FiveExplicits, Ledger, LedgerError, OpOutcome, now_wall_ns};

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0000}'..='\u{001f}' => {
                write!(&mut escaped, "\\u{:04x}", u32::from(ch))
                    .expect("writing to a String cannot fail");
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn fail(detail: &str) -> std::process::ExitCode {
    eprintln!(
        "{{\"error\":\"NightlyLedger\",\"detail\":\"{}\"}}",
        escape_json_string(detail)
    );
    std::process::ExitCode::FAILURE
}

fn rollback_after_error<T>(ledger: &Ledger, error: LedgerError) -> Result<T, LedgerError> {
    ledger.rollback()?;
    Err(error)
}

fn main() -> std::process::ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let [_, db, outcome_text, suite, value_text] = args.as_slice() else {
        return fail("usage: nightly_ledger <db-path> <ok|error> <suite> <value>");
    };
    let Some(db) = db.to_str() else {
        return fail("db-path must be valid UTF-8");
    };
    let Some(outcome_text) = outcome_text.to_str() else {
        return fail("outcome must be valid UTF-8");
    };
    let Some(suite) = suite.to_str() else {
        return fail("suite must be valid UTF-8");
    };
    let Some(value_text) = value_text.to_str() else {
        return fail("value must be valid UTF-8");
    };
    let outcome = match outcome_text {
        "ok" => OpOutcome::Ok,
        "error" => OpOutcome::Error,
        other => return fail(&format!("outcome must be ok|error, got {other}")),
    };
    let Ok(value) = value_text.parse::<f64>() else {
        return fail(&format!("value must be a float, got {value_text}"));
    };
    if !value.is_finite() {
        return fail(&format!("value must be finite, got {value_text}"));
    }

    let sha = std::env::var("GITHUB_SHA").unwrap_or_else(|_| "local".to_string());
    let runner = std::env::var("RUNNER_OS").unwrap_or_else(|_| "local".to_string());
    let versions = format!("{{\"frankensim\":\"{}\"}}", escape_json_string(&sha));
    let capability = format!(
        "{{\"ops\":[\"ci.nightly\"],\"runner\":\"{}\"}}",
        escape_json_string(&runner)
    );
    let suite_json = escape_json_string(suite);
    let outcome_json = escape_json_string(outcome_text);
    let db_json = escape_json_string(db);
    let explicits = FiveExplicits {
        seed: b"nightly",
        versions: &versions,
        budget: "{\"wall_s\":5400}",
        capability: &capability,
    };

    let result = (|| -> Result<i64, LedgerError> {
        let ledger = Ledger::open(db)?;
        ledger.begin()?;
        let write_result = (|| -> Result<i64, LedgerError> {
            let t0 = now_wall_ns();
            let ir = format!("{{\"op\":\"ci.nightly\",\"suite\":\"{suite_json}\"}}");
            let op = ledger.begin_op(Some(b"ci-nightly"), &ir, &explicits, t0)?;
            ledger.record_metric(op, 0, suite, value)?;
            ledger.append_event(&EventRow {
                session: Some(b"ci-nightly"),
                t: t0,
                kind: "benchmark_result",
                payload: Some(&format!(
                    "{{\"kernel\":\"ci.nightly\",\"metric\":\"{suite_json}\",\"value\":{value}}}"
                )),
            })?;
            ledger.finish_op(op, outcome, None, now_wall_ns())?;
            Ok(op)
        })();
        match write_result {
            Ok(op) => {
                if let Err(error) = ledger.commit() {
                    return rollback_after_error(&ledger, error);
                }
                Ok(op)
            }
            Err(error) => rollback_after_error(&ledger, error),
        }
    })();

    match result {
        Ok(op) => {
            println!(
                "{{\"suite\":\"ci.nightly\",\"op\":{op},\"outcome\":\"{outcome_json}\",\
                 \"metric\":\"{suite_json}\",\"value\":{value},\"db\":\"{db_json}\"}}"
            );
            std::process::ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}
