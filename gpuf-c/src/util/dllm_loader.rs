use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::{c_char, c_int, c_uint};
use std::path::Path;

pub const DLLM_ABI_VERSION: u32 = 1;
pub const DEFAULT_DLLM_LIB_PATH: &str = "libdllm.so";
pub const DEFAULT_DLLM_SERVER_KEY: &str = "0xA1FDFFFFFF01FAFAFAFA";

const DLLM_PEER_ID_BYTES: usize = 10;
const DLLM_PEER_HEX_BUFFER_BYTES: usize = 2 + DLLM_PEER_ID_BYTES * 2 + 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DllmPluginInfo {
    pub abi_version: u32,
    pub version: String,
    pub server_key_hex: String,
    pub service_type: u8,
    pub carrier: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DllmLoadError {
    UnsupportedPlatform,
    InvalidPath,
    InvalidServerKey,
    OpenFailed(String),
    MissingSymbol {
        symbol: &'static str,
        error: String,
    },
    AbiMismatch {
        expected: u32,
        actual: u32,
    },
    CallFailed {
        call: &'static str,
        status: i32,
        message: String,
    },
    InvalidUtf8(&'static str),
}

impl fmt::Display for DllmLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(f, "DLLM dynamic loading is not supported here"),
            Self::InvalidPath => write!(f, "DLLM library path contains an interior NUL byte"),
            Self::InvalidServerKey => write!(f, "DLLM server-key contains an interior NUL byte"),
            Self::OpenFailed(err) => write!(f, "failed to open DLLM library: {err}"),
            Self::MissingSymbol { symbol, error } => {
                write!(f, "missing DLLM symbol {symbol}: {error}")
            }
            Self::AbiMismatch { expected, actual } => {
                write!(f, "DLLM ABI mismatch: expected {expected}, got {actual}")
            }
            Self::CallFailed {
                call,
                status,
                message,
            } => write!(f, "DLLM call {call} failed with status {status}: {message}"),
            Self::InvalidUtf8(field) => write!(f, "DLLM returned invalid UTF-8 for {field}"),
        }
    }
}

impl std::error::Error for DllmLoadError {}

#[repr(C)]
#[derive(Clone, Copy)]
struct DllmPeerId {
    bytes: [u8; DLLM_PEER_ID_BYTES],
}

type DllmAbiVersionFn = unsafe extern "C" fn() -> c_uint;
type DllmVersionStringFn = unsafe extern "C" fn() -> *const c_char;
type DllmStatusStringFn = unsafe extern "C" fn(c_int) -> *const c_char;
type DllmPeerIdParseHexFn = unsafe extern "C" fn(*const c_char, *mut DllmPeerId) -> c_int;
type DllmPeerIdToHexFn = unsafe extern "C" fn(*const DllmPeerId, *mut c_char, usize) -> c_int;
type DllmPeerIdServiceTypeFn = unsafe extern "C" fn(*const DllmPeerId) -> u8;
type DllmPeerIdCarrierFn = unsafe extern "C" fn(*const DllmPeerId) -> u8;

struct DllmSymbols {
    abi_version: DllmAbiVersionFn,
    version_string: DllmVersionStringFn,
    status_string: DllmStatusStringFn,
    peer_id_parse_hex: DllmPeerIdParseHexFn,
    peer_id_to_hex: DllmPeerIdToHexFn,
    peer_id_service_type: DllmPeerIdServiceTypeFn,
    peer_id_carrier: DllmPeerIdCarrierFn,
}

#[cfg(unix)]
struct DllmHandle {
    ptr: *mut libc::c_void,
}

#[cfg(unix)]
impl Drop for DllmHandle {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                libc::dlclose(self.ptr);
            }
        }
    }
}

pub fn load_and_probe<P: AsRef<Path>>(
    path: P,
    server_key: &str,
) -> Result<DllmPluginInfo, DllmLoadError> {
    load_and_probe_impl(path.as_ref(), server_key)
}

#[cfg(unix)]
fn load_and_probe_impl(path: &Path, server_key: &str) -> Result<DllmPluginInfo, DllmLoadError> {
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| DllmLoadError::InvalidPath)?;
    let server_key = CString::new(server_key).map_err(|_| DllmLoadError::InvalidServerKey)?;

    let handle = unsafe {
        clear_dlerror();
        let ptr = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if ptr.is_null() {
            return Err(DllmLoadError::OpenFailed(dlerror_string()));
        }
        DllmHandle { ptr }
    };

    let symbols = unsafe { load_symbols(&handle)? };
    let abi_version = unsafe { (symbols.abi_version)() };
    if abi_version != DLLM_ABI_VERSION {
        return Err(DllmLoadError::AbiMismatch {
            expected: DLLM_ABI_VERSION,
            actual: abi_version,
        });
    }

    let version = unsafe { cstr_to_string((symbols.version_string)(), "version")? };

    let mut peer = DllmPeerId {
        bytes: [0; DLLM_PEER_ID_BYTES],
    };
    let rc = unsafe { (symbols.peer_id_parse_hex)(server_key.as_ptr(), &mut peer) };
    if rc != 0 {
        return Err(call_failed("dllm_peer_id_parse_hex", rc, &symbols));
    }

    let mut hex = [0 as c_char; DLLM_PEER_HEX_BUFFER_BYTES];
    let rc = unsafe { (symbols.peer_id_to_hex)(&peer, hex.as_mut_ptr(), hex.len()) };
    if rc != 0 {
        return Err(call_failed("dllm_peer_id_to_hex", rc, &symbols));
    }

    let server_key_hex = unsafe { cstr_to_string(hex.as_ptr(), "server_key_hex")? };
    let service_type = unsafe { (symbols.peer_id_service_type)(&peer) };
    let carrier = unsafe { (symbols.peer_id_carrier)(&peer) };

    Ok(DllmPluginInfo {
        abi_version,
        version,
        server_key_hex,
        service_type,
        carrier,
    })
}

#[cfg(not(unix))]
fn load_and_probe_impl(_path: &Path, _server_key: &str) -> Result<DllmPluginInfo, DllmLoadError> {
    Err(DllmLoadError::UnsupportedPlatform)
}

#[cfg(unix)]
unsafe fn load_symbols(handle: &DllmHandle) -> Result<DllmSymbols, DllmLoadError> {
    Ok(DllmSymbols {
        abi_version: load_symbol(handle, b"dllm_abi_version\0")?,
        version_string: load_symbol(handle, b"dllm_version_string\0")?,
        status_string: load_symbol(handle, b"dllm_status_string\0")?,
        peer_id_parse_hex: load_symbol(handle, b"dllm_peer_id_parse_hex\0")?,
        peer_id_to_hex: load_symbol(handle, b"dllm_peer_id_to_hex\0")?,
        peer_id_service_type: load_symbol(handle, b"dllm_peer_id_service_type\0")?,
        peer_id_carrier: load_symbol(handle, b"dllm_peer_id_carrier\0")?,
    })
}

#[cfg(unix)]
unsafe fn load_symbol<T: Copy>(
    handle: &DllmHandle,
    symbol: &'static [u8],
) -> Result<T, DllmLoadError> {
    clear_dlerror();
    let ptr = libc::dlsym(handle.ptr, symbol.as_ptr() as *const c_char);
    if ptr.is_null() {
        let name = CStr::from_bytes_with_nul(symbol)
            .ok()
            .and_then(|s| s.to_str().ok())
            .unwrap_or("<invalid-symbol>");
        return Err(DllmLoadError::MissingSymbol {
            symbol: name,
            error: dlerror_string(),
        });
    }
    Ok(std::mem::transmute_copy(&ptr))
}

#[cfg(unix)]
unsafe fn clear_dlerror() {
    let _ = libc::dlerror();
}

#[cfg(unix)]
fn dlerror_string() -> String {
    unsafe {
        let err = libc::dlerror();
        if err.is_null() {
            return "unknown dynamic loader error".to_string();
        }
        CStr::from_ptr(err).to_string_lossy().into_owned()
    }
}

#[cfg(unix)]
fn call_failed(call: &'static str, status: c_int, symbols: &DllmSymbols) -> DllmLoadError {
    let message = unsafe {
        let ptr = (symbols.status_string)(status);
        if ptr.is_null() {
            "unknown DLLM status".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };
    DllmLoadError::CallFailed {
        call,
        status,
        message,
    }
}

unsafe fn cstr_to_string(ptr: *const c_char, field: &'static str) -> Result<String, DllmLoadError> {
    if ptr.is_null() {
        return Ok(String::new());
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| DllmLoadError::InvalidUtf8(field))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_is_reported_as_load_error() {
        let err = load_and_probe("/definitely/not/a/real/libdllm.so", DEFAULT_DLLM_SERVER_KEY)
            .unwrap_err();
        if cfg!(unix) {
            assert!(matches!(err, DllmLoadError::OpenFailed(_)));
        } else {
            assert_eq!(err, DllmLoadError::UnsupportedPlatform);
        }
    }

    #[test]
    fn invalid_server_key_nul_is_rejected_before_loading() {
        let err = load_and_probe(DEFAULT_DLLM_LIB_PATH, "0xA1\0FD").unwrap_err();
        assert_eq!(err, DllmLoadError::InvalidServerKey);
    }
}
