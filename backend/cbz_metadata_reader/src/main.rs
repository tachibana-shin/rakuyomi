use std::path::PathBuf;

use anyhow::{Context, Result};
use cbz_metadata_reader::extract_metadata;
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    /// Path to the CBZ file
    file_path: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let file_path = PathBuf::from(&args.file_path);

    let ko_meta = extract_metadata(&file_path)
        .with_context(|| format!("Failed to read metadata from {}", file_path.display()))?;

    let output_json =
        serde_json::to_string(&ko_meta).context("Failed to serialize metadata to JSON")?;
    println!("{}", output_json);

    Ok(())
}
