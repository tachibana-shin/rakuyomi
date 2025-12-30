use anyhow::Context;
use futures_util::StreamExt;
use log::{error, info, warn};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;
use tempfile::{NamedTempFile, TempDir};
use walkdir::WalkDir;

pub async fn install_update(version: String, build_name: String) -> anyhow::Result<()> {
    // Download the asset to a temporary file
    let update_zip_file = download_update_zip(&version, &build_name).await?;

    // Get plugin directory (parent of the executable)
    let current_exe = std::env::current_exe().context("Could not get current executable")?;
    let plugin_dir = current_exe
        .parent()
        .context("Could not get rakuyomi's plugin directory")?;

    // Get the path of the temporary file
    let zip_path = update_zip_file.path().to_path_buf();

    // Extract the update - the zip contains a rakuyomi.koplugin folder
    extract_update(&zip_path, plugin_dir).context("Could not extract update")?;

    // The update_zip_file (TempFile) will be automatically cleaned up when it goes out of scope here
    Ok(())
}

fn cleanup_tmp() -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir();

    for entry in std::fs::read_dir(&tmp_dir)? {
        let path = entry?.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("rakuyomi") {
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    Ok(())
}
/// Downloads the update zip file and saves it to a temporary file.
async fn download_update_zip(version: &str, build_name: &str) -> anyhow::Result<NamedTempFile> {
    let client = reqwest::Client::new();
    let asset_name = format!("rakuyomi-{}.zip", build_name);
    let url = format!(
        "https://github.com/tachibana-shin/rakuyomi/releases/download/v{}/{}",
        version, asset_name
    );

    info!("Downloading update from: {}", url);
    let response = client
        .get(&url)
        .header("User-Agent", "rakuyomi")
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .context("Failed to initiate update download")?
        .error_for_status()
        .context("Failed to download update (server error)")?;

    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = 0;

    // Retry loop: attempt twice (once + retry after cleanup)
    for attempt in 0..2 {
        // Create a named temp file for the download
        let mut update_zip_file = tempfile::Builder::new()
            .prefix("rakuyomi-update-")
            .suffix(".zip")
            .tempfile()
            .context("Could not create named temporary file for download")?;

        // Reset stream on retry
        if attempt == 1 {
            // must re-fetch on retry
            let response_retry = client
                .get(&url)
                .header("User-Agent", "rakuyomi")
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .context("Retry: Failed to initiate update download")?
                .error_for_status()
                .context("Retry: Failed to download update (server error)")?;

            stream = response_retry.bytes_stream();
            downloaded_bytes = 0;
        }

        let mut success = true;

        while let Some(item) = stream.next().await {
            let chunk = item.context("Error while downloading file chunk")?;

            if let Err(e) = update_zip_file.write_all(&chunk) {
                warn!("Write failed: {}", e);
                success = false;
                break;
            }

            downloaded_bytes += chunk.len();
        }

        if success {
            // Flush
            if let Err(e) = update_zip_file.flush() {
                warn!("Flush failed: {}", e);
                success = false;
            }
        }

        if success {
            info!(
                "Update downloaded successfully ({} bytes) and saved to temporary file: {}",
                downloaded_bytes,
                update_zip_file.path().display()
            );
            return Ok(update_zip_file); // SUCCESS
        }

        // If fail → cleanup + retry
        if attempt == 0 {
            info!("Cleaning /tmp before retry…");
            cleanup_tmp()?;
            info!("Retrying download…");
        }
    }

    anyhow::bail!("Failed to download update after retry")
}

/// Orchestrates the update extraction, backup, installation, and rollback/cleanup.
fn extract_update(zip_path: &Path, plugin_dir: &Path) -> anyhow::Result<()> {
    let parent_dir = plugin_dir
        .parent()
        .context("Could not get parent directory for plugin")?;

    // 1. Extract zip to a temporary directory
    let temp_dir = extract_zip_to_temp(zip_path, parent_dir)?;

    // 2. Backup existing plugin directory
    // Note: We proceed even if backup fails, but log the error.
    // The rollback logic handles cases where the backup might not exist.
    let backup_dir = match backup_existing_plugin(plugin_dir) {
        Ok(dir) => dir,
        Err(e) => {
            warn!(
                "Failed to create backup, proceeding with update anyway: {}",
                e
            );
            // Use the expected backup path for potential rollback/cleanup, even if creation failed
            plugin_dir.with_extension("koplugin.bak")
        }
    };

    // 3. Attempt to install the new files
    let install_result = perform_installation(temp_dir.path(), plugin_dir);

    // 4. Handle result: Cleanup on success, Rollback on failure
    match install_result {
        Ok(_) => {
            info!("Update installed successfully to: {}", plugin_dir.display());
            cleanup_backup(&backup_dir);
            Ok(())
        }
        Err(install_err) => {
            error!("Failed to install update: {}", install_err);
            rollback_update(plugin_dir, &backup_dir);
            // Return the original error that triggered the rollback
            Err(install_err.context("Update installation failed, rollback attempted"))
        }
    }
    // The temp_dir will be automatically cleaned up when it goes out of scope here
}

/// Extracts the contents of a zip file to a temporary directory.
fn extract_zip_to_temp(zip_path: &Path, parent_dir: &Path) -> anyhow::Result<TempDir> {
    let file = fs::File::open(zip_path).context("Failed to open zip file")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;
    let temp_dir =
        tempfile::tempdir_in(parent_dir).context("Failed to create temporary directory")?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => {
                let mut target_path = temp_dir.path().to_path_buf();
                for component in path.components().skip(1) {
                    // Skip the top-level folder in zip
                    target_path.push(component);
                }
                target_path
            }
            None => continue,
        };

        if file.is_dir() {
            fs::create_dir_all(&outpath)
                .with_context(|| format!("Failed to create directory: {}", outpath.display()))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).with_context(|| {
                        format!("Failed to create parent directory: {}", p.display())
                    })?;
                }
            }
            let mut outfile = fs::File::create(&outpath)
                .with_context(|| format!("Failed to create output file: {}", outpath.display()))?;
            io::copy(&mut file, &mut outfile).with_context(|| {
                format!("Failed to copy file contents to: {}", outpath.display())
            })?;
        }
    }
    info!(
        "Update extracted to temporary directory: {}",
        temp_dir.path().display()
    );
    Ok(temp_dir)
}

/// Backs up the existing plugin directory by renaming it.
fn backup_existing_plugin(plugin_dir: &Path) -> anyhow::Result<std::path::PathBuf> {
    let backup_dir = plugin_dir.with_extension("koplugin.bak");
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir).context("Failed to remove existing backup directory")?;
    }
    if plugin_dir.exists() {
        fs::rename(plugin_dir, &backup_dir)
            .context("Failed to rename plugin directory to backup location")?;
        info!(
            "Backed up existing plugin directory to: {}",
            backup_dir.display()
        );
    }
    Ok(backup_dir)
}

/// Moves files from the temporary extraction directory to the final plugin directory.
fn perform_installation(temp_dir_path: &Path, plugin_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(plugin_dir).context("Failed to create new plugin directory")?;
    for entry in WalkDir::new(temp_dir_path)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let relative_path = entry
            .path()
            .strip_prefix(temp_dir_path)
            .context("Failed to strip prefix from temp path")?;
        let target_path = plugin_dir.join(relative_path);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target_path).with_context(|| {
                format!("Failed to create directory: {}", target_path.display())
            })?;
        } else {
            // Ensure parent directory exists before moving file
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create parent directory for file: {}",
                        target_path.display()
                    )
                })?;
            }
            fs::rename(entry.path(), &target_path).with_context(|| {
                format!(
                    "Failed to move file from {} to {}",
                    entry.path().display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Cleans up the backup directory after a successful installation.
fn cleanup_backup(backup_dir: &Path) {
    if backup_dir.exists() {
        if let Err(e) = fs::remove_dir_all(backup_dir) {
            warn!(
                "Failed to remove backup directory {}: {}",
                backup_dir.display(),
                e
            );
        } else {
            info!("Removed backup directory: {}", backup_dir.display());
        }
    }
}

/// Rolls back a failed update attempt.
fn rollback_update(plugin_dir: &Path, backup_dir: &Path) {
    info!("Attempting to rollback update...");

    // Rollback Step 1: Remove partially installed directory (if it exists)
    if plugin_dir.exists() {
        if let Err(remove_err) = fs::remove_dir_all(plugin_dir) {
            error!(
                "Rollback step failed: Could not remove partially updated directory {}: {}",
                plugin_dir.display(),
                remove_err
            );
            // Log error but proceed to restore backup if possible
        }
    }

    // Rollback Step 2: Restore backup (if it exists)
    if backup_dir.exists() {
        if let Err(restore_err) = fs::rename(backup_dir, plugin_dir) {
            error!(
                "Rollback failed: Could not restore backup from {} to {}: {}",
                backup_dir.display(),
                plugin_dir.display(),
                restore_err
            );
        } else {
            info!(
                "Rollback successful: Restored backup to {}",
                plugin_dir.display()
            );
        }
    } else {
        warn!(
            "Rollback step skipped: Backup directory {} not found.",
            backup_dir.display()
        );
    }
}
