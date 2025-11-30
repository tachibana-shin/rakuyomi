use anyhow::Result;

use crate::{chapter_storage::ChapterStorage, database::Database, model::NotificationInformation};

pub async fn get_notifications(
    db: &Database,
    chapter_storage: &ChapterStorage,
) -> Result<Vec<NotificationInformation>> {
    let mut notifications = db.get_notifications().await;

    futures::future::join_all(notifications.iter_mut().map(|notify| async {
        if let Some(url) = &notify.manga_cover {
            let output = match chapter_storage.cache_poster(url).await {
                Ok(v) => v,
                Err(_) => return,
            };

            notify.manga_cover = if let Some(path) = output {
                match url::Url::from_file_path(&path) {
                    Ok(url) => Some(url),
                    Err(_) => match url::Url::from_file_path(path.canonicalize().unwrap()) {
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
    }))
    .await;

    Ok(notifications)
}
