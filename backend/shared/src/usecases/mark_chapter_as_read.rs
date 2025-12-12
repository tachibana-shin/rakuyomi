use crate::{database::Database, model::ChapterId};

pub async fn mark_chapter_as_read(db: &Database, id: &ChapterId, value: Option<bool>) {
    db.mark_chapter_as_read(id, value).await
}
