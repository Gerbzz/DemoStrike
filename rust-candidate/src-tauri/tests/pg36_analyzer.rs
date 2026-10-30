use demostrike_rust_candidate::csdm::csdm_analyzer::{self, AnalyzerError, Request, TextEvent};
use std::{path::PathBuf, sync::mpsc, time::Duration};
#[test]
fn source_oracle_arguments_match() {
    let o: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pg35-original-args-oracle.json")).unwrap();
    assert_eq!(
        o["indexSha256"],
        "a2d89e9ec5c73587edf7a9d8e5a1ec33f91fdb82eed026a21de5f104e98be4de"
    );
    let args = csdm_analyzer::build_args(
        std::path::Path::new(r"C:\owned\a demo.dem"),
        std::path::Path::new("out"),
        "valve",
        true,
    )
    .unwrap();
    assert_eq!(
        args,
        [
            r"-demo-path=C:\owned\a demo.dem",
            "-output=out",
            "-format=csdm",
            "-source=valve",
            "-positions=true"
        ]
    );
    assert!(o["rows"][0]["command"]
        .as_str()
        .unwrap()
        .contains("-source=\"valve\" -positions=\"true\""));
}
#[cfg(windows)]
#[test]
fn missing_demo_refuses_before_execution() {
    let root =
        PathBuf::from(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg36-no-such-fixture");
    let req = Request {
        executable: root.join("none.exe"),
        demo: root.join("none.dem"),
        output: root,
        source: "valve".into(),
        positions: false,
        deadline: Duration::from_secs(1),
    };
    let (tx, _rx) = mpsc::sync_channel(1);
    assert_eq!(
        csdm_analyzer::analyze(&req, tx),
        Err(AnalyzerError::MissingDemo)
    );
}
#[cfg(windows)]
#[test]
#[ignore = "requires reviewed PG36 task-owned process supervisor and pinned inputs"]
fn owned_real_and_synthetic_processes() {
    assert_eq!(std::env::var("PG36_OWNED_TEST").as_deref(), Ok("1"));
    let root = PathBuf::from(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6");
    let owned = root.join("pg36-owned-root");
    assert!(owned.join("owner.txt").is_file());
    let demo = root.join(r"pg30-csda-owned\header-only.dem");
    let csda=root.join(r"pg8-renderer-source\upstream\cs-demo-manager\node_modules\@akiver\cs-demo-analyzer\dist\bin\windows-x64\csda.exe");
    let out = owned.join("real");
    std::fs::create_dir(&out).unwrap();
    let req = Request {
        executable: csda,
        demo: demo.clone(),
        output: out,
        source: "valve".into(),
        positions: false,
        deadline: Duration::from_secs(20),
    };
    let (tx, rx) = mpsc::sync_channel(16);
    let collector = std::thread::spawn(move || {
        let mut s = String::new();
        for e in rx {
            let e: TextEvent = e;
            if e.stderr {
                s.push_str(&e.text)
            }
        }
        s
    });
    assert_eq!(
        csdm_analyzer::analyze(&req, tx),
        Err(AnalyzerError::ProcessFailure(1))
    );
    let stderr = collector.join().unwrap();
    let direct = std::fs::read_to_string(root.join(r"pg30-csda-owned\direct-stderr.txt")).unwrap();
    assert_eq!(stderr.trim(), direct.trim());
    let out = owned.join("corrupt");
    std::fs::create_dir(&out).unwrap();
    let req = Request {
        executable: root.join("pg32-owned-emitter.exe"),
        demo: demo.clone(),
        output: out,
        source: "valve".into(),
        positions: false,
        deadline: Duration::from_secs(20),
    };
    let (tx, rx) = mpsc::sync_channel(16);
    let collector = std::thread::spawn(move || {
        let (mut o, mut e) = (String::new(), String::new());
        for item in rx {
            let item: TextEvent = item;
            if item.stderr {
                e.push_str(&item.text)
            } else {
                o.push_str(&item.text)
            }
        }
        (o, e)
    });
    assert_eq!(
        csdm_analyzer::analyze(&req, tx),
        Err(AnalyzerError::Corrupted)
    );
    let (o, e) = collector.join().unwrap();
    assert_eq!(o, "€");
    assert_eq!(e, "ErrUnexpectedEndOfDemo");
    let out = owned.join("backpressure");
    std::fs::create_dir(&out).unwrap();
    let req = Request {
        executable: root.join("pg35-owned-flood.exe"),
        demo,
        output: out,
        source: "".into(),
        positions: false,
        deadline: Duration::from_secs(20),
    };
    let (tx, _rx) = mpsc::sync_channel(0);
    assert_eq!(
        csdm_analyzer::analyze(&req, tx),
        Err(AnalyzerError::CanceledOutput {
            cleanup_confirmed: Some(true)
        })
    );
}
