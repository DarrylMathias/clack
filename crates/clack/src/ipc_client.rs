//! IPC client helpers — send a command to the running daemon and return the response.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clack_core::{
    config::socket_path,
    ipc::{Command, Response},
};

/// Send a command to the daemon and return its response.
/// Returns a descriptive error if the daemon isn't running.
pub fn send(cmd: &Command) -> Result<Response> {
    let sock = socket_path();
    let mut stream = UnixStream::connect(&sock).with_context(|| {
        format!(
            "cannot connect to clack daemon (is it running?)\n  socket: {}\n  start it with: clack start",
            sock.display()
        )
    })?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;

    let json = serde_json::to_string(cmd)?;
    stream.write_all((json + "\n").as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let resp: Response = serde_json::from_str(line.trim())?;
    Ok(resp)
}

/// Send a command and bail with the daemon's error message if `ok` is false.
pub fn send_ok(cmd: &Command) -> Result<Response> {
    let resp = send(cmd)?;
    if !resp.ok {
        bail!("{}", resp.error.as_deref().unwrap_or("unknown daemon error"));
    }
    Ok(resp)
}
