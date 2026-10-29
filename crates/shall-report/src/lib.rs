//! HTML report of `shall`, generated from the exported graph
//! ([`shall_core::export::Document`]).
//!
//! The report is one self-contained HTML file, `index.html`, with its CSS
//! and a small script embedded, so that it opens from disk without a server. Like
//! `trace.json` it is deterministic: the same document gives the same
//! bytes.
//!
//! The start page has a summary and then one table per level:
//!
//! - the summary counts requirements, tags, test verdicts and defects,
//!   and shows the coverage of every level: how many of its items that
//!   are not rejected are covered, deep covered and
//!   [verified](shall_core::verify);
//! - the level tables list the level's requirements (not tag items)
//!   with coverage, whether they are verified, ID, title, status and
//!   location. Coverage is shown with a sign and words besides the
//!   color: `✓` deep covered, `!` covered but with a gap further down,
//!   `✗` not covered, `–` rejected, `?` unknown type; a legend explains
//!   them. Copies of a duplicate ID are marked.
//!
//! The defect list follows: every defect, errors first, with a link to
//! the requirement whose lines in the same file include it.
//!
//! After the tables follow the details of every requirement, anchored at
//! its ID: its fields, its text rendered from Markdown without raw HTML
//! and with only `http`, `https`, `mailto` and relative links, its links
//! upwards as written with the reason of each invalid one, and downwards
//! the requirements, code tags and tests covering it, the tests with their
//! verdict, the requirements it satisfies or that depend on it, and the
//! invalid links to it.
//!
//! An overview shows the roots as tiles in the colour of their light,
//! grouped by component, each linking to its graph.
//!
//! The Green Graph draws the chain of every root as SVG, laid out in
//! columns by the longest path from the root, with code and tests
//! bundled into count nodes and missing types as dashed placeholders.
//!
//! A filter above the level tables narrows them and the details by
//! coverage, status, type and tag, with a search over ID and title. Its
//! script is embedded like the CSS; without JavaScript, the filter stays
//! hidden and everything is shown.
//!
//! The levels are those of `shall.toml`, in their order there; types of
//! requirement items without a level follow in alphabetical order, named
//! by their type. If such a type is unknown, its items are errors (see
//! [`TracedItem::unknown_type`](shall_core::trace::TracedItem::unknown_type))
//! and their level shows no coverage; missing types are still listed.
//! Percentages are rounded down, so that 100 % means complete.
//!
//! The report of a variant names it and its features at the top. For
//! reports of several variants, [`render_overview`] writes a page that
//! links them, with the verdict and counts of each.

pub mod defects;
pub mod diagram;
mod graph;
mod markdown;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use minijinja::Environment;
use minijinja::syntax::SyntaxConfig;
use minijinja::value::{Serde, Value};
use serde::Serialize;
use shall_core::export::{BuildInfo, Document, ItemEntry};
use shall_core::item::{Origin, Reference};
use shall_core::results::Verdict;
use shall_core::trace::{ItemRef, Link, LinkKind, LinkStatus};
use shall_core::{ItemId, ItemType, Location, Severity, Status};

const INDEX: &str = include_str!("../templates/index.html");
const OVERVIEW: &str = include_str!("../templates/overview.html");
const STYLE: &str = include_str!("../templates/style.css");
const FILTER: &str = include_str!("../templates/filter.js");

/// Renders the start page, `index.html`.
///
/// Fails only if a template is broken, since the document's text is
/// passed to the templates as values, never as template source.
pub fn render(document: &Document<'_>) -> Result<String, minijinja::Error> {
    let page = Page::new(document);
    environment()?
        .get_template("index.html")?
        .render(Value::from(Serde(&page)))
}

/// Renders the overview of the reports of several variants, `index.html`
/// above the variants' directories: one row per document, in the given
/// order, linking `<variant>/index.html`.
///
/// # Panics
///
/// If a document has no variant.
pub fn render_overview(documents: &[Document<'_>]) -> Result<String, minijinja::Error> {
    let variants: Vec<OverviewRow> = documents
        .iter()
        .map(|document| {
            let variant = document
                .variant
                .as_ref()
                .expect("the overview lists variants");
            OverviewRow {
                name: variant.name,
                features: variant.features.iter().map(String::as_str).collect(),
                summary: summary(document),
            }
        })
        .collect();
    let overview = Overview {
        generator: documents
            .first()
            .map(|document| generator(document))
            .unwrap_or_default(),
        variants,
    };
    environment()?
        .get_template("overview.html")?
        .render(Value::from(Serde(&overview)))
}

fn environment() -> Result<Environment<'static>, minijinja::Error> {
    let mut env = Environment::new();
    env.set_syntax(
        SyntaxConfig::builder()
            .trim_blocks(true)
            .lstrip_blocks(true)
            .keep_trailing_newline(true)
            .build()
            .expect("valid syntax"),
    );
    env.add_template("index.html", INDEX)?;
    env.add_template("overview.html", OVERVIEW)?;
    env.add_template("style.css", STYLE)?;
    env.add_template("filter.js", FILTER)?;
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    Ok(env)
}

fn generator(document: &Document<'_>) -> String {
    format!("{} {}", document.generator.name, document.generator.version)
}

#[derive(Debug, Serialize)]
struct Overview<'a> {
    generator: String,
    variants: Vec<OverviewRow<'a>>,
}

#[derive(Debug, Serialize)]
struct OverviewRow<'a> {
    name: &'a str,
    features: Vec<&'a str>,
    summary: Summary,
}

#[derive(Debug, Serialize)]
struct VariantView<'a> {
    name: &'a str,
    features: Vec<&'a str>,
}

/// Everything the template shows, computed here so that the template
/// stays simple.
#[derive(Debug, Serialize)]
struct Page<'a> {
    generator: String,
    variant: Option<VariantView<'a>>,
    /// Where the traced files come from.
    build: Option<&'a BuildInfo>,
    summary: Summary,
    /// Every defect, errors first, each group in the order of the
    /// document.
    defects: Vec<DefectView<'a>>,
    /// The tree of requirements from the top down, as a flat list of
    /// entries that open and close nodes.
    tree: Vec<TreeEntry<'a>>,
    /// The Green Graph of every root.
    graphs: Vec<graph::GraphView>,
    /// The roots as tiles, grouped by component.
    tiles: Vec<TileGroup>,
    /// The types of the tables with their names, for the filter.
    filter_types: Vec<(&'a str, &'a str)>,
    /// Every tag of a requirement, sorted, for the filter.
    filter_tags: Vec<&'a str>,
    levels: Vec<LevelView<'a>>,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    requirements: usize,
    tags: usize,
    tests: Option<Tests>,
    errors: usize,
    warnings: usize,
}

/// A defect, as `shall check` prints it.
#[derive(Debug, Serialize)]
struct DefectView<'a> {
    /// `missing` for errors, `partial` for warnings, for the CSS class.
    class: &'static str,
    sign: &'static str,
    severity: &'static str,
    location: String,
    /// Address of the location in the code at the commit, if known.
    url: Option<String>,
    message: &'a str,
    /// Fragment of the requirement the defect is in: the one whose lines
    /// in the same file include the defect's line.
    anchor: Option<String>,
}

/// The tiles of the roots of one component, the part of their names
/// before the first `.`.
#[derive(Debug, Serialize)]
struct TileGroup {
    /// The component; empty for names without one.
    component: String,
    /// How many tiles are green, yellow, red and grey.
    green: usize,
    yellow: usize,
    red: usize,
    grey: usize,
    tiles: Vec<Tile>,
}

/// A root as a tile, linking to its graph.
#[derive(Debug, Serialize)]
struct Tile {
    anchor: String,
    id: String,
    title: String,
    light: &'static str,
    sign: &'static str,
}

/// Groups the roots of the graphs by component, groups sorted by name,
/// tiles in the order of the graphs.
fn tiles(graphs: &[graph::GraphView]) -> Vec<TileGroup> {
    let mut groups: BTreeMap<String, TileGroup> = BTreeMap::new();
    for graph in graphs {
        let Some(anchor) = &graph.anchor else {
            continue;
        };
        let name = graph.id.split('~').nth(1).unwrap_or_default();
        let component = name.split_once('.').map_or("", |(component, _)| component);
        let group = groups
            .entry(component.to_owned())
            .or_insert_with(|| TileGroup {
                component: component.to_owned(),
                green: 0,
                yellow: 0,
                red: 0,
                grey: 0,
                tiles: Vec::new(),
            });
        match graph.light {
            "green" => group.green += 1,
            "yellow" => group.yellow += 1,
            "red" => group.red += 1,
            _ => group.grey += 1,
        }
        group.tiles.push(Tile {
            anchor: anchor.clone(),
            id: graph.id.clone(),
            title: graph.title.clone(),
            light: graph.light,
            sign: graph.sign,
        });
    }
    groups.into_values().collect()
}

/// One entry of the tree: a node with children opens and later closes,
/// one without is a leaf.
#[derive(Debug, Serialize)]
struct TreeEntry<'a> {
    /// `open`, `close` or `leaf`.
    kind: &'static str,
    /// For the CSS class: the coverage of a requirement, the verdict of a
    /// test, `ok` for code, `missing` for a missing type.
    class: &'static str,
    sign: &'static str,
    /// The sign in words.
    label: &'static str,
    /// The requirement's ID, the tag as written, or the missing type.
    text: String,
    /// Fragment of the requirement's details.
    anchor: Option<String>,
    title: &'a str,
    /// Where a tag is.
    location: Option<String>,
    /// Address of the tag in the code at the commit, if known.
    url: Option<String>,
    /// Whether the requirement's children are shown further up already.
    repeated: bool,
}

/// Verdicts of the verification tags.
#[derive(Debug, Default, Serialize)]
struct Tests {
    passed: usize,
    failed: usize,
    skipped: usize,
    not_executed: usize,
    no_test: usize,
}

#[derive(Debug, Serialize)]
struct LevelView<'a> {
    typ: &'a str,
    name: &'a str,
    /// Whether `shall.toml` knows the type, at least from `Needs:` of
    /// items; items of an unknown type are errors, and their coverage is
    /// not shown.
    known: bool,
    needs: Vec<&'a str>,
    /// Items that are not rejected.
    counted: usize,
    covered: usize,
    deep_covered: usize,
    verified: usize,
    covered_percent: Option<u64>,
    deep_percent: Option<u64>,
    verified_percent: Option<u64>,
    items: Vec<ItemView<'a>>,
}

#[derive(Debug, Serialize)]
struct ItemView<'a> {
    /// Fragment of the item's details, its ID; copies of a duplicate ID
    /// after the first get `-2`, `-3` and so on.
    anchor: String,
    id: String,
    typ: &'a str,
    /// Tags of the item, joined by newlines for the filter.
    tags: String,
    /// ID and title in lower case, for the search.
    search: String,
    title: &'a str,
    status: &'static str,
    /// `ok`, `partial`, `missing`, `rejected` or `unknown`, for the CSS
    /// class.
    coverage: &'static str,
    /// The sign of the coverage.
    sign: &'static str,
    /// The coverage in words, for the legend and screen readers.
    label: &'static str,
    /// `yes`, `no`, or `–` for rejected items.
    verified: &'static str,
    /// The light of the item's chain: its sign and its word, which is also
    /// the CSS class.
    light_sign: &'static str,
    light: &'static str,
    duplicate: bool,
    missing: Vec<&'a str>,
    location: String,
    /// Address of the location in the code at the commit, if known.
    location_url: Option<String>,
    detail: DetailView<'a>,
}

/// What the details of a requirement show besides the table columns.
#[derive(Debug, Serialize)]
struct DetailView<'a> {
    /// The description, rationale and comment rendered as HTML.
    description: String,
    rationale: Option<String>,
    comment: Option<String>,
    source: Option<&'a str>,
    tags: &'a [String],
    features: &'a [String],
    import: Option<&'a str>,
    params: Vec<(&'a str, &'a str)>,
    covers: Vec<LinkView>,
    satisfied_by: Vec<LinkView>,
    depends: Vec<LinkView>,
    /// Requirements covering the item, except those of `satisfied_by`.
    covered_by: Vec<RefView<'a>>,
    /// Requirements that this item satisfies, by their valid
    /// `Satisfied-by:` links.
    satisfies: Vec<RefView<'a>>,
    /// Requirements depending on this item with a valid `Depends:`.
    depended_on_by: Vec<RefView<'a>>,
    /// Links to this item that are not valid, with the reason.
    invalid_incoming: Vec<IncomingView>,
    implementations: Vec<TagView<'a>>,
    tests: Vec<TestView<'a>>,
}

/// A link of the item to another one, as written.
#[derive(Debug, Serialize)]
struct LinkView {
    target: String,
    /// Fragment of the target's details, if it is a requirement in the
    /// report.
    anchor: Option<String>,
    /// Why the link is not valid; none for a valid link and for links of
    /// rejected items, which are not traced.
    problem: Option<String>,
}

/// A requirement covering the item.
#[derive(Debug, Serialize)]
struct RefView<'a> {
    id: String,
    anchor: String,
    title: &'a str,
}

/// An invalid link to the item.
#[derive(Debug, Serialize)]
struct IncomingView {
    /// The ID of the linking requirement, or the tag as written.
    source: String,
    /// Fragment of the linking requirement's details.
    anchor: Option<String>,
    /// Where the link is written.
    location: String,
    /// Address of the link in the code at the commit, if known.
    url: Option<String>,
    problem: String,
}

/// A code tag implementing the item.
#[derive(Debug, Serialize)]
struct TagView<'a> {
    text: &'a str,
    location: String,
    /// Address of the tag in the code at the commit, if known.
    url: Option<String>,
}

/// A verification tag of the item and its verdict.
#[derive(Debug, Serialize)]
struct TestView<'a> {
    text: &'a str,
    location: String,
    /// Address of the tag in the code at the commit, if known.
    url: Option<String>,
    /// The test as runners name it; none without a test definition or
    /// without `junit` sources.
    name: Option<&'a str>,
    /// `ok`, `partial` or `missing`, for the CSS class; `unknown` without
    /// `junit` sources.
    class: &'static str,
    sign: &'static str,
    verdict: &'static str,
}

/// Fragments of the requirements' details and the items by location and
/// ID, to resolve links.
struct Index<'d, 'a> {
    anchors: HashMap<(&'d Location, &'d ItemId), String>,
    /// Requirements with their fragment by location.
    requirements: HashMap<&'d Location, Vec<(&'d ItemId, String)>>,
    /// Items by location and ID; several for copies of a duplicate ID at
    /// one location, e.g. two tags on one line.
    items: HashMap<(&'d Location, &'d ItemId), Vec<&'d ItemEntry<'a>>>,
    /// Links by the location they are written at.
    links: HashMap<&'d Location, Vec<&'d Link>>,
    /// Links by the location of their target.
    incoming: HashMap<&'d Location, Vec<&'d Link>>,
    /// Where the files come from and `code_url`, to link locations to the
    /// code.
    build: Option<&'d BuildInfo>,
    code_url: Option<&'d str>,
}

impl<'d, 'a> Index<'d, 'a> {
    fn new(document: &'d Document<'a>) -> Self {
        let mut anchors = HashMap::new();
        let mut requirements: HashMap<&Location, Vec<(&ItemId, String)>> = HashMap::new();
        let mut used: HashMap<String, usize> = HashMap::new();
        let mut items: HashMap<_, Vec<_>> = HashMap::new();
        for item in &document.items {
            items
                .entry((item.location, item.id))
                .or_default()
                .push(item);
            if !matches!(item.origin, Origin::Requirement) {
                continue;
            }
            let id = item.id.to_string();
            let count = used.entry(id.clone()).or_default();
            *count += 1;
            let anchor = if *count == 1 {
                id
            } else {
                format!("{id}-{count}")
            };
            requirements
                .entry(item.location)
                .or_default()
                .push((item.id, anchor.clone()));
            anchors.insert((item.location, item.id), anchor);
        }
        let mut links: HashMap<&Location, Vec<&Link>> = HashMap::new();
        let mut incoming: HashMap<&Location, Vec<&Link>> = HashMap::new();
        for link in document.links {
            links.entry(&link.location).or_default().push(link);
            if let Some(target) = &link.target_location {
                incoming.entry(target).or_default().push(link);
            }
        }
        Self {
            anchors,
            requirements,
            items,
            links,
            incoming,
            build: document.build,
            code_url: document.code_url,
        }
    }

    /// The address of `location` in the code at the commit, if
    /// `code_url` and, if it needs one, the commit are given.
    fn code(&self, location: &Location) -> Option<String> {
        let sha = self.build.and_then(|build| build.sha.as_deref());
        shall_core::export::code_link(self.code_url?, sha, location.path(), location.line())
    }

    /// The fragment of the requirement with the target's type and name at
    /// `location`.
    fn anchor_at(&self, location: &Location, target: &ItemId) -> Option<String> {
        self.requirements
            .get(location)?
            .iter()
            .find(|(id, _)| id.typ() == target.typ() && id.name() == target.name())
            .map(|(_, anchor)| anchor.clone())
    }

    /// The item at `source` that covers `target` with a valid link: of
    /// copies of a duplicate ID at one location, the one whose links name
    /// the target in its current revision.
    fn source_of(&self, source: &ItemRef, target: &ItemEntry<'_>) -> Option<&'d ItemEntry<'a>> {
        self.copy_naming(&source.location, &source.id, |entry| {
            entry.covers.iter().any(|link| link.id == *target.id)
        })
    }

    /// The source of `link`: of copies of a duplicate ID at one location,
    /// the one whose links of the link's kind name its target as written.
    fn link_source(&self, link: &Link) -> Option<&'d ItemEntry<'a>> {
        self.copy_naming(&link.source_location, &link.source, |entry| {
            let references = match link.kind {
                LinkKind::Covers => entry.covers,
                LinkKind::Depends => entry.depends,
                LinkKind::SatisfiedBy => entry.satisfied_by,
            };
            references
                .iter()
                .any(|reference| reference.id == link.target)
        })
    }

    /// The copy at `location` with `id` that `names` accepts, or else the
    /// first.
    fn copy_naming(
        &self,
        location: &Location,
        id: &ItemId,
        names: impl Fn(&ItemEntry<'_>) -> bool,
    ) -> Option<&'d ItemEntry<'a>> {
        let entries = self.items.get(&(location, id))?;
        entries
            .iter()
            .find(|entry| names(entry))
            .or_else(|| entries.first())
            .copied()
    }

    /// The traced link of `item` written as `reference`, if any.
    fn link(&self, item: &ItemEntry<'_>, kind: LinkKind, reference: &Reference) -> Option<&Link> {
        let location = item.location.with_line(reference.line);
        self.links.get(&location)?.iter().copied().find(|link| {
            link.kind == kind
                && link.target == reference.id
                && link.source_location == *item.location
        })
    }
}

impl<'a> Page<'a> {
    fn new(document: &'a Document<'_>) -> Self {
        let index = Index::new(document);
        // Requirement types without a level, e.g. misspelled ones, get a
        // table of their own after the levels.
        let configured: BTreeSet<&ItemType> =
            document.levels.iter().map(|level| level.typ).collect();
        let others: BTreeSet<&ItemType> = document
            .items
            .iter()
            .filter(|item| matches!(item.origin, Origin::Requirement))
            .map(|item| item.typ)
            .filter(|typ| !configured.contains(typ))
            .collect();
        let levels: Vec<LevelView> = document
            .levels
            .iter()
            .map(|entry| level(document, &index, entry.typ, entry.name, entry.needs))
            .chain(
                others
                    .into_iter()
                    .map(|typ| level(document, &index, typ, typ.as_str(), &[])),
            )
            .collect();
        let graphs = graph::graphs(document, &index);
        let filter_types = levels.iter().map(|level| (level.typ, level.name)).collect();
        Self {
            generator: generator(document),
            build: document.build,
            variant: document.variant.as_ref().map(|variant| VariantView {
                name: variant.name,
                features: variant.features.iter().map(String::as_str).collect(),
            }),
            summary: summary(document),
            defects: defects(document, &index),
            tree: tree(document, &index),
            tiles: tiles(&graphs),
            graphs,
            filter_types,
            filter_tags: document
                .items
                .iter()
                .filter(|item| matches!(item.origin, Origin::Requirement))
                .flat_map(|item| item.tags)
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            levels,
        }
    }
}

fn defects<'a>(document: &'a Document<'_>, index: &Index<'a, '_>) -> Vec<DefectView<'a>> {
    // Requirements by file, ordered by line, to find the one a defect is
    // in.
    let mut by_file: HashMap<&str, Vec<(u32, u32, &String)>> = HashMap::new();
    for item in &document.items {
        if let Some(anchor) = index.anchors.get(&(item.location, item.id)) {
            by_file.entry(item.location.path()).or_default().push((
                item.location.line(),
                item.end_line,
                anchor,
            ));
        }
    }
    for requirements in by_file.values_mut() {
        requirements.sort();
    }
    let view = |defect: &shall_core::export::DefectEntry<'a>| {
        let (class, sign, severity) = match defect.severity {
            Severity::Error => ("missing", "✗", "error"),
            Severity::Warning => ("partial", "!", "warning"),
        };
        let line = defect.location.line();
        let anchor = by_file
            .get(defect.location.path())
            .and_then(|requirements| {
                let above = requirements.partition_point(|(start, ..)| *start <= line);
                let (_, end, anchor) = requirements.get(above.checked_sub(1)?)?;
                (line <= *end).then(|| (*anchor).clone())
            });
        DefectView {
            class,
            sign,
            severity,
            location: defect.location.to_string(),
            url: index.code(defect.location),
            message: defect.message,
            anchor,
        }
    };
    let errors = document
        .defects
        .iter()
        .filter(|defect| defect.severity == Severity::Error);
    let warnings = document
        .defects
        .iter()
        .filter(|defect| defect.severity == Severity::Warning);
    errors.chain(warnings).map(view).collect()
}

/// Counts of the summary: requirements, tags, test verdicts and defects.
fn summary(document: &Document<'_>) -> Summary {
    let mut summary = Summary::default();
    let mut tags = BTreeSet::new();
    let mut tests = Tests::default();
    for item in &document.items {
        // A tag with several targets has one item per target, but it
        // is one tag with one test.
        let new_tag = match item.origin {
            Origin::Requirement => {
                summary.requirements += 1;
                false
            }
            Origin::Tag { text, .. } => tags.insert((item.location, text)),
        };
        if let Some(test) = item.test.as_ref().filter(|_| new_tag) {
            match test.verdict {
                Verdict::Passed => tests.passed += 1,
                Verdict::Failed => tests.failed += 1,
                Verdict::Skipped => tests.skipped += 1,
                Verdict::NotExecuted => tests.not_executed += 1,
                Verdict::NoTest => tests.no_test += 1,
            }
        }
    }
    summary.tags = tags.len();
    summary.tests = document.test_results.is_some().then_some(tests);
    for defect in &document.defects {
        match defect.severity {
            Severity::Error => summary.errors += 1,
            Severity::Warning => summary.warnings += 1,
        }
    }
    summary
}

fn level<'a>(
    document: &'a Document<'_>,
    index: &Index<'a, '_>,
    typ: &'a ItemType,
    name: &'a str,
    needs: &'a [ItemType],
) -> LevelView<'a> {
    // Tag items are not listed: a tag with several targets would appear
    // once per target, and the tables are about requirements.
    let items: Vec<&ItemEntry> = document
        .items
        .iter()
        .filter(|item| item.typ == typ && matches!(item.origin, Origin::Requirement))
        .collect();
    let counted: Vec<&&ItemEntry> = items
        .iter()
        .filter(|item| item.status != Status::Rejected)
        .collect();
    let covered = counted.iter().filter(|item| item.covered).count();
    let deep_covered = counted.iter().filter(|item| item.deep_covered).count();
    let verified = counted.iter().filter(|item| item.verified).count();
    // The type is unknown for all items of a level or none, except
    // rejected ones, which are not counted.
    let known = !counted.iter().any(|item| item.unknown_type);
    LevelView {
        typ: typ.as_str(),
        name,
        known,
        needs: needs.iter().map(ItemType::as_str).collect(),
        counted: counted.len(),
        covered,
        deep_covered,
        verified,
        covered_percent: percent(covered, counted.len()),
        deep_percent: percent(deep_covered, counted.len()),
        verified_percent: percent(verified, counted.len()),
        items: items
            .into_iter()
            .map(|item| item_view(document, index, item))
            .collect(),
    }
}

/// `part` of `total` in percent, rounded down; none for an empty level.
fn percent(part: usize, total: usize) -> Option<u64> {
    (total > 0).then(|| (part as u64 * 100) / total as u64)
}

fn item_view<'a>(
    document: &'a Document<'_>,
    index: &Index<'a, '_>,
    item: &'a ItemEntry<'_>,
) -> ItemView<'a> {
    let (coverage, sign, label) = coverage(item);
    ItemView {
        anchor: index.anchors[&(item.location, item.id)].clone(),
        id: item.id.to_string(),
        typ: item.typ.as_str(),
        tags: item.tags.join("\n"),
        search: format!("{} {}", item.id, item.title).to_lowercase(),
        title: item.title,
        label,
        verified: match (item.status, item.verified) {
            (Status::Rejected, _) => "–",
            (_, true) => "yes",
            (_, false) => "no",
        },
        light_sign: item.light.sign(),
        light: item.light.as_str(),
        duplicate: item.duplicate,
        status: item.status.as_str(),
        coverage,
        sign,
        missing: item.missing.iter().map(ItemType::as_str).collect(),
        location: item.location.to_string(),
        location_url: index.code(item.location),
        detail: detail(document, index, item),
    }
}

/// The tree of requirements. Roots are the requirements that cover no
/// other item validly, by `Covers:` or as a target of `Satisfied-by:`,
/// rejected ones included; then, in document order, every requirement not
/// shown yet, such as those of a coverage cycle without a root. Below each
/// node come the items covering it, its missing types, then code and
/// tests; a tag that needs coverage of its own (`>>`) is a node too. A
/// node that appears again shows its children only where it appears
/// first, so that shared parts of the graph do not grow the tree
/// exponentially, and a coverage cycle ends. Built with an explicit stack,
/// so that long chains cannot overflow it.
fn tree<'a>(document: &'a Document<'_>, index: &Index<'a, '_>) -> Vec<TreeEntry<'a>> {
    let covering: HashSet<(&Location, &ItemId)> = document
        .items
        .iter()
        .flat_map(|item| item.covered_by)
        .map(|source| (&source.location, &source.id))
        .collect();
    let requirements = || {
        document
            .items
            .iter()
            .filter(|item| matches!(item.origin, Origin::Requirement))
    };
    let mut walk = Walk {
        document,
        index,
        shown: HashSet::new(),
        entries: Vec::new(),
    };
    for root in requirements().filter(|item| !covering.contains(&(item.location, item.id))) {
        walk.from(root);
    }
    for rest in requirements() {
        if !walk.shown.contains(&(rest.location, rest.id)) {
            walk.from(rest);
        }
    }
    walk.entries
}

/// The state of building the [`tree`].
struct Walk<'a, 'i, 'd> {
    document: &'a Document<'d>,
    index: &'i Index<'a, 'd>,
    shown: HashSet<(&'a Location, &'a ItemId)>,
    entries: Vec<TreeEntry<'a>>,
}

impl<'a> Walk<'a, '_, '_> {
    /// Adds the subtree of `root`.
    fn from(&mut self, root: &'a ItemEntry<'a>) {
        // (item, its children still to visit), the last on top.
        let mut stack: Vec<std::vec::IntoIter<&ItemEntry>> = Vec::new();
        let mut next = Some(root);
        loop {
            if let Some(item) = next.take()
                && let Some(children) = self.node(item)
            {
                stack.push(children.into_iter());
            }
            let Some(children) = stack.last_mut() else {
                break;
            };
            match children.next() {
                Some(child) => next = Some(child),
                None => {
                    stack.pop();
                    self.entries.push(TreeEntry::close());
                }
            }
        }
    }

    /// Adds the entry of `item`, and its missing types if it opens; returns
    /// its children if it opens.
    fn node(&mut self, item: &'a ItemEntry<'a>) -> Option<Vec<&'a ItemEntry<'a>>> {
        let document = self.document;
        let has_below = !item.covered_by.is_empty() || !item.missing.is_empty();
        if !has_below && let Some(entry) = leaf(document, self.index, item) {
            self.entries.push(entry);
            return None;
        }
        let first = self.shown.insert((item.location, item.id));
        let opens = first && has_below;
        // A test keeps its verdict, also when it needs coverage itself.
        let (class, sign, label) = match item.origin {
            Origin::Tag { .. } if document.verification_types.contains(item.typ) => {
                verdict(item.test.as_ref().map(|test| test.verdict))
            }
            _ => coverage(item),
        };
        let (text, location) = match item.origin {
            Origin::Tag { text, .. } => (text.clone(), Some(item.location.to_string())),
            Origin::Requirement => (item.id.to_string(), None),
        };
        self.entries.push(TreeEntry {
            kind: if opens { "open" } else { "leaf" },
            class,
            sign,
            label,
            text,
            anchor: self.index.anchors.get(&(item.location, item.id)).cloned(),
            title: item.title,
            url: location
                .as_ref()
                .and_then(|_| self.index.code(item.location)),
            location,
            repeated: !first && has_below,
        });
        if !opens {
            return None;
        }
        for typ in item.missing {
            self.entries.push(TreeEntry {
                kind: "leaf",
                class: "missing",
                sign: "✗",
                label: "missing",
                text: typ.to_string(),
                ..TreeEntry::close()
            });
        }
        let mut children: Vec<&ItemEntry> = item
            .covered_by
            .iter()
            .filter_map(|source| self.index.source_of(source, item))
            .collect();
        // Requirements first, then code, then tests, each in the order of
        // `covered_by`.
        children.sort_by_key(|child| match child.origin {
            Origin::Requirement => 0,
            Origin::Tag { .. } if !document.verification_types.contains(child.typ) => 1,
            Origin::Tag { .. } => 2,
        });
        Some(children)
    }
}

impl TreeEntry<'_> {
    /// The entry that closes the node opened last.
    fn close() -> Self {
        Self {
            kind: "close",
            class: "",
            sign: "",
            label: "",
            text: String::new(),
            anchor: None,
            title: "",
            location: None,
            url: None,
            repeated: false,
        }
    }
}

/// The tree entry of a tag; none for a requirement.
fn leaf<'a>(
    document: &'a Document<'_>,
    index: &Index<'a, '_>,
    item: &'a ItemEntry<'_>,
) -> Option<TreeEntry<'a>> {
    let Origin::Tag { text, .. } = item.origin else {
        return None;
    };
    let (class, sign, label) = if document.verification_types.contains(item.typ) {
        verdict(item.test.as_ref().map(|test| test.verdict))
    } else {
        ("ok", "✓", "code")
    };
    Some(TreeEntry {
        kind: "leaf",
        class,
        sign,
        label,
        text: text.clone(),
        anchor: None,
        title: "",
        location: Some(item.location.to_string()),
        url: index.code(item.location),
        repeated: false,
    })
}

/// CSS class, sign and words for the coverage of a requirement.
fn coverage(item: &ItemEntry<'_>) -> (&'static str, &'static str, &'static str) {
    if item.status == Status::Rejected {
        ("rejected", "–", "rejected")
    } else if item.unknown_type {
        ("unknown", "?", "unknown type")
    } else if item.deep_covered {
        ("ok", "✓", "deep covered")
    } else if item.covered {
        ("partial", "!", "covered, gap below")
    } else {
        ("missing", "✗", "not covered")
    }
}

/// CSS class, sign and words for the verdict of a test; without `junit`
/// sources there is none.
fn verdict(verdict: Option<Verdict>) -> (&'static str, &'static str, &'static str) {
    match verdict {
        Some(Verdict::Passed) => ("ok", "✓", "passed"),
        Some(Verdict::Failed) => ("missing", "✗", "failed"),
        Some(Verdict::Skipped) => ("partial", "!", "skipped"),
        Some(Verdict::NotExecuted) => ("partial", "!", "not executed"),
        Some(Verdict::NoTest) => ("partial", "!", "no test"),
        None => ("unknown", "?", "no results read"),
    }
}

fn detail<'a>(
    document: &'a Document<'_>,
    index: &Index<'a, '_>,
    item: &'a ItemEntry<'_>,
) -> DetailView<'a> {
    let links = |kind: LinkKind, references: &[Reference]| -> Vec<LinkView> {
        references
            .iter()
            .map(|reference| {
                let link = index.link(item, kind, reference);
                LinkView {
                    target: reference.id.to_string(),
                    anchor: link
                        .and_then(|link| link.target_location.as_ref())
                        .and_then(|location| index.anchor_at(location, &reference.id)),
                    problem: link.and_then(|link| problem(link.status)),
                }
            })
            .collect()
    };
    // Targets of valid `Satisfied-by:` links cover the item too; they are
    // listed there, with the parameters.
    let satisfied: Vec<&Location> = item
        .satisfied_by
        .iter()
        .filter_map(|reference| index.link(item, LinkKind::SatisfiedBy, reference))
        .filter(|link| link.status == LinkStatus::Valid)
        .filter_map(|link| link.target_location.as_ref())
        .collect();
    let mut covered_by = Vec::new();
    let mut implementations = Vec::new();
    let mut tests = Vec::new();
    for source in item.covered_by {
        let Some(entry) = index.source_of(source, item) else {
            continue;
        };
        match entry.origin {
            Origin::Requirement if !satisfied.contains(&&source.location) => {
                covered_by.push(RefView {
                    id: entry.id.to_string(),
                    anchor: index.anchors[&(entry.location, entry.id)].clone(),
                    title: entry.title,
                });
            }
            Origin::Requirement => {}
            Origin::Tag { text, .. } if document.verification_types.contains(entry.typ) => {
                let (class, sign, verdict) = verdict(entry.test.as_ref().map(|test| test.verdict));
                tests.push(TestView {
                    text,
                    location: entry.location.to_string(),
                    url: index.code(entry.location),
                    name: entry.test.as_ref().and_then(|test| test.name),
                    class,
                    sign,
                    verdict,
                });
            }
            Origin::Tag { text, .. } => implementations.push(TagView {
                text,
                location: entry.location.to_string(),
                url: index.code(entry.location),
            }),
        }
    }
    let mut satisfies = Vec::new();
    let mut depended_on_by = Vec::new();
    let mut invalid_incoming = Vec::new();
    let incoming = index.incoming.get(item.location).into_iter().flatten();
    for link in
        incoming.filter(|link| link.target.typ() == item.typ && link.target.name() == item.name)
    {
        let source = index.link_source(link);
        let anchor = index
            .anchors
            .get(&(&link.source_location, &link.source))
            .cloned();
        match (link.status, link.kind, source, &anchor) {
            (LinkStatus::Valid, LinkKind::Covers, _, _) => {}
            (LinkStatus::Valid, kind, Some(source), Some(anchor)) => {
                let view = RefView {
                    id: source.id.to_string(),
                    anchor: anchor.clone(),
                    title: source.title,
                };
                if kind == LinkKind::SatisfiedBy {
                    satisfies.push(view);
                } else {
                    depended_on_by.push(view);
                }
            }
            (LinkStatus::Valid, ..) => {}
            (status, ..) => invalid_incoming.push(IncomingView {
                source: match source.map(|source| source.origin) {
                    Some(Origin::Tag { text, .. }) => text.clone(),
                    _ => link.source.to_string(),
                },
                anchor,
                location: link.location.to_string(),
                url: index.code(&link.location),
                problem: problem(status).unwrap_or_default(),
            }),
        }
    }
    let text = |text: &str| {
        Some(text)
            .filter(|text| !text.trim().is_empty())
            .map(markdown::render)
    };
    DetailView {
        description: markdown::render(item.description),
        rationale: item.rationale.and_then(text),
        comment: item.comment.and_then(text),
        source: item.source,
        tags: item.tags,
        features: item.features,
        import: item.import,
        params: item
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.value.as_str()))
            .collect(),
        covers: links(LinkKind::Covers, item.covers),
        satisfied_by: links(LinkKind::SatisfiedBy, item.satisfied_by),
        depends: links(LinkKind::Depends, item.depends),
        covered_by,
        satisfies,
        depended_on_by,
        invalid_incoming,
        implementations,
        tests,
    }
}

/// Why a link is not valid, in words; none for a valid one.
fn problem(status: LinkStatus) -> Option<String> {
    Some(match status {
        LinkStatus::Valid => return None,
        LinkStatus::Unknown => "unknown item".to_owned(),
        LinkStatus::Outdated { current } => format!("outdated, current revision {current}"),
        LinkStatus::Predated { current } => {
            format!("revision does not exist yet, current revision {current}")
        }
        LinkStatus::Ambiguous => "ID defined more than once".to_owned(),
        LinkStatus::Rejected => "target rejected".to_owned(),
        LinkStatus::Unwanted => "the target does not need this type".to_owned(),
        LinkStatus::Disallowed => "type not allowed by [satisfied_by]".to_owned(),
        LinkStatus::OutsideVariant => "outside the variant".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use shall_core::config::Config;
    use shall_core::export::Input;
    use shall_core::{markdown, trace};

    use super::*;

    fn html(config: &str, requirements: &str) -> String {
        let config = Config::parse(config).unwrap();
        let items = markdown::parse(Path::new("reqs/a.md"), requirements)
            .unwrap()
            .items;
        let trace = trace::trace(&items, &config);
        render(&Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
            variant: None,
            build: None,
        }))
        .unwrap()
    }

    #[test]
    fn unknown_types_show_no_coverage_and_duplicates_are_marked() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n\n`dns~a-k4f2~1`\n\n# B\n\n`dsn~b-k4f2~1`\n\n# B\n\n`dsn~b-k4f2~1`\n",
        );
        assert!(
            html.contains("<td colspan=\"3\" class=\"unknown-type\">? unknown type</td>"),
            "{html}"
        );
        assert!(
            html.contains(
                "\">\n<td class=\"sign\">\
                 <span title=\"unknown type\" aria-hidden=\"true\">?</span>\
                 <span class=\"sr-only\">unknown type</span>"
            ),
            "{html}"
        );
        assert!(html.contains("<tr class=\"unknown\" "), "{html}");
        assert_eq!(html.matches("✗ duplicate ID").count(), 2, "{html}");
    }

    #[test]
    fn types_from_needs_and_rejected_items_are_not_unknown() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n\n`dsn~a-k4f2~1`\n\nNeeds: itst\n\n# T\n\n`itst~t-k4f2~1`\n\n\
             # R\n\n`zzz~r-k4f2~1`\n\nStatus: rejected\n",
        );
        assert!(!html.contains("unknown-type\">"), "{html}");
        assert!(!html.contains("<tr class=\"unknown\""), "{html}");
        assert!(html.contains("<tr class=\"rejected\""), "{html}");
        assert!(html.contains("<h2>itst</h2>"), "{html}");
    }

    #[test]
    fn level_tables_list_requirements_only() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"impl\"]\n",
            "# A\n\n`dsn~a-k4f2~1`\n\n# I\n\n`impl~i-k4f2~1`\n\nCovers:\n- dsn~a-k4f2~1\n",
        );
        let impl_table = &html[html.find("<h2>impl</h2>").unwrap()..];
        assert_eq!(impl_table.matches("<tr class=").count(), 1, "{html}");
    }

    #[test]
    fn a_tag_with_several_targets_counts_its_test_once() {
        use shall_core::results::{self, Packages};
        use shall_core::{junit, tags};

        let config = Config::parse(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"utest\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n",
        )
        .unwrap();
        let mut items = markdown::parse(
            Path::new("reqs/a.md"),
            "# A\n\n`dsn~a-k4f2~1`\n\n# B\n\n`dsn~b-k4f2~1`\n",
        )
        .unwrap()
        .items;
        items.extend(
            tags::scan(
                Path::new("tests/a.cpp"),
                "// [utest->dsn~a-k4f2~1, dsn~b-k4f2~1]\nTEST(S, T) {}\n",
            )
            .unwrap()
            .items,
        );
        let trace = trace::trace(&items, &config);
        let parsed = junit::parse(
            Path::new("r.xml"),
            r#"<testsuite name="S"><testcase classname="S" name="T"/></testsuite>"#,
        )
        .unwrap();
        let evaluation = results::evaluate(&items, &parsed.results, &config, &Packages::default());
        let html = render(&Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: Some((&parsed.results, &evaluation)),
            diagnostics: &[],
            variant: None,
            build: None,
        }))
        .unwrap();
        assert!(html.contains("<dt>Tags</dt><dd>1</dd>"), "{html}");
        assert!(html.contains("<dt>✓ Passed</dt><dd>1</dd>"), "{html}");
    }

    #[test]
    fn types_without_a_level_get_a_table_after_the_levels() {
        let html = html(
            "[[level]]\ntype = \"swr\"\nname = \"Software\"\n",
            "# A\n\n`swr~a-k4f2~1`\n\n# B\n\n`sfr~b-k4f2~1`\n",
        );
        let software = html.find("<h2>Software <code>swr</code></h2>").unwrap();
        let other = html.find("<h2>sfr</h2>").unwrap();
        assert!(software < other, "{html}");
        assert!(html.contains("<code>sfr~b-k4f2~1</code>"), "{html}");
    }

    #[test]
    fn without_levels_every_requirement_type_is_a_level() {
        let html = html(
            "",
            "# <b>Bold</b> & co\n\n`swr~b-k4f2~1`\n\n# A\n\n`dsn~a-k4f2~1`\n\n\
             # Old\n\n`dsn~old-k4f2~1`\n\nStatus: rejected\n",
        );
        insta::assert_snapshot!(html);
    }

    #[test]
    fn percentages_round_down() {
        assert_eq!(percent(0, 0), None);
        assert_eq!(percent(0, 3), Some(0));
        assert_eq!(percent(2, 3), Some(66));
        assert_eq!(percent(199, 200), Some(99));
        assert_eq!(percent(3, 3), Some(100));
    }

    #[test]
    fn details_link_up_and_down() {
        let html = html(
            "[[level]]\ntype = \"hlr\"\nname = \"Customer\"\nneeds = [\"swr\"]\n\n\
             [[level]]\ntype = \"cap\"\nname = \"Capabilities\"\n\n\
             [[level]]\ntype = \"swr\"\nname = \"Software\"\n",
            "# H\n`hlr~h-k4f2~1`\n\nSatisfied-by:\n- cap~c-k4f2~1\n\nParams:\n- limit_c = 85\n\n\
             # C\n`cap~c-k4f2~1`\n\n\
             # S\n`swr~s-k4f2~1`\n\nComment:\n\nCovers:\n- hlr~h-k4f2~1\n- cap~gone-k4f2~1\n\nDepends:\n- cap~c-k4f2~1\n\n\
             # R\n`swr~r-k4f2~1`\n\nStatus: rejected\n\nCovers:\n- hlr~h-k4f2~1\n",
        );
        let detail = |anchor: &str| {
            let start = html.find(&format!("<article id=\"{anchor}\"")).unwrap();
            let end = start + html[start..].find("</article>").unwrap();
            html[start..end].to_owned()
        };
        let h = detail("hlr~h-k4f2~1");
        assert!(
            h.contains("<h4>Satisfied by</h4>\n<ul class=\"links\">\n<li><a href=\"#cap~c-k4f2~1\"><code>cap~c-k4f2~1</code></a></li>"),
            "{h}"
        );
        assert!(
            h.contains("<tr><td><code>limit_c</code></td><td><code>85</code></td></tr>"),
            "{h}"
        );
        // The capability is listed under Satisfied by, not again as cover.
        assert!(!h.contains("<h4>Covered by</h4>"), "{h}");
        let s = detail("swr~s-k4f2~1");
        assert!(
            s.contains("<li><code>cap~gone-k4f2~1</code> <span class=\"problem\">✗ unknown item</span></li>"),
            "{s}"
        );
        assert!(s.contains("<a href=\"#hlr~h-k4f2~1\"><code>hlr~h-k4f2~1</code></a> <span class=\"problem\">✗ the target does not need this type</span>"), "{s}");
        assert!(!s.contains("<h4>Comment</h4>"), "{s}");
        let c = detail("cap~c-k4f2~1");
        assert!(
            c.contains("<h4>Satisfies</h4>\n<ul class=\"links\">\n<li><a href=\"#hlr~h-k4f2~1\"><code>hlr~h-k4f2~1</code></a> H</li>"),
            "{c}"
        );
        assert!(
            c.contains("<h4>Depended on by</h4>\n<ul class=\"links\">\n<li><a href=\"#swr~s-k4f2~1\"><code>swr~s-k4f2~1</code></a> S</li>"),
            "{c}"
        );
        assert!(
            h.contains("<h4>Invalid links to this item</h4>\n<ul class=\"links\">\n<li><a href=\"#swr~s-k4f2~1\"><code>swr~s-k4f2~1</code></a> at <code>reqs&#x2f;a.md:"),
            "{h}"
        );
        // A rejected item keeps its links, which are not traced.
        let r = detail("swr~r-k4f2~1");
        assert!(r.contains("<li><code>hlr~h-k4f2~1</code></li>"), "{r}");
        assert!(
            html.contains("<a href=\"#swr~s-k4f2~1\"><code>swr~s-k4f2~1</code></a>"),
            "{html}"
        );
    }

    #[test]
    fn copies_of_a_duplicate_id_get_their_own_anchor() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n`dsn~a-k4f2~1`\n\n# A again\n`dsn~a-k4f2~1`\n",
        );
        assert!(html.contains("<article id=\"dsn~a-k4f2~1\""), "{html}");
        assert!(html.contains("<article id=\"dsn~a-k4f2~1-2\""), "{html}");
        assert!(html.contains("<a href=\"#dsn~a-k4f2~1-2\">"), "{html}");
    }

    #[test]
    fn texts_are_rendered_without_running_anything() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n`dsn~a-k4f2~1`\n\nSee **this** <img src=x onerror=alert(1)> [x](javascript:alert(1)).\n\n\
             Rationale:\n<script>alert(2)</script>\n",
        );
        assert!(
            html.contains("<p>See <strong>this</strong> &lt;img src=x onerror=alert(1)&gt; x.</p>"),
            "{html}"
        );
        assert!(
            html.contains("&lt;script&gt;alert(2)&lt;/script&gt;"),
            "{html}"
        );
        // The only script is the report's own filter.
        assert_eq!(html.matches("<script").count(), 1, "{html}");
        assert!(!html.contains("<img"), "{html}");
    }

    #[test]
    fn tags_and_tests_appear_below_their_targets() {
        use shall_core::tags;

        let config = Config::parse(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"impl\", \"utest\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n",
        )
        .unwrap();
        let mut items = markdown::parse(
            Path::new("reqs/a.md"),
            "# A\n`dsn~a-k4f2~1`\n\n# B\n`dsn~b-k4f2~1`\n",
        )
        .unwrap()
        .items;
        // Two tags with the same own ID on one line, a duplicate.
        items.extend(
            tags::scan(
                Path::new("src/a.c"),
                "// [impl~x~0->dsn~a-k4f2~1] [impl~x~0->dsn~b-k4f2~1]\nvoid f();\n",
            )
            .unwrap()
            .items,
        );
        items.extend(
            tags::scan(
                Path::new("tests/a.cpp"),
                "// [utest->dsn~a-k4f2~1]\nTEST(S, N) {}\n",
            )
            .unwrap()
            .items,
        );
        let trace = trace::trace(&items, &config);
        let html = render(&Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
            variant: None,
            build: None,
        }))
        .unwrap();
        let b = &html[html.find("<article id=\"dsn~b-k4f2~1\"").unwrap()..];
        let b = &b[..b.find("</article>").unwrap()];
        assert!(b.contains("[impl~x~0-&gt;dsn~b-k4f2~1]"), "{b}");
        assert!(!b.contains("dsn~a-k4f2~1]"), "{b}");
        // Without junit sources, a test has no verdict.
        assert!(
            html.contains("<span class=\"badge unknown\">? no results read</span>"),
            "{html}"
        );
    }

    #[test]
    fn copies_of_a_tag_id_on_one_line_keep_their_links_apart() {
        use shall_core::tags;

        let config =
            Config::parse("[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"impl\"]\n")
                .unwrap();
        let mut items = markdown::parse(Path::new("reqs/a.md"), "# B\n`dsn~b-k4f2~1`\n")
            .unwrap()
            .items;
        items.extend(
            tags::scan(
                Path::new("src/a.c"),
                "// [impl~x~0->dsn~b-k4f2~2] [impl~x~0->dsn~b-k4f2~1]\nvoid f();\n",
            )
            .unwrap()
            .items,
        );
        let trace = trace::trace(&items, &config);
        let html = render(&Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
            variant: None,
            build: None,
        }))
        .unwrap();
        // The valid tag implements, the other one is an invalid link.
        assert!(
            html.contains("<h4>Implementation</h4>\n<ul class=\"links\">\n<li><code>src&#x2f;a.c:1</code> <code>[impl~x~0-&gt;dsn~b-k4f2~1]</code></li>"),
            "{html}"
        );
        assert!(
            html.contains("<li><code>[impl~x~0-&gt;dsn~b-k4f2~2]</code> at <code>src&#x2f;a.c:1</code> <span class=\"problem\">✗ revision does not exist yet, current revision 1</span></li>"),
            "{html}"
        );
    }

    #[test]
    fn lists_errors_before_warnings_with_their_requirement() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"impl\"]\n",
            "# I\n`imp~i-k4f2~1`\n\n# A\n`dsn~a-k4f2~1`\n\nNeeds: imp\n\n# B\n`dsn~b-k4f2~1`\n",
        );
        let defects = &html[html.find("<section id=\"defects\">").unwrap()..];
        let defects = &defects[..defects.find("</section>").unwrap()];
        let error = defects.find("✗</span> error").unwrap();
        let warning = defects.find("!</span> warning").unwrap();
        assert!(error < warning, "{defects}");
        assert!(
            defects.contains("<a href=\"#dsn~b-k4f2~1\"><code>reqs&#x2f;a.md:10</code></a>"),
            "{defects}"
        );
        assert!(html.contains("<a href=\"#defects\">"), "{html}");
        let clean = super::tests::html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n`dsn~a-k4f2~1`\n",
        );
        assert!(!clean.contains("id=\"defects\""), "{clean}");
    }

    #[test]
    fn defects_outside_of_items_link_nowhere() {
        let config = Config::parse("[[level]]\ntype = \"dsn\"\nname = \"Design\"\n").unwrap();
        let parsed = markdown::parse(
            Path::new("reqs/a.md"),
            "Neds: before\n\n# A\n`dsn~a-k4f2~1`\nNeds: inside\n\n`dsn~bad~x`\nNeds: after\n",
        )
        .unwrap();
        let trace = trace::trace(&parsed.items, &config);
        let html = render(&Document::new(Input {
            config: &config,
            items: &parsed.items,
            trace: &trace,
            tests: None,
            diagnostics: &parsed.diagnostics,
            variant: None,
            build: None,
        }))
        .unwrap();
        let defects = &html[html.find("<section id=\"defects\">").unwrap()..];
        assert_eq!(
            defects.matches("<a href=\"#dsn~a-k4f2~1\">").count(),
            1,
            "{defects}"
        );
        assert!(
            defects.contains("<a href=\"#dsn~a-k4f2~1\"><code>reqs&#x2f;a.md:5</code></a>"),
            "{defects}"
        );
        assert!(
            defects.contains("<td><code>reqs&#x2f;a.md:7</code></td>"),
            "{defects}"
        );
    }

    #[test]
    fn defects_of_items_outside_the_variant_link_nowhere() {
        let config = Config::parse("[[level]]\ntype = \"dsn\"\nname = \"Design\"\n").unwrap();
        let parsed = markdown::parse(
            Path::new("reqs/a.md"),
            "# A\n`dsn~a-k4f2~1`\n\n# B\n`dsn~b-k4f2~1`\nFeatures: x\nNeds: inside\n",
        )
        .unwrap();
        let variant = shall_core::config::Variant::new(Vec::new());
        let selection = shall_core::variant::select(&parsed.items, &variant);
        let trace = trace::trace_variant(&selection.inside, &selection.outside, &config);
        let html = render(&Document::new(Input {
            config: &config,
            items: &selection.inside,
            trace: &trace,
            tests: None,
            diagnostics: &parsed.diagnostics,
            variant: Some(("basis", &variant)),
            build: None,
        }))
        .unwrap();
        assert!(
            html.contains("<td><code>reqs&#x2f;a.md:7</code></td>"),
            "{html}"
        );
    }

    /// The tree as lines of `open`/`leaf`/`close` and the text, indented.
    fn tree_lines(config: &str, requirements: &str) -> Vec<String> {
        tree_lines_with_code(config, requirements, "")
    }

    fn tree_lines_with_code(config: &str, requirements: &str, code: &str) -> Vec<String> {
        let config = Config::parse(config).unwrap();
        let mut items = markdown::parse(Path::new("reqs/a.md"), requirements)
            .unwrap()
            .items;
        items.extend(
            shall_core::tags::scan(Path::new("src/a.c"), code)
                .unwrap()
                .items,
        );
        let trace = trace::trace(&items, &config);
        let document = Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
            variant: None,
            build: None,
        });
        let index = Index::new(&document);
        let mut depth = 0;
        let mut lines = Vec::new();
        for entry in tree(&document, &index) {
            match entry.kind {
                "open" => {
                    lines.push(format!(
                        "{}{} {}",
                        "  ".repeat(depth),
                        entry.sign,
                        entry.text
                    ));
                    depth += 1;
                }
                "leaf" => lines.push(format!(
                    "{}{} {}{}",
                    "  ".repeat(depth),
                    entry.sign,
                    entry.text,
                    if entry.repeated { " (again)" } else { "" }
                )),
                _ => depth -= 1,
            }
        }
        assert_eq!(depth, 0, "{lines:?}");
        lines
    }

    #[test]
    fn the_tree_shows_shared_items_once() {
        let lines = tree_lines(
            "[[level]]\ntype = \"sys\"\nname = \"S\"\nneeds = [\"swr\"]\n\n\
             [[level]]\ntype = \"swr\"\nname = \"W\"\nneeds = [\"dsn\"]\n\n\
             [[level]]\ntype = \"dsn\"\nname = \"D\"\n",
            "# A\n`sys~a-k4f2~1`\n\n# B\n`sys~b-k4f2~1`\n\n\
             # S\n`swr~s-k4f2~1`\nCovers:\n- sys~a-k4f2~1\n- sys~b-k4f2~1\n\n\
             # D\n`dsn~d-k4f2~1`\nCovers:\n- swr~s-k4f2~1\n\n\
             # T\n`swr~t-k4f2~1`\nCovers:\n- sys~b-k4f2~1\n",
        );
        assert_eq!(
            lines,
            [
                "✓ sys~a-k4f2~1",
                "  ✓ swr~s-k4f2~1",
                "    ✓ dsn~d-k4f2~1",
                "! sys~b-k4f2~1",
                "  ✓ swr~s-k4f2~1 (again)",
                "  ✗ swr~t-k4f2~1",
                "    ✗ dsn",
            ]
        );
    }

    #[test]
    fn the_tree_ends_cycles() {
        let lines = tree_lines(
            "",
            "# R\n`req~r-k4f2~1`\nNeeds: req\n\n\
             # A\n`req~a-k4f2~1`\nNeeds: req\nCovers:\n- req~r-k4f2~1\n- req~b-k4f2~1\n\n\
             # B\n`req~b-k4f2~1`\nNeeds: req\nCovers:\n- req~a-k4f2~1\n",
        );
        assert_eq!(
            lines,
            [
                "! req~r-k4f2~1",
                "  ! req~a-k4f2~1",
                "    ! req~b-k4f2~1",
                "      ! req~a-k4f2~1 (again)",
            ]
        );
    }

    #[test]
    fn the_tree_shows_rootless_cycles_and_open_tags() {
        // A cycle that nothing above it ends: its items become roots.
        let lines = tree_lines(
            "",
            "# A\n`req~a-k4f2~1`\nNeeds: req\nCovers:\n- req~b-k4f2~1\n\n\
             # B\n`req~b-k4f2~1`\nNeeds: req\nCovers:\n- req~a-k4f2~1\n\n\
             # L\n`req~l-k4f2~1`\nCovers:\n- req~a-k4f2~1\n- req~b-k4f2~1\n",
        );
        assert_eq!(
            lines,
            [
                "! req~a-k4f2~1",
                "  ! req~b-k4f2~1",
                "    ! req~a-k4f2~1 (again)",
                "    ✓ req~l-k4f2~1",
                "  ✓ req~l-k4f2~1",
            ]
        );
        // A tag that needs a test of its own shows the gap.
        let lines = tree_lines_with_code(
            "[[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n",
            "# D\n`dsn~d-k4f2~1`\n",
            "// [impl->dsn~d-k4f2~1>>utest]\nvoid f();\n",
        );
        assert_eq!(
            lines,
            [
                "! dsn~d-k4f2~1",
                "  ✗ [impl->dsn~d-k4f2~1>>utest]",
                "    ✗ utest",
            ]
        );
    }

    #[test]
    fn filters_offer_the_tags_of_requirements() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A \"quoted\"\n`dsn~a-k4f2~1`\n\nTags: a\"b, <t>\n",
        );
        assert!(
            html.contains("<option value=\"&lt;t&gt;\">&lt;t&gt;</option>"),
            "{html}"
        );
        assert!(
            html.contains("<option value=\"a&quot;b\">a&quot;b</option>"),
            "{html}"
        );
        assert!(
            html.contains("data-search=\"dsn~a-k4f2~1 a &quot;quoted&quot;\""),
            "{html}"
        );
        assert!(html.contains("data-tags=\"a&quot;b\n&lt;t&gt;\""), "{html}");
        assert!(
            html.contains("<form id=\"filter\" class=\"filter\" role=\"search\" hidden>"),
            "{html}"
        );
    }

    /// The graphs as lines `column: sign text` per node, sorted by column
    /// and row, and the number of edges.
    fn graph_lines(config: &str, requirements: &str, code: &str) -> Vec<(Vec<String>, usize)> {
        let config = Config::parse(config).unwrap();
        let mut items = markdown::parse(Path::new("reqs/a.md"), requirements)
            .unwrap()
            .items;
        items.extend(
            shall_core::tags::scan(Path::new("src/a.c"), code)
                .unwrap()
                .items,
        );
        let trace = trace::trace(&items, &config);
        let document = Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
            variant: None,
            build: None,
        });
        let index = Index::new(&document);
        graph::graphs(&document, &index)
            .iter()
            .map(|graph| {
                let json = serde_json::to_value(graph).unwrap();
                let mut nodes: Vec<(u64, u64, String)> = json["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|node| {
                        (
                            node["x"].as_u64().unwrap(),
                            node["y"].as_u64().unwrap(),
                            format!(
                                "{}: {} {}",
                                node["x"].as_u64().unwrap() / 268,
                                node["sign"].as_str().unwrap(),
                                node["first"].as_str().unwrap()
                            ),
                        )
                    })
                    .collect();
                nodes.sort();
                (
                    nodes.into_iter().map(|(_, _, line)| line).collect(),
                    json["edges"].as_array().unwrap().len(),
                )
            })
            .collect()
    }

    #[test]
    fn the_graph_puts_nodes_in_columns_and_bundles_tags() {
        let graphs = graph_lines(
            "[[level]]\ntype = \"sys\"\nname = \"S\"\nneeds = [\"swr\", \"dsn\"]\n\n\
             [[level]]\ntype = \"swr\"\nname = \"W\"\nneeds = [\"dsn\"]\n\n\
             [[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\", \"utest\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n",
            "# A\n`sys~a-k4f2~1`\n\n\
             # S\n`swr~s-k4f2~1`\nCovers:\n- sys~a-k4f2~1\n\n\
             # D\n`dsn~d-k4f2~1`\nCovers:\n- swr~s-k4f2~1\n- sys~a-k4f2~1\nNeeds: impl, utest, sys\n",
            "// [impl->dsn~d-k4f2~1]\nvoid f();\n// [impl->dsn~d-k4f2~1]\nvoid g();\n",
        );
        assert_eq!(graphs.len(), 1, "{graphs:?}");
        let (nodes, edges) = &graphs[0];
        assert_eq!(
            nodes,
            &[
                "0: ✗ sys~a-k4f2~1",
                "1: ✗ swr~s-k4f2~1",
                // Below swr~s and, directly, below sys~a: the longest path
                // decides the column.
                "2: ✗ dsn~d-k4f2~1",
                "3: ✓ impl ×2",
                "3: ✗ missing: utest",
                "3: ✗ missing: sys",
            ]
        );
        // sys~a→swr~s, sys~a→dsn~d, swr~s→dsn~d, dsn~d→impl, two missing.
        assert_eq!(*edges, 6);
    }

    #[test]
    fn the_graph_cuts_cycles() {
        let graphs = graph_lines(
            "",
            "# R\n`req~r-k4f2~1`\nNeeds: req\n\n\
             # A\n`req~a-k4f2~1`\nNeeds: req\nCovers:\n- req~r-k4f2~1\n- req~b-k4f2~1\n\n\
             # B\n`req~b-k4f2~1`\nNeeds: req\nCovers:\n- req~a-k4f2~1\n",
            "",
        );
        let (nodes, edges) = &graphs[0];
        assert_eq!(
            nodes,
            &[
                "0: ✗ req~r-k4f2~1",
                "1: ✗ req~a-k4f2~1",
                "2: ✗ req~b-k4f2~1"
            ]
        );
        // r→a, a→b; the edge b→a closes the cycle and is left out.
        assert_eq!(*edges, 2);
    }

    #[test]
    fn the_graph_draws_shared_parts_once() {
        let graphs = graph_lines(
            "[[level]]\ntype = \"sys\"\nname = \"S\"\nneeds = [\"swr\"]\n\n\
             [[level]]\ntype = \"swr\"\nname = \"W\"\nneeds = [\"dsn\"]\n\n\
             [[level]]\ntype = \"dsn\"\nname = \"D\"\n",
            "# A\n`sys~a-k4f2~1`\n\n# B\n`sys~b-k4f2~1`\n\n\
             # S\n`swr~s-k4f2~1`\nCovers:\n- sys~a-k4f2~1\n- sys~b-k4f2~1\n\n\
             # D\n`dsn~d-k4f2~1`\nCovers:\n- swr~s-k4f2~1\n",
            "",
        );
        assert_eq!(graphs.len(), 2);
        assert_eq!(
            graphs[0].0,
            [
                "0: ✓ sys~a-k4f2~1",
                "1: ✓ swr~s-k4f2~1",
                "2: ✓ dsn~d-k4f2~1"
            ]
        );
        // Drawn in full above: one node, without dsn~d.
        assert_eq!(graphs[1].0, ["0: ✓ sys~b-k4f2~1", "1: ✓ swr~s-k4f2~1"]);
    }

    #[test]
    fn tiles_group_the_roots_by_component() {
        let html = html(
            "[[level]]\ntype = \"sys\"\nname = \"S\"\n",
            "# A\n`sys~motor.a-k4f2~1`\n\n# B\n`sys~can.b-k4f2~1`\nStatus: draft\n\n\
             # C\n`sys~plain-k4f2~1`\n\n# D\n`sys~motor.d-k4f2~1`\nStatus: rejected\n",
        );
        let overview = &html[html.find("<section id=\"overview\">").unwrap()..];
        let overview = &overview[..overview.find("</section>").unwrap()];
        // Without a component first, then by name.
        let headings: Vec<&str> = overview
            .lines()
            .filter(|line| line.starts_with("<h3>"))
            .collect();
        assert_eq!(
            headings,
            [
                "<h3>Without component <span class=\"note\">1 green, 0 yellow, 0 red</span></h3>",
                "<h3><code>can</code> <span class=\"note\">0 green, 1 yellow, 0 red</span></h3>",
                "<h3><code>motor</code> <span class=\"note\">1 green, 0 yellow, 0 red, 1 grey</span></h3>",
            ]
        );
        assert!(
            overview.contains("<a class=\"tile yellow\" href=\"#graph-sys~can.b-k4f2~1\""),
            "{overview}"
        );
        assert!(
            html.contains("<details class=\"graph\" id=\"graph-sys~can.b-k4f2~1\">"),
            "{html}"
        );
    }
}
