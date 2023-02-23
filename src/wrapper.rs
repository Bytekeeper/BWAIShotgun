use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub enum ExecutionWrapper {
    Unconfigured,
    NoWrapper,
    Wine,
    Sandboxie {
        executable: PathBuf,
        box_name: String,
    },
}

impl Default for ExecutionWrapper {
    fn default() -> Self {
        #[cfg(target_os = "windows")]
        {
            // Should be Unconfigured if we ever support bot sandboxing
            ExecutionWrapper::NoWrapper
        }
        #[cfg(not(target_os = "windows"))]
        {
            ExecutionWrapper::Wine
        }
    }
}

impl ExecutionWrapper {
    pub fn wrap_executable(&self, exe: impl AsRef<OsStr>) -> Command {
        match self {
            ExecutionWrapper::Sandboxie {
                executable,
                box_name,
            } => {
                let mut cmd = Command::new(executable);
                cmd.arg("/wait");
                cmd.arg("/silent");
                cmd.arg(format!("/box:{box_name}"));
                cmd.arg(exe);
                cmd
            }
            ExecutionWrapper::Unconfigured | ExecutionWrapper::NoWrapper => Command::new(exe),
            ExecutionWrapper::Wine => {
                let mut cmd = Command::new("wine");
                cmd.arg(exe);
                cmd
            }
        }
    }
}
