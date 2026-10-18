//! IPC round-trip integration test (§8, T-I1).

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    thread,
};

use clack_core::ipc::{Command, DaemonStatus, Response};

#[test]
fn test_ipc_protocol_roundtrip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("test_clack.sock");

    let listener = UnixListener::bind(&socket_path).unwrap();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());

        for _ in 0..3 {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let cmd: Command = serde_json::from_str(&line).unwrap();

            let resp = match cmd {
                Command::Status => Response::ok(Some(DaemonStatus {
                    running: true,
                    muted: false,
                    active_pack: Some("test-pack".into()),
                    volume: 80,
                    keyboards_detected: vec!["Test Keyboard".into()],
                })),
                Command::Toggle => Response::ok(None),
                Command::SetMuted { muted } => {
                    assert!(muted);
                    Response::ok(None)
                }
                Command::Volume { value } => {
                    assert_eq!(value, 50);
                    Response::ok(None)
                }
                _ => Response::ok(None),
            };

            let resp_bytes = serde_json::to_vec(&resp).unwrap();
            stream.write_all(&resp_bytes).unwrap();
            stream.write_all(b"\n").unwrap();
        }
    });

    let mut client = UnixStream::connect(&socket_path).unwrap();
    let mut reader = BufReader::new(client.try_clone().unwrap());

    // 1. Send status
    let cmd = serde_json::to_vec(&Command::Status).unwrap();
    client.write_all(&cmd).unwrap();
    client.write_all(b"\n").unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();
    assert!(resp.ok);
    let status = resp.status.unwrap();
    assert_eq!(status.active_pack.as_deref(), Some("test-pack"));
    assert_eq!(status.volume, 80);

    // 2. Send volume
    let cmd = serde_json::to_vec(&Command::Volume { value: 50 }).unwrap();
    client.write_all(&cmd).unwrap();
    client.write_all(b"\n").unwrap();

    line.clear();
    reader.read_line(&mut line).unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();
    assert!(resp.ok);

    // 3. Send set_muted
    let cmd = serde_json::to_vec(&Command::SetMuted { muted: true }).unwrap();
    client.write_all(&cmd).unwrap();
    client.write_all(b"\n").unwrap();

    line.clear();
    reader.read_line(&mut line).unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();
    assert!(resp.ok);

    let _ = client.shutdown(std::net::Shutdown::Both);
    drop(reader);
    drop(client);
    server_handle.join().unwrap();
}
