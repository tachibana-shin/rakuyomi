//! MangaYomi DOM bridge: `MDocument`/`MElement` backed by [`dom_query`].
//! I wrote this based on my understanding of how the `pub/html` package
//! behaves in Dart. I have experience working with it, as well as with SOUP
//! and W3C standards, but I didn’t anticipate the differences in how they
//! handle things.
//!
//! Native values are plain maps: a document is `{"doc": <index>}` and an
//! element is `{"doc": <index>, "node": <handle>}`. Documents are kept alive
//! (append-only) inside [`MangaYomiDom`], and every element id handed out to
//! the interpreter is registered in a per-document handle table, so handles
//! stay stable for the lifetime of the bridge. This is required because
//! extensions hold on to element references across calls (e.g.
//! `el.selectFirst("a")` on an element taken from an earlier `select`).
//!
//! `dom_query::NodeId` is opaque (its inner value is `pub(crate)`), so raw
//! node ids are never serialized across the bridge; only our own handle
//! indices are.

use std::cell::RefCell;

use dom_query::{Document, Matcher, NodeId, NodeRef, Selection};

use d4rt_rs::Value;

use super::bridge::map_int;

/// Owning store for parsed documents.
#[derive(Default)]
pub(crate) struct MangaYomiDom {
    pub documents: Vec<Document>,
    /// Per-document handle tables: `handles[doc][handle] = NodeId`.
    handles: RefCell<Vec<Vec<NodeId>>>,
}

impl MangaYomiDom {
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            handles: RefCell::new(Vec::new()),
        }
    }

    /// Parses HTML and returns the stable document id.
    pub fn parse(&mut self, html: &str) -> usize {
        self.documents.push(Document::from(html));
        let root = self.documents.last().unwrap().root().id;
        self.handles.borrow_mut().push(vec![root]);
        self.documents.len() - 1
    }

    /// Registers a node id and returns its stable handle. Every node exposed
    /// to the interpreter must go through this (or [`Self::parse`], which
    /// registers the root as handle 0).
    pub fn store_id(&self, doc_id: usize, node_id: NodeId) -> usize {
        let mut handles = self.handles.borrow_mut();
        if doc_id >= handles.len() {
            handles.resize(doc_id + 1, Vec::new());
        }
        handles[doc_id].push(node_id);
        handles[doc_id].len() - 1
    }

    pub fn node(&self, doc_id: usize, handle: usize) -> Option<NodeRef<'_>> {
        let node_id = *self.handles.borrow().get(doc_id)?.get(handle)?;
        let document = self.documents.get(doc_id)?;
        document.tree.get(&node_id)
    }

    /// Resolves a native `{"doc": .., "node": ..}` map to a node ref.
    pub fn node_of(&self, native: &Value) -> Option<(usize, usize, NodeRef<'_>)> {
        let doc_id = map_int(native, "doc")? as usize;
        let handle = map_int(native, "node")? as usize;
        let node = self.node(doc_id, handle)?;
        Some((doc_id, handle, node))
    }

    /// The document root element handle (the `<html>` element). `parse`
    /// registers the root as handle 0, so this is constant per document.
    pub fn root(&self, doc_id: usize) -> Option<usize> {
        self.documents.get(doc_id)?;
        Some(0)
    }

    /// Selects elements matching `selector` inside `node` (including `node`
    /// itself, mirroring MangaYomi's `MDocument.select`).
    pub fn select(&self, node: NodeRef<'_>, selector: &str) -> Vec<NodeId> {
        let Ok(matcher) = Matcher::new(selector) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if matcher.match_element(&node) {
            out.push(node.id);
        }
        out.extend(
            Selection::from(node)
                .select_matcher(&matcher)
                .nodes()
                .iter()
                .map(|n| n.id),
        );
        out
    }

    /// Selects the first element matching `selector` (MangaYomi's
    /// `selectFirst` returns null when the node itself matches).
    pub fn select_first(&self, node: NodeRef<'_>, selector: &str) -> Option<NodeId> {
        let Ok(matcher) = Matcher::new(selector) else {
            return None;
        };
        Selection::from(node)
            .select_single_matcher(&matcher)
            .nodes()
            .first()
            .map(|n| n.id)
    }
}

/// The regex MangaYomi uses for `getHref`/`getSrc`/`getDataSrc`/`getImg`
/// fallbacks (`<attr>="..."` on the outer HTML).
pub(crate) fn attr_regex(outer_html: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = outer_html.find(&needle)?;
    let rest = &outer_html[start + needle.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Convenience accessor used by the bridge: attribute lookup with a fallback
/// to the outer-HTML regex, mirroring the `html` package behavior MangaYomi
/// relies on.
pub(crate) fn element_attr(doc: &MangaYomiDom, native: &Value, name: &str) -> Option<String> {
    let (_, _, node) = doc.node_of(native)?;
    if let Some(v) = node.attr(name) {
        return Some(v.to_string());
    }
    let outer = node.html().to_string();
    attr_regex(&outer, name)
}
