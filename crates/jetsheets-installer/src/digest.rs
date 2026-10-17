//! Streaming SHA-256 using Windows CNG.

use std::{fs::File, io::Read, path::Path, ptr};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
    BCryptOpenAlgorithmProvider,
};

struct Algorithm(BCRYPT_ALG_HANDLE);

impl Drop for Algorithm {
    fn drop(&mut self) {
        // SAFETY: CNG created this provider handle, which this wrapper owns.
        unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
    }
}

struct Hash(BCRYPT_HASH_HANDLE);

impl Drop for Hash {
    fn drop(&mut self) {
        // SAFETY: CNG created this hash handle, which this wrapper owns.
        unsafe { BCryptDestroyHash(self.0) };
    }
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut raw_algorithm: BCRYPT_ALG_HANDLE = ptr::null_mut();
    // SAFETY: Output pointer is valid; the algorithm name is a CNG static UTF-16 string.
    if unsafe {
        BCryptOpenAlgorithmProvider(&mut raw_algorithm, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0)
    } < 0
    {
        return Err("Não foi possível iniciar a verificação SHA-256.".into());
    }
    let algorithm = Algorithm(raw_algorithm);
    let mut raw_hash: BCRYPT_HASH_HANDLE = ptr::null_mut();
    // SAFETY: The provider is live. CNG allocates the hash object when the pointer is null.
    if unsafe {
        BCryptCreateHash(
            algorithm.0,
            &mut raw_hash,
            ptr::null_mut(),
            0,
            ptr::null(),
            0,
            0,
        )
    } < 0
    {
        return Err("Não foi possível criar a verificação SHA-256.".into());
    }
    let hash = Hash(raw_hash);
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        // SAFETY: The hash is live and the buffer holds `count` initialized bytes.
        if unsafe { BCryptHashData(hash.0, buffer.as_ptr(), count as u32, 0) } < 0 {
            return Err("Falha durante a verificação SHA-256.".into());
        }
    }
    let mut output = [0u8; 32];
    // SAFETY: The hash is live and output is a 32-byte SHA-256 buffer.
    if unsafe { BCryptFinishHash(hash.0, output.as_mut_ptr(), output.len() as u32, 0) } < 0 {
        return Err("Falha ao concluir a verificação SHA-256.".into());
    }
    Ok(output.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_a_file_with_sha256() {
        let path = std::env::temp_dir().join(format!("jetsheets-hash-{}.txt", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let result = sha256_file(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            result,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
