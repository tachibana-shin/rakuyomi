use anyhow::Result;

use crate::{model::SourceId, source_manager::SourceManager};

pub fn uninstall_source(source_manager: &mut SourceManager, source_id: SourceId) -> Result<()> {
    source_manager.uninstall_any_source(&source_id)?;

    Ok(())
}
