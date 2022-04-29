use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub enum SandboxMode {
    Unconfigured,
    NoSandbox,
    Sandboxie {
        executable: PathBuf,
        box_name: String,
    },
}

impl Default for SandboxMode {
    fn default() -> Self {
        // Should be Unconfigured if we ever support bot sandboxing
        SandboxMode::NoSandbox
    }
}

impl SandboxMode {
    pub fn wrap_executable(&self, exe: impl AsRef<OsStr>) -> Command {
        match self {
            SandboxMode::Sandboxie {
                executable,
                box_name,
            } => {
                let mut cmd = Command::new(executable);
                cmd.arg("/wait");
                cmd.arg("/silent");
                cmd.arg(format!("/box:{}", box_name));
                cmd.arg(exe);
                cmd
            }
            _ => Command::new(exe),
        }
    }
}
