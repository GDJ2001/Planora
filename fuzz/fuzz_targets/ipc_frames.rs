#![no_main]

libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    if bytes.len() <= 64 * 1024 + 4 {
        let _ = planora_helper::receive_request(&mut bytes.as_ref());
    }
});
