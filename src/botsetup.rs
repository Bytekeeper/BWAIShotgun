use anyhow::bail;
use std::fs::read_dir;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum Binary {
    Dll(PathBuf),
    Jar(PathBuf),
    Exe(PathBuf),
}

impl Binary {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| {
                let mut ext = ext.to_string();
                ext.make_ascii_lowercase();
                let result = match ext.as_str() {
                    "dll" => Binary::Dll(path.to_path_buf()),
                    "jar" => Binary::Jar(path.to_path_buf()),
                    "exe" => Binary::Exe(path.to_path_buf()),
                    _ => return None,
                };
                Some(result)
            })
    }

    pub(crate) fn search(search_path: &Path) -> anyhow::Result<Self> {
        let mut executable = None;
        for file in read_dir(search_path)?.flatten() {
            let path = file.path();
            if let Some(detected_binary) = Binary::from_path(&path) {
                executable = Some(match (executable, detected_binary) {
                    (None, dll @ Binary::Dll(_)) | (Some(dll @ Binary::Dll(_)), Binary::Jar(_)) => {
                        dll
                    }
                    (None, jar @ Binary::Jar(_)) => jar,
                    (None, exe @ Binary::Exe(_))
                    | (Some(Binary::Dll(_) | Binary::Jar(_)), exe @ Binary::Exe(_)) => exe,
                    _ => bail!(
                        "Found multiple binary candidates in '{}', please select one in 'bot.toml'",
                        search_path.to_string_lossy()
                    ),
                })
            }
        }
        match executable {
            None => bail!("No binary found in '{}'", search_path.to_string_lossy()),
            Some(x) => Ok(x),
        }
    }
}

pub trait LaunchBuilder {
    fn build_command(&self, bot_binary: &Binary) -> anyhow::Result<Command>;
}
