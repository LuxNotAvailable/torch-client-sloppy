pub(crate) fn install_panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string panic payload>");
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>").to_string();
        let location = match info.location() {
            Some(l) => format!("{}:{}:{}", l.file(), l.line(), l.column()),
            None => "<unknown location>".to_string(),
        };

        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let report = format!(
            "\n\
             ==================== PANIC ====================\n\
             thread : {name} ({:?})\n\
             at     : {location}\n\
             message: {msg}\n\
             \n{}\n\
             ===============================================\n",
            thread.id(),
            backtrace.trim_end(),
        );

        {
            use std::io::Write as _;
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(report.as_bytes());
            let _ = err.flush();
            super::logfile::write(&report, true);
            write_crash_log(&report);
        }
    }));
}

fn write_crash_log(report: &str) {
    use std::io::Write as _;
    let dir = crate::platform::storage::dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = crate::platform::screenshot::timestamp(crate::platform::screenshot::epoch_secs())
        .replace(['_', '.'], "-");
    let path = dir.join(format!("{stamp}-crash-log.txt"));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(report.as_bytes());
        // What led up to it: the session log's last lines (the panic itself
        // is the last of them, so it is not repeated).
        let recent = super::logfile::recent();
        let before = recent.len().saturating_sub(report.lines().count());
        let _ = writeln!(
            file,
            "\n---------------- log before the panic ----------------"
        );
        for line in &recent[..before] {
            let _ = writeln!(file, "{line}");
        }
    }
}
