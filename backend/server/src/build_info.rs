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

#[cfg(feature = "ffi")]
pub fn get_build_info() -> Option<BuildInfo> {
    let contents = include_str!("../../../BUILD_INFO.json");

    let build_info: BuildInfo = serde_json::from_str(contents).ok()?;

    Some(build_info)
}

#[cfg(not(feature = "ffi"))]
pub fn get_build_info() -> Option<BuildInfo> {
    let build_info_path = env::current_exe().ok()?.with_file_name("BUILD_INFO.json");
    let contents = fs::read_to_string(build_info_path).ok()?;
    let build_info: BuildInfo = serde_json::from_str(&contents).ok()?;

    Some(build_info)
}

pub const DEFAULT_SETTINGS_JSON: &str = include_str!("../assets/default-settings.json");

pub fn default_home_path() -> PathBuf {
    PathBuf::from(".")
}
