//! Where the tools and data are: a standalone folder (made by
//! `tools/converter_app/build_dist.py`) or the repo.

use std::path::{Path, PathBuf};

pub const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

pub struct Layout {
    pub convert: PathBuf,
    pub checker: PathBuf,
    pub agent: PathBuf,
    pub agent_model: PathBuf,
    /// The git checkout the port agent works in.
    pub workspace: PathBuf,
    pub convert_work: PathBuf,
    pub port_work: PathBuf,
    /// Vanilla asset names for the checker: an index file or the assets.
    pub vanilla: PathBuf,
    pub settings: PathBuf,
    pub default_in: PathBuf,
    pub default_out: PathBuf,
    pub client_mods: Option<PathBuf>,
}

impl Layout {
    pub fn find() -> Layout {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let client_mods = std::env::var_os("LOCALAPPDATA").map(|l| {
            PathBuf::from(l)
                .join("torch-client")
                .join("config")
                .join("mods")
        });
        if exe_dir
            .join("bin")
            .join(format!("torch-convert{EXE}"))
            .is_file()
        {
            let d = exe_dir;
            return Layout {
                convert: d.join("bin").join(format!("torch-convert{EXE}")),
                checker: d.join("bin").join(format!("torch-port-check{EXE}")),
                agent: d.join("app").join("port_agent.py"),
                agent_model: d.join("app").join("port_agent_model.py"),
                workspace: d.join("workspace"),
                convert_work: d.join("data").join("convert"),
                port_work: d.join("data").join("port"),
                vanilla: d.join("data").join("vanilla-index.txt"),
                settings: d.join("data").join("settings.json"),
                default_in: d.join("mods_in"),
                default_out: d.join("mods_out"),
                client_mods,
            };
        }
        // The repo: the first ancestor (of the exe, then the working
        // directory) holding crates/torch-convert.
        let start = std::env::current_dir().unwrap_or_default();
        let repo = exe_dir
            .ancestors()
            .chain(start.ancestors())
            .find(|p| {
                p.join("crates")
                    .join("torch-convert")
                    .join("Cargo.toml")
                    .is_file()
            })
            .map(Path::to_path_buf)
            .unwrap_or(start);
        let release = |c: &str| {
            repo.join("crates")
                .join(c)
                .join("target")
                .join("release")
                .join(format!("{c}{EXE}"))
        };
        Layout {
            convert: release("torch-convert"),
            checker: release("torch-port-check"),
            agent: repo.join("tools").join("port_agent.py"),
            agent_model: repo.join("tools").join("port_agent_model.py"),
            workspace: repo.clone(),
            convert_work: repo.join("tools").join("moddump"),
            port_work: repo.join("tools").join("port_agent_work"),
            vanilla: repo
                .join("reference")
                .join("minecraft-26.3")
                .join("assets")
                .join("minecraft"),
            settings: repo
                .join("crates")
                .join("torch-converter")
                .join("settings.json"),
            default_in: repo.join("tools").join("moddump").join("Mods"),
            default_out: repo.join("tools").join("moddump").join("converted"),
            client_mods,
        }
    }

    /// Whether the dump server's EULA is accepted already.
    pub fn eula_accepted(&self) -> bool {
        let Ok(entries) = std::fs::read_dir(&self.convert_work) else {
            return false;
        };
        entries.flatten().any(|e| {
            e.file_name().to_string_lossy().starts_with("server-")
                && std::fs::read_to_string(e.path().join("eula.txt"))
                    .is_ok_and(|t| t.replace(' ', "").to_lowercase().contains("eula=true"))
        })
    }
}
