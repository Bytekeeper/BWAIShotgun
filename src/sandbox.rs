use crate::SandboxMode;
use std::ffi::OsStr;
use std::process::Command;

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
