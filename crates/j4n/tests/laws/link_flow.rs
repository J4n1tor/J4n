//! Links on the real binary: the rows of the kind `link` an operator writes in their own settings are read through the pack's kind (no notice), `crew.links =
//! declared` holds an envoy's request to them, and a request a row carries enters Main as it did under the star.

use crate::envoy_flow::{args, open_with, talked};
use std::time::Duration;

const DECLARED: &str = "[[setting]]\nid = \"crew.links\"\nvalue = \"declared\"\n";

/// The charter is drafted and signed in the envoy's chat, as the operator does it.
fn signed(world: &j4n::wire::World, chat: &str, task: &str) {
    let proposal = world.front.envoy_charter(task, &args()).expect("drafted").split_whitespace().next().unwrap().to_string();
    world.front.say(chat, &format!(":accept {proposal}"));
    world.front.say(chat, &format!(":accept {proposal}"));
}

#[test]
fn declared_rows_are_read_and_hold_an_envoys_request_and_a_row_that_carries_it_lets_it_enter_main() {
    // no row carries a request: it is refused with the reason, and Main hears nothing
    let (_d, world) = open_with("link-none", &format!("{DECLARED}[[link]]\nid = \"back\"\nfrom = \"chat\"\nto = \"main\"\ncarries = [\"report\"]\n"));
    assert!(world.notices.is_empty(), "the rows read through the pack's kind: {:?}", world.notices);
    assert!(world.front.cfg().crew.links.declared() && world.front.cfg().crew.links.rows().len() == 1);
    let (chat, task) = talked(&world);
    signed(&world, &chat, &task);
    let why = world.front.envoy_request(&task, "k1", "tidy the docs").unwrap_err();
    assert_eq!(why, "no link carries a request from envoy:c1 to main");
    world.front.log().sync();
    assert!(world.front.log().read(0).any(|e| e.kind == "crew.refused" && e.data["carries"] == "request") && world.front.log().read(0).all(|e| e.kind != "crew.request"));
    world.close();
    // a row that carries it, with one hop: the first request goes, the second is the one that stops
    let (_d, world) = open_with("link-carried", &format!("{DECLARED}[[link]]\nid = \"ask\"\nfrom = \"envoy\"\nto = \"main\"\ncarries = [\"request\"]\nhops = 1\n"));
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let (chat, task) = talked(&world);
    signed(&world, &chat, &task);
    world.front.envoy_request(&task, "k1", "tidy the docs").expect("the row carries it");
    for t in world.front.tasks().book().rows() {
        world.front.tasks().book().wait(&t.id, |r| r.state.is_terminal(), Duration::from_secs(20));
    }
    let why = world.front.envoy_request(&task, "k1", "tidy the docs again").unwrap_err();
    assert_eq!(why, "charter k1 has sent Main 1 requests: ask the operator before another");
    world.close();
}
