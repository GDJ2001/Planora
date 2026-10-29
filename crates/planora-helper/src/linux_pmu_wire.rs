//! Private, versioned IPC between the ordinary helper and its perf worker.
use planora_core::{ErrorCode, ProcessIdentity, ProfileResult};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const SOCKET: &str = "/run/planora-pmu/planora-pmu.sock";
pub const VERSION: u32 = 1;
pub const MAX_REQUEST: usize = 4096;
pub const MAX_RESPONSE: usize = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub command: Command,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    Probe,
    Sample {
        target: Option<ProcessIdentity>,
        duration_seconds: u32,
        sources: Vec<String>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct Reply {
    pub version: u32,
    pub result: Result<Option<ProfileResult>, (ErrorCode, String)>,
}

pub fn write_frame(output: &mut impl Write, value: &impl Serialize, max: usize) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    if data.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "PMU frame too large",
        ));
    }
    output.write_all(&(data.len() as u32).to_le_bytes())?;
    output.write_all(&data)
}

pub fn read_request(input: &mut impl Read) -> io::Result<Request> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_REQUEST {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid PMU request size",
        ));
    }
    let mut data = vec![0u8; length];
    input.read_exact(&mut data)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_request_is_rejected_before_allocation() {
        let mut bytes = io::Cursor::new(((MAX_REQUEST + 1) as u32).to_le_bytes());
        assert_eq!(
            read_request(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn probe_round_trip_has_fixed_version() {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Request {
                version: VERSION,
                command: Command::Probe,
            },
            MAX_REQUEST,
        )
        .unwrap();
        let request = read_request(&mut io::Cursor::new(bytes)).unwrap();
        assert_eq!(request.version, VERSION);
        assert!(matches!(request.command, Command::Probe));
    }
}
