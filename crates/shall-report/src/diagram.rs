//! The traced project as a diagram for other tools, as Graphviz DOT and as
//! a Mermaid flowchart, and the badge for a README.
//!
//! The diagrams hold every requirement once, with an edge to every
//! requirement covering it, its code and test tags bundled per type into
//! one node such as `impl ×2` or `utest 1/2`, and a node for every
//! missing type, like the Green Graph. Every node carries the light of its
//! chain as a sign and a class (`green`, `yellow`, `red`, `grey`,
//! `missing`). Node names are `n0`, `n1`, … in the order of the items.

use std::fmt::Write;

use shall_core::export::{Document, ItemEntry};
use shall_core::item::Origin;
use shall_core::results::Verdict;
use shall_core::verify::Light;

use crate::Index;

/// A node of the diagram.
struct Node {
    label: String,
    class: &'static str,
    sign: &'static str,
}

/// The nodes and edges of the whole project.
fn diagram(document: &Document<'_>, index: &Index<'_, '_>) -> (Vec<Node>, Vec<(usize, usize)>) {
    let mut nodes = Vec::new();
    let mut position = std::collections::HashMap::new();
    let requirements: Vec<&ItemEntry> = document
        .items
        .iter()
        .filter(|item| matches!(item.origin, Origin::Requirement))
        .collect();
    for item in &requirements {
        position.insert((item.location, item.id), nodes.len());
        let label = if item.title.is_empty() {
            item.id.to_string()
        } else {
            format!("{}\n{}", item.id, item.title)
        };
        nodes.push(Node {
            label,
            class: item.light.as_str(),
            sign: item.light.sign(),
        });
    }
    let mut edges = Vec::new();
    for item in &requirements {
        let from = position[&(item.location, item.id)];
        let mut groups: Vec<(String, Vec<&ItemEntry>)> = Vec::new();
        for source in item.covered_by {
            let Some(entry) = index.source_of(source, item) else {
                continue;
            };
            match entry.origin {
                Origin::Requirement => {
                    if let Some(&to) = position.get(&(entry.location, entry.id)) {
                        edges.push((from, to));
                    }
                }
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
            let label = if test {
                let passed = tags
                    .iter()
                    .filter(|tag| {
                        tag.test
                            .as_ref()
                            .is_some_and(|test| test.verdict == Verdict::Passed)
                    })
                    .count();
                format!("{typ} {passed}/{}", tags.len())
            } else {
                format!("{typ} ×{}", tags.len())
            };
            let light = tags
                .iter()
                .map(|tag| tag.light)
                .max()
                .unwrap_or(Light::Green);
            edges.push((from, nodes.len()));
            nodes.push(Node {
                label,
                class: light.as_str(),
                sign: light.sign(),
            });
        }
        for typ in item.missing {
            edges.push((from, nodes.len()));
            nodes.push(Node {
                label: format!("missing: {typ}"),
                class: "missing",
                sign: "✗",
            });
        }
    }
    (nodes, edges)
}

/// The project as a Graphviz DOT graph, from left to right.
pub fn dot(document: &Document<'_>) -> String {
    let index = Index::new(document);
    let (nodes, edges) = diagram(document, &index);
    let mut out = String::from(
        "digraph shall {\n  rankdir=LR;\n  node [shape=box, style=\"rounded,filled\", fontname=\"sans-serif\"];\n",
    );
    for (number, node) in nodes.iter().enumerate() {
        let (fill, stroke) = colours(node.class);
        let dashed = if node.class == "missing" {
            ",dashed"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  n{number} [label=\"{} {}\", class=\"{}\", fillcolor=\"{fill}\", color=\"{stroke}\", style=\"rounded,filled{dashed}\"];",
            node.sign,
            dot_text(&node.label),
            node.class
        );
    }
    for (from, to) in edges {
        let style = if nodes[to].class == "missing" {
            " [style=dashed]"
        } else {
            ""
        };
        let _ = writeln!(out, "  n{from} -> n{to}{style};");
    }
    out.push_str("}\n");
    out
}

/// The project as a Mermaid flowchart, from left to right.
pub fn mermaid(document: &Document<'_>) -> String {
    let index = Index::new(document);
    let (nodes, edges) = diagram(document, &index);
    let mut out = String::from("flowchart LR\n");
    for (number, node) in nodes.iter().enumerate() {
        let _ = writeln!(
            out,
            "  n{number}[\"{} {}\"]:::{}",
            node.sign,
            mermaid_text(&node.label),
            node.class
        );
    }
    for (from, to) in edges {
        let arrow = if nodes[to].class == "missing" {
            "-.->"
        } else {
            "-->"
        };
        let _ = writeln!(out, "  n{from} {arrow} n{to}");
    }
    for class in ["green", "yellow", "red", "grey", "missing"] {
        let (fill, stroke) = colours(class);
        let dash = if class == "missing" {
            ",stroke-dasharray:4 3"
        } else {
            ""
        };
        let _ = writeln!(out, "  classDef {class} fill:{fill},stroke:{stroke}{dash}");
    }
    out
}

/// Fill and stroke colours of a class, those of the report's light theme.
fn colours(class: &str) -> (&'static str, &'static str) {
    match class {
        "green" => ("#dff3e4", "#1f7a3d"),
        "yellow" => ("#fbf0d6", "#8a5d00"),
        "red" | "missing" => ("#f9e0e0", "#b3261e"),
        _ => ("#eceef1", "#5f6670"),
    }
}

/// Text for a quoted DOT string: `"` and `\` escaped, line breaks as `\n`.
fn dot_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Text for a quoted Mermaid label: characters that end or change the
/// label as entity codes, line breaks as `<br>`.
fn mermaid_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '"' => out.push_str("#quot;"),
            '#' => out.push_str("#35;"),
            // Mermaid inserts labels as HTML; `&amp;` stays as written.
            '&' => out.push_str("#38;"),
            '<' => out.push_str("#lt;"),
            '>' => out.push_str("#gt;"),
            '\n' => out.push_str("<br>"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// An SVG badge such as `requirements | 2/3 green`, for a README: how many
/// of the requirements at the top of a chain (not rejected) are green;
/// green if all are, red if one is red, yellow otherwise, and grey if
/// there are none.
pub fn badge(document: &Document<'_>) -> String {
    let lights: Vec<Light> = crate::graph::top(document)
        .into_iter()
        .map(|item| item.light)
        .filter(|&light| light != Light::Grey)
        .collect();
    let green = lights
        .iter()
        .filter(|&&light| light == Light::Green)
        .count();
    let worst = lights.iter().max().copied().unwrap_or(Light::Grey);
    let (_, colour) = colours(worst.as_str());
    let label = "requirements";
    let value = format!("{green}/{} green", lights.len());
    // About 7 pixels per character of the 11 pixel font, plus padding.
    let width_of = |text: &str| u32::try_from(text.chars().count()).unwrap_or(u32::MAX) * 7 + 10;
    let (left, right) = (width_of(label), width_of(&value));
    let total = left + right;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total}\" height=\"20\" role=\"img\" aria-label=\"{label}: {value}\">\n\
         <title>{label}: {value}</title>\n\
         <rect width=\"{left}\" height=\"20\" fill=\"#555\"/>\n\
         <rect x=\"{left}\" width=\"{right}\" height=\"20\" fill=\"{colour}\"/>\n\
         <g fill=\"#fff\" font-family=\"Verdana,DejaVu Sans,sans-serif\" font-size=\"11\" text-anchor=\"middle\">\n\
         <text x=\"{}\" y=\"14\">{label}</text>\n\
         <text x=\"{}\" y=\"14\">{value}</text>\n\
         </g>\n</svg>\n",
        left / 2,
        left + right / 2
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_labels() {
        assert_eq!(dot_text("a \"b\" \\ c\nd\u{1}"), "a \\\"b\\\" \\\\ c\\nd ");
        assert_eq!(
            mermaid_text("a \"b\" #1 <c>&amp;\nd"),
            "a #quot;b#quot; #35;1 #lt;c#gt;#38;amp;<br>d"
        );
    }
}
