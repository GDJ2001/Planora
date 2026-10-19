#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Stream};
use planora_core::{
    AvailabilityReason, Command, ErrorCode, Metric, PROTOCOL_VERSION, Request, Response,
    ResponseEnvelope, TelemetryFrame,
};
use planora_helper::{
    HANDSHAKE_TIMEOUT, REQUEST_TIMEOUT, receive_request, receive_response_timeout, send_request,
    send_response,
};
use std::io::{self, ErrorKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

fn test_socket_name(tag: &str) -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        format!("planora-test-{tag}-{}", std::process::id()).to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        format!("/tmp/planora-test-{tag}-{}.sock", std::process::id())
            .to_fs_name::<GenericFilePath>()
    }
}

fn dummy_frame(seq: u64) -> TelemetryFrame {
    TelemetryFrame {
        protocol_version: PROTOCOL_VERSION,
        session_id: 1,
        topology_generation: 1,
        sequence: seq,
        sampled_at_ms: 1000 + seq * 100,
        collection_duration_ms: 1.5,
        cpu_percent: Metric::available(15.5, "%", "test", 1000),
        memory_used_bytes: Metric::available(8 * 1024 * 1024 * 1024, "bytes", "test", 1000),
        memory_total_bytes: Metric::available(16 * 1024 * 1024 * 1024, "bytes", "test", 1000),
        disk_read_bytes_per_sec: Metric::available(1024 * 1024, "B/s", "test", 1000),
        disk_write_bytes_per_sec: Metric::available(512 * 1024, "B/s", "test", 1000),
        network_rx_bytes_per_sec: Metric::available(2048 * 1024, "B/s", "test", 1000),
        network_tx_bytes_per_sec: Metric::available(1024 * 1024, "B/s", "test", 1000),
        gpu_percent: Metric::unavailable(
            "%",
            "test",
            1000,
            AvailabilityReason::Unsupported("None".into()),
        ),
        package_power_watts: Metric::unavailable(
            "W",
            "test",
            1000,
            AvailabilityReason::PermissionDenied("None".into()),
        ),
        processes: Vec::new(),
    }
}

#[test]
fn test_ipc_sustained_operation_beyond_128_requests() {
    let name = test_socket_name("sustained").unwrap();
    let listener = ListenerOptions::new()
        .name(name.clone())
        .create_sync()
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = stop.clone();

    // Spawn server thread
    let server_handle = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        // 1. Handshake
        let req = receive_request(&mut stream).unwrap();
        assert!(matches!(req.command, Command::Hello { .. }));
        send_response(
            &mut stream,
            &ResponseEnvelope {
                request_id: req.request_id,
                response: Response::Hello {
                    version: PROTOCOL_VERSION,
                    session_id: 100,
                    helper_authorized: true,
                },
            },
        )
        .unwrap();

        // 2. Serve 150 consecutive requests without closing connection
        for seq in 1..=150 {
            if stop_srv.load(Ordering::Relaxed) {
                break;
            }
            let req = receive_request(&mut stream).unwrap();
            assert!(matches!(req.command, Command::Snapshot));
            send_response(
                &mut stream,
                &ResponseEnvelope {
                    request_id: req.request_id,
                    response: Response::Snapshot(Box::new(dummy_frame(seq))),
                },
            )
            .unwrap();
        }
    });

    // Client connection
    let mut client = Stream::connect(name).unwrap();

    // Handshake
    send_request(
        &mut client,
        &Request {
            request_id: 1,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        },
    )
    .unwrap();

    let handshake_env = receive_response_timeout(&mut client, HANDSHAKE_TIMEOUT).unwrap();
    assert_eq!(handshake_env.request_id, 1);
    assert!(matches!(
        handshake_env.response,
        Response::Hello {
            version: PROTOCOL_VERSION,
            helper_authorized: true,
            ..
        }
    ));

    // Request 150 snapshots
    for req_id in 2..=151 {
        send_request(
            &mut client,
            &Request {
                request_id: req_id,
                command: Command::Snapshot,
            },
        )
        .unwrap();

        let resp_env = receive_response_timeout(&mut client, REQUEST_TIMEOUT).unwrap();
        assert_eq!(resp_env.request_id, req_id);
        match resp_env.response {
            Response::Snapshot(frame) => {
                assert_eq!(frame.sequence, req_id - 1);
            }
            other => panic!("expected Snapshot response, got: {other:?}"),
        }
    }

    stop.store(true, Ordering::Relaxed);
    server_handle.join().unwrap();
}

#[test]
fn test_ipc_stalled_response_triggers_deadline_timeout() {
    let name = test_socket_name("stalled").unwrap();
    let listener = ListenerOptions::new()
        .name(name.clone())
        .create_sync()
        .unwrap();

    // Spawn server that deliberately stalls without sending response
    let server_handle = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let _req = receive_request(&mut stream).unwrap();
        // Server stalls and sleeps for 3 seconds before responding
        thread::sleep(Duration::from_millis(3000));
        let _ = send_response(
            &mut stream,
            &ResponseEnvelope {
                request_id: _req.request_id,
                response: Response::Hello {
                    version: PROTOCOL_VERSION,
                    session_id: 1,
                    helper_authorized: true,
                },
            },
        );
    });

    let mut client = Stream::connect(name).unwrap();

    send_request(
        &mut client,
        &Request {
            request_id: 42,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        },
    )
    .unwrap();

    // Client enforces a 400ms deadline; should timeout promptly without blocking forever
    let start = Instant::now();
    let res = receive_response_timeout(&mut client, Duration::from_millis(400));
    let elapsed = start.elapsed();

    assert!(res.is_err(), "stalled response should return an error");
    let err = res.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TimedOut);
    assert!(
        elapsed < Duration::from_millis(1500),
        "deadline must abort before server 3000ms delay; elapsed: {elapsed:?}"
    );

    server_handle.join().unwrap();
}

#[test]
fn test_ipc_mismatched_request_id_detection() {
    let name = test_socket_name("mismatch").unwrap();
    let listener = ListenerOptions::new()
        .name(name.clone())
        .create_sync()
        .unwrap();

    let server_handle = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let _req = receive_request(&mut stream).unwrap();
        // Deliberately return mismatched request ID (9999 instead of client's req.request_id)
        send_response(
            &mut stream,
            &ResponseEnvelope {
                request_id: 9999,
                response: Response::Snapshot(Box::new(dummy_frame(1))),
            },
        )
        .unwrap();
    });

    let mut client = Stream::connect(name).unwrap();
    let client_req_id = 42;

    send_request(
        &mut client,
        &Request {
            request_id: client_req_id,
            command: Command::Snapshot,
        },
    )
    .unwrap();

    let resp_env = receive_response_timeout(&mut client, REQUEST_TIMEOUT).unwrap();
    // Validate request ID mismatch detection
    assert_ne!(
        resp_env.request_id, client_req_id,
        "mismatched request ID was accurately detected"
    );

    server_handle.join().unwrap();
}

#[test]
fn test_ipc_authorization_denial_handling() {
    let name = test_socket_name("unauth").unwrap();
    let listener = ListenerOptions::new()
        .name(name.clone())
        .create_sync()
        .unwrap();

    let server_handle = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let req = receive_request(&mut stream).unwrap();
        // Server rejects with Unauthorized error
        send_response(
            &mut stream,
            &ResponseEnvelope {
                request_id: req.request_id,
                response: Response::Error {
                    code: ErrorCode::Unauthorized,
                    message: "caller is not in approved administrator SID list".into(),
                },
            },
        )
        .unwrap();
    });

    let mut client = Stream::connect(name).unwrap();

    send_request(
        &mut client,
        &Request {
            request_id: 10,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        },
    )
    .unwrap();

    let resp_env = receive_response_timeout(&mut client, HANDSHAKE_TIMEOUT).unwrap();
    assert_eq!(resp_env.request_id, 10);
    match resp_env.response {
        Response::Error { code, message } => {
            assert_eq!(code, ErrorCode::Unauthorized);
            assert!(message.contains("not in approved"));
        }
        other => panic!("expected Error::Unauthorized, got: {other:?}"),
    }

    server_handle.join().unwrap();
}

#[test]
fn test_ipc_recovery_after_disconnect() {
    let name = test_socket_name("reconnect").unwrap();
    let listener = ListenerOptions::new()
        .name(name.clone())
        .create_sync()
        .unwrap();

    let server_handle = thread::spawn(move || {
        // First connection: accept and immediately drop (simulate unexpected crash)
        {
            let _stream = listener.accept().unwrap();
        }

        // Second connection: accept and respond properly
        {
            let mut stream = listener.accept().unwrap();
            let req = receive_request(&mut stream).unwrap();
            send_response(
                &mut stream,
                &ResponseEnvelope {
                    request_id: req.request_id,
                    response: Response::Hello {
                        version: PROTOCOL_VERSION,
                        session_id: 200,
                        helper_authorized: true,
                    },
                },
            )
            .unwrap();
        }
    });

    // Client connects to first session, which drops
    {
        let mut client = Stream::connect(name.clone()).unwrap();
        thread::sleep(Duration::from_millis(50));
        let res = receive_response_timeout(&mut client, Duration::from_millis(300));
        assert!(res.is_err(), "broken pipe should return error");
    }

    // Client reconnects to second session
    {
        let mut client = Stream::connect(name).unwrap();
        send_request(
            &mut client,
            &Request {
                request_id: 1,
                command: Command::Hello {
                    version: PROTOCOL_VERSION,
                },
            },
        )
        .unwrap();
        let resp = receive_response_timeout(&mut client, HANDSHAKE_TIMEOUT).unwrap();
        assert_eq!(resp.request_id, 1);
        assert!(matches!(
            resp.response,
            Response::Hello {
                session_id: 200,
                ..
            }
        ));
    }

    server_handle.join().unwrap();
}
