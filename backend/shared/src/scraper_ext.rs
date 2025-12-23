use scraper::ElementRef;
use scraper::Selector;

pub trait SelectSoup<'a> {
    fn select_soup(&'a self, selector: &'a Selector) -> impl Iterator<Item = ElementRef<'a>> + 'a;
}

enum SoupIter<'a> {
    One(std::iter::Once<ElementRef<'a>>),
    Empty(std::iter::Empty<ElementRef<'a>>),
}

impl<'a> Iterator for SoupIter<'a> {
    type Item = ElementRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            SoupIter::One(iter) => iter.next(),
            SoupIter::Empty(iter) => iter.next(),
        }
    }
}

impl<'a> SelectSoup<'a> for ElementRef<'a> {
    fn select_soup(&'a self, selector: &'a Selector) -> impl Iterator<Item = ElementRef<'a>> + 'a {
        let root_iter = if selector.matches(self) {
            SoupIter::One(std::iter::once(*self))
        } else {
            SoupIter::Empty(std::iter::empty())
        };

        let children_iter = self.select(selector);

        root_iter.chain(children_iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{Html, Selector};

    /// Test that select_soup returns the root element when it matches the selector.
    #[test]
    fn test_select_soup_includes_root_if_matches() {
        let html = r#"
            <div id="root" class="item">
                <span class="item">Child</span>
            </div>
        "#;

        let document = Html::parse_fragment(html);
        let root = document
            .select(&Selector::parse("div.item").unwrap())
            .next()
            .expect("Root div not found");

        let selector = Selector::parse(".item").unwrap();

        let items: Vec<ElementRef> = root.select_soup(&selector).collect();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].value().id(), Some("root"));
    }

    /// Test when root does NOT match selector.
    #[test]
    fn test_select_soup_no_root_match() {
        let html = r#"
            <div id="root">
                <span class="item">Child 1</span>
                <p class="item">Child 3</p>
            </div>
        "#;

        let document = Html::parse_fragment(html);
        let root = document
            .select(&Selector::parse("div").unwrap())
            .next()
            .unwrap();
        let selector = Selector::parse(".item").unwrap();

        let items: Vec<ElementRef> = root.select_soup(&selector).collect();

        assert_eq!(items.len(), 2);
    }

    /// Test empty result.
    #[test]
    fn test_select_soup_no_matches() {
        let html = r#"<div><p>hello</p></div>"#;

        let document = Html::parse_fragment(html);
        let root = document
            .select(&Selector::parse("div").unwrap())
            .next()
            .unwrap();
        let selector = Selector::parse(".not-exist").unwrap();

        let items: Vec<ElementRef> = root.select_soup(&selector).collect();

        assert!(items.is_empty());
    }

    /// Test `<div>fwef<div>fe</div></div>` and ensure the outer div is self-matching.
    #[test]
    fn test_select_soup_div_nested_text() {
        let html = r#"<div>fwef<div>fe</div></div>"#;

        let document = Html::parse_fragment(html);

        // Select the OUTER div
        let root = document
            .select(&Selector::parse("div").unwrap())
            .next()
            .expect("Outer div not found");

        let selector = Selector::parse("div").unwrap();

        let items: Vec<ElementRef> = root.select_soup(&selector).collect();

        // Expect 2 divs: outer first, inner second
        assert_eq!(items.len(), 2, "Expected 2 matching divs");
        assert!(
            items[0].value().name() == "div",
            "Outer div should be first match"
        );
        assert!(
            items[1].value().name() == "div",
            "Inner div should be second match"
        );
    }

    /// Test `<a>` to verify that it self-matches with selector "a".
    #[test]
    fn test_select_soup_anchor_self_match() {
        let html = r#"<a href="https://example.com">Hello</a>"#;

        let document = Html::parse_fragment(html);

        // Select the <a> node
        let root = document
            .select(&Selector::parse("a").unwrap())
            .next()
            .expect("Anchor not found");

        let selector = Selector::parse("a").unwrap();

        let items: Vec<ElementRef> = root.select_soup(&selector).collect();

        // Should match itself, only 1 element
        assert_eq!(items.len(), 1, "Expected <a> to match itself");
        assert_eq!(items[0].value().name(), "a");
    }
}
