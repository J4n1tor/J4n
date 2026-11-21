//! The parts library: what every tab is drawn and keyed with, written once. A page, a field or a
//! card takes these and writes none of them again: `rows` (the pipe of a row), `select` (the cursor and its keys), `scroll` (keep the row in
//! sight), `line` (a line of text with a cursor), `popup` (the frame of a box), `toast` (one line of news), `pick` (a choice drawn as an enum), `form` (fields kept as a draft), `track` (the sign that a line is being taken, or that a part waits for the core), `hint` (the words a running step takes, in the composer), `suggest` (what Tab completes).

pub mod form;
pub mod hint;
pub mod line;
pub mod pick;
pub mod popup;
pub mod rows;
pub mod scroll;
pub mod select;
pub mod suggest;
pub mod toast;
pub mod track;
