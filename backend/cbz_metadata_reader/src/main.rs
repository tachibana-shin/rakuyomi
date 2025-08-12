use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use shared::cbz_metadata::{non_empty_string, ComicInfo};

#[derive(Serialize, Debug, Default)]
struct KoReaderMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series_index: Option<String>, // Store as string
    #[serde(skip_serializing_if = "Option::is_none")]
    authors: Option<String>, // Concatenated authors
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publication_year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>, // Map 'Comments' to 'notes'
    #[serde(skip_serializing_if = "Option::is_none")]
    keywords: Option<String>, // Map 'Genre' to 'keywords'
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f64>,
}

#[derive(Parser, Debug)]
struct Args {
    /// Path to the CBZ file
    file_path: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let file_path = PathBuf::from(&args.file_path);

    // Read ComicInfo.xml from the CBZ file using the shared function
    let comic_info = ComicInfo::from_file(&file_path)
        .with_context(|| format!("Could not read metadata from {}", file_path.display()))?;

    // Transform ComicInfo.xml into KoReaderMetadata
    let ko_meta = transform_from_comic_info_xml(comic_info);

    let output_json =
        serde_json::to_string(&ko_meta).context("Failed to serialize metadata to JSON")?;
    println!("{}", output_json);

    Ok(())
}

// Transform ComicInfo.xml data into KoReaderMetadata
fn transform_from_comic_info_xml(comic_info: ComicInfo) -> KoReaderMetadata {
    let mut ko_meta = KoReaderMetadata {
        title: non_empty_string(comic_info.title),
        series: non_empty_string(comic_info.series),
        series_index: non_empty_string(comic_info.number),
        publisher: non_empty_string(comic_info.publisher),
        language: non_empty_string(comic_info.language_iso),
        notes: non_empty_string(comic_info.summary),
        keywords: non_empty_string(comic_info.genre),
        ..Default::default()
    };

    // Combine writer, penciller, inker as authors
    let mut authors = Vec::new();
    if !comic_info.writer.is_empty() {
        authors.push(comic_info.writer);
    }

    if !comic_info.penciller.is_empty() && !authors.contains(&comic_info.penciller) {
        authors.push(comic_info.penciller);
    }

    if !comic_info.inker.is_empty() && !authors.contains(&comic_info.inker) {
        authors.push(comic_info.inker);
    }

    if !authors.is_empty() {
        ko_meta.authors = Some(authors.join(" & "));
    }

    if comic_info.year > 0 {
        ko_meta.publication_year = Some(comic_info.year);
    }

    // Community rating (0-5 scale)
    ko_meta.rating = comic_info.community_rating.map(|r| r.into());

    ko_meta
}
