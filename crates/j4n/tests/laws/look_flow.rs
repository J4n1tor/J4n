//! `:look` says how the screen looks while J4n runs. The word is written for this project as a proposal that is accepted at once and
//! that `:undo` takes back; the screen draws it as soon as it is said, and the next start reads it from the file. The front, the proposer,
//! the Writer and the screen are the real ones, wired as the binary wires them.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use std::fs;
use std::time::{Duration, Instant};

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn held(d: &Dirs) -> String {
    fs::read_to_string(here_dir(&d.j4n_home(), &d.project()).join("look.toml")).unwrap_or_default()
}

/// A turn drawn in the gutter look stands apart from the left edge, in a column of its own.
fn in_gutter(s: &str) -> bool {
    s.lines().any(|l| l.starts_with("        ") && l.contains("hello"))
}

#[test]
fn a_word_of_the_look_is_drawn_at_once_is_written_for_the_project_and_is_taken_back_by_undo() {
    let d = Dirs::new("look-live");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":look frame gutter");
    op.until("the word said back", |s| s.contains("frame is gutter"));
    let file = held(&d);
    assert!(file.contains("theme.frame") && file.contains("gutter"), "{file}");
    op.type_line("hello");
    // the screen draws a line it sent at once, as its own (`sending`), before the core has taken it: the turn the next start reads
    // is the Log's, so the world is closed only once the Log holds the line and its task is over
    let end = Instant::now() + crate::support::PATIENCE;
    while !world.front.log().read(0).any(|e| e.kind == "chat.say" && e.data.get("text").and_then(|t| t.as_str()) == Some("hello")) {
        assert!(Instant::now() < end, "the line never reached the Log");
        std::thread::sleep(Duration::from_millis(20));
    }
    crate::screen_drive::wait_idle(&world);
    let s = op.until("the turn in the gutter", |s| in_gutter(s) && s.lines().filter(|l| l.contains("hello")).count() >= 2);
    assert!(!s.lines().any(|l| l.contains("├─")), "pipes in the gutter look:\n{s}");
    // the next start reads it from the file
    drop(op);
    world.close();
    drop(world);
    let again = open(&d);
    assert_eq!(again.front.look().frame, "gutter");
    let mut op = Operator::sit(&again);
    op.type_line(":undo");
    op.until("the word taken back", |s| s.contains("took back 1 change"));
    assert_eq!(again.front.look().frame, "round");
    op.until("round again", |s| s.lines().any(|l| l.contains("├─")) && !in_gutter(s));
    assert!(!held(&d).contains("gutter"), "{}", held(&d));
    drop(op);
    again.close();
}

#[test]
fn a_word_the_screen_does_not_know_is_refused_and_nothing_is_written() {
    let d = Dirs::new("look-refused");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":look frame cloud");
    op.until("the words said", |s| s.contains("cloud is not a word of frame") && s.contains("pipes, gutter, hang"));
    op.type_line(":look hue red");
    op.until("the parts said", |s| s.contains("no part hue") && s.contains("title_place"));
    op.type_line(":look frame round");
    op.until("said already", |s| s.contains("frame is round already"));
    op.type_line(":look motion false");
    op.until("a switch is a word too", |s| s.contains("motion is false"));
    assert_eq!(held(&d).matches("[[setting]]").count(), 1, "{}", held(&d));
    assert!(held(&d).contains("value = false"), "{}", held(&d));
    drop(op);
    world.close();
}

/// The marks are `auto` out of the box and the screen is sent the tier it comes to (the icons in a test run, which says
/// `J4N_FONTS=icons`); a word the operator says wins, the tier `auto` came to among them, and `auto` is a word that takes it back.
#[test]
fn the_marks_are_auto_until_a_word_is_said_and_the_screen_is_always_sent_a_tier() {
    let d = Dirs::new("look-marks");
    let world = open(&d);
    assert_eq!(world.front.look().marks, "icons", "auto, sent as its tier");
    let mut op = Operator::sit(&world);
    op.type_line(":look marks");
    op.until("what auto came to, and why", |s| s.contains("marks is auto · icons: J4N_FONTS") && s.contains("auto, icons"));
    drop(op);
    world.front.say("main", ":look marks icons");
    assert!(held(&d).contains("theme.marks") && held(&d).contains("value = \"icons\""), "the tier auto came to is a word of its own: {}", held(&d));
    world.front.say("main", ":look marks unicode");
    assert_eq!(world.front.look().marks, "unicode", "the operator's word wins");
    world.front.say("main", ":look marks auto");
    assert!(held(&d).contains("value = \"auto\""), "{}", held(&d));
    assert_eq!(world.front.look().marks, "icons", "auto is never sent as a word");
    world.close();
}

#[test]
fn a_screen_that_did_not_say_it_is_sent_the_look_when_another_changes_it() {
    use j4n_proto::{Down, Up};
    let d = Dirs::new("look-other");
    let world = open(&d);
    let (up, down) = world.front.connect();
    up.send(Up::Hello { api: j4n_proto::API }).unwrap();
    let mut frame = None;
    let end = Instant::now() + crate::support::PATIENCE;
    while let Ok(msg) = down.recv_timeout(Duration::from_millis(200)) {
        if let Down::Theme { look } = msg {
            frame = Some(look.frame);
            break;
        }
        assert!(Instant::now() < end, "no look came with the hello");
    }
    assert_eq!(frame.as_deref(), Some("round"), "the hello carries the look");
    world.front.say("main", ":look frame hang");
    let end = Instant::now() + crate::support::PATIENCE;
    let look = loop {
        match down.recv_timeout(Duration::from_millis(200)) {
            Ok(Down::Theme { look }) => break look,
            _ => assert!(Instant::now() < end, "the look was not sent again"),
        }
    };
    assert_eq!(look.frame, "hang");
    drop(up);
    world.close();
}

#[test]
fn an_undo_falls_back_to_what_a_layer_below_says() {
    let d = Dirs::new("look-below");
    fs::create_dir_all(d.j4n_home().join("ui")).unwrap();
    let row = "api = 1\n[[setting]]\nid = \"theme.frame\"\ndoc = \"x\"\nty = \"str\"\ndefault = \"pipes\"\nvalue = \"hang\"\n";
    fs::write(d.j4n_home().join("ui/look.toml"), row).unwrap();
    let world = open(&d);
    assert_eq!(world.front.look().frame, "hang", "the home file says it");
    world.front.say("main", ":look frame gutter");
    assert_eq!(world.front.look().frame, "gutter");
    world.front.say("main", ":undo");
    assert_eq!(world.front.look().frame, "hang", "the layer below, not the default");
    world.close();
}

#[test]
fn the_screen_that_says_it_is_sent_the_look_at_once_and_one_that_says_hello_after_gets_the_look_it_is_at() {
    use j4n_proto::{Down, Intent, Up};
    let d = Dirs::new("look-serve");
    let world = open(&d);
    let mut said = j4n_core::front::serve::Serve::new(world.front.clone());
    let mut late = j4n_core::front::serve::Serve::new(world.front.clone());
    said.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = said.up(Up::Intent { intent: Intent::Say { text: ":look frame hang".into(), with: Vec::new() } });
    assert!(downs.iter().any(|d| matches!(d, Down::Theme { look } if look.frame == "hang")), "sent with the answer, not at the next tick: {downs:?}");
    let (hello, _) = late.up(Up::Hello { api: j4n_proto::API });
    assert!(hello.iter().any(|d| matches!(d, Down::Theme { look } if look.frame == "hang")), "{hello:?}");
    assert!(!late.tick().iter().any(|d| matches!(d, Down::Theme { .. })), "and it is not sent again");
    world.close();
}
