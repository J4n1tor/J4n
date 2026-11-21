//! A change as lines: the files it touches, and for each, the hunks with their added and removed lines and the numbers they stand
//! at. Read from `git diff` (unified), or made for a file taken as it stands (every line added).

/// One line of a hunk, with the number it has in the file (new for an added line, old for a removed one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub no: u32,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hunk {
    /// What git says the hunk is in (the function it sits in), or empty.
    pub header: String,
    pub added: Vec<Line>,
    pub removed: Vec<Line>,
    /// The hunk as unified diff text, for the state a judge reads.
    pub text: String,
}

impl Hunk {
    pub fn changed(&self) -> usize {
        self.added.len() + self.removed.len()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub deleted: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn added(&self) -> impl Iterator<Item = &Line> {
        self.hunks.iter().flat_map(|h| h.added.iter())
    }
}

/// A file taken as it stands: one hunk with every line added.
pub fn whole(path: &str, content: &str) -> FileDiff {
    let added: Vec<Line> = content.lines().enumerate().map(|(i, l)| Line { no: i as u32 + 1, text: l.to_string() }).collect();
    let text = added.iter().map(|l| format!("+{}", l.text)).collect::<Vec<_>>().join("\n");
    FileDiff { path: path.to_string(), deleted: false, hunks: vec![Hunk { header: String::new(), added, removed: Vec::new(), text }] }
}

/// `@@ -12,3 +14,5 @@ fn name` → (old start, new start, the rest).
fn hunk_head(line: &str) -> Option<(u32, u32, String)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, tail) = rest.split_once(" @@")?;
    let start = |s: &str| s.split(',').next().and_then(|n| n.parse::<u32>().ok());
    Some((start(old)?, start(new)?, tail.trim().to_string()))
}

/// The files of a unified diff (`git diff --no-color`), in the order git wrote them.
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // `a/x b/x`: the new name is the second half
            let path = rest.rsplit_once(" b/").map_or("", |(_, b)| b).to_string();
            files.push(FileDiff { path, deleted: false, hunks: Vec::new() });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if !in_hunk {
            if let Some(p) = line.strip_prefix("+++ ") {
                if let Some(path) = p.strip_prefix("b/") {
                    file.path = path.to_string();
                }
            } else if line == "+++ /dev/null" || line.starts_with("deleted file mode") {
                file.deleted = true;
            }
        }
        if let Some((old, new, header)) = hunk_head(line) {
            (old_no, new_no, in_hunk) = (old, new, true);
            file.hunks.push(Hunk { header, text: line.to_string(), ..Hunk::default() });
            continue;
        }
        if !in_hunk {
            continue;
        }
        let Some(h) = file.hunks.last_mut() else { continue };
        match line.chars().next() {
            Some('+') => {
                h.added.push(Line { no: new_no, text: line[1..].to_string() });
                new_no += 1;
            }
            Some('-') => {
                h.removed.push(Line { no: old_no, text: line[1..].to_string() });
                old_no += 1;
            }
            Some(' ') => {
                (old_no, new_no) = (old_no + 1, new_no + 1);
            }
            _ => continue,
        }
        h.text.push('\n');
        h.text.push_str(line);
    }
    files.retain(|f| !f.hunks.is_empty() || f.deleted);
    files
}
