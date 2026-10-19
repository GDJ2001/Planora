use planora_core::{Command, PROTOCOL_VERSION, Request, Response};
use planora_helper::{HANDSHAKE_TIMEOUT, REQUEST_TIMEOUT, connect, exchange_request_timeout};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires installed PlanoraHelper service and runs for 5 minutes"]
fn test_live_service_telemetry_sustained() {
    let mut stream = connect().expect("Installed PlanoraHelper service must accept connections");

    // 1. Handshake
    let env = exchange_request_timeout(
        &mut stream,
        &Request {
            request_id: 1,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        },
        HANDSHAKE_TIMEOUT,
    )
    .expect("Should complete handshake with PlanoraHelper");
    assert_eq!(env.request_id, 1);
    match env.response {
        Response::Hello {
            version,
            session_id,
            helper_authorized,
        } => {
            assert_eq!(version, PROTOCOL_VERSION);
            assert!(helper_authorized, "Helper must report authorized client");
            println!(
                "Handshake successful: version={version}, session_id={session_id}, authorized={helper_authorized}"
            );
        }
        other => panic!("Unexpected handshake response: {:?}", other),
    }

    println!("Handshake successful with installed PlanoraHelper service!");

    let capability_started = Instant::now();
    let capabilities = exchange_request_timeout(
        &mut stream,
        &Request {
            request_id: 2,
            command: Command::Capabilities,
        },
        REQUEST_TIMEOUT,
    )
    .expect("Should complete capabilities exchange within deadline");
    assert!(matches!(capabilities.response, Response::Capabilities(_)));
    println!(
        "Capabilities received in {:?}",
        capability_started.elapsed()
    );

    // A short override is useful for diagnosing an installed build; the default
    // remains a five-minute release acceptance run.
    let duration_secs = std::env::var("PLANORA_SERVICE_TEST_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(305);

    // 2. Poll snapshots for more than five minutes by default.
    let start = Instant::now();
    let mut count = 0u64;
    while start.elapsed() < Duration::from_secs(duration_secs) {
        count += 1;
        let req_id = count + 10;
        let env = exchange_request_timeout(
            &mut stream,
            &Request {
                request_id: req_id,
                command: Command::Snapshot,
            },
            REQUEST_TIMEOUT,
        )
        .expect("Should complete snapshot exchange within deadline");
        assert_eq!(env.request_id, req_id);

        match env.response {
            Response::Snapshot(frame) => {
                let now_ms = planora_telemetry::unix_ms();
                let age = now_ms.saturating_sub(frame.cpu_percent.sampled_at_ms);
                if count % 30 == 1 {
                    println!(
                        "[{:>3}s elapsed] Sample #{:<3}: seq={:<4}, CPU={:>5.1}%, Mem={:>6.1} MB, Age={:>3}ms (FRESH)",
                        start.elapsed().as_secs(),
                        count,
                        frame.sequence,
                        frame.cpu_percent.value.unwrap_or(0.0),
                        frame.memory_used_bytes.value.unwrap_or(0) as f64 / 1_048_576.0,
                        age
                    );
                }
                assert!(
                    age < 2500,
                    "Telemetry must remain fresh (< 2500ms): got {}ms",
                    age
                );
            }
            other => panic!("Unexpected response from service: {:?}", other),
        }

        std::thread::sleep(Duration::from_secs(1));
    }

    println!(
        "Successfully verified {} consecutive fresh telemetry samples over {:.1} minutes from the running PlanoraHelper service!",
        count,
        start.elapsed().as_secs_f64() / 60.0
    );
}
