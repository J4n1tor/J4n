//! The glyph slots and the four tiers that fill them: `icons` (Material Design outline, a slot of two
//! cells each), `unicode` (letters this font has), `ascii` (7-bit) and `words`. A tier is chosen whole, and every slot has a glyph in
//! every tier: the table below is one row a slot, four glyphs a row, and a test reads it. The words tier takes the unicode bricks and keys.

use super::palette::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Icons,
    Unicode,
    Ascii,
    Words,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::Icons, Tier::Unicode, Tier::Ascii, Tier::Words];

    pub fn name(self) -> &'static str {
        ["icons", "unicode", "ascii", "words"][self as usize]
    }

    pub fn parse(s: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == s)
    }
}

/// The mark of a setting changed since the start that acts from the next one: `⟳`, or `~` and words where the tier has no such
/// glyph. Not a slot, so the board of the slots (`sheet::glyphs`) does not list it.
pub fn waits(tier: Tier) -> &'static str {
    match tier {
        Tier::Icons | Tier::Unicode => "⟳ next start",
        Tier::Ascii => "~ next start",
        Tier::Words => "next start",
    }
}

/// `Slot => (icons, unicode, ascii, words, the role it is drawn in)`.
macro_rules! slots {
    ($($slot:ident $name:literal => ($i:literal, $u:literal, $a:literal, $w:literal, $role:ident)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Slot { $($slot),+ }

        impl Slot {
            pub const ALL: &'static [Slot] = &[$(Slot::$slot),+];

            pub fn name(self) -> &'static str {
                match self { $(Slot::$slot => $name),+ }
            }

            /// The slot called `name`, as a node names its icon.
            pub fn by_name(name: &str) -> Option<Slot> {
                Slot::ALL.iter().copied().find(|s| s.name() == name)
            }

            /// The glyph in a tier.
            pub fn cell(self, tier: Tier) -> &'static str {
                let row = match self { $(Slot::$slot => [$i, $u, $a, $w]),+ };
                row[tier as usize]
            }

            /// The role the glyph is drawn in.
            pub fn role(self) -> Role {
                match self { $(Slot::$slot => Role::$role),+ }
            }
        }
    };
}

slots! {
    You "you" => ("❯", "❯", ">", "you", You),
    Route "route" => ("\u{F0641}", "➜", "^", "route", Route),
    Queued "queued" => ("\u{F0130}", "⊸", "o", "queued", Faint),
    Working "working" => ("\u{F0F1B}", "▷", ">", "●", Accent),
    Ask "ask" => ("\u{F0625}", "◇", "?", "ask", Warn),
    Cooling "cooling" => ("\u{F051B}", "◔", "z", "cool", Warn),
    Done "done" => ("\u{F05E1}", "✓", "v", "✓", Ok),
    Failed "failed" => ("\u{F015A}", "✗", "x", "✗", Error),
    Stopped "stopped" => ("\u{F0667}", "■", "#", "stopped", Faint),
    Read "read" => ("\u{F06D0}", "≡", "r", "read", Dim),
    Search "search" => ("\u{F0349}", "⊙", "/", "grep", Dim),
    Edit "edit" => ("\u{F0CB6}", "±", "e", "edit", Accent),
    New "new" => ("\u{F0EED}", "⊕", "+", "write", Accent),
    Run "run" => ("\u{F07B7}", "❱", "$", "run", Run),
    Fetch "fetch" => ("\u{F059F}", "↓", "@", "fetch", Fetch),
    Tool "tool" => ("\u{F0BE0}", "◈", "*", "tool", Fetch),
    Output "output" => ("\u{F09ED}", "¶", "|", "out", Dim),
    Todo "todo" => ("\u{F0131}", "◌", "[ ]", "todo", Dim),
    TodoDone "todo-done" => ("\u{F0132}", "✓", "[x]", "✓", Ok),
    Think "think" => ("\u{F0336}", "∴", ":", "think", Faint),
    Error "error" => ("\u{F05D6}", "⊗", "!", "fail", Error),
    Control "control" => ("\u{F08BB}", "∙", ".", "·", Faint),
    Session "session" => ("\u{F0339}", "∞", "=", "session", Dim),
    Grant "grant" => ("\u{F0CC8}", "⊞", "g", "grant", Dim),
    Compact "compact" => ("\u{F084C}", "⊟", "c", "compact", Dim),
    Notice "notice" => ("\u{F009C}", "△", "i", "note", Dim),
    Worktree "worktree" => ("\u{F062C}", "⊢", "w", "wt", Dim),
    Commit "commit" => ("\u{F0718}", "◍", "%", "commit", Dim),
    ReadOnly "read-only" => ("\u{F0341}", "⊘", "-", "ro", Dim),
    Connector "connector" => ("\u{F06A5}", "↔", "&", "link", Fetch),
    Message "message" => ("\u{F01EE}", "◫", "m", "msg", Route),
    Decide "decide" => ("\u{F05D1}", "⇌", "d", "decider", Accent),
    DecideOff "decide-off" => ("\u{F05D1}", "⇌", "d", "decider", Faint),
    Round "round" => ("\u{F0456}", "↩", "'", "round", Route),
    Yes "yes" => ("\u{F012C}", "✓", "y", "yes", Ok),
    No "no" => ("\u{F0156}", "✗", "n", "no", Error),
    FoldClosed "fold-closed" => ("▸", "▸", ">", "▸", Faint),
    FoldOpen "fold-open" => ("▾", "▾", "v", "▾", Faint),
    Hint "hint" => ("⇥", "⇥", "tab", "⇥", Accent),
    Made "made" => ("~", "~", "~", "~", Made),
    Pane "pane" => ("\u{F0BCC}", "◁", "]", "pane", Accent),
    PipeBar "pipe-bar" => ("│", "│", "|", "│", Edge),
    PipeTee "pipe-tee" => ("├─", "├─", "|-", "├─", Edge),
    PipeElbow "pipe-elbow" => ("╰─", "╰─", "`-", "╰─", Edge),
    PipeTop "pipe-top" => ("╭─", "╭─", ".-", "╭─", Edge),
    PipeSquare "pipe-square" => ("└─", "└─", "`-", "└─", Edge),
    AiryTop "airy-top" => ("·", "·", ".", "·", Edge),
    AiryChild "airy-child" => ("›", "›", ">", "›", Edge),
    YouCapL "you-cap-l" => ("▌", "▌", "[", "▌", Accent),
    YouCapR "you-cap-r" => ("▐", "▐", "]", "▐", Accent),
    BranchTee "branch-tee" => ("├", "├", "|", "├", Edge),
    BranchElbow "branch-elbow" => ("╰", "╰", "`", "╰", Edge),
    BranchSquare "branch-square" => ("└", "└", "`", "└", Edge),
    HangBar "hang-bar" => ("┆", "┆", ":", "┆", Edge),
    Focus "focus" => ("┃", "┃", "#", "┃", Focus),
    BoxTl "box-tl" => ("╭", "╭", "+", "╭", Edge),
    BoxTr "box-tr" => ("╮", "╮", "+", "╮", Edge),
    BoxBl "box-bl" => ("╰", "╰", "+", "╰", Edge),
    BoxBr "box-br" => ("╯", "╯", "+", "╯", Edge),
    PopTl "pop-tl" => ("┌", "┌", "+", "┌", Edge),
    PopTr "pop-tr" => ("┐", "┐", "+", "┐", Edge),
    PopBl "pop-bl" => ("└", "└", "+", "└", Edge),
    PopBr "pop-br" => ("┘", "┘", "+", "┘", Edge),
    BoxH "box-h" => ("─", "─", "-", "─", Edge),
    BoxV "box-v" => ("│", "│", "|", "│", Edge),
    RuleDashed "rule-dashed" => ("╌", "╌", ".", "╌", Edge),
    RuleTop "rule-top" => ("▔", "▔", "-", "▔", Accent),
    RuleHeavy "rule-heavy" => ("━", "━", "=", "━", Accent),
    FillLeft "fill-left" => ("▕", "▕", "[", "▕", Edge),
    FillRight "fill-right" => ("▏", "▏", "]", "▏", Edge),
    FillFull "fill-full" => ("█", "█", "#", "█", Accent),
    FillHalf "fill-half" => ("▌", "▌", "#", "▌", Accent),
    FillEmpty "fill-empty" => ("░", "░", "-", "░", Edge),
    Spark "spark" => ("▁▃▅█", "▁▃▅█", "_.-#", "▁▃▅█", Accent),
    KeyEnter "key-enter" => ("⏎", "⏎", "enter", "⏎", Dim),
    KeyTab "key-tab" => ("⇥", "⇥", "tab", "⇥", Dim),
    KeyShift "key-shift" => ("⇧", "⇧", "S-", "⇧", Dim),
    KeyCtrl "key-ctrl" => ("⌃", "⌃", "C-", "⌃", Dim),
    KeyAlt "key-alt" => ("⌥", "⌥", "M-", "⌥", Dim),
    KeyArrows "key-arrows" => ("↑↓←→", "↑↓←→", "^v<>", "↑↓←→", Dim),
    BoolOn "bool-on" => ("\u{F0521}", "●", "[x]", "on", Ok),
    BoolOff "bool-off" => ("\u{F0522}", "○", "[ ]", "off", Dim),
    RadioOn "radio-on" => ("\u{F043E}", "◉", "(*)", "(x)", Accent),
    RadioOff "radio-off" => ("\u{F043D}", "◎", "( )", "( )", Dim),
    TickOn "tick-on" => ("\u{F0132}", "☑", "[x]", "[x]", Accent),
    TickOff "tick-off" => ("\u{F0131}", "☐", "[ ]", "[ ]", Dim),
    Folder "folder" => ("\u{F0256}", "◧", "/", "dir", Dim),
    Add "add" => ("\u{F0415}", "✚", "+", "add", Accent),
    Remove "remove" => ("\u{F0374}", "⊖", "-", "del", Dim),
    StepLeft "step-left" => ("‹", "‹", "<", "‹", Accent),
    StepRight "step-right" => ("›", "›", ">", "›", Accent),
    Chevron "chevron" => ("›", "›", ">", "›", Faint),
    Caret "caret" => ("▏", "▏", "|", "▏", Accent),
    Mask "mask" => ("•", "•", "*", "•", Dim),
    SparkLevels "spark-levels" => ("▁▂▃▄▅▆▇█", "▁▂▃▄▅▆▇█", "_.-=#", "▁▂▃▄▅▆▇█", Accent),
    MeterParts "meter-parts" => ("▏▎▍▌▋▊▉", "▏▎▍▌▋▊▉", "#", "▏▎▍▌▋▊▉", Accent),
    HeavyTl "heavy-tl" => ("┏", "┏", "+", "┏", Focus),
    HeavyTr "heavy-tr" => ("┓", "┓", "+", "┓", Focus),
    HeavyBl "heavy-bl" => ("┗", "┗", "+", "┗", Focus),
    HeavyBr "heavy-br" => ("┛", "┛", "+", "┛", Focus),
    Thumb "thumb" => ("┃", "┃", "#", "┃", Faint),
    SortUp "sort-up" => ("⇑", "⇑", "^", "⇑", Accent),
    SortDown "sort-down" => ("⇓", "⇓", "v", "⇓", Accent),
    DeltaUp "delta-up" => ("▲", "▲", "+", "▲", Ok),
    DeltaDown "delta-down" => ("▼", "▼", "-", "▼", Error),
    DeltaFlat "delta-flat" => ("─", "─", "=", "─", Dim),
    Tag "tag" => ("\u{F09EE}", "#", "#", "tag", Accent),
    Target "target" => ("→", "→", "->", "→", Dim),
    FillMid "fill-mid" => ("▒", "▒", "=", "▒", Accent),
    Bullet "bullet" => ("•", "•", "*", "•", Dim),
    Bullet2 "bullet-2" => ("◦", "◦", "-", "◦", Dim),
    TaskOpen "task-open" => ("\u{F0131}", "◌", "[ ]", "[ ]", Dim),
    TaskDone "task-done" => ("\u{F0135}", "▣", "[x]", "[x]", Ok),
    QuoteBar "quote-bar" => ("▏", "▏", "|", "▏", Dim),
    Rule "rule" => ("─", "─", "-", "─", Edge),
    Quote "quote" => ("▎", "▎", "|", "▎", Edge),
    Risky "risky" => ("\u{F0CC8}", "⚠", "!", "risk", Warn),
    AskChoice "ask-choice" => ("\u{F0279}", "⋮", "=", "pick", Dim),
    AskScore "ask-score" => ("\u{F04D2}", "★", "#", "score", Dim),
    AskText "ask-text" => ("\u{F0A16}", "✎", "\"", "text", Dim),
    TabChat "chat" => ("\u{F0369}", "❝", "c", "chat", Dim),
    TabTasks "tasks" => ("\u{F0756}", "▤", "t", "tasks", Dim),
    TabModels "models" => ("\u{F09D1}", "❖", "m", "models", Dim),
    TabGrow "grow" => ("\u{F0E66}", "↗", "g", "grow", Dim),
    TabPacks "packs" => ("\u{F03D6}", "▦", "p", "packs", Dim),
    TabSpend "spend" => ("\u{F01C1}", "$", "$", "spend", Dim),
    TabLog "log" => ("\u{F09AA}", "▥", "l", "log", Dim),
    TabSettings "settings" => ("\u{F0493}", "⊚", "s", "settings", Dim),
    // the numbers of the tabs 1–9: small beside a tab, plain under one; a tier of plain digits always puts them under
    TabNumber "tab-number" => ("¹²³⁴⁵⁶⁷⁸⁹", "¹²³⁴⁵⁶⁷⁸⁹", "123456789", "123456789", Dim),
    // where a word of a list is cut short: one cell, three dots in ascii
    Cut "cut" => ("…", "…", "...", "…", Dim),
}

impl Slot {
    /// The square-cornered twin of a rounded corner: what `corners = "square"` draws (popups and trays follow the theme).
    pub fn squared(self) -> Slot {
        match self {
            Slot::BoxTl => Slot::PopTl,
            Slot::BoxTr => Slot::PopTr,
            Slot::BoxBl => Slot::PopBl,
            Slot::BoxBr => Slot::PopBr,
            s => s,
        }
    }
}
