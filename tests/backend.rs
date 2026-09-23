#![cfg(windows)]

use magicodex::{
    backend::{Backend, BackendEvent, BackendKind},
    protocol::{self, Message},
};
use serde_json::json;
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn receive(backend: &Backend) -> Message {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match backend.try_recv() {
            Ok(BackendEvent::Message(message)) => return message,
            Ok(other) => panic!("Unexpected backend event: {other:?}"),
            Err(_) => thread::sleep(Duration::from_millis(5)),
        }
    }
    panic!("Fixture response timed out")
}

#[test]
fn owned_windows_process_and_bidirectional_approval() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for allow in [false, true] {
        let mut backend = Backend::start(
            BackendKind::Official,
            Some(&root.join(r"tests\fixtures\app-server.ps1")),
            root,
        )
        .unwrap();
        let id = backend
            .request("initialize", protocol::initialize())
            .unwrap();
        assert!(
            matches!(receive(&backend), Message::Response { id: actual, result: Ok(_) } if actual == id)
        );
        backend.request("fixture/approval", json!({})).unwrap();
        assert!(matches!(receive(&backend), Message::Response { .. }));
        let Message::Request { id, .. } = receive(&backend) else {
            panic!("Expected approval");
        };
        assert!(
            backend.try_recv().is_err(),
            "No execution before a decision"
        );
        backend
            .send(&json!({"id":id,"result":{"decision":if allow {"accept"} else {"decline"}}}))
            .unwrap();
        assert!(
            matches!(receive(&backend), Message::Notification { params, .. } if params["executed"] == allow)
        );
        backend.shutdown().unwrap();
    }
}

#[test]
fn dropping_backend_cleans_only_its_owned_descendants() {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut backend = Backend::start(
        BackendKind::Official,
        Some(&root.join(r"tests\fixtures\app-server.ps1")),
        root,
    )
    .unwrap();
    backend
        .request("fixture/spawn-owned-child", json!({}))
        .unwrap();
    let Message::Response {
        result: Ok(value), ..
    } = receive(&backend)
    else {
        panic!("Missing child process");
    };
    let pid = u32::try_from(value["pid"].as_u64().unwrap()).unwrap();
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        assert!(!process.is_null());
        drop(backend);
        let result = WaitForSingleObject(process, 5000);
        CloseHandle(process);
        assert_eq!(
            result, WAIT_OBJECT_0,
            "Owned descendant survived backend cleanup"
        );
    }
}

#[test]
fn batch_launcher_path_with_spaces_is_supported() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut backend = Backend::start(
        BackendKind::Official,
        Some(&root.join(r"tests\fixtures\launcher space\server.cmd")),
        root,
    )
    .unwrap();
    backend
        .request("initialize", protocol::initialize())
        .unwrap();
    assert!(matches!(
        receive(&backend),
        Message::Response { result: Ok(_), .. }
    ));
    backend.shutdown().unwrap();
}

#[test]
fn blocked_backend_stdin_cannot_block_the_ui_thread() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut backend = Backend::start(
        BackendKind::Official,
        Some(&root.join(r"tests\fixtures\app-server.ps1")),
        root,
    )
    .unwrap();
    backend.request("fixture/block-input", json!({})).unwrap();
    assert!(matches!(
        receive(&backend),
        Message::Response { result: Ok(_), .. }
    ));
    let value = json!({"padding":"x".repeat(128 * 1024)});
    let start = Instant::now();
    let mut refused = false;
    for _ in 0..16 {
        if let Err(error) = backend.send(&value) {
            assert!(error.to_string().contains("队列繁忙"));
            refused = true;
            break;
        }
    }
    assert!(refused, "Bounded writer queue did not apply backpressure");
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "Writing blocked the UI"
    );
    drop(backend);
}

#[test]
fn default_copilot_mode_never_falls_through_to_an_unrecognized_provider() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_magicodex"))
        .args(["--backend", "copilot", "--probe"])
        .env("PATH", root.join(r"tests\fixtures\unrecognized"))
        .env_remove("MAGICODEX_COPILOT_ENTRY")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("不是可识别的 Copilot 桥接"));
    assert!(!error.contains("UNEXPECTED_PROVIDER_EXECUTION"));
}
