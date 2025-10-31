use crate::{database::Database, model::ChapterId};

pub async fn update_last_read_chapter(db: &Database, id: &ChapterId) {
    db.update_last_read_chapter(id).await
}
