//! IPC message types — newline-delimited JSON over a Unix socket (§4.2).

use serde::{Deserialize, Serialize};

/// Commands sent from the CLI to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Status,
    Toggle,
    SetMuted { muted: bool },
    Use { pack: String },
    Volume { value: u8 },
    AddPack { path: String },
    Reload,
    Shutdown,
}

/// Status snapshot returned by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub muted: bool,
    pub active_pack: Option<String>,
    pub volume: u8,
    pub keyboards_detected: Vec<String>,
}

/// Top-level response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DaemonStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(status: Option<DaemonStatus>) -> Self {
        Response {
            ok: true,
            status,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Response {
            ok: false,
            status: None,
            error: Some(msg.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrip_json() {
        let cmd = Command::Use {
            pack: "ibm-model-m".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Command::Use { pack } if pack == "ibm-model-m"));
    }

    #[test]
    fn response_roundtrip_json() {
        let r = Response::err("daemon not running");
        let json = serde_json::to_string(&r).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert!(!back.ok);
        assert_eq!(back.error.unwrap(), "daemon not running");
    }
}
