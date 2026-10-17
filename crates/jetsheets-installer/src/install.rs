use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    digest::sha256_file,
    release::{EXECUTABLE_NAME, RECORD_NAME, REPOSITORY, Release, Version},
};

const BACKUP_EXE: &str = ".jetsheets-rollback.exe";
const BACKUP_RECORD: &str = ".jetsheets-rollback.json";
const NEXT_RECORD: &str = ".jetsheets-install.next.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstallRecord {
    repository: String,
    pub version: String,
    pub sha256: String,
}

impl InstallRecord {
    fn for_release(release: &Release) -> Self {
        Self {
            repository: REPOSITORY.into(),
            version: release.tag.clone(),
            sha256: release.sha256.clone(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if !self.repository.eq_ignore_ascii_case(REPOSITORY) {
            return Err("Esta pasta pertence a outra instalação.".into());
        }
        Version::parse(&self.version)?;
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("O registro da instalação contém um SHA-256 inválido.".into());
        }
        Ok(())
    }
}

pub fn partial_path(folder: &Path, release: &Release) -> PathBuf {
    folder.join(format!(
        ".jetsheets-{}-{}.part",
        release.tag,
        &release.sha256[..12]
    ))
}

fn record_from(path: &Path) -> Result<InstallRecord, String> {
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size > 16 * 1024 {
        return Err("O registro da instalação é grande demais.".into());
    }
    let record: InstallRecord =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Registro de instalação inválido: {error}"))?;
    record.validate()?;
    Ok(record)
}

fn matches_record(executable: &Path, record_path: &Path) -> bool {
    record_from(record_path).is_ok_and(|record| {
        sha256_file(executable).is_ok_and(|digest| digest.eq_ignore_ascii_case(&record.sha256))
    })
}

/// Completes a committed swap or restores the previous executable after interruption.
pub fn recover(folder: &Path) -> Result<(), String> {
    if !folder.exists() {
        return Ok(());
    }
    let executable = folder.join(EXECUTABLE_NAME);
    let record = folder.join(RECORD_NAME);
    let backup_exe = folder.join(BACKUP_EXE);
    let backup_record = folder.join(BACKUP_RECORD);
    let next_record = folder.join(NEXT_RECORD);

    if backup_exe.exists() {
        if executable.exists() && record.exists() && matches_record(&executable, &record) {
            fs::remove_file(&backup_exe)
                .map_err(|error| format!("Falha ao limpar versão anterior: {error}"))?;
            if backup_record.exists() {
                fs::remove_file(&backup_record).map_err(|error| error.to_string())?;
            }
        } else {
            if executable.exists() {
                fs::remove_file(&executable).map_err(|error| {
                    format!("Feche o JetSheets antes de recuperar a atualização: {error}")
                })?;
            }
            fs::rename(&backup_exe, &executable)
                .map_err(|error| format!("Falha ao recuperar versão anterior: {error}"))?;
            if backup_record.exists() {
                if record.exists() {
                    fs::remove_file(&record).map_err(|error| error.to_string())?;
                }
                fs::rename(&backup_record, &record).map_err(|error| error.to_string())?;
            }
        }
    }
    if !backup_exe.exists()
        && backup_record.exists()
        && executable.exists()
        && record.exists()
        && matches_record(&executable, &record)
    {
        fs::remove_file(&backup_record).map_err(|error| error.to_string())?;
    }
    if next_record.exists() {
        if !record.exists() && executable.exists() && matches_record(&executable, &next_record) {
            fs::rename(&next_record, &record).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(&next_record).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn installed(folder: &Path) -> Result<Option<InstallRecord>, String> {
    recover(folder)?;
    let executable = folder.join(EXECUTABLE_NAME);
    let record = folder.join(RECORD_NAME);
    match (executable.exists(), record.exists()) {
        (false, false) => Ok(None),
        (true, true) => record_from(&record).map(Some),
        _ => Err("Esta pasta contém uma instalação incompleta ou não reconhecida.".into()),
    }
}

/// Installs only a fully verified file, with a recoverable same-folder swap.
pub fn install_verified(folder: &Path, part: &Path, release: &Release) -> Result<(), String> {
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let existing = installed(folder)?;
    if let Some(existing) = &existing
        && Version::parse(&existing.version)? > release.version
    {
        return Err("A versão instalada é mais recente; o instalador não fará downgrade.".into());
    }
    if fs::metadata(part).map_err(|error| error.to_string())?.len() != release.size {
        return Err("O tamanho baixado não corresponde ao publicado no GitHub.".into());
    }
    if sha256_file(part)? != release.sha256 {
        return Err("SHA-256 diferente do informado pelo GitHub. Instalação recusada.".into());
    }

    let executable = folder.join(EXECUTABLE_NAME);
    let record = folder.join(RECORD_NAME);
    let backup_exe = folder.join(BACKUP_EXE);
    let backup_record = folder.join(BACKUP_RECORD);
    let next_record = folder.join(NEXT_RECORD);
    let data = serde_json::to_vec_pretty(&InstallRecord::for_release(release))
        .map_err(|error| error.to_string())?;
    let mut next = File::create_new(&next_record).map_err(|error| error.to_string())?;
    next.write_all(&data).map_err(|error| error.to_string())?;
    next.sync_all().map_err(|error| error.to_string())?;
    drop(next);

    let transaction = (|| -> Result<(), String> {
        if executable.exists() {
            fs::rename(&executable, &backup_exe)
                .map_err(|error| format!("Feche o JetSheets antes de atualizar: {error}"))?;
            fs::rename(&record, &backup_record)
                .map_err(|error| format!("Falha ao proteger a versão anterior: {error}"))?;
        }
        fs::rename(part, &executable)
            .map_err(|error| format!("Falha ao ativar a nova versão: {error}"))?;
        fs::rename(&next_record, &record)
            .map_err(|error| format!("Falha ao registrar a nova versão: {error}"))?;
        Ok(())
    })();
    if let Err(error) = transaction {
        return match recover(folder) {
            Ok(()) => Err(error),
            Err(recovery) => Err(format!("{error} Recuperação pendente: {recovery}")),
        };
    }
    recover(folder)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_recovers_an_interrupted_update() {
        let folder =
            std::env::temp_dir().join(format!("jetsheets-install-test-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let part = folder.join("download.part");
        fs::write(&part, b"first").unwrap();
        let digest = sha256_file(&part).unwrap();
        let first = Release {
            tag: "v1.0.0".into(),
            version: Version(1, 0, 0),
            notes: String::new(),
            page_url: String::new(),
            download_url: String::new(),
            size: 5,
            sha256: digest,
        };
        install_verified(&folder, &part, &first).unwrap();
        assert_eq!(installed(&folder).unwrap().unwrap().version, "v1.0.0");
        fs::rename(folder.join(EXECUTABLE_NAME), folder.join(BACKUP_EXE)).unwrap();
        fs::rename(folder.join(RECORD_NAME), folder.join(BACKUP_RECORD)).unwrap();
        fs::write(folder.join(EXECUTABLE_NAME), b"broken").unwrap();
        recover(&folder).unwrap();
        assert_eq!(fs::read(folder.join(EXECUTABLE_NAME)).unwrap(), b"first");
        assert_eq!(installed(&folder).unwrap().unwrap().version, "v1.0.0");
        let second_part = folder.join("second.part");
        fs::write(&second_part, b"second").unwrap();
        let second = Release {
            tag: "v1.1.0".into(),
            version: Version(1, 1, 0),
            notes: String::new(),
            page_url: String::new(),
            download_url: String::new(),
            size: 6,
            sha256: sha256_file(&second_part).unwrap(),
        };
        install_verified(&folder, &second_part, &second).unwrap();
        assert_eq!(installed(&folder).unwrap().unwrap().version, "v1.1.0");
        assert_eq!(fs::read(folder.join(EXECUTABLE_NAME)).unwrap(), b"second");
        assert!(!folder.join(BACKUP_EXE).exists());
        let downgrade = folder.join("downgrade.part");
        fs::write(&downgrade, b"first").unwrap();
        assert!(install_verified(&folder, &downgrade, &first).is_err());
        fs::remove_dir_all(folder).unwrap();
    }
}
