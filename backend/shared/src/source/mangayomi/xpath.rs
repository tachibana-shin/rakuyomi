//! Minimal XPath engine for MangaYomi extensions.
//!
//! A Rust port of the `xpath_selector` Dart package (v3.0.2) used by the
//! MangaYomi app, evaluated over `dom_query::NodeRef`. The behavior mirrors
//! the Dart implementation faithfully, including its quirks (e.g. `//x` is
//! implemented as "children of descendants", `text()` as a node test matches
//! elements and yields their concatenated text, sibling axes skip non-element
//! nodes).
//!
//! Only the subset of XPath that the mangayomi-extensions rely on is
//! supported: element/attribute/text node tests, the `ancestor*`, `child`,
//! `descendant*`, `following*`, `parent`, `preceding-sibling`, `self` and
//! `attribute` axes, numeric/attribute/function predicates and the
//! `contains`/`starts-with`/`ends-with` string functions. Anything else
//! produces an [`XPathError`].

use std::fmt;

use dom_query::NodeRef;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};

/// Errors produced while parsing or executing an XPath expression.
#[derive(Debug)]
pub struct XPathError {
    message: String,
}

impl fmt::Display for XPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for XPathError {}

impl XPathError {
    fn parse(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// The result of a query: the matched nodes plus one entry per node for the
/// extracted output (attribute value, function result or `None`), mirroring
/// `XPathResult.attrs` of the Dart package.
pub struct XPathResult<'a> {
    pub nodes: Vec<NodeRef<'a>>,
    pub attrs: Vec<Option<String>>,
}

impl XPathResult<'_> {
    /// The first non-`None` extracted value, mirroring `XPathResult.attr`.
    pub fn first_attr(&self) -> Option<&str> {
        self.attrs.iter().flatten().map(String::as_str).next()
    }
}

// ---------------------------------------------------------------------------
// Regular expressions (ported from `reg.dart`)
// ---------------------------------------------------------------------------

static XPATH_GROUP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"/{0,2}@?[\w\*-]+:{0,2}[\*\w]*(?:\[.+?\])*(?:\(\))?").unwrap());
static SIMPLE_POSITION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"position\(\s*\)\s*(?<op><|<=|>|>=)\s*(?<num>\d+)").unwrap());
static SIMPLE_LAST: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"last\(\s*\)\s*(?<op>\+|\-|\*|/%\^)\s*(?<num>\d+)").unwrap());
static SIMPLE_SINGLE_LAST: Lazy<Regex> = Lazy::new(|| Regex::new(r"last\(\s*\)").unwrap());
static PREDICATE_INT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?<num>\d+)$").unwrap());
static PREDICATE_CHILD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<child>\w+)\s*(?<op><|<=|>|>=)\s*(?<num>\d+)").unwrap());
static PREDICATE_EQUAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?<not>(?:not)?)\s*\(?\s*(?<function>@?[\w-]+\(?\s*\)?)\s*(?<op>=|~=|\|=|\^=|\$=|\*=|!=)\s*['"](?<value>.+?)['"]\)?"#,
    )
    .unwrap()
});
static FUNCTION_PREDICATE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?<not>(?:not)?)\s*\(?\s*(?<function>[\w-]{4,})\s*\(\s*(?<param1>.+?)\s*,\s*['"](?<param2>.+?)\s*['"]\s*\)\)?"#,
    )
    .unwrap()
});
static PREDICATE_REG: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[(?<predicate>.+?)\]").unwrap());
static FUNCTION_NODE_TEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\w*\(\s*\)$").unwrap());

// ---------------------------------------------------------------------------
// Selector model (ported from `selector.dart`)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum SelectorType {
    Descendant,
    Self_,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AxesAxis {
    Ancestor,
    AncestorOrSelf,
    Child,
    Descendant,
    DescendantOrSelf,
    Following,
    FollowingSibling,
    Parent,
    PrecedingSibling,
    Self_,
    Attribute,
}

/// One step of an XPath, e.g. `//*[contains(@class, "x")]`.
struct Selector {
    selector_type: SelectorType,
    axis: Option<AxesAxis>,
    node_test: String,
    predicate: Vec<String>,
    attr: Option<String>,
    function: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing (ported from `parser.dart`)
// ---------------------------------------------------------------------------

/// Splits an XPath on `|` and parses every alternative into selector steps.
fn parse_select_group(xpath: &str) -> Result<Vec<Vec<Selector>>, XPathError> {
    let mut groups = Vec::new();
    for path in xpath.split('|') {
        let path = path.trim();
        let mut selectors = Vec::new();
        for m in XPATH_GROUP.find_iter(path) {
            selectors.push(parse_selector(m.as_str().trim())?);
        }
        groups.push(selectors);
    }
    Ok(groups)
}

fn parse_selector(input: &str) -> Result<Selector, XPathError> {
    let (selector_type, source) = if let Some(rest) = input.strip_prefix("//") {
        (SelectorType::Descendant, rest)
    } else if let Some(rest) = input.strip_prefix('/') {
        (SelectorType::Self_, rest)
    } else {
        return Err(XPathError::parse(format!(
            "'{input}' is not a valid xpath query string"
        )));
    };

    if let Some(selector) = parse_simple_selector(selector_type, source) {
        return Ok(selector);
    }

    let (axis, without_axis) = if let Some(idx) = source.find("::") {
        let (name, rest) = source.split_at(idx);
        (
            Some(create_axis(name.trim())?),
            rest.strip_prefix("::")
                .unwrap_or_default()
                .trim()
                .to_string(),
        )
    } else {
        (None, source.to_string())
    };

    let mut node_test = without_axis.clone();
    let mut predicates = Vec::new();
    for cap in PREDICATE_REG.captures_iter(&without_axis) {
        let matched = cap.get(0).expect("full match").as_str();
        node_test = node_test.replace(matched, "");
        predicates.push(
            cap.name("predicate")
                .expect("predicate group")
                .as_str()
                .to_string(),
        );
    }

    let (axis, node_test) = match node_test.as_str() {
        "." => (Some(AxesAxis::Self_), "*".to_string()),
        ".." => (Some(AxesAxis::Parent), "*".to_string()),
        _ => (axis, node_test),
    };

    Ok(Selector {
        selector_type,
        axis,
        node_test,
        predicate: predicates,
        attr: None,
        function: None,
    })
}

fn parse_simple_selector(selector_type: SelectorType, source: &str) -> Option<Selector> {
    let base = Selector {
        selector_type,
        axis: None,
        node_test: String::new(),
        predicate: Vec::new(),
        attr: None,
        function: None,
    };
    if let Some(rest) = source.strip_prefix('@') {
        return Some(Selector {
            axis: Some(AxesAxis::Self_),
            node_test: "*".to_string(),
            attr: Some(rest.to_string()),
            ..base
        });
    }
    match source {
        ".." => Some(Selector {
            axis: Some(AxesAxis::Parent),
            node_test: "*".to_string(),
            ..base
        }),
        "." => Some(Selector {
            axis: Some(AxesAxis::Self_),
            node_test: "*".to_string(),
            ..base
        }),
        "node()" => Some(Selector {
            axis: Some(AxesAxis::Child),
            node_test: "node()".to_string(),
            ..base
        }),
        _ => {
            if FUNCTION_NODE_TEST.is_match(source) {
                Some(Selector {
                    axis: Some(AxesAxis::Self_),
                    node_test: "*".to_string(),
                    function: Some(source.to_string()),
                    ..base
                })
            } else {
                None
            }
        }
    }
}

fn create_axis(axis: &str) -> Result<AxesAxis, XPathError> {
    Ok(match axis {
        "ancestor" => AxesAxis::Ancestor,
        "ancestor-or-self" => AxesAxis::AncestorOrSelf,
        "child" => AxesAxis::Child,
        "descendant" => AxesAxis::Descendant,
        "descendant-or-self" => AxesAxis::DescendantOrSelf,
        "following" => AxesAxis::Following,
        "following-sibling" => AxesAxis::FollowingSibling,
        "parent" => AxesAxis::Parent,
        "preceding-sibling" => AxesAxis::PrecedingSibling,
        "self" => AxesAxis::Self_,
        "attribute" => AxesAxis::Attribute,
        _ => {
            return Err(XPathError::parse(format!("not support axis: {axis}")));
        }
    })
}

// ---------------------------------------------------------------------------
// Axis helpers (ported from `dom_selector.dart`)
// ---------------------------------------------------------------------------

/// The topmost ancestor of a node (the document root).
fn top<'a>(mut node: NodeRef<'a>) -> NodeRef<'a> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

/// All element ancestors (parent, grandparent, ...) of a node.
fn ancestor<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = Vec::new();
    let mut current = node;
    while let Some(parent) = current.parent() {
        current = parent;
        if current.is_element() {
            result.push(current);
        }
    }
    result
}

/// The node itself followed by its element ancestors.
fn ancestor_or_self<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = vec![node];
    result.extend(ancestor(node));
    result
}

/// All element descendants of a node (children, grandchildren, ...).
fn descendant<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = Vec::new();
    for child in element_children(node) {
        result.push(child);
        result.extend(descendant(child));
    }
    result
}

/// The node itself followed by its element descendants.
fn descent_or_self<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = vec![node];
    result.extend(descendant(node));
    result
}

/// Everything in the document after the closing tag of the current node.
fn following<'a>(root: NodeRef<'a>, node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = Vec::new();
    let mut found = false;
    fn dfs<'a>(
        parent: NodeRef<'a>,
        node: NodeRef<'a>,
        found: &mut bool,
        result: &mut Vec<NodeRef<'a>>,
    ) {
        for child in element_children(parent) {
            if child.id == node.id {
                *found = true;
            }
            if *found {
                result.push(child);
            }
            dfs(child, node, found, result);
        }
    }
    dfs(root, node, &mut found, &mut result);
    result
}

/// All element siblings after the current node.
fn following_sibling<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = Vec::new();
    let mut current = node;
    while let Some(sibling) = current.next_element_sibling() {
        result.push(sibling);
        current = sibling;
    }
    result
}

/// All element siblings before the current node.
fn preceding_sibling<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut result = Vec::new();
    let mut current = node;
    while let Some(sibling) = current.prev_element_sibling() {
        result.push(sibling);
        current = sibling;
    }
    result
}

/// The element children of a node (the Dart `html` package's `children`
/// only yields elements, text and comment nodes are skipped).
fn element_children<'a>(node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    node.children()
        .into_iter()
        .filter(|c| c.is_element())
        .collect()
}

fn add_if_not_exist<'a>(list: &mut Vec<NodeRef<'a>>, node: NodeRef<'a>) {
    if !list.iter().any(|n| n.id == node.id) {
        list.push(node);
    }
}

fn add_all_if_not_exist<'a>(list: &mut Vec<NodeRef<'a>>, nodes: Vec<NodeRef<'a>>) {
    for node in nodes {
        add_if_not_exist(list, node);
    }
}

// ---------------------------------------------------------------------------
// Execution (ported from `execute.dart`)
// ---------------------------------------------------------------------------

/// Evaluates every selector step against `element` and returns the matched
/// nodes in document order.
fn execute<'a>(
    selectors: &[Selector],
    element: NodeRef<'a>,
) -> Result<Vec<NodeRef<'a>>, XPathError> {
    let mut tmp = vec![element];
    for selector in selectors {
        let mut root_match: Vec<NodeRef<'a>> = Vec::new();
        for element in &tmp {
            let path_nodes = match_select_path(selector, *element);
            let mut selector_match: Vec<NodeRef<'a>> = Vec::new();
            for element in &path_nodes {
                let mut axis_nodes = match_axis(selector, *element);

                let mut remove_index: Vec<usize> = Vec::new();
                for (i, node) in axis_nodes.iter().enumerate() {
                    if !match_selector(selector, *node) {
                        remove_index.push(i);
                    }
                }
                for i in remove_index.iter().rev() {
                    axis_nodes.remove(*i);
                }

                for predicate in &selector.predicate {
                    remove_index.clear();
                    for (i, node) in axis_nodes.iter().enumerate() {
                        if !match_predicates(*node, i, axis_nodes.len(), predicate)? {
                            remove_index.push(i);
                        }
                    }
                    for i in remove_index.iter().rev() {
                        axis_nodes.remove(*i);
                    }
                }
                add_all_if_not_exist(&mut selector_match, axis_nodes);
            }
            add_all_if_not_exist(&mut root_match, selector_match);
        }
        tmp = root_match;
    }
    Ok(tmp)
}

fn match_select_path<'a>(selector: &Selector, node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    match selector.selector_type {
        SelectorType::Descendant => descent_or_self(node),
        SelectorType::Self_ => vec![node],
    }
}

fn match_axis<'a>(selector: &Selector, node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut waiting_select: Vec<NodeRef<'a>> = Vec::new();
    match selector.axis {
        Some(AxesAxis::Child) | None => {
            add_all_if_not_exist(&mut waiting_select, element_children(node));
        }
        Some(AxesAxis::Ancestor) => add_all_if_not_exist(&mut waiting_select, ancestor(node)),
        Some(AxesAxis::AncestorOrSelf) => {
            add_all_if_not_exist(&mut waiting_select, ancestor_or_self(node));
        }
        Some(AxesAxis::Descendant) => {
            add_all_if_not_exist(&mut waiting_select, descendant(node));
        }
        Some(AxesAxis::DescendantOrSelf) => {
            add_all_if_not_exist(&mut waiting_select, descent_or_self(node));
        }
        Some(AxesAxis::Following) => {
            add_all_if_not_exist(&mut waiting_select, following(top(node), node));
        }
        Some(AxesAxis::Parent) => {
            if let Some(parent) = node.parent() {
                waiting_select.push(parent);
            }
        }
        Some(AxesAxis::FollowingSibling) => {
            add_all_if_not_exist(&mut waiting_select, following_sibling(node));
        }
        Some(AxesAxis::PrecedingSibling) => {
            add_all_if_not_exist(&mut waiting_select, preceding_sibling(node));
        }
        Some(AxesAxis::Attribute) | Some(AxesAxis::Self_) => {
            add_if_not_exist(&mut waiting_select, node);
        }
    }
    waiting_select
}

fn match_selector(selector: &Selector, node: NodeRef<'_>) -> bool {
    if selector.attr.is_some() || selector.axis == Some(AxesAxis::Attribute) {
        return true;
    }
    let node_test = &selector.node_test;
    if node_test != "node()" {
        if !node.is_element() {
            return false;
        }
        if node_test != "*" {
            let Some(name) = node.node_name() else {
                return false;
            };
            if name.as_ref() != node_test {
                return false;
            }
        }
    }
    true
}

fn match_predicates(
    node: NodeRef<'_>,
    position: usize,
    length: usize,
    predicate: &str,
) -> Result<bool, XPathError> {
    let predicate = predicate
        .replace(" and ", " && ")
        .replace(" or ", " || ")
        .replace(" div ", " / ")
        .replace(" mod ", " % ");

    if predicate.contains(" && ") || predicate.contains(" || ") {
        return multiple_compare(&predicate, node, position);
    }
    if SIMPLE_SINGLE_LAST.is_match(&predicate) || PREDICATE_INT.is_match(&predicate) {
        return single_position(&predicate, position, length);
    }
    single_compare(&predicate, node, position)
}

fn single_position(predicate: &str, position: usize, length: usize) -> Result<bool, XPathError> {
    if let Some(cap) = SIMPLE_LAST.captures(predicate) {
        let num = num_of(&cap, "num");
        let op = cap.name("op").expect("op group").as_str();
        return Ok(op_num(length as i64, num, op)? == position as i64 + 1);
    }
    if SIMPLE_SINGLE_LAST.is_match(predicate) {
        return Ok(length == position + 1);
    }
    if let Some(cap) = PREDICATE_INT.captures(predicate) {
        let num = num_of(&cap, "num");
        return Ok(num == position as i64 + 1);
    }
    Err(XPathError::unsupported(format!(
        "Unsupported predicate: {predicate}"
    )))
}

fn multiple_compare(
    predicate: &str,
    node: NodeRef<'_>,
    position: usize,
) -> Result<bool, XPathError> {
    let mut expression = predicate.to_string();

    for cap in SIMPLE_POSITION.captures_iter(predicate) {
        let matched = cap.get(0).expect("full match").as_str();
        let result = position_match(position, &cap);
        expression = expression.replace(matched, if result { "true" } else { "false" });
    }
    for cap in PREDICATE_EQUAL.captures_iter(predicate) {
        let matched = cap.get(0).expect("full match").as_str();
        let result = equal_match(node, &cap)?;
        expression = expression.replace(matched, if result { "true" } else { "false" });
    }
    for cap in PREDICATE_CHILD.captures_iter(predicate) {
        let matched = cap.get(0).expect("full match").as_str();
        let result = child_match(node, &cap)?;
        expression = expression.replace(matched, if result { "true" } else { "false" });
    }
    for cap in FUNCTION_PREDICATE.captures_iter(predicate) {
        let matched = cap.get(0).expect("full match").as_str();
        let result = function_match(node, &cap)?;
        expression = expression.replace(matched, if result { "true" } else { "false" });
    }

    eval_boolean(&expression)
}

fn single_compare(predicate: &str, node: NodeRef<'_>, position: usize) -> Result<bool, XPathError> {
    if let Some(position_result) = SIMPLE_POSITION
        .captures(predicate)
        .map(|c| position_match(position, &c))
    {
        return Ok(position_result);
    }
    if let Some(equal_result) = PREDICATE_EQUAL
        .captures(predicate)
        .map(|c| equal_match(node, &c))
    {
        return equal_result;
    }
    if let Some(child_result) = PREDICATE_CHILD
        .captures(predicate)
        .map(|c| child_match(node, &c))
    {
        return child_result;
    }
    if let Some(function_result) = FUNCTION_PREDICATE
        .captures(predicate)
        .map(|c| function_match(node, &c))
    {
        return function_result;
    }
    Err(XPathError::unsupported(format!(
        "Unsupported predicate: {predicate}"
    )))
}

fn position_match(position: usize, cap: &Captures<'_>) -> bool {
    let op = cap.name("op").expect("op group").as_str();
    let num = num_of(cap, "num");
    op_compare(position as i64 + 1, num, op)
}

fn equal_match(node: NodeRef<'_>, cap: &Captures<'_>) -> Result<bool, XPathError> {
    let key = cap
        .name("function")
        .expect("function group")
        .as_str()
        .replace(' ', "");
    let right_value = cap.name("value").expect("value group").as_str();
    let op = cap.name("op").expect("op group").as_str();
    let not = cap
        .name("not")
        .map(|m| m.as_str() == "not")
        .unwrap_or(false);

    let left_value = element_function(node, &key)?;
    let Some(left_value) = left_value else {
        return Ok(false);
    };
    let result = op_string(&left_value, right_value, op)?;
    Ok(if not { !result } else { result })
}

fn child_match(node: NodeRef<'_>, cap: &Captures<'_>) -> Result<bool, XPathError> {
    let child_name = cap.name("child").expect("child group").as_str();
    let op = cap.name("op").expect("op group").as_str();
    let num = num_of(cap, "num");

    let child_value = element_children(node)
        .into_iter()
        .filter(|e| {
            e.node_name()
                .map(|n| n.as_ref() == child_name)
                .unwrap_or(false)
        })
        .filter_map(|e| e.text().parse::<i64>().ok())
        .next();
    let Some(child_value) = child_value else {
        return Ok(false);
    };
    Ok(op_compare(child_value, num, op))
}

fn function_match(node: NodeRef<'_>, cap: &Captures<'_>) -> Result<bool, XPathError> {
    let not = cap
        .name("not")
        .map(|m| m.as_str() == "not")
        .unwrap_or(false);
    let function = cap.name("function").expect("function group").as_str();
    let function = function.trim().to_lowercase();
    let param1 = cap.name("param1").expect("param1 group").as_str();
    let param1 = param1.trim().to_lowercase();
    let param2 = cap.name("param2").expect("param2 group").as_str();

    let left_value = element_function(node, &param1)?;
    let Some(left_value) = left_value else {
        return Ok(false);
    };
    let result = match function.as_str() {
        "contains" => left_value.contains(param2),
        "starts-with" => left_value.starts_with(param2),
        "ends-with" => left_value.ends_with(param2),
        _ => {
            return Err(XPathError::unsupported(format!("UnSupport {function}")));
        }
    };
    Ok(if not { !result } else { result })
}

/// Evaluates a string/node test function against a node. Returns `Ok(None)`
/// when the function has no value for the node (e.g. an absent attribute).
fn element_function(node: NodeRef<'_>, function: &str) -> Result<Option<String>, XPathError> {
    if let Some(attr) = function.strip_prefix('@') {
        return Ok(node.attr(attr).map(|v| v.to_string()));
    }
    Ok(match function {
        "text()" | "string()" => Some(node.text().to_string()),
        "name()" | "qualified()" => node.node_name().map(|v| v.to_string()),
        "local-name()" => node.node_name().map(|v| v.to_string()),
        // HTML has no namespaces.
        "namespace()" | "prefix()" => None,
        _ => {
            return Err(XPathError::unsupported(format!(
                "Unsupported function: {function}"
            )));
        }
    })
}

fn num_of(cap: &Captures<'_>, name: &str) -> i64 {
    cap.name(name)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

fn op_num(a: i64, b: i64, op: &str) -> Result<i64, XPathError> {
    Ok(match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" => a / b,
        "%" => a % b,
        _ => return Err(XPathError::unsupported(format!("Unknown operator: {op}"))),
    })
}

fn op_compare(a: i64, b: i64, op: &str) -> bool {
    match op {
        "<" => a < b,
        "<=" => a <= b,
        ">" => a > b,
        ">=" => a >= b,
        "==" | "=" => a == b,
        "!=" => a != b,
        _ => false,
    }
}

fn op_string(attr: &str, value: &str, op: &str) -> Result<bool, XPathError> {
    Ok(match op {
        "=" => attr == value,
        "!=" => attr != value,
        "~=" => attr.split(' ').any(|part| part == value),
        "*=" => attr.contains(value),
        "^=" => attr.starts_with(value),
        "$=" => attr.ends_with(value),
        _ => {
            return Err(XPathError::unsupported(format!("Unknown operator: {op}")));
        }
    })
}

// ---------------------------------------------------------------------------
// Boolean expression evaluation (`_multipleCompare` in `execute.dart`)
// ---------------------------------------------------------------------------

/// Evaluates a boolean expression made of `true`/`false`, `&&`, `||` and
/// parentheses (everything else was substituted before).
fn eval_boolean(expression: &str) -> Result<bool, XPathError> {
    let mut parser = BoolParser::new(expression);
    let result = parser.parse_or()?;
    if parser.peek().is_some() {
        return Err(XPathError::parse(format!(
            "Expression parse error, raw: {expression}"
        )));
    }
    Ok(result)
}

#[derive(Clone, Copy, PartialEq)]
enum BoolToken {
    True,
    False,
    And,
    Or,
    LParen,
    RParen,
}

struct BoolParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    lookahead: Option<(BoolToken, usize)>,
}

impl<'a> BoolParser<'a> {
    fn new(expression: &'a str) -> Self {
        Self {
            chars: expression.chars().peekable(),
            lookahead: None,
        }
    }

    fn peek(&mut self) -> Option<BoolToken> {
        if self.lookahead.is_none() {
            self.lookahead = self.next_token();
        }
        self.lookahead.map(|(t, _)| t)
    }

    fn consume(&mut self) -> Option<BoolToken> {
        self.peek()?;
        let (token, pos) = self.lookahead.take().expect("lookahead");
        for _ in 0..pos {
            self.chars.next();
        }
        Some(token)
    }

    fn next_token(&self) -> Option<(BoolToken, usize)> {
        let mut chars = self.chars.clone();
        let mut start = 0;
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
            start += 1;
        }
        let c = *chars.peek()?;
        let token = match c {
            '(' => BoolToken::LParen,
            ')' => BoolToken::RParen,
            '&' => {
                chars.next();
                if chars.peek() == Some(&'&') {
                    BoolToken::And
                } else {
                    return None;
                }
            }
            '|' => {
                chars.next();
                if chars.peek() == Some(&'|') {
                    BoolToken::Or
                } else {
                    return None;
                }
            }
            't' => {
                let s: String = chars.take(4).collect();
                if s == "true" {
                    BoolToken::True
                } else {
                    return None;
                }
            }
            'f' => {
                let s: String = chars.take(5).collect();
                if s == "false" {
                    BoolToken::False
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let width = match token {
            BoolToken::And | BoolToken::Or => 2,
            BoolToken::True => 4,
            BoolToken::False => 5,
            _ => 1,
        };
        Some((token, start + width))
    }

    fn parse_or(&mut self) -> Result<bool, XPathError> {
        let mut value = self.parse_and()?;
        while self.peek() == Some(BoolToken::Or) {
            self.consume();
            let rhs = self.parse_and()?;
            value = value || rhs;
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<bool, XPathError> {
        let mut value = self.parse_primary()?;
        while self.peek() == Some(BoolToken::And) {
            self.consume();
            let rhs = self.parse_primary()?;
            value = value && rhs;
        }
        Ok(value)
    }

    fn parse_primary(&mut self) -> Result<bool, XPathError> {
        match self.consume() {
            Some(BoolToken::True) => Ok(true),
            Some(BoolToken::False) => Ok(false),
            Some(BoolToken::LParen) => {
                let value = self.parse_or()?;
                if self.consume() != Some(BoolToken::RParen) {
                    return Err(XPathError::parse("unbalanced parentheses".to_string()));
                }
                Ok(value)
            }
            _ => Err(XPathError::parse("invalid boolean expression".to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Output extraction (ported from `builder.dart` + `parser.dart`)
// ---------------------------------------------------------------------------

/// Extracts the output values for the matched nodes: the attribute value for
/// `@attr`-terminated selectors, the function result (e.g. `text()`) for
/// function-terminated ones, otherwise nothing (mirroring `parseAttr`).
fn parse_attr(
    selectors: &[Selector],
    elements: &[NodeRef<'_>],
) -> Result<Vec<Option<String>>, XPathError> {
    let mut result = Vec::new();
    let Some(last) = selectors.last() else {
        return Ok(result);
    };
    for element in elements {
        if let Some(attr) = &last.attr {
            if attr == "*" {
                result.extend(
                    element
                        .attrs()
                        .into_iter()
                        .map(|a| Some(a.value.to_string())),
                );
            } else {
                result.push(element.attr(attr).map(|v| v.to_string()));
            }
        } else if let Some(function) = &last.function {
            result.push(element_function(*element, function)?);
        } else if last.axis == Some(AxesAxis::Attribute) {
            if last.node_test == "*" {
                result.extend(
                    element
                        .attrs()
                        .into_iter()
                        .map(|a| Some(a.value.to_string())),
                );
            } else {
                result.push(element.attr(&last.node_test).map(|v| v.to_string()));
            }
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluates an XPath expression with `root` as the context node and returns
/// the matched nodes plus the extracted output values.
pub fn query<'a>(root: NodeRef<'a>, expression: &str) -> Result<XPathResult<'a>, XPathError> {
    let groups = parse_select_group(expression)?;
    let mut result: Vec<NodeRef<'a>> = Vec::new();
    let mut result_attrs: Vec<Option<String>> = Vec::new();

    for selectors in &groups {
        let new_result = execute(selectors, root)?;
        let fresh: Vec<NodeRef<'a>> = new_result
            .into_iter()
            .filter(|node| !result.iter().any(|n| n.id == node.id))
            .collect();
        result.extend(fresh.iter().copied());
        result_attrs.extend(parse_attr(selectors, &fresh)?);
    }

    Ok(XPathResult {
        nodes: result,
        attrs: result_attrs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str = r#"<html><body>
<div class="manga-list-1-list">
<li><a title="Manga One" href="/manga-one"><img class="manga-list-1-cover" src="/cover1.jpg"></a></li>
<li><a title="Manga Two" href="/manga-two"><img class="manga-list-1-cover" src="/cover2.jpg"></a></li>
<li><a href="/manga-three"><img class="manga-list-1-cover" src="/cover3.jpg"></a></li>
</div>
<div class="fullcontent" id="desc">This is the description text.</div>
<div class="detail-info-right-say"><a>Author Name</a></div>
<div class="detail-info-right-tag-list"><a>Action</a><a>Adventure</a><a>Drama</a></div>
<div class="readerarea" id="readerarea"><p><img src="/page1.jpg"></p><p><img src="/page2.jpg"></p></div>
</body></html>"#;

    fn attrs(html: &str, expr: &str) -> Vec<Option<String>> {
        let doc = dom_query::Document::from(html);
        query(doc.html_root(), expr).unwrap().attrs
    }

    fn node_count(html: &str, expr: &str) -> usize {
        let doc = dom_query::Document::from(html);
        query(doc.html_root(), expr).unwrap().nodes.len()
    }

    #[test]
    fn contains_predicate_with_leading_space() {
        let titles = attrs(
            HTML,
            r#"//*[ contains(@class, "manga-list-1-list")]/li/a/@title"#,
        );
        assert_eq!(titles.len(), 3);
        assert_eq!(titles[0].as_deref(), Some("Manga One"));
        assert_eq!(titles[1].as_deref(), Some("Manga Two"));
        assert_eq!(titles[2], None);
    }

    #[test]
    fn hrefs_and_covers() {
        let urls = attrs(
            HTML,
            r#"//*[ contains(@class, "manga-list-1-list")]/li/a/@href"#,
        );
        assert_eq!(
            urls,
            vec![
                Some("/manga-one".to_string()),
                Some("/manga-two".to_string()),
                Some("/manga-three".to_string()),
            ]
        );
        let images = attrs(
            HTML,
            r#"//*[ contains(@class, "manga-list-1-list")]/li/a/img[@class="manga-list-1-cover"]/@src"#,
        );
        assert_eq!(
            images,
            vec![
                Some("/cover1.jpg".to_string()),
                Some("/cover2.jpg".to_string()),
                Some("/cover3.jpg".to_string()),
            ]
        );
    }

    #[test]
    fn text_function_yields_element_text() {
        let texts = attrs(HTML, r#"//*[@class="fullcontent"]/text()"#);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].as_deref(), Some("This is the description text."));
        assert_eq!(node_count(HTML, r#"//*[@class="fullcontent"]/text()"#), 1);

        let authors = attrs(HTML, r#"//*[@class="detail-info-right-say"]/a/text()"#);
        assert_eq!(authors, vec![Some("Author Name".to_string())]);

        let genres = attrs(HTML, r#"//*[@class="detail-info-right-tag-list"]/a/text()"#);
        assert_eq!(
            genres,
            vec![
                Some("Action".to_string()),
                Some("Adventure".to_string()),
                Some("Drama".to_string()),
            ]
        );
    }

    #[test]
    fn readerarea_pages() {
        let pages = attrs(HTML, r#"//*[@id="readerarea"]/p/img/@src"#);
        assert_eq!(
            pages,
            vec![
                Some("/page1.jpg".to_string()),
                Some("/page2.jpg".to_string())
            ]
        );
    }

    #[test]
    fn long_absolute_path() {
        let html =
            r#"<html><body><div><div><span><a>Deep Link</a></span></div></div></body></html>"#;
        let doc = dom_query::Document::from(html);
        let result = query(doc.html_root(), "//body/div/div/span/a/text()").unwrap();
        assert_eq!(result.attrs, vec![Some("Deep Link".to_string())]);
    }

    #[test]
    fn numeric_predicates() {
        assert_eq!(
            attrs(HTML, r#"//div[1]/@class"#),
            vec![Some("manga-list-1-list".to_string())]
        );
        assert_eq!(
            attrs(HTML, r#"//div[2]/@class"#),
            vec![Some("fullcontent".to_string())]
        );
        assert_eq!(
            attrs(HTML, r#"//div[last()]/@id"#),
            vec![Some("readerarea".to_string())]
        );
        assert_eq!(
            attrs(HTML, r#"//div[last()-1]/@class"#),
            vec![Some("detail-info-right-tag-list".to_string())]
        );
        assert_eq!(
            attrs(HTML, r#"//div[position()<2]/@class"#),
            vec![Some("manga-list-1-list".to_string())]
        );
        assert_eq!(
            attrs(HTML, r#"//div[position()<=2]/@class"#),
            vec![
                Some("manga-list-1-list".to_string()),
                Some("fullcontent".to_string()),
            ]
        );
        assert_eq!(
            attrs(HTML, r#"//li[2]/a/@title"#),
            vec![Some("Manga Two".to_string())]
        );
    }

    #[test]
    fn boolean_predicates() {
        let ids = attrs(
            HTML,
            r#"//div[@class="readerarea" or @id="readerarea"]/@id"#,
        );
        assert_eq!(ids, vec![Some("readerarea".to_string())]);

        let texts = attrs(HTML, r#"//div[@class="fullcontent" and @id="desc"]/text()"#);
        assert_eq!(
            texts,
            vec![Some("This is the description text.".to_string())]
        );
    }

    #[test]
    fn not_equal_and_contains_predicates() {
        let titles = attrs(
            HTML,
            r#"//*[ contains(@class, "manga-list-1-list")]/li/a[@title!="Manga One"]/@title"#,
        );
        assert_eq!(titles, vec![Some("Manga Two".to_string())]);
        assert_eq!(
            attrs(
                HTML,
                r#"//*[@class="manga-list-1-list"]/li/a[not(@title="Manga One")]/@title"#
            ),
            vec![Some("Manga Two".to_string())]
        );
        assert_eq!(
            attrs(HTML, r#"//*[ starts-with(@class, "detail")]/@class"#),
            vec![
                Some("detail-info-right-say".to_string()),
                Some("detail-info-right-tag-list".to_string()),
            ]
        );
        assert_eq!(
            attrs(HTML, r#"//*[ ends-with(@class, "say")]/@class"#),
            vec![Some("detail-info-right-say".to_string())]
        );
    }

    #[test]
    fn node_test_children() {
        assert_eq!(
            node_count(HTML, r#"//*[@class="detail-info-right-tag-list"]/node()"#),
            3
        );
        assert_eq!(node_count(HTML, r#"//div/@class"#), 5);
    }

    #[test]
    fn first_attr() {
        let doc = dom_query::Document::from(HTML);
        let result = query(doc.html_root(), r#"//*[@class="fullcontent"]/text()"#).unwrap();
        assert_eq!(result.first_attr(), Some("This is the description text."));
        let empty = query(doc.html_root(), r#"//*[@class="missing"]/@href"#).unwrap();
        assert_eq!(empty.first_attr(), None);
    }

    #[test]
    fn unsupported_predicate_is_error() {
        let doc = dom_query::Document::from(HTML);
        assert!(query(doc.html_root(), r#"//a[foo]"#).is_err());
        assert!(query(doc.html_root(), r#"//a[ not-a-function() ]"#).is_err());
    }

    #[test]
    fn invalid_expression_is_error() {
        let doc = dom_query::Document::from(HTML);
        assert!(query(doc.html_root(), "a[1]").is_err());
        assert!(query(doc.html_root(), "descendant::bogus/x").is_err());
    }
}
