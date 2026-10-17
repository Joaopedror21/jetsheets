use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Condvar, Mutex},
};

use crate::{
    http::Response,
    install::partial_path,
    release::{REPOSITORY, Release},
};

#[derive(Default)]
struct Flags {
    paused: bool,
    cancelled: bool,
    verifying: bool,
}

#[derive(Default)]
pub struct Control {
    flags: Mutex<Flags>,
    wake: Condvar,
}

impl Control {
    pub fn pause(&self) -> bool {
        let mut flags = self.flags.lock().unwrap();
        if flags.verifying || flags.cancelled {
            return false;
        }
        flags.paused = true;
        true
    }

    pub fn resume(&self) {
        self.flags.lock().unwrap().paused = false;
        self.wake.notify_all();
    }

    pub fn cancel(&self) -> bool {
        let mut flags = self.flags.lock().unwrap();
        if flags.verifying {
            return false;
        }
        flags.cancelled = true;
        self.wake.notify_all();
        true
    }

    pub fn is_paused(&self) -> bool {
        self.flags.lock().unwrap().paused
    }

    pub fn is_cancelled(&self) -> bool {
        self.flags.lock().unwrap().cancelled
    }

    pub fn begin_verifying(&self) -> bool {
        let mut flags = self.flags.lock().unwrap();
        if flags.cancelled {
            return false;
        }
        flags.verifying = true;
        true
    }

    fn wait_active(&self) -> bool {
        let mut flags = self.flags.lock().unwrap();
        while flags.paused && !flags.cancelled {
            flags = self.wake.wait(flags).unwrap();
        }
        !flags.cancelled
    }
}

pub fn latest_release() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let mut response = Response::open(&url, None)?;
    if response.status != 200 {
        return Err(format!(
            "GitHub respondeu HTTP {} ao buscar atualizações. Confira se há uma Release publicada.",
            response.status
        ));
    }
    Release::from_api_json(&response.read_limited(2 * 1024 * 1024)?)
}

fn validate_range(header: &str, offset: u64, total: u64) -> Result<(), String> {
    let value = header
        .trim()
        .strip_prefix("bytes ")
        .ok_or("Content-Range inválido.")?;
    let (range, size) = value.split_once('/').ok_or("Content-Range inválido.")?;
    let (start, end) = range.split_once('-').ok_or("Content-Range inválido.")?;
    let start = start
        .parse::<u64>()
        .map_err(|_| "Content-Range inválido.")?;
    let end = end.parse::<u64>().map_err(|_| "Content-Range inválido.")?;
    let size = size.parse::<u64>().map_err(|_| "Content-Range inválido.")?;
    if start != offset || end < start || end >= size || size != total {
        return Err("A resposta parcial não corresponde ao arquivo publicado.".into());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum DownloadResult {
    Complete,
    Cancelled,
}

/// Downloads to a same-folder `.part` file. Pausing closes the connection; resuming uses HTTP Range.
pub fn download(
    release: &Release,
    folder: &Path,
    control: &Control,
    mut progress: impl FnMut(u64),
) -> Result<DownloadResult, String> {
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let part = partial_path(folder, release);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&part)
        .map_err(|error| format!("Não foi possível criar arquivo temporário: {error}"))?;
    let mut offset = file.metadata().map_err(|error| error.to_string())?.len();
    if offset > release.size {
        file.set_len(0).map_err(|error| error.to_string())?;
        offset = 0;
    }
    progress(offset);
    let mut retries = 0;
    while offset < release.size {
        if !control.wait_active() {
            drop(file);
            fs::remove_file(&part).map_err(|error| error.to_string())?;
            return Ok(DownloadResult::Cancelled);
        }
        let mut response = Response::open(&release.download_url, (offset > 0).then_some(offset))?;
        match (offset, response.status) {
            (0, 200) => {}
            (n, 206) if n > 0 => validate_range(&response.content_range()?, n, release.size)?,
            (n, 200) if n > 0 => {
                file.set_len(0).map_err(|error| error.to_string())?;
                offset = 0;
                progress(0);
            }
            (_, status) => return Err(format!("Download recusado pelo GitHub (HTTP {status}).")),
        }
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if control.is_cancelled() {
                drop(response);
                drop(file);
                fs::remove_file(&part).map_err(|error| error.to_string())?;
                return Ok(DownloadResult::Cancelled);
            }
            if control.is_paused() {
                file.sync_all().map_err(|error| error.to_string())?;
                break;
            }
            let count = match response.read(&mut buffer) {
                Ok(count) => count,
                Err(error) if retries < 3 => {
                    retries += 1;
                    let _ = error;
                    break;
                }
                Err(error) => return Err(format!("Download interrompido: {error}")),
            };
            if count == 0 {
                if offset < release.size {
                    retries += 1;
                }
                break;
            }
            if offset + count as u64 > release.size {
                return Err("O servidor enviou mais dados que o tamanho publicado.".into());
            }
            file.write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
            offset += count as u64;
            progress(offset);
            retries = 0;
            if offset == release.size {
                break;
            }
        }
        if retries > 3 {
            return Err("O download terminou antes de receber o arquivo completo.".into());
        }
    }
    file.sync_all().map_err(|error| error.to_string())?;
    if control.is_cancelled() {
        drop(file);
        fs::remove_file(&part).map_err(|error| error.to_string())?;
        return Ok(DownloadResult::Cancelled);
    }
    Ok(DownloadResult::Complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_requested_partial_range() {
        assert!(validate_range("bytes 100-199/200", 100, 200).is_ok());
        assert!(validate_range("bytes 0-199/200", 100, 200).is_err());
        assert!(validate_range("bytes 100-199/201", 100, 200).is_err());
    }

    #[test]
    fn cancellation_releases_a_paused_worker() {
        let control = std::sync::Arc::new(Control::default());
        assert!(control.pause());
        let worker = std::sync::Arc::clone(&control);
        let thread = std::thread::spawn(move || worker.wait_active());
        assert!(control.cancel());
        assert!(!thread.join().unwrap());
    }

    #[test]
    fn cancel_is_rejected_after_installation_begins() {
        let control = Control::default();
        assert!(control.begin_verifying());
        assert!(!control.cancel());
    }
}
