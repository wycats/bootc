//! Wire protocol for daemon communication.
//!
//! The protocol uses a simple binary format with fd passing via SCM_RIGHTS.
//!
//! # Request Format
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ Header (16 bytes, little-endian)                             │
//! │   n_argv: u32      - Number of argument strings              │
//! │   n_envp: u32      - Number of environment strings           │
//! │   cwd_len: u32     - Length of working directory path        │
//! │   reserved: u32    - Reserved for future use                 │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Body (variable length)                                       │
//! │   cwd: [u8; cwd_len]           - Working directory (UTF-8)   │
//! │   argv: [NUL-terminated]*n_argv - Command arguments          │
//! │   envp: [NUL-terminated]*n_envp - Environment (KEY=VALUE)    │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Ancillary data (SCM_RIGHTS)                                  │
//! │   fds[0]: stdin                                              │
//! │   fds[1]: stdout                                             │
//! │   fds[2]: stderr                                             │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Response Format
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ wait_status: i32 (little-endian)                             │
//! │   Raw waitpid(2) status, use WIFEXITED/WEXITSTATUS macros    │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use anyhow::{Context, Result, bail};
use nix::sys::socket::{self, ControlMessage, ControlMessageOwned, MsgFlags, UnixAddr};
use std::io::{IoSlice, IoSliceMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// Header size in bytes (4 u32 fields).
const HEADER_SIZE: usize = 16;

/// Maximum message size (16 MB should be plenty for env + args).
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// A request to execute a command on the host.
#[derive(Debug, Clone)]
pub struct Request {
    /// Command arguments (argv[0] is the program).
    pub argv: Vec<String>,
    /// Environment variables as KEY=VALUE pairs.
    pub envp: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
}

/// Response from the daemon after command execution.
#[derive(Debug, Clone, Copy)]
pub struct Response {
    /// Raw waitpid(2) status.
    pub wait_status: i32,
}

impl Response {
    /// Check if the process exited normally.
    pub fn exited(&self) -> bool {
        // WIFEXITED: (status & 0x7f) == 0
        (self.wait_status & 0x7f) == 0
    }

    /// Get the exit code if the process exited normally.
    pub fn exit_code(&self) -> Option<i32> {
        if self.exited() {
            // WEXITSTATUS: (status >> 8) & 0xff
            Some((self.wait_status >> 8) & 0xff)
        } else {
            None
        }
    }
}

/// Send a request over the socket with fd passing.
pub fn send_request(
    stream: &UnixStream,
    request: &Request,
    stdin: RawFd,
    stdout: RawFd,
    stderr: RawFd,
) -> Result<()> {
    // Build the body: cwd + NUL-terminated argv + NUL-terminated envp
    let cwd_bytes = request.cwd.to_string_lossy().as_bytes().to_vec();

    let mut body = Vec::new();
    body.extend_from_slice(&cwd_bytes);

    for arg in &request.argv {
        body.extend_from_slice(arg.as_bytes());
        body.push(0); // NUL terminator
    }

    for env in &request.envp {
        body.extend_from_slice(env.as_bytes());
        body.push(0); // NUL terminator
    }

    // Build the header
    let header = [
        (request.argv.len() as u32).to_le_bytes(),
        (request.envp.len() as u32).to_le_bytes(),
        (cwd_bytes.len() as u32).to_le_bytes(),
        [0u8; 4], // reserved
    ]
    .concat();

    // Combine header + body
    let mut message = header;
    message.extend_from_slice(&body);

    // Send with SCM_RIGHTS for fd passing
    let fds = [stdin, stdout, stderr];
    let cmsg = [ControlMessage::ScmRights(&fds)];
    let iov = [IoSlice::new(&message)];

    socket::sendmsg::<UnixAddr>(stream.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None)
        .context("Failed to send request")?;

    Ok(())
}

/// Receive a request from the socket with fd passing.
pub fn recv_request(stream: &UnixStream) -> Result<(Request, [OwnedFd; 3])> {
    // Allocate buffer for message
    let mut buf = vec![0u8; MAX_MESSAGE_SIZE];

    // Allocate space for control messages (fd passing)
    let mut cmsg_buf = nix::cmsg_space!([RawFd; 3]);

    // Receive message and extract fds in a scope so borrows end
    let (bytes_received, fds) = {
        let mut iov = [IoSliceMut::new(&mut buf)];
        let msg = socket::recvmsg::<UnixAddr>(
            stream.as_raw_fd(),
            &mut iov,
            Some(&mut cmsg_buf),
            MsgFlags::empty(),
        )
        .context("Failed to receive request")?;

        // Extract file descriptors from control message
        let mut fds: Option<[OwnedFd; 3]> = None;
        for cmsg in msg.cmsgs()? {
            if let ControlMessageOwned::ScmRights(received_fds) = cmsg
                && received_fds.len() >= 3 {
                    // SAFETY: We received these fds from the kernel via SCM_RIGHTS
                    unsafe {
                        fds = Some([
                            OwnedFd::from_raw_fd(received_fds[0]),
                            OwnedFd::from_raw_fd(received_fds[1]),
                            OwnedFd::from_raw_fd(received_fds[2]),
                        ]);
                    }
                }
        }

        (msg.bytes, fds)
    };
    // msg and iov are now dropped, buf is no longer borrowed

    let fds = fds.context("No file descriptors received")?;

    if bytes_received < HEADER_SIZE {
        bail!("Message too short: {} bytes", bytes_received);
    }

    // Parse header
    let n_argv = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
    let n_envp = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let cwd_len = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
    // buf[12..16] is reserved

    // Parse body
    let body = &buf[HEADER_SIZE..bytes_received];

    if body.len() < cwd_len {
        bail!("Message body too short for cwd");
    }

    let cwd = PathBuf::from(String::from_utf8_lossy(&body[..cwd_len]).to_string());

    // Parse NUL-terminated strings
    let mut pos = cwd_len;
    let mut argv = Vec::with_capacity(n_argv);
    let mut envp = Vec::with_capacity(n_envp);

    for _ in 0..n_argv {
        let start = pos;
        while pos < body.len() && body[pos] != 0 {
            pos += 1;
        }
        argv.push(String::from_utf8_lossy(&body[start..pos]).to_string());
        pos += 1; // Skip NUL
    }

    for _ in 0..n_envp {
        let start = pos;
        while pos < body.len() && body[pos] != 0 {
            pos += 1;
        }
        envp.push(String::from_utf8_lossy(&body[start..pos]).to_string());
        pos += 1; // Skip NUL
    }

    let request = Request { argv, envp, cwd };

    Ok((request, fds))
}

/// Send a response over the socket.
pub fn send_response(stream: &UnixStream, response: &Response) -> Result<()> {
    use std::io::Write;

    let bytes = response.wait_status.to_le_bytes();
    (&*stream)
        .write_all(&bytes)
        .context("Failed to send response")?;
    Ok(())
}

/// Receive a response from the socket.
pub fn recv_response(stream: &UnixStream) -> Result<Response> {
    use std::io::Read;

    let mut bytes = [0u8; 4];
    (&*stream)
        .read_exact(&mut bytes)
        .context("Failed to receive response")?;

    Ok(Response {
        wait_status: i32::from_le_bytes(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_exit_code() {
        // Normal exit with code 0
        let resp = Response { wait_status: 0 };
        assert!(resp.exited());
        assert_eq!(resp.exit_code(), Some(0));

        // Normal exit with code 42
        let resp = Response {
            wait_status: 42 << 8,
        };
        assert!(resp.exited());
        assert_eq!(resp.exit_code(), Some(42));

        // Killed by signal (not exited)
        let resp = Response { wait_status: 9 }; // SIGKILL
        assert!(!resp.exited());
        assert_eq!(resp.exit_code(), None);
    }
}
