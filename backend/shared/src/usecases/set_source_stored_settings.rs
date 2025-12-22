use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::Mutex;

use anyhow::Result;

use crate::{
    model::SourceId,
    settings::{Settings, SourceSettingValue},
    source_manager::SourceManager,
};

pub fn set_source_stored_settings(
    settings: &mut Settings,
    settings_path: &Path,
    source_manager: &mut SourceManager,
    arc_source_manager: &Arc<Mutex<SourceManager>>,
    source_id: &SourceId,
    stored_settings: HashMap<String, SourceSettingValue>,
) -> Result<()> {
    // Clone the settings and save the cloned one first, so that we only change the application settings
    // iff everything goes well
    let mut updated_settings = settings.clone();
    updated_settings
        .source_settings
        .insert(source_id.value().clone(), stored_settings);
    updated_settings.save_to_file(settings_path)?;

    source_manager.update_settings(updated_settings.clone(), arc_source_manager)?;
    *settings = updated_settings;

    Ok(())
}
