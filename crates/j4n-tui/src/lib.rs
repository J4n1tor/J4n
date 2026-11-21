//! The TUI framework. It draws what `j4n-proto` carries and nothing else (`docs/CHARTER.md`).
//!
//! - [`app`]: the state (the tree, the streams, the rows, the draft) and what a key does;
//! - [`view`]: the tree drawn, field by field, in a plain style; [`wait`]: a part that waits for the core says so (loading, empty, filled);
//! - [`term`]: the real terminal; [`headless`]: the same screen in memory, for scripts and tests;
//! - [`nodes`] and [`fuzzy`]: the tree of pages (tabs, sub-tabs, sections, stack, split, grid; lazy children) and the jump to a node;
//! - [`scene`] and [`golden`]: a script of messages and keys run into memory, and the screen kept as text and as ANSI;
//! - [`fields`]: the value fields, one component for each type of value, in each state;
//! - [`display`]: the fields that show (numbers, series, chips, buttons, tables, trees, cards, logs) and the windows of rows they hold;
//! - [`ask`]: the questions the core waits on: inline, the popup, the tray, and the keys that answer them;
//! - [`chat`]: the transcript on pipes (turn › lane › step) and the composer;
//! - [`shape`]: the card above the composer, and the draft's timing;
//! - [`theme`]: the palette's twenty-four roles, the four tiers of glyphs, and `NO_COLOR`;
//! - [`parts`]: the parts every tab is built from (the pipe row, the cursor and its scroll, a line of text, the popup frame, the toast, the form);
//! - [`keysrow`]: the keys row at the bottom, from the hints the core sent, and the sheet `?` opens;
//! - [`keymap`]: which key a pressed key is taken for, from the map the core sent;
//! - [`wrap`]: text cut to the width a terminal shows it at.

pub mod age;
pub mod app;
pub mod ask;
pub mod chat;
pub mod display;
pub mod fields;
pub mod fuzzy;
pub mod golden;
pub mod headless;
pub mod keymap;
pub mod keysrow;
pub mod leader;
pub mod nodes;
pub mod parts;
pub mod read;
pub mod restart;
pub mod scene;
pub mod shape;
pub mod theme;
pub mod term;
pub mod view;
pub mod wait;
pub mod wrap;

pub use app::App;
