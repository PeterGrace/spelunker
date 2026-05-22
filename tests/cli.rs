mod common;

use assert_cmd::Command;
use common::Fixture;

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
