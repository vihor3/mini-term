#[cfg(unix)]
use std::path::PathBuf;

pub const ENDPOINT_ENV: &str = "MINITERM_TERMINAL_HOST_ENDPOINT";

pub fn endpoint() -> String {
    std::env::var(ENDPOINT_ENV).unwrap_or_else(|_| default_endpoint())
}

pub fn default_endpoint() -> String {
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\mini-term.terminal-host.{}", current_user_tag())
    }
    #[cfg(unix)]
    {
        socket_path().to_string_lossy().into_owned()
    }
}

#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let runtime_dir = PathBuf::from(runtime_dir);
        if runtime_dir.is_dir() {
            return runtime_dir.join("mini-term").join("terminal-host.sock");
        }
    }

    if let Some(config) = mt_core::config_json_path()
        && let Some(parent) = config.parent()
    {
        return parent.join("terminal-host").join("terminal-host.sock");
    }

    std::env::temp_dir()
        .join(format!("mini-term-terminal-host-{}", current_uid()))
        .join("terminal-host.sock")
}

#[cfg(unix)]
pub(crate) fn prepare_socket_parent(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    let parent = path
        .parent()
        .ok_or_else(|| format!("terminal host endpoint has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create endpoint directory failed: {error}"))?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("chmod endpoint directory 0700 failed: {error}"))
}

#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

#[cfg(windows)]
fn current_user_tag() -> String {
    match windows_security::current_user_sid_string() {
        Ok(sid) => sanitize_tag(&sid),
        Err(error) => {
            eprintln!("[mt-terminal-host] cannot resolve user SID ({error}); using username");
            sanitize_tag(&std::env::var("USERNAME").unwrap_or_else(|_| "default".into()))
        }
    }
}

#[cfg(windows)]
fn sanitize_tag(raw: &str) -> String {
    let value: String = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect();
    if value.is_empty() {
        "default".to_string()
    } else {
        value
    }
}

pub trait IpcStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> IpcStream for T {}

pub async fn connect(endpoint: &str) -> std::io::Result<Box<dyn IpcStream>> {
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;

        const ERROR_PIPE_BUSY: i32 = 231;
        let mut last_error = None;
        for _ in 0..5 {
            match ClientOptions::new().open(endpoint) {
                Ok(client) => return Ok(Box::new(client)),
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    last_error = Some(error);
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| std::io::Error::other("named pipe busy")))
    }
    #[cfg(unix)]
    {
        Ok(Box::new(tokio::net::UnixStream::connect(endpoint).await?))
    }
}

#[cfg(windows)]
pub mod windows_security {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub fn current_user_sid_string() -> Result<String, String> {
        unsafe {
            let mut token: HANDLE = 0;
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err("OpenProcessToken failed".into());
            }

            let mut needed = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            if needed == 0 {
                CloseHandle(token);
                return Err("GetTokenInformation size probe failed".into());
            }

            let mut bytes = vec![0u8; needed as usize];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                needed,
                &mut needed,
            );
            CloseHandle(token);
            if ok == 0 {
                return Err("GetTokenInformation failed".into());
            }

            let token_user = &*(bytes.as_ptr().cast::<TOKEN_USER>());
            let mut sid_text: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_text) == 0 {
                return Err("ConvertSidToStringSidW failed".into());
            }
            let mut len = 0;
            while *sid_text.add(len) != 0 {
                len += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, len));
            LocalFree(sid_text.cast::<c_void>());
            Ok(sid)
        }
    }

    #[repr(C)]
    struct SecurityAttributes {
        n_length: u32,
        lp_security_descriptor: *mut c_void,
        b_inherit_handle: i32,
    }

    pub struct PipeSecurity {
        security_descriptor: *mut c_void,
        attributes: Box<SecurityAttributes>,
    }

    unsafe impl Send for PipeSecurity {}

    impl PipeSecurity {
        pub fn current_user_only() -> Result<Self, String> {
            let sid = current_user_sid_string()?;
            let sddl = format!("D:P(A;;GA;;;{sid})");
            let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
            let mut descriptor: *mut c_void = std::ptr::null_mut();
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide.as_ptr(),
                    SDDL_REVISION_1,
                    (&mut descriptor as *mut *mut c_void).cast(),
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || descriptor.is_null() {
                return Err("cannot create current-user pipe security descriptor".into());
            }
            let attributes = Box::new(SecurityAttributes {
                n_length: std::mem::size_of::<SecurityAttributes>() as u32,
                lp_security_descriptor: descriptor,
                b_inherit_handle: 0,
            });
            Ok(Self {
                security_descriptor: descriptor,
                attributes,
            })
        }

        pub fn attributes_ptr(&self) -> *mut c_void {
            (&*self.attributes as *const SecurityAttributes)
                .cast_mut()
                .cast()
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.security_descriptor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_endpoint_is_terminal_host_specific() {
        let endpoint = default_endpoint();
        assert!(endpoint.contains("terminal-host"));
        assert!(!endpoint.contains("ssh-cli"));
    }
}
