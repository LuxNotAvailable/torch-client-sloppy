//! Machine-readable progress for front ends (`--progress`): one JSON
//! object a line on stdout, prefixed `@@`. Plain lines print either way.
//!
//! Events: `stage` (a phase starts; `total` steps when known), `step` (one
//! item of the current stage done, with a detail line), `command` (a
//! process about to run), `done` (a stage finished, with a summary).

use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

static ON: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

fn emit(value: Value) {
    if ON.load(Ordering::Relaxed) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "@@{value}");
        let _ = out.flush();
    }
}

pub fn stage(id: &str, label: &str, total: usize) {
    emit(json!({"ev": "stage", "id": id, "label": label, "total": total}));
}

pub fn step(stage: &str, label: &str, detail: &str, done: usize, total: usize) {
    emit(
        json!({"ev": "step", "stage": stage, "label": label, "detail": detail, "done": done, "total": total}),
    );
}

pub fn done(stage: &str, summary: &str) {
    emit(json!({"ev": "done", "stage": stage, "summary": summary}));
}

/// A process about to run, as a shell would show it.
pub fn command(cmd: &Command) {
    let mut line = cmd.get_program().to_string_lossy().into_owned();
    for arg in cmd.get_args() {
        let arg = arg.to_string_lossy();
        if arg.contains(' ') {
            line.push_str(&format!(" \"{arg}\""));
        } else {
            line.push(' ');
            line.push_str(&arg);
        }
    }
    emit(json!({"ev": "command", "cmd": line}));
}
