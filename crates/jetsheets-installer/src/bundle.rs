//! Optional application binary embedded into the downloadable installer.

use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

use crate::{
    digest::sha256_file,
    install::{install_verified, partial_path},
    release::{Release, Version},
};

const APP_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bundled.exe"));

pub fn available() -> bool {
    APP_BYTES.starts_with(b"MZ") && APP_BYTES.len() > 1024
}

pub fn version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("versão do instalador válida")
}

pub fn install(folder: &Path) -> Result<(), String> {
    if !available() {
        return Err("Esta compilação não inclui o aplicativo. Use uma Release publicada.".into());
    }
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let temporary = folder.join(".jetsheets-bundled.part");
    let mut file = File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(APP_BYTES)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    let sha256 = sha256_file(&temporary)?;
    let release = Release {
        tag,
        version: version(),
        notes: String::new(),
        page_url: String::new(),
        download_url: String::new(),
        size: APP_BYTES.len() as u64,
        sha256,
    };
    let part = partial_path(folder, &release);
    if part.exists() {
        fs::remove_file(&part).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, &part).map_err(|error| error.to_string())?;
    let result = install_verified(folder, &part, &release);
    if result.is_err() && part.exists() {
        let _ = fs::remove_file(part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_file_is_either_empty_or_a_windows_executable() {
        assert!(APP_BYTES.is_empty() || available());
    }

    #[test]
    fn embedded_application_installs_when_present() {
        if !available() {
            return;
        }
        let folder = std::env::temp_dir().join(format!(
            "jetsheets-bundle-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        install(&folder).unwrap();
        assert_eq!(
            std::fs::metadata(folder.join(crate::release::EXECUTABLE_NAME))
                .unwrap()
                .len(),
            APP_BYTES.len() as u64
        );
        assert_eq!(
            crate::install::installed(&folder).unwrap().unwrap().version,
            format!("v{}", env!("CARGO_PKG_VERSION"))
        );
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    #[ignore = "opens the installed Windows desktop application"]
    fn installed_application_starts_on_windows() {
        assert!(
            available(),
            "build the installer with JETSHEETS_BUNDLE_PATH"
        );
        let folder =
            std::env::temp_dir().join(format!("jetsheets-launch-test-{}", std::process::id()));
        install(&folder).unwrap();
        let mut app = std::process::Command::new(folder.join(crate::release::EXECUTABLE_NAME))
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let status = app.try_wait().unwrap();
        if status.is_none() {
            app.kill().unwrap();
            app.wait().unwrap();
        }
        std::fs::remove_dir_all(folder).unwrap();
        assert!(
            status.is_none(),
            "installed application exited immediately: {status:?}"
        );
    }
}
