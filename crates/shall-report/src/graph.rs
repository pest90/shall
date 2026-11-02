//! The Green Graph: for every requirement at the top of a chain, the
//! graph of everything below it, laid out from left to right as an SVG.
//!
//! The layout follows the steps of Sugiyama's method, kept simple:
//!
//! 1. The graph of a root holds the requirements covering it, directly or
//!    further down, each once. Code and test tags are bundled per item and
//!    type into one count node, e.g. `utest 2/3` with the worst light of
//!    its tags, and every missing type becomes a dashed placeholder node.
//!    Edges that close a coverage cycle are left out; the cycle is red.
//!    A requirement drawn in an earlier graph is a single node with its
//!    light and a link, without what is below it, so that the drawings
//!    grow with the number of items, not with roots times shared parts.
//! 2. Every node is put into the column of its longest path from the
//!    root, so that every edge points to the right.
//! 3. A few sweeps of the barycenter heuristic order the nodes of each
//!    column by the mean position of their neighbours, to reduce
//!    crossings.
//! 4. Nodes get fixed sizes and positions, edges are cubic curves; an
//!    edge that spans several columns is drawn directly, without the
//!    dummy nodes of the full method.
//!
//! Every node shows its light as a sign and a word, not only as a colour,
//! and links to the requirement's details.

use std::collections::HashMap;

use serde::Serialize;
use shall_core::export::{Document, ItemEntry};
use shall_core::item::Origin;
use shall_core::verify::Light;
use shall_core::{ItemId, Location};

use crate::Index;

/// Width and height of a node, and the space between columns and rows.
const NODE_WIDTH: u32 = 220;
const NODE_HEIGHT: u32 = 44;
const COLUMN_GAP: u32 = 48;
const ROW_GAP: u32 = 12;
/// Space around the drawing.
const MARGIN: u32 = 4;
/// Longest text of a node line, in characters.
const MAX_TEXT: usize = 28;
/// Sweeps of the barycenter heuristic.
const SWEEPS: usize = 4;

/// The graph of one root, ready to draw.
#[derive(Debug, Serialize)]
pub(crate) struct GraphView {
    /// The root, for the summary above the drawing.
    pub(crate) anchor: Option<String>,
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) light: &'static str,
    pub(crate) sign: &'static str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) nodes: Vec<NodeView>,
    pub(crate) edges: Vec<EdgeView>,
}

/// A node of the drawing.
#[derive(Debug, Serialize)]
pub(crate) struct NodeView {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// The light's word, which is also the CSS class, or `missing` for a
    /// placeholder.
    class: &'static str,
    sign: &'static str,
    /// The first and the second line of text, shortened.
    first: String,
    second: String,
    /// The full text, for the tooltip.
    tooltip: String,
    /// Fragment of the requirement's details.
    anchor: Option<String>,
}

/// An edge of the drawing, from an item to an item covering it.
#[derive(Debug, Serialize)]
pub(crate) struct EdgeView {
    /// The SVG path.
    path: String,
    /// Whether it leads to a placeholder.
    dashed: bool,
}

/// What a node stands for.
#[derive(Debug, Clone)]
enum Kind<'a> {
    Requirement(&'a ItemEntry<'a>),
    /// A requirement drawn with what is below it in an earlier graph, and
    /// the root of that graph.
    Elsewhere(&'a ItemEntry<'a>, String),
    /// Tags of one type covering one item: their type, how many, how many
    /// passed (for tests), and their worst light.
    Tags {
        typ: String,
        count: usize,
        passed: Option<usize>,
        light: Light,
    },
    /// A type the item needs but nothing of it covers it.
    Missing(String),
}

/// The graphs of all roots, in document order: the same roots as the tree,
/// requirements that cover no other item, and then those no root
/// reaches.
pub(crate) fn graphs<'a>(document: &'a Document<'_>, index: &Index<'a, '_>) -> Vec<GraphView> {
    let requirements = document
        .items
        .iter()
        .filter(|item| matches!(item.origin, Origin::Requirement));
    let mut reached = HashMap::new();
    let mut graphs = Vec::new();
    for root in top(document) {
        graphs.push(graph(document, index, root, &mut reached));
    }
    for rest in requirements {
        if !reached.contains_key(&(rest.location, rest.id)) {
            graphs.push(graph(document, index, rest, &mut reached));
        }
    }
    graphs
}

/// The requirements at the top of a chain: those that cover no other item
/// with a valid link, in document order.
pub(crate) fn top<'a>(document: &'a Document<'_>) -> Vec<&'a ItemEntry<'a>> {
    let covering: std::collections::HashSet<(&Location, &ItemId)> = document
        .items
        .iter()
        .flat_map(|item| item.covered_by)
        .map(|source| (&source.location, &source.id))
        .collect();
    document
        .items
        .iter()
        .filter(|item| {
            matches!(item.origin, Origin::Requirement)
                && !covering.contains(&(item.location, item.id))
        })
        .collect()
}

/// The graph below `root`; records its requirements in `reached`, with
/// the root that draws them in full.
fn graph<'a>(
    document: &'a Document<'_>,
    index: &Index<'a, '_>,
    root: &'a ItemEntry<'a>,
    reached: &mut HashMap<(&'a Location, &'a ItemId), String>,
) -> GraphView {
    // Nodes and edges (parent, child), by depth-first search without
    // recursion; an edge to a node on the stack closes a cycle and is
    // left out.
    let mut nodes: Vec<Kind> = vec![Kind::Requirement(root)];
    let mut position: HashMap<(&Location, &ItemId), usize> = HashMap::new();
    position.insert((root.location, root.id), 0);
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut on_stack = vec![true];
    let mut stack: Vec<(usize, std::vec::IntoIter<&ItemEntry>)> = Vec::new();
    stack.push((
        0,
        expand(
            document,
            index,
            root,
            &mut nodes,
            &mut edges,
            0,
            &mut on_stack,
        ),
    ));
    while let Some((node, children)) = stack.last_mut() {
        let node = *node;
        match children.next() {
            Some(child) => {
                let key = (child.location, child.id);
                if let Some(&known) = position.get(&key) {
                    if !on_stack[known] {
                        edges.push((node, known));
                    }
                    continue;
                }
                if let Some(root) = reached.get(&key) {
                    let new = nodes.len();
                    nodes.push(Kind::Elsewhere(child, root.clone()));
                    on_stack.push(false);
                    position.insert(key, new);
                    edges.push((node, new));
                    continue;
                }
                let new = nodes.len();
                nodes.push(Kind::Requirement(child));
                on_stack.push(true);
                position.insert(key, new);
                edges.push((node, new));
                let grandchildren = expand(
                    document,
                    index,
                    child,
                    &mut nodes,
                    &mut edges,
                    new,
                    &mut on_stack,
                );
                stack.push((new, grandchildren));
            }
            None => {
                on_stack[node] = false;
                stack.pop();
            }
        }
    }
    for (key, &node) in &position {
        if matches!(nodes[node], Kind::Requirement(_)) {
            reached.insert(*key, root.id.to_string());
        }
    }
    let columns = columns(nodes.len(), &edges);
    let order = order(&columns, &edges);
    draw(index, root, &nodes, &edges, &columns, &order)
}

/// Adds the tag groups and placeholders of `item` as children of `node`
/// and returns the requirements covering it.
fn expand<'a>(
    document: &'a Document<'_>,
    index: &Index<'a, '_>,
    item: &'a ItemEntry<'a>,
    nodes: &mut Vec<Kind<'a>>,
    edges: &mut Vec<(usize, usize)>,
    node: usize,
    on_stack: &mut Vec<bool>,
) -> std::vec::IntoIter<&'a ItemEntry<'a>> {
    let mut requirements = Vec::new();
    // Tag groups by type, in the order of their first tag.
    let mut groups: Vec<(String, Vec<&ItemEntry>)> = Vec::new();
    for source in item.covered_by {
        let Some(entry) = index.source_of(source, item) else {
            continue;
        };
        match entry.origin {
            Origin::Requirement => requirements.push(entry),
            Origin::Tag { .. } => {
                let typ = entry.typ.to_string();
                match groups.iter_mut().find(|(group, _)| *group == typ) {
                    Some((_, tags)) => tags.push(entry),
                    None => groups.push((typ, vec![entry])),
                }
            }
        }
    }
    for (typ, tags) in groups {
        let test = document
            .verification_types
            .iter()
            .any(|t| t.as_str() == typ);
        let passed = test.then(|| {
            tags.iter()
                .filter(|tag| {
                    tag.test
                        .as_ref()
                        .is_some_and(|test| test.verdict == shall_core::results::Verdict::Passed)
                })
                .count()
        });
        let light = tags
            .iter()
            .map(|tag| tag.light)
            .max()
            .unwrap_or(Light::Green);
        add_leaf(
            nodes,
            edges,
            on_stack,
            node,
            Kind::Tags {
                typ,
                count: tags.len(),
                passed,
                light,
            },
        );
    }
    for typ in item.missing {
        add_leaf(nodes, edges, on_stack, node, Kind::Missing(typ.to_string()));
    }
    requirements.into_iter()
}

fn add_leaf<'a>(
    nodes: &mut Vec<Kind<'a>>,
    edges: &mut Vec<(usize, usize)>,
    on_stack: &mut Vec<bool>,
    parent: usize,
    kind: Kind<'a>,
) {
    let new = nodes.len();
    nodes.push(kind);
    on_stack.push(false);
    edges.push((parent, new));
}

/// The column of every node: its longest path from the root, over edges
/// that form no cycle.
fn columns(count: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut parents = vec![0usize; count];
    for &(parent, child) in edges {
        children[parent].push(child);
        parents[child] += 1;
    }
    // Kahn's algorithm: a node gets its column when all its parents have
    // theirs.
    let mut column = vec![0; count];
    let mut ready: Vec<usize> = (0..count).filter(|&node| parents[node] == 0).collect();
    while let Some(node) = ready.pop() {
        for &child in &children[node] {
            column[child] = column[child].max(column[node] + 1);
            parents[child] -= 1;
            if parents[child] == 0 {
                ready.push(child);
            }
        }
    }
    column
}

/// The nodes of every column, ordered to reduce crossings: first in the
/// order they were found, then by the barycenter of their neighbours in
/// the column before (sweeping right) or after (sweeping left).
fn order(columns: &[usize], edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let count = columns.iter().max().map_or(0, |&last| last + 1);
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (node, &column) in columns.iter().enumerate() {
        order[column].push(node);
    }
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); columns.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); columns.len()];
    for &(parent, child) in edges {
        parents[child].push(parent);
        children[parent].push(child);
    }
    let mut rank = vec![0.0f64; columns.len()];
    let set_ranks = |order: &[Vec<usize>], rank: &mut [f64]| {
        for nodes in order {
            for (index, &node) in nodes.iter().enumerate() {
                rank[node] = index as f64;
            }
        }
    };
    set_ranks(&order, &mut rank);
    for sweep in 0..SWEEPS {
        let right = sweep % 2 == 0;
        let columns: Vec<usize> = if right {
            (1..count).collect()
        } else {
            (0..count.saturating_sub(1)).rev().collect()
        };
        for column in columns {
            let neighbours = if right { &parents } else { &children };
            let mut keyed: Vec<(f64, usize)> = order[column]
                .iter()
                .map(|&node| {
                    let near = &neighbours[node];
                    let key = if near.is_empty() {
                        rank[node]
                    } else {
                        near.iter().map(|&n| rank[n]).sum::<f64>() / near.len() as f64
                    };
                    (key, node)
                })
                .collect();
            // Stable, so that ties keep their order.
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            order[column] = keyed.into_iter().map(|(_, node)| node).collect();
            for (index, &node) in order[column].iter().enumerate() {
                rank[node] = index as f64;
            }
        }
    }
    order
}

fn draw(
    index: &Index<'_, '_>,
    root: &ItemEntry<'_>,
    nodes: &[Kind<'_>],
    edges: &[(usize, usize)],
    columns: &[usize],
    order: &[Vec<usize>],
) -> GraphView {
    let mut row = vec![0usize; nodes.len()];
    for nodes in order {
        for (index, &node) in nodes.iter().enumerate() {
            row[node] = index;
        }
    }
    let x = |node: usize| offset(columns[node], NODE_WIDTH + COLUMN_GAP);
    let y = |node: usize| offset(row[node], NODE_HEIGHT + ROW_GAP);
    let views = nodes
        .iter()
        .enumerate()
        .map(|(node, kind)| {
            let (class, sign, first, second, tooltip, anchor) = match kind {
                Kind::Requirement(item) => {
                    let id = item.id.to_string();
                    let tooltip = if item.title.is_empty() {
                        format!("{id}: {}", item.light.as_str())
                    } else {
                        format!("{id} {}: {}", item.title, item.light.as_str())
                    };
                    (
                        item.light.as_str(),
                        item.light.sign(),
                        shorten(&id),
                        shorten(item.title),
                        tooltip,
                        index.anchors.get(&(item.location, item.id)).cloned(),
                    )
                }
                Kind::Elsewhere(item, root) => {
                    let id = item.id.to_string();
                    let tooltip = format!(
                        "{id} {}: {}, drawn in full in the graph of {root}",
                        item.title,
                        item.light.as_str()
                    );
                    (
                        item.light.as_str(),
                        item.light.sign(),
                        shorten(&id),
                        "↗ shown above".to_owned(),
                        tooltip,
                        index.anchors.get(&(item.location, item.id)).cloned(),
                    )
                }
                Kind::Tags {
                    typ,
                    count,
                    passed,
                    light,
                } => {
                    let text = match passed {
                        Some(passed) => format!("{typ} {passed}/{count}"),
                        None => format!("{typ} ×{count}"),
                    };
                    let tooltip = format!("{text}: {}", light.as_str());
                    (
                        light.as_str(),
                        light.sign(),
                        text,
                        String::new(),
                        tooltip,
                        None,
                    )
                }
                Kind::Missing(typ) => (
                    "missing",
                    "✗",
                    format!("missing: {typ}"),
                    String::new(),
                    format!("missing: {typ}"),
                    None,
                ),
            };
            NodeView {
                x: x(node),
                y: y(node),
                width: NODE_WIDTH,
                height: NODE_HEIGHT,
                class,
                sign,
                first,
                second,
                tooltip,
                anchor,
            }
        })
        .collect();
    let edges = edges
        .iter()
        .map(|&(parent, child)| {
            let (x1, y1) = (
                x(parent).saturating_add(NODE_WIDTH),
                y(parent).saturating_add(NODE_HEIGHT / 2),
            );
            let (x2, y2) = (x(child), y(child).saturating_add(NODE_HEIGHT / 2));
            let middle = x1 / 2 + x2 / 2;
            EdgeView {
                path: format!("M{x1},{y1} C{middle},{y1} {middle},{y2} {x2},{y2}"),
                dashed: matches!(nodes[child], Kind::Missing(_)),
            }
        })
        .collect();
    let rows = order.iter().map(Vec::len).max().unwrap_or(1);
    let width = offset(order.len(), NODE_WIDTH + COLUMN_GAP) - COLUMN_GAP + MARGIN;
    let height = offset(rows, NODE_HEIGHT + ROW_GAP) - ROW_GAP + MARGIN;
    GraphView {
        anchor: index.anchors.get(&(root.location, root.id)).cloned(),
        id: root.id.to_string(),
        title: root.title.to_owned(),
        light: root.light.as_str(),
        sign: root.light.sign(),
        width,
        height,
        nodes: views,
        edges,
    }
}

/// The margin plus `index` steps of `step`, saturating instead of
/// overflowing for absurdly large drawings.
fn offset(index: usize, step: u32) -> u32 {
    u32::try_from(index)
        .unwrap_or(u32::MAX)
        .saturating_mul(step)
        .saturating_add(MARGIN)
}

/// The text shortened to [`MAX_TEXT`] characters, with `…` if cut.
fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        text.to_owned()
    } else {
        let mut short: String = text.chars().take(MAX_TEXT - 1).collect();
        short.push('…');
        short
    }
}
