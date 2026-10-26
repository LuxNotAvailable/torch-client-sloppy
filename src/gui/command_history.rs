use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use crate::platform::storage;

const MAX_SAVED: usize = 50;

struct History {
    commands: VecDeque<String>,
    dirty: bool,
}

static HISTORY: OnceLock<Mutex<History>> = OnceLock::new();

fn history() -> &'static Mutex<History> {
    HISTORY.get_or_init(|| {
        let mut commands = VecDeque::new();
        if let Some(text) = storage::COMMAND_HISTORY.load() {
            for line in text.lines().filter(|l| !l.is_empty()) {
                push_back_capped(&mut commands, line.to_string(), MAX_SAVED);
            }
        }
        Mutex::new(History {
            commands,
            dirty: false,
        })
    })
}

pub fn load() -> Vec<String> {
    history().lock().unwrap().commands.iter().cloned().collect()
}

pub fn record(command: &str) {
    let mut h = history().lock().unwrap();
    if push_capped(&mut h.commands, command, MAX_SAVED) {
        h.dirty = true;
    }
}

pub(crate) fn push_capped(list: &mut VecDeque<String>, line: &str, cap: usize) -> bool {
    if list.back().map(String::as_str) == Some(line) {
        return false;
    }
    push_back_capped(list, line.to_string(), cap);
    true
}

fn push_back_capped(list: &mut VecDeque<String>, line: String, cap: usize) {
    if list.len() >= cap {
        list.pop_front();
    }
    list.push_back(line);
}

pub fn save() {
    let Some(lock) = HISTORY.get() else {
        return;
    };
    let mut h = lock.lock().unwrap();
    if !std::mem::take(&mut h.dirty) {
        return;
    }
    let len = h.commands.iter().map(|c| c.len() + 1).sum();
    let mut out = String::with_capacity(len);
    for command in &h.commands {
        out.push_str(command);
        out.push('\n');
    }
    storage::COMMAND_HISTORY.store(&out);
}
