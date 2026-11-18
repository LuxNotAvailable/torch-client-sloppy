pub(crate) struct Slot(pub &'static str);

macro_rules! slots {
    ($($(#[doc = $doc:literal])* $(#[cfg($cfg:meta)])? $name:ident = $file:literal;)*) => {
        $(
            $(#[doc = $doc])*
            $(#[cfg($cfg)])?
            pub(crate) const $name: Slot = Slot($file);
        )*


        pub(crate) const ALL: &[Slot] = &[$($(#[cfg($cfg)])? $name,)*];
    };
}

slots! {
    SERVERS = "servers.json";
    OPTIONS = "options.json";
    KEYBINDS = "keybinds.json";
    MODULES = "modules.json";
    PROFILE = "profile.json";
    ACCOUNTS = "accounts.json";
    COMMAND_HISTORY = "command_history.txt";
    #[cfg(feature = "hud_editor")]
    HUD = "hud.json";
    #[cfg(resource_packs)]
    RESOURCE_PACKS = "resourcepacks.json";
    #[cfg(explore_modrinth)]
    MODRINTH_PACKS = "modrinth_packs.json";
    #[cfg(feature = "shader_support")]
    SHADER_PACK = "shaderpack.txt";
    #[cfg(audio)]
    SOUNDS_SKIPPED = "sounds_skipped.txt";
}

impl Slot {
    pub(crate) fn load(&self) -> Option<String> {
        std::fs::read_to_string(self.path()).ok()
    }

    pub(crate) fn store(&self, value: &str) {
        if let Err(e) = self.store_native(value) {
            crate::log_warn!(
                "storage",
                "could not save {}: {e}. The change is only in memory and will be lost when this \
                 client exits.",
                self.0
            );
        }
    }

    fn store_native(&self, value: &str) -> std::io::Result<()> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, value)?;
        match std::fs::rename(&temp, &path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                Err(e)
            }
        }
    }

    pub(crate) fn path(&self) -> std::path::PathBuf {
        dir().join(self.0)
    }
}

#[cfg(not(test))]
pub(crate) fn dir() -> std::path::PathBuf {
    match crate::platform::env::var_os("MC_CLIENT_CONFIG_DIR") {
        Some(p) => std::path::PathBuf::from(p),
        None => default_dir(),
    }
}

/// Tests keep their files apart from the player's: a test that saves
/// accounts or options must never overwrite the real ones.
#[cfg(test)]
pub(crate) fn dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("torch-client-test-{}", std::process::id()))
}

#[cfg(target_os = "windows")]
pub(crate) const WINDOWS_FOLDER: &str = "torch-client";

#[cfg(all(not(any(target_os = "android", target_os = "windows")), not(test)))]
fn default_dir() -> std::path::PathBuf {
    std::path::PathBuf::from("/tmp/torch-client")
}

#[cfg(all(target_os = "windows", not(test)))]
fn default_dir() -> std::path::PathBuf {
    match crate::platform::env::var_os("LOCALAPPDATA").filter(|p| !p.is_empty()) {
        Some(local) => std::path::PathBuf::from(local)
            .join(WINDOWS_FOLDER)
            .join("config"),
        None => std::env::temp_dir().join(WINDOWS_FOLDER).join("config"),
    }
}

#[cfg(target_os = "android")]
fn default_dir() -> std::path::PathBuf {
    bevy::android::ANDROID_APP
        .get()
        .and_then(|app| app.internal_data_path())
        .unwrap_or_else(std::env::temp_dir)
}
