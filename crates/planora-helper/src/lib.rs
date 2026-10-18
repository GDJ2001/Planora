//! Narrow, versioned local IPC for the privileged collector.
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerOptions, Stream, prelude::*};
use planora_core::{Command, PROTOCOL_VERSION, Response};
use std::io::{self, Read, Write};

const MAX_MESSAGE: usize = 8 * 1024 * 1024;
#[cfg(windows)]
const ENDPOINT: &str = "planora-v1";
#[cfg(target_os = "linux")]
const ENDPOINT: &str = "/run/planora/planora.sock";
#[cfg(target_os = "macos")]
const ENDPOINT: &str = "/var/run/planora/planora.sock";

fn name() -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        ENDPOINT.to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        ENDPOINT.to_fs_name::<GenericFilePath>()
    }
}

pub fn connect() -> io::Result<Stream> {
    Stream::connect(name()?)
}

pub fn transact(command: &Command) -> io::Result<Response> {
    let mut stream = connect()?;
    send(
        &mut stream,
        &Command::Hello {
            version: PROTOCOL_VERSION,
        },
    )?;
    match receive::<Response>(&mut stream)? {
        Response::Hello {
            version,
            helper_authorized: true,
        } if version == PROTOCOL_VERSION => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "helper handshake failed",
            ));
        }
    }
    send(&mut stream, command)?;
    receive(&mut stream)
}

pub fn send<T: serde::Serialize>(stream: &mut impl Write, value: &T) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    if data.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    stream.write_all(&(data.len() as u32).to_le_bytes())?;
    stream.write_all(&data)
}

pub fn receive<T: serde::de::DeserializeOwned>(stream: &mut impl Read) -> io::Result<T> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let len = u32::from_le_bytes(length) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut data = vec![0; len];
    stream.read_exact(&mut data)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}

pub fn protocol_ok(command: &Command) -> bool {
    matches!(command, Command::Hello { version } if *version == PROTOCOL_VERSION)
}

pub fn listener() -> io::Result<interprocess::local_socket::Listener> {
    let options = ListenerOptions::new().name(name()?);
    #[cfg(unix)]
    {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            let directory = std::path::Path::new("/var/run/planora");
            std::fs::create_dir_all(directory)?;
            let name = std::ffi::CString::new(directory.as_os_str().as_bytes())
                .map_err(io::Error::other)?;
            if unsafe { libc::chmod(name.as_ptr(), 0o755) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let result = options.mode(0o660).create_sync()?;
        #[cfg(target_os = "macos")]
        {
            let name = std::ffi::CString::new(ENDPOINT).map_err(io::Error::other)?;
            let group = std::ffi::CString::new("staff").unwrap();
            let entry = unsafe { libc::getgrnam(group.as_ptr()) };
            if entry.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "staff group unavailable",
                ));
            }
            let gid = unsafe { (*entry).gr_gid };
            if unsafe { libc::chown(name.as_ptr(), u32::MAX, gid) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(result)
    }
    #[cfg(windows)]
    {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sid = std::env::var("PLANORA_ALLOWED_SID").map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "service installer must configure PLANORA_ALLOWED_SID",
            )
        })?;
        if !sid.starts_with("S-1-")
            || !sid
                .chars()
                .all(|c| c.is_ascii_digit() || c == 'S' || c == '-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid authorized user SID",
            ));
        }
        let sddl = widestring::U16CString::from_str(format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{sid})"))
            .map_err(io::Error::other)?;
        let descriptor = SecurityDescriptor::deserialize(&sddl)?;
        options.security_descriptor(descriptor).create_sync()
    }
}

/// Unix peer credentials are checked in addition to filesystem socket permissions.
#[cfg(unix)]
pub fn peer_authorized(stream: &Stream) -> bool {
    let peer_uid = stream.peer_creds().ok().and_then(|p| p.euid());
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let path = "/Library/Application Support/Planora/authorized-uid";
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return false;
        }
        let approved_uid = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        return peer_uid.is_some() && peer_uid == approved_uid;
    }
    #[cfg(not(target_os = "macos"))]
    {
        peer_uid.is_some()
    }
}
#[cfg(windows)]
pub fn peer_authorized(stream: &Stream) -> bool {
    stream.peer_creds().ok().and_then(|p| p.pid()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversize_frame() {
        let input = ((MAX_MESSAGE + 1) as u32).to_le_bytes();
        let result: io::Result<Command> = receive(&mut input.as_slice());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
    #[test]
    fn version_is_exact() {
        assert!(protocol_ok(&Command::Hello {
            version: PROTOCOL_VERSION
        }));
        assert!(!protocol_ok(&Command::Hello { version: 0 }));
    }
}
