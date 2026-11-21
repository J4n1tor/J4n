//! A line the operator types is on the screen at once, whatever the core is doing about it. The front, the loop,
//! the Log and the server thread (`Front::connect`) are the real ones; Jev, the light model that routes and weighs a line, is the fixture's
//! decider made slow (`Fx::lag`), as the operator's is. What is timed is the span from the `Up` the screen sends for ⏎ to the first `Down::Rows`
//! that holds the line.

use crate::front_cases::{Fx, chatty, fx_deciding};
use j4n_core::front::Cfg;
use j4n_proto::{Down, Intent, Up};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// A model that takes this long over a question, as the operator's does.
const LAG: Duration = Duration::from_millis(700);

fn holds(d: &Down, text: &str) -> bool {
    matches!(d, Down::Rows { bind, rows, .. } if bind == "chat:main" && rows.iter().any(|r| r.parent.is_none() && r.cells.first().is_some_and(|c| c.text == text)))
}

/// Milliseconds from the line sent to the first rows that hold it, over a client on threads.
fn echo_ms(f: &Fx, text: &str) -> u128 {
    let (tx, rx): (_, Receiver<Down>) = f.front.connect();
    tx.send(Up::Hello { api: j4n_proto::API }).unwrap();
    tx.send(Up::Watch { bind: "chat:main".into() }).unwrap();
    // the client is up when the chat's first rows are there
    let ready = Instant::now() + Duration::from_secs(10);
    while Instant::now() < ready {
        if matches!(rx.recv_timeout(Duration::from_millis(50)), Ok(Down::Rows { bind, .. }) if bind == "chat:main") {
            break;
        }
    }
    *f.lag.lock().unwrap() = LAG;
    let began = Instant::now();
    tx.send(Up::Intent { intent: Intent::Say { text: text.into(), with: Vec::new() } }).unwrap();
    while began.elapsed() < Duration::from_secs(10) {
        if let Ok(d) = rx.recv_timeout(Duration::from_millis(5))
            && holds(&d, text)
        {
            return began.elapsed().as_millis();
        }
    }
    panic!("the line never came back as a row: {}", f.rig.dump());
}

fn on(c: &mut Cfg) {
    c.chats.on = true;
}

#[test]
fn a_line_to_the_orchestrator_is_a_row_before_jev_has_said_where_it_goes() {
    let f = fx_deciding(vec![chatty("claude", &["one", "two", "three"])], Some("chats=new"), on);
    f.turn("fix trailing commas in parser.rs");
    // no file of the first chat is named, so code cannot route it and Jev is asked, and Jev takes `LAG`
    let ms = echo_ms(&f, "rename the helper in lexer.rs");
    eprintln!("typed line, route: the line is a row {ms} ms after it was sent; Jev takes {} ms", LAG.as_millis());
    assert!(ms < LAG.as_millis() / 2, "the row waited for the routing: {ms} ms of {} ms", LAG.as_millis());
}

#[test]
fn a_line_in_a_chat_is_a_row_before_the_weighing_of_it_is_over() {
    let f = fx_deciding(vec![chatty("claude", &["one", "two", "three"])], Some("weigh=task"), |c| c.weigh.on = true);
    f.turn("what does main.rs do?");
    let ms = echo_ms(&f, "explain how the tokenizer decides where a string literal ends");
    assert!(f.asked.lock().unwrap().iter().any(|q| q.point == "weigh"), "Jev was asked to weigh it: {:?}", f.asked.lock().unwrap().iter().map(|q| q.point.clone()).collect::<Vec<_>>());
    eprintln!("typed line, weigh: the line is a row {ms} ms after it was sent; Jev takes {} ms", LAG.as_millis());
    assert!(ms < LAG.as_millis() / 2, "the row waited for the weighing: {ms} ms of {} ms", LAG.as_millis());
}
