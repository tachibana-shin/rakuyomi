use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct BuildInfo {
    pub version: String,
    pub build: String,
}

impl BuildInfo {
    pub fn format_display(&self) -> String {
        format!("{} ({})", self.version, self.build)
    }
}

#[cfg(target_os = "android")]
pub fn get_build_info() -> Option<BuildInfo> {
    use std::fs;
    use std::path::Path;

    let paths = [
        "/sdcard/koreader/plugins/rakuyomi.koplugin/BUILD_INFO.json",
        "/storage/emulated/0/koreader/plugins/rakuyomi.koplugin/BUILD_INFO.json",
    ];

    let mut contents = None;

    for path in &paths {
        if Path::new(path).exists() {
            if let Ok(data) = fs::read_to_string(path) {
                contents = Some(data);
                break;
            }
        }
    }

    let contents = contents.or_else(|| {
        let home = std::env::var("HOME").ok()?;
        let path = format!(
            "{}/koreader/plugins/rakuyomi.koplugin/BUILD_INFO.json",
            home
        );
        fs::read_to_string(path).ok()
    })?;

    let build_info: BuildInfo = serde_json::from_str(&contents).ok()?;

    Some(build_info)
}

#[cfg(not(target_os = "android"))]
pub fn get_build_info() -> Option<BuildInfo> {
    let build_info_path = std::env::current_exe()
        .ok()?
        .with_file_name("BUILD_INFO.json");
    let contents = std::fs::read_to_string(build_info_path).ok()?;
    let build_info: BuildInfo = serde_json::from_str(&contents).ok()?;

    Some(build_info)
}

pub const DEFAULT_SETTINGS_JSON: &str = include_str!("../assets/default-settings.json");

pub fn default_home_path() -> PathBuf {
    PathBuf::from(".")
}
