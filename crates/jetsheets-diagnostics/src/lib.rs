//! Diagnostics that remain available when Windows GUI binaries have no console.

use std::{
    backtrace::Backtrace,
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn diagnostics_directory() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("JetSheets")
        .join("logs")
}

fn write_at(directory: &Path, file_name: &str, message: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let path = directory.join(file_name);
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    writeln!(file, "[{timestamp}] {message}")?;
    file.flush()?;
    Ok(path)
}

/// Appends an error to `%LOCALAPPDATA%\JetSheets\logs` (or the temp directory).
pub fn record(file_name: &str, message: &str) -> io::Result<PathBuf> {
    write_at(&diagnostics_directory(), file_name, message)
}

/// Keeps panics in a local file while preserving Rust's normal debug output.
pub fn install_panic_hook(file_name: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let details = format!("Panic: {info}\nBacktrace:\n{}", Backtrace::force_capture());
        let _ = record(file_name, &details);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_file_survives_multiple_errors() {
        let directory = env::temp_dir().join(format!(
            "jetsheets-diagnostics-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = write_at(&directory, "app.log", "first error").unwrap();
        write_at(&directory, "app.log", "second error").unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("first error"));
        assert!(contents.contains("second error"));
        fs::remove_dir_all(directory).unwrap();
    }
}
