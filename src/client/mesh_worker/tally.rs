use std::cell::RefCell;

use crate::platform::time::Instant;

struct Entry {
    name: &'static str,
    calls: u64,
    us: u64,
    n: u64,
}

struct Table {
    entries: Vec<Entry>,
    since: Option<Instant>,
}

thread_local! {
    static TABLE: RefCell<Table> = const {
        RefCell::new(Table {
            entries: Vec::new(),
            since: None,
        })
    };
}

pub fn time<T>(name: &'static str, f: impl FnOnce() -> T, n: impl FnOnce(&T) -> u64) -> T {
    if !cfg!(feature = "budget") {
        return f();
    }
    let started = Instant::now();
    let out = f();
    let us = started.elapsed().as_micros() as u64;
    add(name, us, n(&out));
    out
}

pub fn count(name: &'static str, n: u64) {
    if !cfg!(feature = "budget") {
        return;
    }
    add(name, 0, n);
}

fn add(name: &'static str, us: u64, n: u64) {
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        match table.entries.iter_mut().find(|e| e.name == name) {
            Some(e) => {
                e.calls += 1;
                e.us += us;
                e.n += n;
            }
            None => table.entries.push(Entry {
                name,
                calls: 1,
                us,
                n,
            }),
        }
    });
}

pub fn flush(side: &str) {
    if !cfg!(feature = "budget") {
        return;
    }
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        let now = Instant::now();
        let since = *table.since.get_or_insert(now);
        if now.duration_since(since).as_secs_f32() < 1.0 {
            return;
        }
        table.since = Some(now);
        if table.entries.iter().all(|e| e.calls == 0) {
            return;
        }
        let line = table
            .entries
            .iter()
            .filter(|e| e.calls > 0)
            .map(|e| {
                format!(
                    "{} {}x {:.1}ms n={}",
                    e.name,
                    e.calls,
                    e.us as f32 / 1000.0,
                    e.n
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        for e in &mut table.entries {
            e.calls = 0;
            e.us = 0;
            e.n = 0;
        }
        crate::log_info!("budget", "{side} {line}");
    });
}
