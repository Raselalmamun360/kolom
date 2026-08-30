//! `সময়`'s calendar functions and `সিস্টেম`'s argv/env access both return
//! values that depend on the live moment or the invoking process, so they
//! can't go through the golden-fixture harness (checked-in `expected.txt`
//! exact-match). Custom range/format assertions here instead, same spirit
//! as golden_m3's র‍্যান্ডম test. Covers both the interpreter and the native
//! (Cranelift) backend — `সময়` was silently unreachable on native before
//! this session (never wired into Cranelift at all), so this is also the
//! first regression coverage that would have caught that.

use std::path::PathBuf;
use std::process::Command;

fn kolom_exe() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("kolom-time-sys-test").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const TIME_SRC: &str = r#"ইম্পোর্ট সময়

অ্যাপ {
    লেখো(সময়.এখন_মিলিসেকেন্ড())
    লেখো(সময়.বছর())
    লেখো(সময়.মাস())
    লেখো(সময়.দিন())
    লেখো(সময়.ঘণ্টা())
    লেখো(সময়.মিনিট())
    লেখো(সময়.সেকেন্ড_অংশ())
    লেখো(সময়.বর্তমান_তারিখ_লেখা())
}
"#;

fn assert_time_output(out: &str) {
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 8, "expected 8 lines, got: {out:?}");

    let ms: i64 = lines[0].trim().parse().unwrap_or_else(|_| panic!("not a number: {:?}", lines[0]));
    assert!(ms > 1_700_000_000_000, "epoch ms implausibly small: {ms}");

    let year: i64 = lines[1].trim().parse().unwrap_or_else(|_| panic!("not a number: {:?}", lines[1]));
    assert!((2024..=2100).contains(&year), "year out of plausible range: {year}");

    let month: i64 = lines[2].trim().parse().unwrap();
    assert!((1..=12).contains(&month), "month out of range: {month}");

    let day: i64 = lines[3].trim().parse().unwrap();
    assert!((1..=31).contains(&day), "day out of range: {day}");

    let hour: i64 = lines[4].trim().parse().unwrap();
    assert!((0..=23).contains(&hour), "hour out of range: {hour}");

    let minute: i64 = lines[5].trim().parse().unwrap();
    assert!((0..=59).contains(&minute), "minute out of range: {minute}");

    let second: i64 = lines[6].trim().parse().unwrap();
    assert!((0..=59).contains(&second), "second out of range: {second}");

    let formatted = lines[7];
    assert_eq!(formatted.len(), 19, "unexpected timestamp format: {formatted:?}");
    assert_eq!(&formatted[4..5], "-", "unexpected timestamp format: {formatted:?}");
    assert_eq!(&formatted[7..8], "-", "unexpected timestamp format: {formatted:?}");
    assert_eq!(&formatted[10..11], " ", "unexpected timestamp format: {formatted:?}");
    assert_eq!(&formatted[13..14], ":", "unexpected timestamp format: {formatted:?}");
    assert_eq!(&formatted[16..17], ":", "unexpected timestamp format: {formatted:?}");
    assert_eq!(&formatted[0..4], &year.to_string(), "date string's year should match সময়.বছর()");
}

#[test]
fn time_interpreted() {
    let dir = workdir("time-interp");
    let src = dir.join("main.ক");
    std::fs::write(&src, TIME_SRC).unwrap();
    let out = Command::new(kolom_exe()).arg("চালাও").arg(&src).output().expect("failed to run kolom");
    assert!(out.status.success(), "কলম চালাও failed:\n{}", String::from_utf8_lossy(&out.stderr));
    assert_time_output(&String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"));
}

#[test]
fn time_native() {
    let dir = workdir("time-native");
    let src = dir.join("main.ক");
    std::fs::write(&src, TIME_SRC).unwrap();
    let build = Command::new(kolom_exe()).arg("বিল্ড").arg(&src).output().expect("failed to run kolom");
    assert!(
        build.status.success(),
        "কলম বিল্ড failed — সময় should build on the native backend:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();
    let run = Command::new(&exe).output().expect("failed to run produced exe");
    assert!(run.status.success(), "produced exe exited {:?}:\n{}", run.status.code(), String::from_utf8_lossy(&run.stderr));
    assert_time_output(&String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"));
}

const SYS_SRC: &str = r#"ইম্পোর্ট সিস্টেম

অ্যাপ {
    ধরি আর্গ = সিস্টেম.আর্গুমেন্ট()
    লেখো(দৈর্ঘ্য(আর্গ))
    প্রতি (a : আর্গ) {
        লেখো(a)
    }
    লেখো(সিস্টেম.পরিবেশ("KLOM_TIME_SYS_TEST_VAR"))
}
"#;

fn assert_sys_output(out: &str) {
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["2", "প্রথম", "দ্বিতীয়", "যাচাই-মান"], "got: {out:?}");
}

#[test]
fn system_args_and_env_interpreted() {
    let dir = workdir("sys-interp");
    let src = dir.join("main.ক");
    std::fs::write(&src, SYS_SRC).unwrap();
    let out = Command::new(kolom_exe())
        .arg("চালাও")
        .arg(&src)
        .arg("প্রথম")
        .arg("দ্বিতীয়")
        .env("KLOM_TIME_SYS_TEST_VAR", "যাচাই-মান")
        .output()
        .expect("failed to run kolom");
    assert!(out.status.success(), "কলম চালাও failed:\n{}", String::from_utf8_lossy(&out.stderr));
    assert_sys_output(&String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"));
}

#[test]
fn system_args_and_env_native() {
    let dir = workdir("sys-native");
    let src = dir.join("main.ক");
    std::fs::write(&src, SYS_SRC).unwrap();
    let build = Command::new(kolom_exe()).arg("বিল্ড").arg(&src).output().expect("failed to run kolom");
    assert!(build.status.success(), "কলম বিল্ড failed:\n{}", String::from_utf8_lossy(&build.stderr));
    let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();
    let run = Command::new(&exe)
        .arg("প্রথম")
        .arg("দ্বিতীয়")
        .env("KLOM_TIME_SYS_TEST_VAR", "যাচাই-মান")
        .output()
        .expect("failed to run produced exe");
    assert!(run.status.success(), "produced exe exited {:?}:\n{}", run.status.code(), String::from_utf8_lossy(&run.stderr));
    assert_sys_output(&String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"));
}
