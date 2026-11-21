//! The formatter table, the `when` table and the template parser, with their values written in J4n's kinds.
//! Where J4n answers differently from the common choice on purpose the case says so.

use j4n_core::eval::Scope;
use j4n_core::expr::Expr;
use j4n_core::fmt::{Fmt, Format, Piece, format_value};
use j4n_core::value::Value;
use std::collections::BTreeMap;

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn ms(s: u64) -> Value {
    Value::Dur(s * 1000)
}

#[test]
fn every_formatter_over_every_kind_it_takes_and_the_words_for_the_rest() {
    use Format::*;
    let usd = Value::usd;
    // (value, format, shown)
    let cases: Vec<(Value, Option<Format>, &str)> = vec![
        // usd
        (Value::Usd(0), Some(Usd), "$0.00"),
        (usd(0.125), Some(Usd), "$0.13"), // not $0.12 (float ties to even): money rounds half up
        (usd(12.5), Some(Usd), "$12.50"),
        (Value::Num(3.0), Some(Usd), "$3.00"),
        (usd(1.999), None, "$2.00"),
        (usd(0.004), None, "$0.0040"), // new: under half a cent, four places
        (usd(0.00001), None, "<$0.0001"),
        // pct: a share of one (a bare number is not already a percent: 7 is 700%) and held to nothing: 104% is a
        // limit passed, not a fraction to clamp
        (Value::Num(0.0), Some(Pct), "0%"),
        (Value::Num(0.666), Some(Pct), "67%"),
        (Value::Num(1.0), Some(Pct), "100%"),
        (Value::Num(1.7), Some(Pct), "170%"),
        (Value::Num(-0.2), Some(Pct), "-20%"),
        (Value::Num(7.0), Some(Pct), "700%"),
        // n
        (Value::Num(0.0), Some(N), "0"),
        (Value::Num(3.0), Some(N), "3"),
        (Value::Num(2.5), Some(N), "2.5"),
        (Value::Num(2.345), None, "2.35"),
        (Value::Num(1234567.0), Some(N), "1234567"),
        // dur
        (Value::Dur(0), Some(Dur), "0s"),
        (ms(59), Some(Dur), "59s"),
        (ms(60), Some(Dur), "1m00s"),
        (ms(185), None, "3m05s"),
        (ms(3599), Some(Dur), "59m59s"),
        (ms(3600), Some(Dur), "1h00m"),
        (ms(3720), Some(Dur), "1h02m"),
        (ms(86_400), Some(Dur), "1d00h"),
        (ms(183_600), Some(Dur), "2d03h"),
        (Value::Num(90.0), Some(Dur), "1m30s"),
        (Value::Num(-5.0), Some(Dur), "0s"),
        (Value::Dur(850), None, "850ms"), // new: under a second
        // text, and the kinds that are their own words
        (Value::text("plant"), Some(Text), "plant"),
        (Value::text("plant"), None, "plant"),
        (Value::Num(4.0), Some(Text), "4"),
        (usd(0.5), Some(Text), "$0.50"),
        // a formatter that does not fit is the kind's own words, not an error
        (Value::text("done"), Some(Usd), "done"),
        (Value::text("x"), Some(Pct), "x"),
        (Value::Usd(2_000_000), Some(N), "$2.00"),
        (Value::Dur(500_000), Some(Pct), "8m20s"),
        // nothing behind it
        (Value::Missing, None, "—"),
        (Value::Missing, Some(Usd), "—"),
        (Value::Missing, Some(Dur), "—"),
        (Value::text(""), None, "—"),
        (Value::Num(f64::NAN), Some(N), "—"),
        (Value::Num(f64::INFINITY), Some(Usd), "—"),
        // a list is its items (the lint keeps it out of text; this is only totality)
        (Value::List(vec![]), None, "—"),
        (Value::List(vec!["a".into(), "b".into()]), None, "a, b"),
    ];
    for (v, f, want) in cases {
        assert_eq!(format_value(&v, f), want, "{v:?} as {f:?}");
    }
    for f in Format::ALL {
        assert!(
            ["usd", "pct", "n", "dur", "text", "short", "rungs", "runs", "fallback"].contains(&f.word()),
            "a formatter was added: give it rows here"
        );
    }
}

fn rig() -> Scope {
    Scope::new().with(
        "rig",
        map(&[
            ("standing", "running".into()),
            ("spent_usd", Value::usd(0.5)),
            ("progress", Value::Num(0.25)),
            ("elapsed", ms(90)),
            ("phases_done", Value::Num(0.0)),
            ("awaiting", "".into()),
            ("id", "plant".into()),
            ("phases", Value::List(vec!["a  done".into()])),
        ]),
    )
}

#[test]
fn when_is_a_table_of_comparisons_and_nothing_compares_with_nothing() {
    let (with, without) = (rig(), Scope::new());
    let cases: &[(&str, bool, bool)] = &[
        // (when, with a rig, with no rig at all)
        ("rig.standing == 'running'", true, false),
        ("rig.standing != 'running'", false, false),
        ("rig.standing != 'done'", true, false),
        ("rig.standing == 'done'", false, false),
        ("rig.spent_usd >= $0.5", true, false),
        ("rig.spent_usd > $0.5", false, false),
        ("rig.spent_usd < $0.51", true, false),
        ("rig.spent_usd <= $0.49", false, false),
        ("rig.spent_usd == $0.5", true, false),
        ("rig.spent_usd != $0.5", false, false),
        ("rig.spent_usd > 0.4", true, false), // a bare number against money counts in dollars
        ("rig.progress < 0.5", true, false),
        ("rig.elapsed >= 60s", true, false),
        ("rig.elapsed < 60s", false, false),
        ("rig.id == 'plant'", true, false),
        ("rig.id == \"plant\"", true, false),
        // a path alone: yes when it is there and not empty or zero
        ("rig.id", true, false),
        ("rig.standing", true, false),
        ("rig.spent_usd", true, false),
        ("rig.phases_done", false, false),
        ("rig.awaiting", false, false),
        ("rig.phases", true, false),
        // nothing to read, or kinds that do not compare: no, never a panic
        ("rig.nope", false, false),
        ("rig.spent_usd > 'cheap'", false, false),
        ("rig.phases == 'a'", false, false),
        ("rig.elapsed > 60", false, false), // a bare number is not a duration
    ];
    for (when, yes, none) in cases {
        let e = Expr::parse(when).unwrap_or_else(|e| panic!("`{when}`: {e}"));
        assert_eq!(e.holds(&with), *yes, "`{when}` with a rig");
        assert_eq!(e.holds(&without), *none, "`{when}` with no rig");
    }
    for bad in ["", "not a path", "rig.a rig.b", "== 3", "rig.id ==", "rig.id = 'x'"] {
        assert!(Expr::parse(bad).is_err(), "`{bad}` should not parse");
    }
}

#[test]
fn templates_split_into_text_and_holes_and_braces_can_be_literal() {
    let pieces = Fmt::parse("{rig.phases_done}/{rig.phases_total} done {{ok}}").unwrap();
    assert_eq!(pieces.pieces().len(), 4);
    assert!(matches!(&pieces.pieces()[1], Piece::Lit(l) if l == "/"));
    assert!(matches!(&pieces.pieces()[3], Piece::Lit(l) if l == " done {ok}"));
    assert!(
        matches!(&pieces.pieces()[0], Piece::Ref { expr, format: None } if *expr == Expr::parse("rig.phases_done").unwrap())
    );
    let formatted = Fmt::parse("{rig.spent_usd:usd}").unwrap();
    assert!(matches!(formatted.pieces(), [Piece::Ref { format: Some(Format::Usd), .. }]));
    for bad in ["{rig.spent_usd", "a } b", "{}", "{rig.x:eur}", "{ :usd}"] {
        assert!(Fmt::parse(bad).is_err(), "`{bad}` should not parse");
    }
}

#[test]
fn a_template_fills_its_holes_and_a_missing_one_is_a_dash_in_place() {
    let s = Scope::new().with(
        "rig",
        map(&[
            ("id", "plant".into()),
            ("standing", "parked".into()),
            ("spent_usd", Value::usd(0.31)),
            ("progress", Value::Num(0.5)),
            ("elapsed", ms(125)),
        ]),
    );
    let f = Fmt::parse("{rig.id} is {rig.standing} · {rig.spent_usd:usd} of {rig.cap_usd:usd}").unwrap();
    assert_eq!(f.render(&s), "plant is parked · $0.31 of —");
    let g = Fmt::parse("{rig.progress:pct} through, {rig.elapsed:dur} in").unwrap();
    assert_eq!(g.render(&s), "50% through, 2m05s in");
    assert_eq!(f.render(&Scope::new()), "— is — · — of —");
}
