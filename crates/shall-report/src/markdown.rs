//! Rendering the Markdown of requirement texts as HTML.
//!
//! Requirement files are written by many people and reviewed as text, so
//! the report must not run anything from them: raw HTML is shown as text,
//! and links and images keep only addresses without a scheme (relative
//! ones and `#anchors`) or with `http`, `https` or `mailto`. A link with
//! another scheme, such as `javascript:`, is shown as its text, an image
//! as its description. Headings are moved below the heading of the
//! requirement: the shallowest heading of a text becomes `<h5>`, the
//! others keep their distance to it, down to `<h6>`.

use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};

/// Renders Markdown as HTML that is safe to embed in the report.
pub fn render(text: &str) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    // Whether each open link or image was dropped, to drop its end too.
    let mut dropped = Vec::new();
    let shallowest = Parser::new_ext(text, options)
        .filter_map(|event| match event {
            Event::Start(Tag::Heading { level, .. }) => Some(level as usize),
            _ => None,
        })
        .min()
        .unwrap_or(1);
    let below = |level: HeadingLevel| {
        let shifted = (level as usize + 5 - shallowest).min(6);
        HeadingLevel::try_from(shifted).expect("a heading level from 5 to 6")
    };
    let events = Parser::new_ext(text, options).filter_map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Some(Event::Text(raw)),
        Event::Start(Tag::Link { ref dest_url, .. } | Tag::Image { ref dest_url, .. }) => {
            let safe = is_safe_url(dest_url);
            dropped.push(!safe);
            safe.then_some(event)
        }
        Event::End(TagEnd::Link | TagEnd::Image) => {
            let was_dropped = dropped.pop().unwrap_or(false);
            (!was_dropped).then_some(event)
        }
        Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) => Some(Event::Start(Tag::Heading {
            level: below(level),
            id,
            classes,
            attrs,
        })),
        Event::End(TagEnd::Heading(level)) => Some(Event::End(TagEnd::Heading(below(level)))),
        event => Some(event),
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

/// Whether a link address may stay: without a scheme, or with `http`,
/// `https` or `mailto`.
fn is_safe_url(url: &CowStr<'_>) -> bool {
    let url = url.trim_start();
    let scheme_end = url.find(':');
    let path_start = url.find(['/', '?', '#']);
    match (scheme_end, path_start) {
        (None, _) => true,
        (Some(colon), Some(path)) if path < colon => true,
        (Some(colon), _) => {
            let scheme = url[..colon].to_ascii_lowercase();
            matches!(scheme.as_str(), "http" | "https" | "mailto")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_common_markdown() {
        assert_eq!(
            render("Shuts down within **100 ms**.\n\n- one\n- `two`\n"),
            "<p>Shuts down within <strong>100 ms</strong>.</p>\n\
             <ul>\n<li>one</li>\n<li><code>two</code></li>\n</ul>\n"
        );
        assert!(render("| a | b |\n|---|---|\n| 1 | 2 |\n").contains("<table>"));
        assert_eq!(
            render("# One\n## Two\n### Three\n"),
            "<h5>One</h5>\n<h6>Two</h6>\n<h6>Three</h6>\n"
        );
        assert_eq!(
            render("### Inputs\n#### Sensor\n"),
            "<h5>Inputs</h5>\n<h6>Sensor</h6>\n"
        );
    }

    #[test]
    fn shows_raw_html_as_text() {
        assert_eq!(
            render("<script>alert(1)</script>\n\nText <b onclick=\"x\">bold</b>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;\n\
             <p>Text &lt;b onclick=\"x\"&gt;bold&lt;/b&gt;</p>\n"
        );
    }

    #[test]
    fn keeps_only_safe_links() {
        assert_eq!(
            render("[spec](https://example.org/a) [rel](../x.md#y) [me](mailto:a@b.c)"),
            "<p><a href=\"https://example.org/a\">spec</a> <a href=\"../x.md#y\">rel</a> \
             <a href=\"mailto:a@b.c\">me</a></p>\n"
        );
        assert_eq!(
            render("[click](javascript:alert(1)) ![x](data:image/png;base64,AA) [ok](a/b:c)"),
            "<p>click x <a href=\"a/b:c\">ok</a></p>\n"
        );
        assert_eq!(render("[x]( JavaScript:alert(1))"), "<p>x</p>\n");
    }
}
