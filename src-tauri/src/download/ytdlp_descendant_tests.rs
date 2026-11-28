use super::wait_with_deadline;
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{process::Command as TokioCommand, time::sleep};

const FIXTURE_ENV: &str = "BLACKTAPE_DESCENDANT_TEST_FIXTURE";
const MARKER_ENV: &str = "BLACKTAPE_DESCENDANT_TEST_MARKER";

#[test]
#[ignore = "subprocess fixture"]
fn descendant_fixture_parent() {
    if std::env::var(FIXTURE_ENV).as_deref() != Ok("parent") {
        return;
    }
    std::thread::sleep(Duration::from_millis(100));
    let marker = std::env::var_os(MARKER_ENV).expect("marker path should be passed to fixture");
    let executable = std::env::current_exe().expect("Cargo test executable path should exist");
    let _grandchild = Command::new(executable)
        .args([
            "--ignored",
            "--exact",
            "download::ytdlp::descendant_tests::descendant_fixture_grandchild",
            "--nocapture",
        ])
        .env(FIXTURE_ENV, "grandchild")
        .env(MARKER_ENV, marker)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("grandchild fixture should spawn");
    // Intentionally exit without waiting; the grandchild inherits our output pipes.
}

#[test]
#[ignore = "subprocess fixture"]
fn descendant_fixture_grandchild() {
    if std::env::var(FIXTURE_ENV).as_deref() != Ok("grandchild") {
        return;
    }
    std::thread::sleep(Duration::from_secs(1));
    let marker = std::env::var_os(MARKER_ENV).expect("marker path should be passed to fixture");
    fs::write(marker, b"descendant survived timeout cleanup")
        .expect("fixture marker should be writable");
}

#[tokio::test]
async fn deadline_cleanup_stops_descendant_after_parent_exits() {
    let directory = tempfile::tempdir().expect("temporary marker directory");
    let marker = directory.path().join("descendant-survived.txt");
    let executable = std::env::current_exe().expect("Cargo test executable path should exist");
    let mut command = TokioCommand::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            "download::ytdlp::descendant_tests::descendant_fixture_parent",
            "--nocapture",
        ])
        .env(FIXTURE_ENV, "parent")
        .env(MARKER_ENV, marker.as_os_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let child = command
        .kill_on_drop(true)
        .spawn()
        .expect("parent fixture should spawn");

    let result = wait_with_deadline(child, None, Duration::from_millis(500)).await;
    assert!(result.is_err(), "grandchild-held output pipes should exceed the deadline");

    // If cleanup killed only the exited parent, its descendant survives and writes after one second.
    sleep(Duration::from_millis(900)).await;
    assert!(!marker.exists(), "descendant survived deadline cleanup");
}
