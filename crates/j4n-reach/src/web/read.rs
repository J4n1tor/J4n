//! The words of a page. HTML is read as text: what is inside `<main>` or `<article>` when the page has one, else the body; script,
//! style, navigation, headers, footers and asides are left out; block tags end a line, list items begin one with a dash;
//! entities are decoded. Anything that is text already (plain, JSON, XML) is itself. The result is cut, and the cut is said.

const SKIPPED: [&str; 9] = ["script", "style", "noscript", "svg", "template", "head", "nav", "footer", "aside"];
const BLOCKS: [&str; 24] = ["p", "div", "section", "article", "main", "br", "tr", "ul", "ol", "table", "pre", "blockquote", "h1", "h2", "h3", "h4", "h5", "h6", "form", "fieldset", "figure", "details", "dl", "hr"];

/// The text of a character reference, if it is one.
fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        "mdash" => Some('—'),
        "ndash" => Some('–'),
        "hellip" => Some('…'),
        "copy" => Some('©'),
        _ => {
            let n = name.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match after.find(';').filter(|e| *e <= 10).and_then(|e| entity(&after[..e]).map(|c| (e, c))) {
            Some((e, c)) => {
                out.push(c);
                rest = &after[e + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The name of a tag: `<p class=a>` is `p`, `</p>` is `/p`.
fn tag_name(inside: &str) -> String {
    let t = inside.trim_start();
    let (close, t) = t.strip_prefix('/').map_or((false, t), |r| (true, r));
    let name: String = t.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    if close { format!("/{name}") } else { name }
}

/// The part of the page worth reading: inside `<main>` or `<article>` when there is one.
fn main_part(html: &str) -> &str {
    let low = html.to_ascii_lowercase();
    for tag in ["main", "article"] {
        if let Some(open) = low.find(&format!("<{tag}")).and_then(|o| low[o..].find('>').map(|c| o + c + 1))
            && let Some(close) = low[open..].rfind(&format!("</{tag}"))
        {
            return &html[open..open + close];
        }
    }
    html
}

/// The title of the page, if it has one.
pub fn title(html: &str) -> Option<String> {
    let low = html.to_ascii_lowercase();
    let open = low.find("<title").and_then(|o| low[o..].find('>').map(|c| o + c + 1))?;
    let close = low[open..].find("</title")?;
    let t = decode(html[open..open + close].trim());
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty()).then_some(t)
}

pub fn html_text(html: &str) -> String {
    let html = main_part(html);
    let (mut out, mut skip, mut chars) = (String::new(), Vec::<String>::new(), html.char_indices().peekable());
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut String| {
        let words = decode(text).split_whitespace().collect::<Vec<_>>().join(" ");
        if !words.is_empty() {
            if !out.is_empty() && !out.ends_with(['\n', ' ']) {
                out.push(' ');
            }
            out.push_str(&words);
        }
        text.clear();
    };
    while let Some((i, c)) = chars.next() {
        if c != '<' {
            if skip.is_empty() {
                text.push(c);
            }
            continue;
        }
        // a comment, a doctype, or a tag
        if html[i..].starts_with("<!--") {
            let end = html[i + 4..].find("-->").map_or(html.len(), |e| i + 4 + e + 3);
            while chars.peek().is_some_and(|(j, _)| *j < end) {
                chars.next();
            }
            continue;
        }
        let end = html[i..].find('>').map_or(html.len(), |e| i + e + 1);
        let name = tag_name(&html[i + 1..end.saturating_sub(1).max(i + 1)]);
        while chars.peek().is_some_and(|(j, _)| *j < end) {
            chars.next();
        }
        let bare = name.trim_start_matches('/');
        let closing = name.starts_with('/');
        if SKIPPED.contains(&bare) {
            if closing {
                if let Some(at) = skip.iter().rposition(|s| s == bare) {
                    skip.truncate(at);
                }
            } else if !html[i..end].ends_with("/>") {
                flush(&mut text, &mut out);
                skip.push(bare.to_string());
            }
            continue;
        }
        if !skip.is_empty() {
            continue;
        }
        if BLOCKS.contains(&bare) {
            flush(&mut text, &mut out);
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
        } else if bare == "li" && !closing {
            flush(&mut text, &mut out);
            if !out.ends_with('\n') && !out.is_empty() {
                out.push('\n');
            }
            out.push_str("- ");
        } else if bare == "li" || bare == "td" || bare == "th" {
            flush(&mut text, &mut out);
            if bare == "li" {
                out.push('\n');
            } else {
                out.push_str(" | ");
            }
        }
    }
    flush(&mut text, &mut out);
    // runs of blank lines are one
    let mut lines: Vec<&str> = Vec::new();
    for l in out.lines().map(str::trim_end) {
        if l.is_empty() && lines.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        lines.push(l);
    }
    lines.join("\n").trim().to_string()
}
