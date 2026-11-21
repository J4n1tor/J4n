//! Cutting text to a width: by what a terminal shows, not by what a string holds.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// `text` as lines no wider than `width` columns: words stay whole when they fit, a word wider than a line is cut, a newline
/// is a newline. An empty text is one empty line.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let (mut line, mut used) = (String::new(), 0);
        let mut space = false;
        for word in raw.split(' ') {
            let w = word.width();
            // the space between two words is one more column, or the place where the line breaks
            if space {
                if used > 0 && used + 1 + w > width {
                    out.push(std::mem::take(&mut line));
                    used = 0;
                } else {
                    line.push(' ');
                    used += 1;
                }
            }
            space = true;
            if w <= width.saturating_sub(used) {
                line.push_str(word);
                used += w;
                continue;
            }
            for ch in word.chars() {
                let cw = ch.width().unwrap_or(0);
                if used + cw > width {
                    out.push(std::mem::take(&mut line));
                    used = 0;
                }
                line.push(ch);
                used += cw;
            }
        }
        out.push(line);
    }
    out
}

/// `text` with blanks after it up to `width` columns.
pub fn pad(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

/// `text` cut to `width` columns, with an ellipsis when it was longer.
pub fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let (mut out, mut used) = (String::new(), 0);
    for ch in text.chars() {
        let cw = ch.width().unwrap_or(0);
        if used + cw + 1 > width {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}
