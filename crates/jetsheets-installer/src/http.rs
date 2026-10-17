//! Small synchronous HTTPS client using Windows WinHTTP. Runs only on worker threads.

use std::{ffi::c_void, io, ptr};
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP, WINHTTP_QUERY_CONTENT_RANGE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};

struct Handle(*mut c_void);

impl Handle {
    fn new(value: *mut c_void) -> io::Result<Self> {
        if value.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(value))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: This handle was returned by WinHTTP and is owned by this wrapper.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn https_host_path(url: &str) -> Result<(&str, &str), String> {
    let value = url
        .strip_prefix("https://")
        .ok_or("Somente endereços HTTPS são aceitos.")?;
    let (host, path) = value
        .split_once('/')
        .ok_or("O endereço de download está incompleto.")?;
    if !matches!(host, "api.github.com" | "github.com") || path.contains('#') {
        return Err("O servidor de atualização não é permitido.".into());
    }
    Ok((host, &value[host.len()..]))
}

pub struct Response {
    request: Handle,
    _connection: Handle,
    _session: Handle,
    pub status: u32,
}

impl Response {
    pub fn open(url: &str, range_from: Option<u64>) -> Result<Self, String> {
        let (host, path) = https_host_path(url)?;
        let agent = wide(concat!("JetSheets-Installer/", env!("CARGO_PKG_VERSION")));
        // SAFETY: All pointers refer to live UTF-16 buffers or are optional null pointers.
        let session = Handle::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        })
        .map_err(|error| format!("Falha ao iniciar conexão: {error}"))?;
        // SAFETY: The session handle is valid and remains alive until the response is dropped.
        if unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 5_000) } == 0 {
            return Err(format!(
                "Falha ao configurar conexão: {}",
                io::Error::last_os_error()
            ));
        }
        let host_wide = wide(host);
        // SAFETY: The host buffer and session handle are live during this call.
        let connection =
            Handle::new(unsafe { WinHttpConnect(session.0, host_wide.as_ptr(), 443, 0) })
                .map_err(|error| format!("Falha ao conectar ao GitHub: {error}"))?;
        let path_wide = wide(path);
        let get = wide("GET");
        // SAFETY: The connection and UTF-16 buffers are valid; optional fields are null.
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                get.as_ptr(),
                path_wide.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })
        .map_err(|error| format!("Falha ao preparar download: {error}"))?;
        let policy = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
        // SAFETY: `policy` is a live u32 and the request handle is valid.
        if unsafe {
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_REDIRECT_POLICY,
                &policy as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            )
        } == 0
        {
            return Err(format!(
                "Falha ao proteger redirecionamento: {}",
                io::Error::last_os_error()
            ));
        }
        let mut headers = String::from("Accept: application/vnd.github+json\r\n");
        if let Some(offset) = range_from {
            headers.push_str(&format!("Range: bytes={offset}-\r\n"));
        }
        let headers = wide(&headers);
        // SAFETY: The request, header buffer, and optional null fields are valid for this call.
        if unsafe {
            WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, ptr::null(), 0, 0, 0)
        } == 0
        {
            return Err(format!(
                "Falha ao enviar solicitação: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: The request handle is valid; reserved argument must be null.
        if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
            return Err(format!(
                "Falha ao receber resposta: {}",
                io::Error::last_os_error()
            ));
        }
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        // SAFETY: The status output and length pointers are valid and correctly sized.
        if unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                &mut status as *mut u32 as *mut c_void,
                &mut length,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "Falha ao ler status HTTP: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self {
            request,
            _connection: connection,
            _session: session,
            status,
        })
    }

    pub fn content_range(&self) -> Result<String, String> {
        let mut buffer = [0u16; 128];
        let mut length = (buffer.len() * size_of::<u16>()) as u32;
        // SAFETY: The buffer is writable for `length` bytes; the request handle is valid.
        if unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_CONTENT_RANGE,
                ptr::null(),
                buffer.as_mut_ptr() as *mut c_void,
                &mut length,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "Resposta parcial sem Content-Range: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(String::from_utf16_lossy(&buffer[..length as usize / 2])
            .trim_end_matches('\0')
            .to_owned())
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut read = 0u32;
        // SAFETY: The request handle is valid and buffer is writable for the given length.
        if unsafe {
            WinHttpReadData(
                self.request.0,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
                &mut read,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(read as usize)
    }

    pub fn read_limited(&mut self, limit: usize) -> Result<Vec<u8>, String> {
        let mut data = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let count = self.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                return Ok(data);
            }
            if data.len() + count > limit {
                return Err("A resposta do GitHub excede o tamanho permitido.".into());
            }
            data.extend_from_slice(&buffer[..count]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_expected_https_hosts_are_accepted() {
        assert_eq!(
            https_host_path("https://api.github.com/repos/a/b").unwrap(),
            ("api.github.com", "/repos/a/b")
        );
        assert!(https_host_path("http://github.com/a").is_err());
        assert!(https_host_path("https://evil.example/a").is_err());
        assert!(https_host_path("https://github.com.evil.example/a").is_err());
    }

    #[test]
    #[ignore = "requires access to api.github.com"]
    fn winhttp_can_read_the_github_api() {
        let mut response =
            Response::open("https://api.github.com/repos/rust-lang/rust", None).unwrap();
        assert_eq!(response.status, 200);
        assert!(!response.read_limited(2 * 1024 * 1024).unwrap().is_empty());
    }
}
