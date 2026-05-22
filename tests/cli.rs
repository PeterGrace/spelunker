mod common;

use assert_cmd::Command;
use common::Fixture;
use std::collections::HashMap;

#[test]
fn happy_path_one_branch_matches() {
    let fx = Fixture::new();
    fx.commit("x.txt", "the needle is here\n", "init");
    fx.branch("no-needle", "main");
    fx.checkout("no-needle");
    fx.commit("x.txt", "nothing of interest\n", "diverge");
    fx.checkout("main");

    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:the needle is here\n");
}

#[test]
fn file_missing_is_silent_in_human_mode_and_marked_in_json() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    fx.branch("deleted", "main");
    fx.checkout("deleted");
    std::fs::remove_file(fx.path().join("x.txt")).unwrap();
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "rm"]);
    fx.checkout("main");

    // Human mode: only the matching branch appears; the file-missing branch is silent.
    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:needle\n");

    // JSON mode: both branches appear, with appropriate status fields.
    let out = Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(fx.path())
        .output()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = parsed.as_array().unwrap();
    let by_branch: HashMap<_, _> = arr
        .iter()
        .map(|v| (v["branch"].as_str().unwrap().to_string(), v.clone()))
        .collect();
    assert_eq!(by_branch["main"]["status"], "matched");
    assert_eq!(by_branch["deleted"]["status"], "file_missing");
}

#[test]
fn regex_mode_matches_and_bad_regex_exits_2() {
    let fx = Fixture::new();
    fx.commit("x.txt", "foo1\nbar\nfoo23\n", "init");

    // A valid regex should match all lines that satisfy the pattern.
    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["--regex", r"foo\d+", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:foo1\nmain:3:foo23\n");

    // An invalid regex must exit with code 2 (usage/fatal error).
    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["--regex", "(", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .code(2);
}
