//! The TUI through its public API only: messages in (`Down`), keys in, `Up` out, and the screen drawn into memory.
//! No core, no files, no terminal: the framework draws what the protocol carries and nothing else.

mod age_cases;
mod app_cases;
mod ask_cases;
mod chat_cases;
// the CPU a thread spent, for the tests that hold a speed: one file for both test crates
#[path = "../../../j4n-core/tests/core/cpu.rs"]
mod cpu;
mod display_cases;
mod dither_cases;
mod event_cases;
mod frame_cases;
mod gauge_cases;
mod golden_cases;
mod immediate_answer_cases;
mod ground_cases;
mod key_cases;
mod layout_cases;
mod keys_row_cases;
mod node_cases;
mod parts_cases;
mod plot_cases;
mod list_rows_cases;
mod read_cases;
mod tab_cases;
mod theme_cases;
mod approval_card_cases;
mod value_cases;
mod wait_cases;
mod screen_cases;
mod settings_cases;
mod shape_cases;
mod speed_cases;
mod world;
mod wrap_cases;
