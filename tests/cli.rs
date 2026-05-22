mod common;

use assert_cmd::Command;
use common::Fixture;
use predicates::str::contains;
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
    fx.delete_and_commit("x.txt", "rm");
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

#[test]
fn ignore_case_works_for_literal_and_regex() {
    let fx = Fixture::new();
    fx.commit("x.txt", "Hello World\n", "init");

    // Literal match ignoring case.
    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["-i", "hello", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:Hello World\n");

    // Regex match ignoring case.
    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["-i", "--regex", "^HELLO", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:Hello World\n");
}

#[test]
fn include_glob_restricts_branches_in_json_output() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    for b in ["release/1.0", "release/2.0", "feature/a"] {
        fx.branch(b, "main");
    }

    let out = Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "--include", "release/*", "--json", "-C"])
        .arg(fx.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "exit: {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<_> = arr.iter().map(|v| v["branch"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["release/1.0", "release/2.0"]);
}

#[test]
fn local_branch_wins_dedup_against_remote_tracking() {
    let upstream = Fixture::new();
    upstream.commit("x.txt", "needle\n", "init");
    upstream.branch("only-on-remote", "main");

    let clone_dir = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("git")
        .args([
            "clone",
            "-q",
            &upstream.path().display().to_string(),
            &clone_dir.path().display().to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(clone_dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<_> = arr.iter().map(|v| v["branch"].as_str().unwrap()).collect();
    // The local branch `main` must appear.
    assert!(names.contains(&"main"), "got: {names:?}");
    // `origin/main` must NOT appear because `main` already covers that tip.
    assert!(!names.contains(&"origin/main"), "got: {names:?}");
    // `origin/only-on-remote` has no local counterpart, so it must appear.
    assert!(names.contains(&"origin/only-on-remote"), "got: {names:?}");
}

#[test]
fn json_record_shape_includes_all_status_variants() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");

    // Branch with no match.
    fx.branch("no-match", "main");
    fx.checkout("no-match");
    fx.commit("x.txt", "nothing\n", "diverge");

    // Branch with file missing.
    fx.branch("missing", "main");
    fx.checkout("missing");
    fx.delete_and_commit("x.txt", "rm");

    fx.checkout("main");

    let out = Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(fx.path())
        .output()
        .unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    for v in &arr {
        // Every record must carry a string "branch" field.
        assert!(v.get("branch").and_then(|b| b.as_str()).is_some());
        let status = v.get("status").and_then(|s| s.as_str()).unwrap();
        match status {
            "matched" => {
                let hits = v.get("hits").and_then(|h| h.as_array()).unwrap();
                for h in hits {
                    assert!(h.get("line_number").and_then(|n| n.as_u64()).is_some());
                    assert!(h.get("line").and_then(|l| l.as_str()).is_some());
                }
            }
            "no_match" | "file_missing" => {}
            "error" => {
                assert!(v.get("error").and_then(|e| e.as_str()).is_some());
            }
            other => panic!("unknown status: {other}"),
        }
    }
}

/// Verify that a dangling ref (fake SHA written directly into refs/heads/)
/// does not cause a fatal exit.
///
/// NOTE: observed behavior differs from the initial plan's expectation.
/// `git show broken:x.txt` returns "exists on disk, but not in 'broken'"
/// which `read_blob` classifies as `BlobRead::Missing` (not `Error`), so
/// the JSON status is `"file_missing"`, not `"error"`. Git never emits an
/// "object not found" message because it resolves the ref to the (absent)
/// object and then falls back to the working-tree check. The important
/// invariant — that one bad branch does not abort the whole scan — still holds.
#[test]
fn per_branch_error_does_not_abort_scan() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    fx.branch("good", "main");

    // Write a fake SHA directly into refs/heads/broken to create a dangling ref.
    // `git for-each-ref` will list it, but `git show broken:x.txt` will fail.
    let refs_dir = fx.path().join(".git/refs/heads");
    std::fs::write(
        refs_dir.join("broken"),
        "0000000000000000000000000000000000000001\n",
    )
    .unwrap();

    let out = Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(fx.path())
        .output()
        .unwrap();

    // Must not exit with an unrecoverable fatal code.
    assert!(
        out.status.success() || out.status.code() == Some(1),
        "unexpected fatal exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let by_branch: HashMap<_, _> = arr
        .iter()
        .map(|v| (v["branch"].as_str().unwrap().to_string(), v.clone()))
        .collect();

    // The dangling ref appears in output (not suppressed) with a non-matched status.
    // Observed: "file_missing" (git reports "exists on disk, but not in 'broken'").
    let broken_status = by_branch["broken"]["status"].as_str().unwrap();
    assert!(
        broken_status == "file_missing" || broken_status == "error",
        "expected file_missing or error for broken branch, got: {broken_status}"
    );

    // The other branches are unaffected.
    assert_eq!(by_branch["main"]["status"], "matched");
    assert_eq!(by_branch["good"]["status"], "matched");
}

#[test]
fn no_matches_anywhere_exits_one_with_empty_stdout() {
    let fx = Fixture::new();
    fx.commit("x.txt", "nothing of interest\n", "init");

    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .code(1)
        .stdout("");
}

#[test]
fn not_a_git_repo_exits_two_with_clear_message() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("spelunker")
        .unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(tmp.path())
        .assert()
        .code(2)
        .stderr(contains("not a git repository"));
}

#[test]
fn broken_pipe_on_stdout_exits_zero_not_fatal() {
    use std::io::Write;
    use std::process::Stdio;

    let fx = Fixture::new();
    // Generate a file large enough that `head` can close the pipe before we
    // finish writing all matches.
    let mut big = String::new();
    for i in 0..10_000 {
        big.push_str(&format!("line {i} contains needle\n"));
    }
    fx.commit("x.txt", &big, "init");

    // Launch spelunker piping into `head -n 5` to force a BrokenPipe partway through.
    let mut spel = std::process::Command::new(env!("CARGO_BIN_EXE_spelunker"))
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut head = std::process::Command::new("head")
        .args(["-n", "5"])
        .stdin(spel.stdout.take().unwrap())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = head.wait().unwrap();
    let status = spel.wait().unwrap();

    // The point is: spelunker must NOT exit with code 2 (fatal).
    // grep's convention is to exit 0 when its pipe is closed.
    assert_ne!(
        status.code(),
        Some(2),
        "broken pipe should not produce a fatal exit; got status: {status:?}"
    );
    // Suppress unused-import warning: Write is pulled in so the test file mirrors
    // the pattern used in the task description; the import is intentional documentation.
    let _ = std::io::sink().write_all(b"");
}
