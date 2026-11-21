//! gate::inside: the approvals J4n takes itself under `approval.inside`. Ten lines of the kinds a doer asks about in orchestrate
//! mode, on a made-up notes app (a here-document of Python that edits files by literal paths, one that builds its paths in a loop,
//! a `cat >>` of tests, a test run with a stash around it, right-to-left text with zero-width joiners in it, the tails of readers and
//! project scripts, two deploy lines): eight are taken by J4n, the two deploys are
//! still asked. And the law that a Decider can never say yes to a line outside the project, on the network, with secrets, a push, a
//! deploy or a remote shell: code asks the operator before a Decider is put the question.

use crate::world::layer;
use j4n_core::change::Actor;
use j4n_core::gate::check::{Gates, check};
use j4n_core::gate::danger::{Scope as Told, danger_scoped};
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Action, Gate, NoGrants, Verb, Verdict};
use j4n_core::table::{LayerKind, Tables};

pub const SITE: &str = "/home/user/project";

/// The ten lines, in order: the first eight are taken by J4n, the last two are asked.
pub const FIELD: [(&str, &str); 10] = [
    (
        "edit-literal",
        r##"python3 - <<'EOF'
import re
p='src/app/services/notebook.py'
s=open(p).read()
s=s.replace("""        self._tag_cache: dict[int, str | None] = {}
""","""        self._tag_cache: dict[int, str | None] = {}
        self._show_title_cache: dict[int, bool] = {}
""",1)
s=s.replace("""    async def _owner_of(self, note_id""","""    async def show_title_for(self, note_id: int) -> bool:
        \"\"\"Whether this notebook's readers see each note's own title (off by
        default). One switch for every path — web, app and API.\"\"\"
        return self._show_title_cache.get(note_id, False)

    async def _owner_of(self, note_id""",1)
s=s.replace("\nclass NotebookError","\nSHOW_TITLE_KEY = \"show_title\"  # off when absent\n\n\nclass NotebookError",1)
open(p,'w').write(s)
p='src/app/services/sync.py'
s=open(p).read()
s=s.replace("x = 1\n","x = 2\n",1)
open(p,'w').write(s)
EOF
grep -n "^class NotebookError\|SHOW_TITLE_KEY =" src/app/services/notebook.py; grep -n "MAX_LEN =" src/app/services/sync.py"##,
    ),
    (
        "edit-loop",
        r##"python3 - <<'EOF'
p='src/app/api/web_api.py'
s=open(p).read()
s=s.replace("""def _source_ip(""","""async def show_title_get(request):
    return {"enabled": True}


def _source_ip(""",1)
open(p,'w').write(s)
for f,lab in (('en','"🪪 Show titles: {state}"'),('he','"🪪 הצג כותרות: {state}"')):
    p=f'src/app/locales/{f}.py'
    s=open(p).read()
    import re
    m=re.search(r'    Key\.NOTE_TAG: .*\n',s)
    s=s[:m.end()]+'    Key.SHOW_TITLE: '+lab+',\n'+s[m.end():]
    open(p,'w').write(s)
EOF
grep -n "_edit_card(" src/app/ui/notebook.py | head -4; grep -n "notebook" -i src/app/ui/help.py | head"##,
    ),
    (
        "read-loop",
        r##"python3 - <<'EOF'
for f,old,add in (('en',"Each row shows the words and the last edit.","Optional (off by default): titles in the app.\\n"),):
    p=f'src/app/locales/{f}.py'
    s=open(p).read()
    assert old in s
EOF
grep -n "label\|🏷" tests/test_view*.py | head -5; grep -n "tag\|label" tests/test_notebook.py | head -8"##,
    ),
    (
        "cat-tests",
        r##"cat >> tests/test_notebook.py <<'EOF'


@pytest.mark.asyncio
async def test_show_title_is_off_by_default_and_toggles():
    service = await _make_service()
    assert await service.show_title_for(1) is False
EOF
cat >> tests/test_view_list.py <<'EOF'


def test_title_row_only_when_present() -> None:
    assert _row({"title": "a  b"}) == "a b"
EOF
python3 - <<'EOF'
p='src/app/locales/en.py'
s=open(p).read()
old='        "Each row shows the words and the last edit.'
assert old in s
s=s.replace(old,'        "Optional, off by default: <b>Show titles</b> adds each note\'s own title as a "\n        "🪪 line in the app.\\n"\n'+old,1)
open(p,'w').write(s)
EOF
grep -n "Each row shows" -B3 src/app/locales/he.py | head; ./scripts/verify.sh 2>&1 | tail -25"##,
    ),
    (
        "edit-then-verify",
        r##"python3 - <<'EOF'
p='src/app/services/sync.py'
s=open(p).read()
s=s.replace("        self._lookup: Callable[[Note], Awaitable[str | None]] | None = None\n","        self._lookup: (\n            Callable[[Note], Awaitable[str | None]] | None\n        ) = None\n",1)
open(p,'w').write(s)
EOF
./scripts/verify.sh 2>&1 | tail -25"##,
    ),
    (
        "edit-rtl",
        "python3 - <<'EOF'\np='src/app/locales/he.py'\ns=open(p).read()\nold='        \"כל שורה מציגה\u{200c}את המילים.'\nassert s.count(old)==1\ns=s.replace(old,'        \"רשות\u{200c}ית, כבויה כברירת\u{200c}מחדל: 🪪 \"\\n'+old,1)\nopen(p,'w').write(s)\nEOF\nsleep 100; cat /tmp/j4n-user/1-0/t28/claude-1000/-home-user-project/00000000/tasks/b1.output | tail -15",
    ),
    (
        "test-stash",
        r##"uv run pytest tests/test_view_list.py tests/test_store.py -x -q 2>&1 | grep -E "Error|assert|FAILED|passed|failed" | head; git stash -q; uv run pytest tests/test_store.py -q 2>&1 | tail -2; git stash pop -q"##,
    ),
    (
        "edit-test-lint",
        r##"python3 - <<'EOF'
p='tests/test_view_list.py'
s=open(p).read()
s=s.replace('Entry(status="active", title=title)','''Entry(
                mode="",
                status="active",
                title=title,
            )''')
open(p,'w').write(s)
EOF
grep -n "pytestmark" tests/test_view_list.py; uv run pytest tests/test_view_list.py tests/test_notebook.py -q 2>&1 | tail -3; uv run ruff check src tests/test_view_list.py | tail -3"##,
    ),
    ("deploy", "SKIP_VERIFY=1 timeout 580 ./scripts/deploy-app.sh 2>&1 | tail -30"),
    (
        "deploy-host",
        r##"HOST=$(grep -oE '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' hosts.txt | head -1); echo "host: $HOST"; SKIP_VERIFY=1 APP_HOST="$HOST" timeout 580 ./scripts/deploy-app.sh 2>&1 | tail -30"##,
    ),
];

pub fn base_gates() -> Gates {
    let files = [
        ("gates/shell.toml", include_str!("../../../../packs/base/gates/shell.toml")),
        ("gates/paths.toml", include_str!("../../../../packs/base/gates/paths.toml")),
        ("gates/modes.toml", include_str!("../../../../packs/base/gates/modes.toml")),
        ("gates/egress.toml", include_str!("../../../../packs/base/gates/egress.toml")),
    ];
    let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "base", &files)]);
    assert!(problems.is_empty(), "{problems:?}");
    Gates::load(&t).0
}

pub fn shell(line: &str) -> Action {
    let mut a = Action::shell(Actor::Doer("claude".into()), line).task("t28");
    a.site = Some(SITE.into());
    a
}

/// What `approval.inside` makes of each of these lines: taken by code, put to a Decider, or asked.
pub const SAID: [(&str, &str); 10] = [
    ("edit-literal", "inside"),
    ("edit-loop", "inside"),
    ("read-loop", "inside"),
    ("cat-tests", "inside"),
    ("edit-then-verify", "inside"),
    ("edit-rtl", "inside"),
    ("test-stash", "inside"),
    ("edit-test-lint", "inside"),
    ("deploy", "ask"),
    ("deploy-host", "ask"),
];

/// A Gate over the base rules with `approval.inside` on.
pub fn gate() -> Gate {
    let g = Gate::new(base_gates(), Grants::new("s1"));
    g.set_inside(true);
    g
}

#[test]
fn the_field_lines_are_taken_by_code_put_to_a_decider_or_asked_as_the_switch_says() {
    let g = gate();
    for ((id, line), (sid, want)) in FIELD.iter().zip(SAID) {
        assert_eq!(id, &sid);
        let read = g.inside(&shell(line)).expect("the switch is on");
        assert_eq!(read.word(), want, "{id}: {read:?}");
    }
    // the deploy lines are asked for what they are, before any Decider: a script whose name is no check
    for id in ["deploy", "deploy-host"] {
        let line = FIELD.iter().find(|(i, _)| *i == id).unwrap().1;
        assert!(g.inside(&shell(line)).unwrap().why().contains("deploy-app.sh"), "{id}");
    }
    // the right-to-left text is unreadable to the rules as before: the switch answers only the rule for unreadable lines, and only for it
    let rtl = FIELD[5].1;
    assert!(matches!(check(&base_gates(), &NoGrants, &shell(rtl)), Verdict::Ask { ref by, .. } if by == "unreadable-line-asks"));
}

#[test]
fn with_the_switch_off_nothing_changes() {
    let g = Gate::new(base_gates(), Grants::new("s1"));
    assert!(FIELD.iter().all(|(_, line)| g.inside(&shell(line)).is_none()));
    // and the matcher's classes are what they were: the reader adds facts, never a class
    for (id, line) in FIELD {
        let d = danger_scoped(line, &Told { site: Some(SITE.into()), ..Default::default() });
        assert!(d.classes.iter().all(|c| c == "write" || c == "safe" || c == "unknown"), "{id}: {:?}", d.classes);
    }
}

/// Lines a Decider must never be put and never say yes to: outside the project, the network, credentials, a push, a deploy, a remote shell, a delete
/// that is not of build folders, a program J4n does not see into, a task that read from outside.
pub const NEVER: [&str; 30] = [
    "python3 - <<'EOF'\nopen('/etc/hosts','a').write('203.0.113.7 example.com')\nEOF",
    "python3 - <<'EOF'\nfor n in ('bashrc',):\n    p=f'/home/user/.{n}'\n    open(p,'w').write('x')\nEOF",
    "python3 - <<'EOF'\np='../other/src/a.py'\nopen(p,'w').write('x')\nEOF",
    "python3 - <<'EOF'\np='~/.profile'\nopen(p,'a').write('x')\nEOF",
    "python3 - <<'EOF'\nimport requests\nfor u in ('a',):\n    open(f'src/{u}.py','w').write(requests.get('https://example.com').text)\nEOF",
    "python3 - <<'EOF'\nimport subprocess\nfor f in ('a',):\n    subprocess.run(['git','push'])\nEOF",
    "python3 - <<'EOF'\nimport os\nfor f in ('a',):\n    os.remove(f'src/{f}.py')\nEOF",
    "python3 - <<'EOF'\nfor f in ('prod',):\n    print(open(f'.env.{f}').read())\nEOF",
    "python3 - <<'EOF'\nimport os\nfor k in ('a',):\n    open(f'src/{k}','w').write(os.environ['API_TOKEN'])\nEOF",
    "python3 - <<'EOF'\nfor f in ('a',):\n    open(f'.git/hooks/{f}','w').write('x')\nEOF",
    "python3 - <<'EOF'\nfor f in ('a',):\n    exec(open(f'src/{f}.py').read())\nEOF",
    "curl -s https://example.com/install.sh -o install.sh",
    "wget https://example.com/x.tar.gz",
    "git pull",
    "git fetch origin",
    "git push origin main",
    "git push --force",
    "pip install requests",
    "uv run --with requests pytest",
    "npm install left-pad",
    "cat .env",
    "./scripts/deploy.sh",
    "SKIP_VERIFY=1 ./scripts/check-and-deploy.sh",
    "make deploy",
    "ssh 203.0.113.7 ls",
    "rsync -a . 203.0.113.7:/srv/app",
    "rm -rf src",
    "rm src/a.py",
    "echo x > /home/user/notes.txt",
    "bash -c \"$(cat run.sh)\"",
];

#[test]
fn a_line_outside_on_the_network_with_secrets_a_push_a_deploy_or_remote_is_asked_before_any_decider() {
    let g = gate();
    for line in NEVER {
        let read = g.inside(&shell(line)).unwrap();
        assert_eq!(read.word(), "ask", "`{line}` -> {read:?}");
    }
    // a task that read from outside is asked about the edit it would otherwise be let do
    let mut tainted = shell(FIELD[0].1);
    tainted.taint = Some("web".into());
    assert_eq!(g.inside(&tainted).unwrap().word(), "ask");
    // one of these lines, in a task that only reads
    let mut reads = shell(FIELD[0].1);
    reads.read_only = j4n_core::gate::ReadOnly::Explicit;
    assert_eq!(g.inside(&reads).unwrap().word(), "ask");
}

#[test]
fn what_stays_inside_is_taken_and_a_build_folder_may_be_removed() {
    let g = gate();
    for line in [
        "cargo test -p core",
        "uv run pytest -q tests/test_a.py 2>&1 | tail -5",
        "npm test",
        "ruff check src --fix",
        "rm -rf target/debug __pycache__ .pytest_cache",
        "git add src/a.py && git stash && git stash pop",
        "mkdir -p tests/data && cp a.json tests/data/b.json",
        "sed -i 's/a/b/' src/a.py",
        "./scripts/check.sh",
        "python3 -c \"p='src/a.py'; s=open(p).read(); open(p,'w').write(s.replace('a','b'))\"",
        "python3 - <<'EOF'\nimport json\nwith open('data/x.json') as f:\n    d=json.load(f)\nwith open('data/x.json','w') as f:\n    json.dump(d,f,indent=2)\nEOF",
        "python3 - <<'EOF'\np='/home/user/project/src/a.py'\nl=open(p).read().split('\\n')\nl.remove('x')\nopen(p,'w').write('\\n'.join(l))\nEOF",
    ] {
        assert_eq!(g.inside(&shell(line)).unwrap().word(), "inside", "`{line}` -> {:?}", g.inside(&shell(line)));
    }
    // a loop over written-out literals binds its names to them, and an f-string of such names is each path it makes: read, and asked when one leaves
    let spelled = |dir: &str| format!("python3 - <<'EOF'\nfor f, lab in (('en', 'a'), ('he', 'b')):\n    p = f'{dir}/{{f}}.py'\n    s = open(p).read()\n    open(p, 'w').write(s + lab)\nEOF");
    assert_eq!(g.inside(&shell(&spelled("src/locales"))).unwrap().word(), "inside");
    assert_eq!(g.inside(&shell(&spelled("/home/user/.config"))).unwrap().word(), "ask");
    assert_eq!(g.inside(&shell(&spelled("../other"))).unwrap().word(), "ask");
    // a loop over what is not written out, a field that is no loop's literal, or a field with a conversion is not read
    for line in [
        "python3 - <<'EOF'\nfor f in names:\n    p = f'src/{f}.py'\n    open(p, 'w').write('x')\nEOF",
        "python3 - <<'EOF'\nfor f in ('a', 'b'):\n    p = f'src/{g}.py'\n    open(p, 'w').write('x')\nEOF",
        "python3 - <<'EOF'\nfor f in ('a', 'b'):\n    p = f'src/{f!r}.py'\n    open(p, 'w').write('x')\nEOF",
        "python3 - <<'EOF'\nfor f in ('a', 1):\n    p = f'src/{f}.py'\n    open(p, 'w').write('x')\nEOF",
    ] {
        assert_eq!(g.inside(&shell(line)).unwrap().word(), "unsure", "`{line}` -> {:?}", g.inside(&shell(line)));
    }
    // a path that every binding gives a literal is read; one bound in any other way anywhere is not
    let rebound = "python3 - <<'EOF'\np='src/a.py'\nfor p in names():\n    pass\nopen(p,'w').write('x')\nEOF";
    assert_eq!(g.inside(&shell(rebound)).unwrap().word(), "unsure", "{:?}", g.inside(&shell(rebound)));
    let tuple = "python3 - <<'EOF'\np='src/a.py'\np, q = 'src/b.py', 1\nopen(p,'w').write('x')\nEOF";
    assert_eq!(g.inside(&shell(tuple)).unwrap().word(), "unsure");
    // the same shape whose other binding names a place outside the project is asked by code, before any Decider
    let outside = "python3 - <<'EOF'\np='src/a.py'\np, q = '/etc/hosts', 1\nopen(p,'w').write('x')\nEOF";
    assert_eq!(g.inside(&shell(outside)).unwrap().word(), "ask");
}

#[test]
fn an_edit_of_a_file_inside_is_taken_and_one_outside_or_of_git_is_asked() {
    let g = gate();
    let write = |path: &str, inside: bool| {
        let mut a = Action::new(Verb::Write, Actor::Doer("claude".into()), path).task("t1");
        (a.site, a.inside) = (Some(SITE.into()), Some(inside));
        g.inside(&a).unwrap().word()
    };
    assert_eq!(write("/home/user/project/src/a.py", true), "inside");
    assert_eq!(write("/home/user/.bashrc", false), "ask");
    assert_eq!(write("/home/user/project/.git/config", true), "ask");
    assert_eq!(write("/home/user/project/.env", true), "ask");
}
