use std::collections::HashMap;

use anyhow::{bail, Context, Result};

use crate::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::{Chapter, ChapterId, MangaId},
};

pub async fn mark_chapters_as_read(
    db: &Database,
    chapter_storage: &ChapterStorage,
    manga_id: MangaId,
    text: &String,
    state: bool,
) -> Result<Option<usize>> {
    let chapters = super::get_cached_manga_chapters(db, chapter_storage, &manga_id).await?;

    let selected_ids = parse_chapter_ranges(&chapters, text)?;

    db.set_chapters_read_state(&manga_id, &selected_ids, state)
        .await
}

fn parse_chapter_ranges(chapters: &Vec<Chapter>, text: &str) -> Result<Vec<ChapterId>> {
    if text.trim().is_empty() {
        return Ok(chapters
            .into_iter()
            .map(|c| c.information.id.clone())
            .collect());
    }

    let chapters: HashMap<_, _> = chapters
        .into_iter()
        .enumerate()
        .map(|(idx, ch)| {
            let key = ch
                .information
                .chapter_number
                .map(|n| n.to_string())
                .unwrap_or(idx.to_string());
            (key, ch)
        })
        .collect();

    // Build a sorted list of all numeric chapter keys (float)
    // This allows range scanning even with decimal chapter numbers
    let mut sorted_keys: Vec<f64> = chapters
        .keys()
        .filter_map(|k| k.parse::<f64>().ok())
        .collect();

    sorted_keys.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mut output = Vec::<ChapterId>::new();

    for part in text.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        // Case: range "a-b"
        if let Some((start_raw, stop_raw)) = part.split_once('-') {
            let start = start_raw.trim();
            let stop = stop_raw.trim();

            // Parse as float
            let start_val: f64 = start.parse().context("Invalid float range start")?;
            let stop_val: f64 = stop.parse().context("Invalid float range stop")?;

            if start_val > stop_val {
                bail!("Invalid range {}: start > stop", part);
            }

            // Iterate through sorted chapter keys and select those inside range
            for key in sorted_keys.iter().copied() {
                if key >= start_val && key <= stop_val {
                    let key_s = key.to_string();

                    if let Some(ch) = chapters.get(&key_s) {
                        output.push(ch.information.id.clone());
                    }
                }
            }
        } else {
            // Case: single chapter (float allowed)
            let key = part.trim();

            let ch = chapters
                .get(key)
                .context(format!("Can't find chapter {}", key))?;

            output.push(ch.information.id.clone());
        }
    }

    Ok(output)
}
