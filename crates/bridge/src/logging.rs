use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
};

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::{BridgeError, BridgeErrorKind, paths::BridgePaths};

const MAX_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024;

static LOGGER_STATE: OnceLock<Arc<Mutex<LoggerState>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogStatus {
    pub logs_root: PathBuf,
    pub active_session: String,
    pub active_file: PathBuf,
    pub active_file_size_bytes: u64,
}

pub struct ApplicationLogger {
    inner: Arc<Mutex<LoggerState>>,
}

struct LoggerState {
    logs_root: PathBuf,
    active_session: String,
    active_file_id: u64,
    active_file: PathBuf,
    active_file_size_bytes: u64,
    file: Option<File>,
}

impl ApplicationLogger {
    /// # Errors
    ///
    /// Returns an error when the log session directory or initial file cannot
    /// be created.
    pub fn new(paths: &BridgePaths) -> Result<Self, BridgeError> {
        let logs_root = paths.logs_root();
        let state = LoggerState::new(logs_root)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(state)),
        })
    }

    /// # Errors
    ///
    /// Returns an error when the log session cannot be initialized.
    pub fn install(paths: &BridgePaths) -> Result<(), BridgeError> {
        let logger = Self::new(paths)?;
        let inner = Arc::clone(&logger.inner);
        if LOGGER_STATE.set(inner).is_err() {
            return Ok(());
        }

        log::set_boxed_logger(Box::new(logger)).map_err(|error| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: format!("failed to install application logger: {error}"),
        })?;

        let max_level = std::env::var("WALLPAPER_ENGINE_LOG_LEVEL")
            .ok()
            .and_then(|value| LevelFilter::from_str(&value).ok())
            .unwrap_or(LevelFilter::Info);
        log::set_max_level(max_level);
        Ok(())
    }

    pub fn emit_gui_log(level: Level, file: &str, line: u32, message: &str) {
        let args = format_args!("{message}");
        let line = format_record(level, Some(file), Some(line), &args);
        if let Some(inner) = LOGGER_STATE.get()
            && let Ok(mut state) = inner.lock()
        {
            state.write_line(level, &line);
        }
    }

    #[must_use]
    pub fn status() -> Option<LogStatus> {
        LOGGER_STATE
            .get()
            .and_then(|inner| inner.lock().ok().map(|state| state.status()))
    }

    #[must_use]
    pub fn logs_root() -> Option<PathBuf> {
        Self::status().map(|status| status.logs_root)
    }

    /// # Errors
    ///
    /// Returns an error when a new active log session cannot be created.
    pub fn clear() -> Result<LogStatus, BridgeError> {
        let inner = LOGGER_STATE.get().ok_or_else(|| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: "application logger is not installed".to_string(),
        })?;
        let mut state = inner.lock().map_err(|_| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: "application logger state is poisoned".to_string(),
        })?;
        state.clear()
    }

    #[cfg(test)]
    fn instance_status(&self) -> Option<LogStatus> {
        self.inner.lock().ok().map(|state| state.status())
    }

    #[cfg(test)]
    fn clear_instance(&self) -> Result<LogStatus, BridgeError> {
        self.inner
            .lock()
            .map_err(|_| BridgeError::Error {
                kind: BridgeErrorKind::Io,
                message: "application logger state is poisoned".to_string(),
            })?
            .clear()
    }
}

impl Log for ApplicationLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format_record(record.level(), record.file(), record.line(), record.args());
        if let Ok(mut state) = self.inner.lock() {
            state.write_line(record.level(), &line);
        }
    }

    fn flush(&self) {}
}

fn format_record(
    level: Level,
    file: Option<&str>,
    line: Option<u32>,
    args: &std::fmt::Arguments<'_>,
) -> String {
    let file = file
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let line = line.unwrap_or(0);
    format!("{level} {file}:{line} {args}\n")
}

impl LoggerState {
    #[allow(clippy::single_call_fn)]
    fn new(logs_root: PathBuf) -> Result<Self, BridgeError> {
        fs::create_dir_all(&logs_root).map_err(|error| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: error.to_string(),
        })?;
        let active_session = unique_session_name(&logs_root);
        let session_root = logs_root.join(&active_session);
        fs::create_dir_all(&session_root).map_err(|error| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: error.to_string(),
        })?;
        let active_file = session_root.join("0.log");
        let file = open_log_file(&active_file).map_err(|error| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: error.to_string(),
        })?;
        let active_file_size_bytes = file.metadata().map_or(0, |metadata| metadata.len());

        Ok(Self {
            logs_root,
            active_session,
            active_file_id: 0,
            active_file,
            active_file_size_bytes,
            file: Some(file),
        })
    }

    fn status(&self) -> LogStatus {
        LogStatus {
            logs_root: self.logs_root.clone(),
            active_session: self.active_session.clone(),
            active_file: self.active_file.clone(),
            active_file_size_bytes: self.active_file_size_bytes,
        }
    }

    fn clear(&mut self) -> Result<LogStatus, BridgeError> {
        let old_sessions = fs::read_dir(&self.logs_root)
            .map_err(|error| BridgeError::Error {
                kind: BridgeErrorKind::Io,
                message: error.to_string(),
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();

        let active_session = unique_session_name(&self.logs_root);
        let session_root = self.logs_root.join(&active_session);
        fs::create_dir_all(&session_root).map_err(|error| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: error.to_string(),
        })?;
        let active_file = session_root.join("0.log");
        let file = open_log_file(&active_file).map_err(|error| BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: error.to_string(),
        })?;

        self.file = Some(file);
        self.active_session = active_session;
        self.active_file_id = 0;
        self.active_file = active_file;
        self.active_file_size_bytes = 0;

        for path in old_sessions {
            if path == session_root {
                continue;
            }
            let result = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(error) = result {
                eprintln!("ERROR logging.rs:0 failed to remove old log path: {error}");
            }
        }

        Ok(self.status())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.active_file_id = self.active_file_id.saturating_add(1);
        self.active_file = self
            .logs_root
            .join(&self.active_session)
            .join(format!("{}.log", self.active_file_id));
        let file = open_log_file(&self.active_file)?;
        self.active_file_size_bytes = file.metadata().map_or(0, |metadata| metadata.len());
        self.file = Some(file);
        Ok(())
    }

    fn write_line(&mut self, level: Level, line: &str) {
        match level {
            Level::Error | Level::Warn => eprint!("{line}"),
            Level::Info | Level::Debug | Level::Trace => print!("{line}"),
        }

        if let Err(error) = self.write_line_to_file(line) {
            eprintln!("ERROR logging.rs:0 failed to persist log: {error}");
        }
    }

    fn write_line_to_file(&mut self, line: &str) -> io::Result<()> {
        let line_len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.active_file_size_bytes > 0
            && self.active_file_size_bytes.saturating_add(line_len) > MAX_LOG_FILE_BYTES
        {
            self.rotate()?;
        }

        if let Some(file) = &mut self.file {
            file.write_all(line.as_bytes())?;
            file.flush()?;
            self.active_file_size_bytes = self.active_file_size_bytes.saturating_add(line_len);
        }
        Ok(())
    }
}

fn unique_session_name(logs_root: &Path) -> String {
    let base = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    if !logs_root.join(&base).exists() {
        return base;
    }

    for suffix in 1.. {
        let candidate = format!("{base}-{suffix}");
        if !logs_root.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!("unbounded suffix loop must return");
}

fn open_log_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use log::Level;

    use super::{ApplicationLogger, format_record};
    use crate::paths::BridgePaths;

    #[test]
    fn formats_file_name_line_and_message() {
        let args = format_args!("hello {}", "world");
        let line = format_record(Level::Info, Some("/tmp/source.rs"), Some(42), &args);
        assert_eq!(line, "INFO source.rs:42 hello world\n");
    }

    #[test]
    fn clear_switches_to_new_session_and_removes_old_sessions() {
        let root = tempfile::tempdir().unwrap();
        let paths = BridgePaths::for_home(root.path());
        let logger = ApplicationLogger::new(&paths).unwrap();
        let before = logger.instance_status().unwrap();
        let old = before.logs_root.join("old-session");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("ignored.log"), b"old").unwrap();

        let after = logger.clear_instance().unwrap();

        assert_ne!(before.active_session, after.active_session);
        assert!(after.active_file.exists());
        assert_eq!(fs::read_dir(&after.logs_root).unwrap().count(), 1);
    }
}
