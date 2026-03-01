use anyhow::Result;

use crate::{chapter_storage::ChapterStorage, database::Database, model::NotificationInformation};

pub async fn get_notifications(
    db: &Database,
    chapter_storage: &ChapterStorage,
) -> Result<Vec<NotificationInformation>> {
    let mut notifications = db.get_notifications().await?;

    for notify in &mut notifications {
        if notify.manga_cover.is_none() {
            continue;
        }

        let output = chapter_storage.poster_exists(notify.chapter_id.manga_id());

        notify.manga_cover = if let Some(path) = output {
            match url::Url::from_file_path(&path) {
                Ok(url) => Some(url),
                Err(_) => match path.canonicalize() {
                    Ok(canonical_path) => url::Url::from_file_path(canonical_path).ok(),
                    Err(e) => {
                        println!("Error canonicalizing path {:?}: {}", path, e);
                        None
                    }
                },
                    Ok(url) => Some(url),
                    Err(_) => {
                        println!("Error converting path to URL");
                        None
                    }
                },
            }
        } else {
            None
        };
    }

    Ok(notifications)
}
