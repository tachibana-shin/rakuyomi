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

        Some(NodeRef::new(self.node_id, &document.tree).into())
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

        let matcher = Matcher::new(selector).map_err(|err| anyhow!("[{selector}]{:?}", err))?;
        let mut elements = if matcher.match_element(&node) {
            vec![self.clone()]
        } else {
            vec![]
        };

        elements.extend(
            Selection::from(node)
                .select_matcher(&matcher)
                .nodes()
                .into_iter()
                .map(|node| self.to_element(node.id))
                .collect::<Vec<_>>(),
        );

        Ok(Some(elements))
    }
    pub fn select_soup_first(&self, store: &mut WasmStore, selector: &str) -> Result<Option<Self>> {
        let Some(node) = self.node_ref(store) else {
            return Ok(None);
        };

        let matcher = Matcher::new(selector).map_err(|err| anyhow!("[{selector}]{:?}", err))?;
        if matcher.match_element(&node) {
            return Ok(Some(self.clone()));
        }

        Ok(Selection::from(node)
            .select_single_matcher(&matcher)
            .nodes()
            .first()
            .map(|node| self.to_element(node.id)))
    }
    pub fn attr(&self, store: &mut WasmStore, name: &str) -> Option<String> {
        let node = self.node_ref(store)?;

        node.attr(name).map(|v| v.to_string())
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
            .into_iter()
            .map(|node| self.to_element(node.id))
            .collect::<Vec<_>>()
            .into()
    }

    pub fn text(&self, store: &mut WasmStore) -> Option<String> {
        self.untrimmed_text(store).map(|v| v.trim().to_owned())
    }

    pub fn untrimmed_text(&self, store: &mut WasmStore) -> Option<String> {
        let node = self.node_ref(store)?;

        node.text().to_string().into()
    }

    pub fn own_text(&self, store: &mut WasmStore) -> Option<String> {
        self.data(store)
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
