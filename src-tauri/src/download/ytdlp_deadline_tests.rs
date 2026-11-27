use super::wait_with_deadline;
use std::{process::Stdio, time::{Duration, Instant}};
use tokio::process::Command as TokioCommand;

#[test]
#[ignore = "subprocess fixture"]
fn slow_child() {
    if std::env::var("BLACKTAPE_TIMEOUT_TEST_CHILD").as_deref() == Ok("1") {
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn fixture_child(slow: bool) -> tokio::process::Child {
    let executable = std::env::current_exe().expect("Cargo test executable path should exist");
    let mut command = TokioCommand::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            "download::ytdlp::deadline_tests::slow_child",
            "--nocapture",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if slow {
        command.env("BLACKTAPE_TIMEOUT_TEST_CHILD", "1");
    } else {
        command.env_remove("BLACKTAPE_TIMEOUT_TEST_CHILD");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    command.kill_on_drop(true).spawn().expect("test fixture should spawn")
}

#[tokio::test]
async fn wait_with_deadline_stops_a_slow_owned_test_child() {
    let started = Instant::now();
    let result = wait_with_deadline(fixture_child(true), None, Duration::from_millis(40)).await;

    let error = result.expect_err("slow child should exceed the deadline");
    assert!(error.contains("timed out"), "unexpected error: {error}");
    assert!(started.elapsed() < Duration::from_secs(2), "deadline should bound process wait");
}

#[tokio::test]
async fn wait_with_deadline_returns_success_for_a_fast_test_child() {
    let output = wait_with_deadline(fixture_child(false), None, Duration::from_secs(5))
        .await
        .expect("fast child should complete successfully");
    assert!(output.status.success(), "fast child failed: {output:?}");
}
