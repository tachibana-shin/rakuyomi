//! Conversion of keiyoushi (mihon) extension results to the RakuYomi source
//! models.
//!
//! Manga, chapter and page URLs from extensions are often relative to the
//! site root. The keiyoushi bridge does not expose the extension's `baseUrl`
//! getter, so relative URLs are resolved against the URL of the last request
//! the extension made during the call (the list/detail/page request itself),
//! which is recorded by the HTTP callback.

use chrono::TimeZone;
use url::Url;

use crate::source::model::{
    Chapter, Manga, MangaContentRating, MangaViewer, Page, PublishingStatus,
};

/// Maps the mihon `SManga.Status` constants onto the rakuyomi publishing
/// status. `LICENSED` and `PUBLISHING_FINISHED` fold into `Completed`.
fn status_from_mihon(index: i32) -> PublishingStatus {
    match index {
        0 => PublishingStatus::Ongoing,
        1 => PublishingStatus::Completed,
        2 | 3 => PublishingStatus::Completed,
        4 => PublishingStatus::Cancelled,
        5 => PublishingStatus::Hiatus,
        _ => PublishingStatus::Unknown,
    }
}

/// Resolves a possibly relative URL against the base of the last request
/// made by the extension. Protocol-relative URLs (`//host/path`) join
/// through the base scheme.
pub(crate) fn resolve_url(base: Option<&Url>, url: &str) -> Option<Url> {
    if url.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(url) {
        return Some(url);
    }
    match base {
        Some(base) => base.join(url).ok(),
        None => None,
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// Converts a keiyoushi manga into the rakuyomi model. The raw manga URL is
/// the id, mirroring how mihon addresses mangas; it is handed back verbatim
/// to `mangaDetailsRequest`/`chapterListRequest`.
pub(crate) fn manga_from_keiyoushi(
    source_id: &str,
    base: Option<&Url>,
    manga: dexvm::keiyoushi::Manga,
) -> Manga {
    let genre = manga.genre;
    let raw_url = manga.url.clone();
    Manga {
        source_id: source_id.to_string(),
        id: manga.url,
        title: nonempty(manga.title),
        author: nonempty(manga.author),
        artist: nonempty(manga.artist),
        description: nonempty(manga.description),
        tags: nonempty(genre).map(|genre| {
            genre
                .split(',')
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty())
                .collect()
        }),
        cover_url: resolve_url(base, &manga.thumbnail_url),
        url: resolve_url(base, &raw_url),
        status: status_from_mihon(manga.status),
        nsfw: MangaContentRating::Safe,
        viewer: MangaViewer::DefaultViewer,
        last_updated: None,
        last_opened: None,
        last_read: None,
        date_added: None,
    }
}

/// Converts a `MangasPage` result into rakuyomi mangas.
pub(crate) fn mangas_from_page(
    source_id: &str,
    base: Option<&Url>,
    mangas: Vec<dexvm::keiyoushi::Manga>,
) -> Vec<Manga> {
    mangas
        .into_iter()
        .map(|manga| manga_from_keiyoushi(source_id, base, manga))
        .collect()
}

/// Converts the chapter list of a manga into rakuyomi chapters. Each
/// chapter's raw URL is its id, handed back verbatim to `pageListRequest`.
pub(crate) fn chapters_from_keiyoushi(
    source_id: &str,
    manga_id: &str,
    base: Option<&Url>,
    chapters: Vec<dexvm::keiyoushi::Chapter>,
) -> Vec<Chapter> {
    chapters
        .into_iter()
        .enumerate()
        .map(|(index, chapter)| Chapter {
            source_id: source_id.to_string(),
            // Raw chapter URL as the id; the downloader hands it straight to
            // `get_page_list`, and extensions may join it with their own base
            // URL (some keep bare ids in `chapter.url`).
            id: chapter.url.clone(),
            manga_id: manga_id.to_string(),
            title: nonempty(chapter.name),
            scanlator: nonempty(chapter.scanlator),
            url: resolve_url(base, &chapter.url),
            lang: None,
            chapter_num: None,
            volume_num: None,
            // mihon publishes the upload timestamp as epoch milliseconds.
            date_uploaded: chrono::Utc
                .timestamp_millis_opt(chapter.date_upload)
                .single()
                .map(|dt| dt.with_timezone(&chrono_tz::UTC)),
            source_order: index,
            thumbnail: None,
            locked: Some(false),
        })
        .collect()
}

/// Converts a page list result into rakuyomi pages.
pub(crate) fn pages_from_keiyoushi(
    source_id: &str,
    chapter_id: &str,
    base: Option<&Url>,
    pages: Vec<dexvm::keiyoushi::PageRef>,
) -> Vec<Page> {
    pages
        .into_iter()
        .map(|page| Page {
            source_id: source_id.to_string(),
            chapter_id: chapter_id.to_string(),
            index: page.index.max(0) as usize,
            // Tachiyomi `Page.url` can be a comma-joined payload whose first
            // element is the image host (mangadex at-home base URL); the
            // actual image path is `Page.imageUrl`. Prefer that combination
            // over resolving the raw url against the last request URL.
            image_url: page_image_url(base, &page),
            base64: None,
            text: None,
            ctx: None,
        })
        .collect()
}

fn page_image_url(base: Option<&Url>, page: &dexvm::keiyoushi::PageRef) -> Option<Url> {
    let host = page
        .url
        .split(',')
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| Url::parse(s).ok())
        .or_else(|| base.cloned());
    if page.image_url.is_empty() {
        return resolve_url(host.as_ref(), &page.url);
    }
    resolve_url(host.as_ref(), &page.image_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://akuma.example.com").unwrap()
    }

    #[test]
    fn test_resolve_url() {
        assert_eq!(
            resolve_url(Some(&base()), "/manga/one.json")
                .unwrap()
                .as_str(),
            "https://akuma.example.com/manga/one.json"
        );
        assert_eq!(
            resolve_url(Some(&base()), "https://x.example.com/a.png")
                .unwrap()
                .as_str(),
            "https://x.example.com/a.png"
        );
        assert_eq!(
            resolve_url(Some(&base()), "//cdn.example.com/a.png")
                .unwrap()
                .as_str(),
            "https://cdn.example.com/a.png"
        );
        assert!(resolve_url(Some(&base()), "").is_none());
        assert!(resolve_url(None, "relative/path").is_none());
    }

    #[test]
    fn test_status_mapping() {
        assert_eq!(status_from_mihon(0), PublishingStatus::Ongoing);
        assert_eq!(status_from_mihon(1), PublishingStatus::Completed);
        assert_eq!(status_from_mihon(2), PublishingStatus::Completed);
        assert_eq!(status_from_mihon(3), PublishingStatus::Completed);
        assert_eq!(status_from_mihon(4), PublishingStatus::Cancelled);
        assert_eq!(status_from_mihon(5), PublishingStatus::Hiatus);
        assert_eq!(status_from_mihon(99), PublishingStatus::Unknown);
    }

    #[test]
    fn test_manga_conversion() {
        let source_id = "eu.kanade.tachiyomi.all.akuma";
        let manga = dexvm::keiyoushi::Manga {
            title: "One Piece".to_string(),
            author: "Oda Eiichiro".to_string(),
            description: "Pirate story".to_string(),
            genre: "Action, Adventure, Shounen".to_string(),
            status: 1,
            thumbnail_url: "/uploads/one.jpg".to_string(),
            url: "/manga/one-piece-2".to_string(),
            ..Default::default()
        };
        let converted = manga_from_keiyoushi(source_id, Some(&base()), manga);
        assert_eq!(converted.id, "/manga/one-piece-2");
        assert_eq!(converted.title.as_deref(), Some("One Piece"));
        assert_eq!(
            converted.cover_url.map(|u| u.to_string()),
            Some("https://akuma.example.com/uploads/one.jpg".to_string())
        );
        assert_eq!(converted.status, PublishingStatus::Completed);
        assert_eq!(
            converted.tags.as_deref(),
            Some(
                &[
                    "Action".to_string(),
                    "Adventure".to_string(),
                    "Shounen".to_string()
                ][..]
            )
        );
    }

    #[test]
    fn test_chapters_and_pages() {
        let source_id = "eu.kanade.tachiyomi.all.akuma";
        let chapters = chapters_from_keiyoushi(
            source_id,
            "/manga/one-piece-2",
            Some(&base()),
            vec![dexvm::keiyoushi::Chapter {
                name: "Chapter 1".to_string(),
                url: "/manga/one-piece-2/c1".to_string(),
                date_upload: 1_734_595_200_000,
                scanlator: "ScanTeam".to_string(),
            }],
        );
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].id, "/manga/one-piece-2/c1");
        assert_eq!(chapters[0].scanlator.as_deref(), Some("ScanTeam"));
        assert_eq!(chapters[0].source_order, 0);
        assert!(chapters[0].date_uploaded.is_some());

        let pages = pages_from_keiyoushi(
            source_id,
            "/manga/one-piece-2/c1",
            Some(&base()),
            vec![
                dexvm::keiyoushi::PageRef {
                    index: 0,
                    name: "1".to_string(),
                    url: "/img/1.jpg".to_string(),
                    image_url: "".to_string(),
                },
                dexvm::keiyoushi::PageRef {
                    index: 1,
                    name: "2".to_string(),
                    url: "https://cdn.example.com/2.jpg".to_string(),
                    image_url: "".to_string(),
                },
            ],
        );
        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[0].image_url.clone().map(|u| u.to_string()),
            Some("https://akuma.example.com/img/1.jpg".to_string())
        );
        assert_eq!(
            pages[1].image_url.clone().map(|u| u.to_string()),
            Some("https://cdn.example.com/2.jpg".to_string())
        );
    }
}
