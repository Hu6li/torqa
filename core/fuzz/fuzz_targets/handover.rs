#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| orqwa_net::fuzz::handover(data));
