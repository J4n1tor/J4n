//! What a doer asks and what is answered, as far as the loop needs it: the question's shape, the
//! answer, and the one check every answer passes — a pick is one of the offered options, a text is one short line. The
//! chain of answerers, the ask points and the deadlines of policy are the next step's; here the operator is the only
//! one who answers.

use crate::gate::Action;
use serde::{Deserialize, Serialize};

/// The longest answer text, and the longest question a doer may put (characters).
pub const TEXT_MAX: usize = 240;
pub const QUESTION_MAX: usize = 4000;
/// The most options one question offers.
pub const OPTIONS_MAX: usize = 12;

/// One offered option of a choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opt {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum AskShape {
    YesNo,
    /// Pick one of the options; free text is allowed too.
    Choice { options: Vec<Opt> },
    /// Permission for one action, `risk` 0–100. Only the operator (or a grant) answers it, never a model.
    Approval { action: String, risk: u8 },
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "lowercase")]
pub enum Answer {
    Yes,
    No,
    Pick { id: String },
    Text { text: String },
}

impl Answer {
    /// The answer as words, for a doer that is started again with it.
    pub fn render(&self) -> String {
        match self {
            Answer::Yes => "yes".into(),
            Answer::No => "no".into(),
            Answer::Pick { id } => id.clone(),
            Answer::Text { text } => text.clone(),
        }
    }
}

/// A question a doer puts to whoever can answer. `local` is the doer's own name for it, which comes back with the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskSpec {
    pub local: String,
    pub question: String,
    #[serde(flatten)]
    pub shape: AskShape,
    /// The doer waits for the answer (true), or ends its run and expects to be started again with it (false).
    pub inband: bool,
    /// The ask point that says who may answer it; left out, the point of its shape (`approval` or `question`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    /// For an approval: the effect it asks about, so the Gate can answer it before the operator is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act: Option<Box<Action>>,
}

fn one_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

impl AskShape {
    /// Why `a` is not an answer to this shape, in words the operator can read.
    pub fn check(&self, a: &Answer) -> Result<(), String> {
        let text_ok = |t: &str| {
            if t.trim().is_empty() || t.chars().count() > TEXT_MAX || !one_line(t) {
                Err(format!("a text answer is one line of at most {TEXT_MAX} characters"))
            } else {
                Ok(())
            }
        };
        match (self, a) {
            (AskShape::YesNo | AskShape::Approval { .. }, Answer::Yes | Answer::No) => Ok(()),
            (AskShape::Choice { options }, Answer::Pick { id }) => {
                if options.iter().any(|o| &o.id == id) {
                    Ok(())
                } else {
                    let offered: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
                    Err(format!("`{id}` is not one of the offered options ({})", offered.join(", ")))
                }
            }
            (AskShape::Choice { .. } | AskShape::Text, Answer::Text { text }) => text_ok(text),
            (AskShape::YesNo, _) => Err("this question takes yes or no".into()),
            (AskShape::Approval { .. }, _) => Err("an approval takes yes or no".into()),
            (AskShape::Choice { .. }, _) => Err("this question takes one of the options or a line of text".into()),
            (AskShape::Text, _) => Err("this question takes a line of text".into()),
        }
    }
}

impl AskSpec {
    /// Whether a doer's question can be shown at all.
    pub fn check(&self) -> Result<(), String> {
        if self.local.is_empty() || self.local.len() > 64 || !one_line(&self.local) {
            return Err("a question needs a short name of its own".into());
        }
        if self.question.trim().is_empty() || self.question.chars().count() > QUESTION_MAX {
            return Err(format!("a question is between 1 and {QUESTION_MAX} characters"));
        }
        if let AskShape::Choice { options } = &self.shape {
            if options.is_empty() || options.len() > OPTIONS_MAX {
                return Err(format!("a choice offers between 1 and {OPTIONS_MAX} options"));
            }
            for (i, o) in options.iter().enumerate() {
                if o.id.is_empty() || o.id.len() > 64 || !one_line(&o.id) || !one_line(&o.label) {
                    return Err("an option has a short id and a one-line label".into());
                }
                if options[..i].iter().any(|p| p.id == o.id) {
                    return Err(format!("the option `{}` is offered twice", o.id));
                }
            }
        }
        if let AskShape::Approval { risk, .. } = &self.shape
            && *risk > 100
        {
            return Err("an approval's risk is 0 to 100".into());
        }
        Ok(())
    }
}
