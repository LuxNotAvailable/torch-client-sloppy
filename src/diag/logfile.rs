//! The session log: every line the client logs, and every warning or error
//! Bevy, wgpu and azalea report through `tracing`, written to
//! `<storage>/logs/latest.log` as the game runs. The previous session's log
//! is kept as `<stamp>.log` (the last `KEEP` of them), and a crash report
//! ends with the lines that led up to the crash.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::PathBuf;
use std::sync::Mutex;

/// Session logs kept besides `latest.log`.
const KEEP: usize = 10;
/// Lines a crash report carries from before the crash.
const RECENT: usize = 300;

struct Log {
    file: Option<BufWriter<File>>,
    recent: VecDeque<String>,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    file: None,
    recent: VecDeque::new(),
});

pub(crate) fn dir() -> PathBuf {
    crate::platform::storage::dir().join("logs")
}

pub(crate) fn latest() -> PathBuf {
    dir().join("latest.log")
}

/// Opens `latest.log`, keeping the last session's under its date.
pub(crate) fn init() {
    let dir = dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let latest = latest();
    if let Ok(meta) = std::fs::metadata(&latest) {
        let secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        let stamp = crate::platform::screenshot::timestamp(secs).replace(['_', '.'], "-");
        let _ = std::fs::rename(&latest, dir.join(format!("{stamp}.log")));
    }
    prune(&dir);
    let Ok(file) = File::create(&latest) else {
        return;
    };
    let mut file = BufWriter::new(file);
    let _ = writeln!(
        file,
        "Torch Client {} (protocol {}, {} {}), started {}",
        env!("CARGO_PKG_VERSION"),
        crate::protocol::version::NATIVE.name,
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::platform::screenshot::timestamp(crate::platform::screenshot::epoch_secs()),
    );
    let _ = file.flush();
    if let Ok(mut log) = LOG.lock() {
        log.file = Some(file);
    }
}

/// Deletes the oldest session logs past `KEEP`; crash reports stay.
fn prune(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|e| e == "log")
                && p.file_name().is_some_and(|n| n != "latest.log")
        })
        .collect();
    logs.sort();
    let over = logs.len().saturating_sub(KEEP);
    for old in &logs[..over] {
        let _ = std::fs::remove_file(old);
    }
}

/// Appends a line (or several, `\n`-separated). Warnings and errors are
/// flushed at once, so a crash right after still leaves them on disk.
pub(crate) fn write(text: &str, urgent: bool) {
    let Ok(mut log) = LOG.lock() else { return };
    for line in text.split('\n') {
        if log.recent.len() >= RECENT {
            log.recent.pop_front();
        }
        log.recent.push_back(line.to_owned());
    }
    if let Some(file) = log.file.as_mut() {
        let _ = writeln!(file, "{text}");
        if urgent {
            let _ = file.flush();
        }
    }
}

pub(crate) fn flush() {
    if let Ok(mut log) = LOG.lock()
        && let Some(file) = log.file.as_mut()
    {
        let _ = file.flush();
    }
}

/// The lines logged last, oldest first, for a crash report.
pub(crate) fn recent() -> Vec<String> {
    LOG.lock()
        .map(|log| log.recent.iter().cloned().collect())
        .unwrap_or_default()
}

/// Writes Bevy's, wgpu's and azalea's `tracing` events at info and above
/// into the session log.
pub(crate) struct FileLayer;

struct Message(String);

impl bevy::log::tracing::field::Visit for Message {
    fn record_debug(
        &mut self,
        field: &bevy::log::tracing::field::Field,
        value: &dyn std::fmt::Debug,
    ) {
        use std::fmt::Write as _;
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

impl<S> bevy::log::tracing_subscriber::Layer<S> for FileLayer
where
    S: bevy::log::tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &bevy::log::tracing::Event<'_>,
        _ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        use bevy::log::tracing::Level;
        let level = *event.metadata().level();
        if level > Level::INFO {
            return;
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        let label = match level {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN ",
            _ => "INFO ",
        };
        write(
            &format!("{label} [{}] {}", event.metadata().target(), message.0),
            level <= Level::WARN,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_lines_are_kept_up_to_the_limit() {
        for i in 0..RECENT + 5 {
            write(&format!("line {i}"), false);
        }
        let recent = recent();
        assert_eq!(recent.len(), RECENT);
        assert_eq!(
            recent.last().map(String::as_str),
            Some(format!("line {}", RECENT + 4).as_str())
        );
    }
}
