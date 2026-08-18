use anyhow::{anyhow, Result};
use dom_query::{Matcher, NodeId, NodeRef, Selection};

use crate::source::wasm_store::WasmStore;

#[derive(Debug, Clone)]
pub struct HTMLElement {
    pub document: usize,
    pub node_id: NodeId,
    pub base_uri: Option<String>,
}
impl HTMLElement {
    fn node_ref<'a>(&'a self, store: &'a mut WasmStore) -> Option<NodeRef<'a>> {
        let document = store.get_html(self.document)?;

        Some(NodeRef::new(self.node_id, &document.tree))
    }
    fn to_element(&self, node_id: NodeId) -> Self {
        Self {
            document: self.document,
            node_id,
            base_uri: self.base_uri.to_owned(),
        }
    }
    pub fn select_soup(&self, store: &mut WasmStore, selector: &str) -> Result<Option<Vec<Self>>> {
        let Some(node) = self.node_ref(store) else {
            return Ok(None);
        };

        let matcher = Matcher::new(&normalize_selector(selector))
            .map_err(|err| anyhow!("[{selector}]{:?}", err))?;
        let mut elements = if matcher.match_element(&node) {
            vec![self.clone()]
        } else {
            vec![]
        };

        elements.extend(
            Selection::from(node)
                .select_matcher(&matcher)
                .nodes()
                .iter()
                .map(|node| self.to_element(node.id))
                .collect::<Vec<_>>(),
        );

        Ok(Some(elements))
    }
    pub fn select_soup_first(&self, store: &mut WasmStore, selector: &str) -> Result<Option<Self>> {
        let Some(node) = self.node_ref(store) else {
            return Ok(None);
        };

        let matcher = Matcher::new(&normalize_selector(selector))
            .map_err(|err| anyhow!("[{selector}]{:?}", err))?;
        if matcher.match_element(&node) {
            return Ok(Some(self.clone()));
        }

        Ok(Selection::from(node)
            .select_single_matcher(&matcher)
            .nodes()
            .first()
            .map(|node| self.to_element(node.id)))
    }
    /// Reads an attribute. The `abs:` prefix resolves the value against
    /// the element's base URI (Aidoku's `HtmlAttribute::abs`).
    pub fn attr(&self, store: &mut WasmStore, name: &str) -> Option<String> {
        let node = self.node_ref(store)?;

        let (name, absolute) = match name.strip_prefix("abs:") {
            Some(name) => (name, true),
            None => (name, false),
        };
        let value = node.attr(name)?.to_string();
        if !absolute {
            return Some(value);
        }

        let base_uri = self.base_uri.as_deref().unwrap_or("");
        let absolute_url = url::Url::parse(base_uri)
            .unwrap_or_else(|_| url::Url::parse("file:///").unwrap())
            .join(&value)
            .ok()?;
        Some(absolute_url.to_string())
    }
    pub fn next(&self, store: &mut WasmStore) -> Option<Self> {
        let node = self.node_ref(store)?;

        node.next_element_sibling()
            .map(|node| self.to_element(node.id))
    }
    pub fn previous(&self, store: &mut WasmStore) -> Option<Self> {
        let node = self.node_ref(store)?;

        node.prev_element_sibling()
            .map(|node| self.to_element(node.id))
    }
    pub fn parent(&self, store: &mut WasmStore) -> Option<Self> {
        let node = self.node_ref(store)?;

        node.parent().map(|node| self.to_element(node.id))
    }
    pub fn kind(&self, store: &mut WasmStore) -> Option<i32> {
        let node = self.node_ref(store)?;
        let kind = if node.is_document() {
            7
        } else if node.is_text() {
            2
        } else if node.is_comment() {
            4
        } else if node.is_element() {
            5
        } else {
            0
        };
        Some(kind)
    }
    pub fn child_nodes(&self, store: &mut WasmStore) -> Option<Vec<Self>> {
        let node = self.node_ref(store)?;
        node.children()
            .into_iter()
            .map(|node| self.to_element(node.id))
            .collect::<Vec<_>>()
            .into()
    }
    pub fn children(&self, store: &mut WasmStore) -> Option<Vec<Self>> {
        let node = self.node_ref(store)?;

        node.children()
            .into_iter()
            .map(|node| self.to_element(node.id))
            .collect::<Vec<_>>()
            .into()
    }
    // pub fn next_siblings(&self, store: &mut WasmStore) -> Option<Vec<Self>> {
    //     let mut node = self.node_ref(store)?;

    //     let mut elements: Vec<Self> = Vec::new();

    //     while let Some(current) = node.next_element_sibling() {
    //         elements.push(self.to_element(current.id));
    //         node = current;
    //     }

    //     elements.into()
    // }
    // pub fn prev_siblings(&self, store: &mut WasmStore) -> Option<Vec<Self>> {
    //     let mut node = self.node_ref(store)?;

    //     let mut elements: Vec<Self> = Vec::new();

    //     while let Some(current) = node.prev_element_sibling() {
    //         elements.push(self.to_element(current.id));
    //         node = current;
    //     }

    //     elements.into()
    // }
    pub fn siblings(&self, store: &mut WasmStore) -> Option<Vec<Self>> {
        let document = store.get_html(self.document)?;

        let node = NodeRef::new(self.node_id, &document.tree);

        node.parent()?
            .children()
            .into_iter()
            .filter(|p| p.id != node.id)
            .map(|node| self.to_element(node.id))
            .collect::<Vec<_>>()
            .into()
    }

    pub fn text(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        text_nodes(node)
            .into_iter()
            .map(|n| n.text().trim().to_string())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned()
            .into()
    }

    pub fn untrimmed_text(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        text_nodes(node)
            .into_iter()
            .map(|n| n.text().to_string())
            .collect::<Vec<_>>()
            .join(" ")
            .to_owned()
            .into()
    }

    pub fn own_text(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.immediate_text().to_string().into()
    }

    pub fn html(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.inner_html().to_string().into()
    }

    pub fn outer_html(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.html().to_string().into()
    }

    pub fn id(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.id_attr().map(|v| v.to_string())
    }
    pub fn tag(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.node_name().map(|v| v.to_string())
    }
    pub fn class(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.class().map(|v| v.to_string())
    }

    pub fn has_class(&self, store: &mut WasmStore, name: &str) -> Option<bool> {
        let node = self.node_ref(store)?;

        Some(node.has_class(name))
    }
    pub fn has_attr(&self, store: &mut WasmStore, name: &str) -> Option<bool> {
        let node = self.node_ref(store)?;

        Some(node.has_attr(name))
    }

    pub fn data(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        // let mut result = String::new();

        // for child in  node.children() {
        //     if child.is_text() {
        //         result.push_str(child.text().to_string())
        //     }
        // }

        // result

        // equal

        node.immediate_text().to_string().into()
    }

    pub fn set_text(&self, store: &mut WasmStore, text: &str) -> Option<()> {
        let node = self.node_ref(store)?;

        node.set_text(text);

        ().into()
    }
    pub fn set_html(&self, store: &mut WasmStore, html: &str) -> Option<()> {
        let node = self.node_ref(store)?;

        node.set_html(html);

        ().into()
    }
    pub fn append(&self, store: &mut WasmStore, text: &str) -> Option<()> {
        let document = store.get_html(self.document)?;

        let node = NodeRef::new(self.node_id, &document.tree);

        let new_node = document.tree.new_text(text);
        node.append_child(&new_node.id);

        ().into()
    }
    pub fn prepend(&self, store: &mut WasmStore, text: &str) -> Option<()> {
        let document = store.get_html(self.document)?;

        let node = NodeRef::new(self.node_id, &document.tree);

        let new_node = document.tree.new_text(text);
        node.prepend_child(&new_node.id);

        ().into()
    }
    pub fn remove(&self, store: &mut WasmStore) -> Option<()> {
        let node = self.node_ref(store)?;

        Selection::from(node).remove();

        ().into()
    }
    pub fn add_class(&self, store: &mut WasmStore, name: &str) -> Option<()> {
        let node = self.node_ref(store)?;

        node.add_class(name);

        ().into()
    }
    pub fn remove_class(&self, store: &mut WasmStore, name: &str) -> Option<()> {
        let node = self.node_ref(store)?;

        node.remove_class(name);

        ().into()
    }
    pub fn set_attr(&self, store: &mut WasmStore, name: &str, value: &str) -> Option<()> {
        let node = self.node_ref(store)?;

        if value.is_empty() {
            node.remove_attr(name);
        } else {
            node.set_attr(name, value);
        }

        ().into()
    }
    pub fn remove_attr(&self, store: &mut WasmStore, name: &str) -> Option<()> {
        let node = self.node_ref(store)?;

        node.remove_attr(name);

        ().into()
    }
}

/// Normalises a CSS selector for [`Matcher`], whose `selectors`-crate
/// parser rejects two common inputs: the jQuery-style `:contains(...)`
/// pseudo-class and unquoted `[...]` attribute values (e.g.
/// `[href=/manga/1]` — only identifiers or quoted strings parse).
///
/// Both are rewritten into the quoted forms the parser accepts.
pub(crate) fn normalize_selector(selector: &str) -> String {
    let mut out = String::with_capacity(selector.len());
    let chars: Vec<char> = selector.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i..].starts_with(&[':', 'c', 'o', 'n', 't', 'a', 'i', 'n', 's', '(']) {
            out.push_str(":contains(");
            i += 10;

            let mut inner = String::new();
            let mut depth = 1;

            while i < chars.len() && depth > 0 {
                let c = chars[i];

                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }

                inner.push(c);
                i += 1;
            }

            let inner_trim = inner.trim();
            if inner_trim.starts_with('"') && inner_trim.ends_with('"') {
                out.push_str(inner_trim);
            } else {
                out.push('"');
                out.push_str(inner_trim);
                out.push('"');
            }

            out.push(')');
            continue;
        }

        if chars[i] == '[' {
            // Copy the whole bracket up to the matching `]` (quote-aware),
            // normalising the attribute selector inside.
            let mut j = i + 1;
            let mut quote: Option<char> = None;
            while j < chars.len() {
                let c = chars[j];
                if let Some(q) = quote {
                    if c == q {
                        quote = None;
                    }
                } else if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c == ']' {
                    break;
                }
                j += 1;
            }
            if j >= chars.len() {
                // Unclosed bracket: copy verbatim.
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let inner: String = chars[i + 1..j].iter().collect();
            out.push('[');
            out.push_str(&normalize_attribute(&inner));
            out.push(']');
            i = j + 1;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Normalises the inside of one `[...]` attribute selector: wraps an
/// unquoted attribute value in double quotes (the `selectors`-crate parser
/// rejects values like `/manga/1`, which are neither identifiers nor
/// strings). Already quoted values and the bare existence form `[attr]` are
/// kept as-is, as are the case-insensitive `i`/`s` flags.
fn normalize_attribute(inner: &str) -> String {
    let inner = inner.trim();
    if inner.is_empty() {
        return inner.to_string();
    }

    // Find the first operator: `~=`, `|=`, `^=`, `$=`, `*=` or `=`. A bare
    // `|` (e.g. the `xlink|href` namespace prefix) is not an operator.
    let bytes = inner.as_bytes();
    let mut op: Option<(usize, usize)> = None;
    for (idx, byte) in bytes.iter().enumerate() {
        if matches!(byte, b'~' | b'|' | b'^' | b'$' | b'*') {
            if bytes.get(idx + 1) == Some(&b'=') {
                op = Some((idx, 2));
                break;
            }
        } else if *byte == b'=' {
            op = Some((idx, 1));
            break;
        }
    }
    let Some((op_start, op_len)) = op else {
        // `[attr]` existence check.
        return inner.to_string();
    };

    let key = inner[..op_start].trim();
    let operator = &inner[op_start..op_start + op_len];
    let value_part = inner[op_start + op_len..].trim();

    if value_part.is_empty() {
        return format!("{key}{operator}\"\"");
    }

    let (value, flag) = split_attr_value(value_part);
    if value.starts_with('"') || value.starts_with('\'') || value.contains('"') {
        // Already quoted, or unsafe to quote: reassemble untouched.
        if flag.is_empty() {
            return format!("{key}{operator}{value}");
        }
        return format!("{key}{operator}{value} {flag}");
    }
    if flag.is_empty() {
        return format!("{key}{operator}\"{value}\"");
    }
    format!("{key}{operator}\"{value}\" {flag}")
}

/// Splits an attribute value part into the value and an optional trailing
/// `i`/`s` case flag, honouring quoted values (which may contain spaces).
fn split_attr_value(value_part: &str) -> (&str, &str) {
    if let Some(first) = value_part.chars().next() {
        if first == '"' || first == '\'' {
            let close = value_part[1..]
                .find(first)
                .map(|p| p + 2)
                .unwrap_or(value_part.len());
            return (&value_part[..close], value_part[close..].trim());
        }
    }
    match value_part.find(char::is_whitespace) {
        Some(pos) => (&value_part[..pos], value_part[pos..].trim()),
        None => (value_part, ""),
    }
}

fn text_nodes<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    if node.is_text() {
        return vec![node];
    }

    node.children()
        .into_iter()
        .flat_map(|node| {
            let tag = node.node_name().unwrap_or_default().to_string();
            if tag == "script" || tag == "style" {
                return vec![];
            }

            text_nodes(node)
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::normalize_selector;

    fn setup_html_store(html: &str) -> (crate::source::wasm_store::WasmStore, super::HTMLElement) {
        use crate::settings::Settings;
        use crate::source::source_settings::SourceSettings;
        use crate::source_manager::SourceManager;
        use dom_query::Document;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let source_settings = SourceSettings::new(
            "test".to_owned(),
            &[],
            &HashMap::new(),
            &Arc::new(tokio::sync::Mutex::new(SourceManager::new(
                PathBuf::new(),
                HashMap::new(),
                Settings::default(),
            ))),
        )
        .unwrap();

        let mut store = crate::source::wasm_store::WasmStore::default(source_settings);
        let document = Document::from(html);
        let root_id = document.root().id;
        let html_idx = store.set_html(document);
        let element = super::HTMLElement {
            document: html_idx,
            node_id: root_id,
            base_uri: None,
        };
        (store, element)
    }

    #[test]
    fn test_kind_element() {
        let (mut store, _) = setup_html_store("<div><p>hello</p><span>world</span></div>");
        let doc = store.get_html(0).unwrap();
        let root_id = doc.root().id;
        let root_element = super::HTMLElement {
            document: 0,
            node_id: root_id,
            base_uri: None,
        };
        assert_eq!(root_element.kind(&mut store).unwrap(), 7);
    }

    #[test]
    fn test_kind_returns_element_for_div() {
        let (mut store, element) = setup_html_store("<div><p>hello</p></div>");
        let children = element.children(&mut store).unwrap();
        assert_eq!(children.len(), 1);
        let kind = children[0].kind(&mut store).unwrap();
        assert_eq!(kind, 5);
    }

    #[test]
    fn test_child_nodes_returns_all_children() {
        let (mut store, element) = setup_html_store("<div><span>a</span><span>b</span></div>");
        let div = element.select_soup(&mut store, "div").unwrap().unwrap();
        let child_nodes = div[0].child_nodes(&mut store).unwrap();
        assert_eq!(child_nodes.len(), 2);
    }

    #[test]
    fn test_child_nodes_includes_text_nodes() {
        let (mut store, element) = setup_html_store("<div>hello<span>world</span></div>");
        let div = element.select_soup(&mut store, "div").unwrap().unwrap();
        let child_nodes = div[0].child_nodes(&mut store).unwrap();
        let text_kinds: Vec<i32> = child_nodes
            .iter()
            .map(|n| n.kind(&mut store).unwrap())
            .collect();
        assert!(text_kinds.contains(&2));
        assert!(text_kinds.contains(&5));
    }

    #[test]
    fn keeps_selector_without_contains_unchanged() {
        let sel = "div.content > a[href^=\"https\"]";
        assert_eq!(normalize_selector(sel), sel);
    }

    #[test]
    fn adds_quotes_when_missing_simple() {
        let sel = ":contains(hello)";
        assert_eq!(normalize_selector(sel), ":contains(\"hello\")");
    }

    #[test]
    fn preserves_existing_double_quotes() {
        let sel = ":contains(\"hello world\")";
        assert_eq!(normalize_selector(sel), ":contains(\"hello world\")");
    }

    #[test]
    fn trims_inner_whitespace() {
        let sel = ":contains(   hello   )";
        assert_eq!(normalize_selector(sel), ":contains(\"hello\")");
    }

    #[test]
    fn handles_multiple_contains_in_selector() {
        let sel = "div:contains(hello) span:contains(world)";
        let expected = "div:contains(\"hello\") span:contains(\"world\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn nested_parentheses_inside_contains() {
        // Inner has parentheses that should be treated as content; algorithm tracks depth.
        let sel = ":contains(text(with(parens)))";
        let expected = ":contains(\"text(with(parens))\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn nested_contains_inside_not() {
        let sel = ":not(:contains(hello world))";
        let expected = ":not(:contains(\"hello world\"))";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn keeps_other_colon_pseudo_selectors_intact() {
        let sel = ":first-child:contains(foo)";
        let expected = ":first-child:contains(\"foo\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn special_characters_are_preserved() {
        let sel = ":contains(he[llo].*+?|^$)";
        let expected = ":contains(\"he[llo].*+?|^$\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn unicode_is_preserved() {
        let sel = ":contains(xin chào 🌟)";
        let expected = ":contains(\"xin chào 🌟\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn mixed_content_around_contains() {
        let sel = "ul li.item:contains(Item 1) > a.active";
        let expected = "ul li.item:contains(\"Item 1\") > a.active";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn inner_already_quoted_with_extra_spaces() {
        let sel = ":contains(   \"hello world\"   )";
        let expected = ":contains(\"hello world\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn does_not_add_quotes_twice_when_already_quoted() {
        let sel = "div:contains(\"a(b)c\")";
        let expected = "div:contains(\"a(b)c\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn malformed_missing_closing_paren_consumes_until_end() {
        // Current implementation will read until EOF if no closing ')', thus inner becomes the rest.
        // It will still wrap in quotes.
        let sel = "div:contains(unclosed";
        let expected = "div:contains(\"unclosed\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn handles_contains_followed_by_trailing_text() {
        let sel = ":contains(abc)) trailing";
        // The parser will stop at the first matching ')' that closes depth to 0.
        // The extra ')' should be copied verbatim after processing.
        let expected = ":contains(\"abc\")) trailing";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn long_selector_with_attributes_and_contains() {
        let sel = "section[data-id=\"123\"] .card:contains(New (2025)) .title:contains(Đặc biệt)";
        let expected =
            "section[data-id=\"123\"] .card:contains(\"New (2025)\") .title:contains(\"Đặc biệt\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn quotes_unquoted_attribute_value() {
        let sel = "a[href=/manga/1]";
        let expected = "a[href=\"/manga/1\"]";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn quotes_unquoted_numeric_value() {
        let sel = "[data-id=123]";
        let expected = "[data-id=\"123\"]";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn quotes_each_operator_form() {
        for (sel, expected) in [
            ("[href^=/manga]", "[href^=\"/manga\"]"),
            ("[href$=.jpg]", "[href$=\".jpg\"]"),
            ("[href*=manga/1]", "[href*=\"manga/1\"]"),
            ("[lang~=en]", "[lang~=\"en\"]"),
            ("[lang|=en]", "[lang|=\"en\"]"),
        ] {
            assert_eq!(normalize_selector(sel), expected);
        }
    }

    #[test]
    fn keeps_already_quoted_attribute_values() {
        let sel = "a[href=\"/manga/1\"]";
        assert_eq!(normalize_selector(sel), sel);
    }

    #[test]
    fn keeps_single_quoted_attribute_values() {
        let sel = "a[href='/manga/1']";
        assert_eq!(normalize_selector(sel), sel);
    }

    #[test]
    fn keeps_existence_attribute_selector() {
        let sel = "input[disabled]";
        assert_eq!(normalize_selector(sel), sel);
    }

    #[test]
    fn quotes_empty_attribute_value() {
        let sel = "[data-x=]";
        let expected = "[data-x=\"\"]";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn keeps_case_insensitive_flag_on_quoted_value() {
        let sel = "[data-x=\"foo\" i]";
        assert_eq!(normalize_selector(sel), sel);
    }

    #[test]
    fn quotes_value_with_case_flag() {
        let sel = "[data-x=foo i]";
        let expected = "[data-x=\"foo\" i]";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn preserves_namespace_prefix() {
        let sel = "[xlink|href=foo]";
        let expected = "[xlink|href=\"foo\"]";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn bracket_scan_ignores_quotes_and_nested_contains() {
        let sel = "div[data-x=\"a]b\"] span:contains(hello)";
        let expected = "div[data-x=\"a]b\"] span:contains(\"hello\")";
        assert_eq!(normalize_selector(sel), expected);
    }

    #[test]
    fn selector_with_unquoted_attribute_matches_document() {
        let (mut store, element) =
            setup_html_store("<div><a href=\"/manga/1\">One</a><a href=\"/other\">Two</a></div>");
        let links = element
            .select_soup(&mut store, "a[href^=/manga]")
            .unwrap()
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].attr(&mut store, "href").unwrap(), "/manga/1");
    }

    #[test]
    fn attr_with_abs_prefix_resolves_against_base_uri() {
        let (mut store, element) = setup_html_store("<div><a href=\"/manga/1\">One</a></div>");
        let element = super::HTMLElement {
            base_uri: Some("https://example.com".to_owned()),
            ..element
        };
        let link = element.select_soup_first(&mut store, "a").unwrap().unwrap();
        assert_eq!(
            link.attr(&mut store, "abs:href").unwrap(),
            "https://example.com/manga/1"
        );
    }

    #[test]
    fn attr_abs_with_absolute_value_keeps_url() {
        let (mut store, element) =
            setup_html_store("<div><a href=\"https://cdn.example.com/manga/1.jpg\">One</a></div>");
        let element = super::HTMLElement {
            base_uri: Some("https://example.com".to_owned()),
            ..element
        };
        let link = element.select_soup_first(&mut store, "a").unwrap().unwrap();
        assert_eq!(
            link.attr(&mut store, "abs:href").unwrap(),
            "https://cdn.example.com/manga/1.jpg"
        );
    }

    #[test]
    fn attr_without_abs_prefix_returns_raw_value() {
        let (mut store, element) = setup_html_store("<div><a href=\"/manga/1\">One</a></div>");
        let element = super::HTMLElement {
            base_uri: Some("https://example.com".to_owned()),
            ..element
        };
        let link = element.select_soup_first(&mut store, "a").unwrap().unwrap();
        assert_eq!(link.attr(&mut store, "href").unwrap(), "/manga/1");
    }
}
