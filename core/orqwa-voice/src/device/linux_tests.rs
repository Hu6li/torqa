//! Real CPAL/ALSA selection with a process-local null PCM, never actual audio hardware.
use super::*;

#[test]
fn alsa_default_direction_is_unknown_but_requested_configuration_can_be_opened() {
    fixture(
        "default",
        "pcm.!default { type null hint { show on description \"ORQWA fixture default\" } }\n",
    );
}

fn fixture(mode: &str, contents: &str) {
    let token = format!(
        "{:032x}",
        u128::from_be_bytes(orqwa_net::code::RideId::generate().unwrap().0)
    );
    let directory = std::env::temp_dir().join(format!("orqwa-alsa-{token}"));
    std::fs::create_dir(&directory).unwrap();
    let config = directory.join("asound.conf");
    std::fs::write(&config, contents).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "device::linux_tests::isolated_alsa_selection",
            "--ignored",
            "--nocapture",
        ])
        .env("ALSA_CONFIG_PATH", &config)
        .env("ORQWA_ALSA_FIXTURE", mode)
        .output()
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn desktop_bridges_are_preferred_explicit_choices_stay_explicit_and_dummy_routes_are_hidden() {
    fixture(
        "bridges",
        r#"
pcm.!default { type null hint { show on description "Fixture default" } }
pcm.null { type null hint { show on description "Discard all samples" } }
pcm.pipewire { type null hint { show on description "Fixture PipeWire" } }
pcm.pulse { type null hint { show on description "Fixture PulseAudio" } }
pcm.outonly { type null hint { show off } }
namehint.pcm.outonly "outonly|Output only|IOIDOutput"
"#,
    );
}

#[test]
fn an_unavailable_bridge_falls_back_to_the_working_system_route() {
    fixture(
        "fallback",
        r#"
pcm.!default { type null hint { show on description "Fixture default" } }
pcm.pipewire { type definitely_not_an_installed_plugin hint { show on description "Unavailable server" } }
pcm.pulse { type null hint { show on description "Fixture PulseAudio" } }
"#,
    );
}

#[test]
#[ignore = "launched by the parent regression in an isolated ALSA process"]
fn isolated_alsa_selection() {
    let mode = std::env::var("ORQWA_ALSA_FIXTURE").unwrap();
    let host = cpal::default_host();
    assert_eq!(host.id(), cpal::HostId::Alsa);
    let default = host.default_output_device().unwrap();
    assert_eq!(
        default.description().unwrap().direction(),
        cpal::DeviceDirection::Unknown
    );
    assert!(
        !default.supports_output(),
        "ALSA's metadata-only capability check cannot decide"
    );
    assert!(
        default.default_output_config().is_ok(),
        "the fixture provides a valid output route"
    );
    assert!(
        default.default_input_config().is_ok(),
        "the fixture provides a valid input route"
    );
    let output = choose(&host, None, false)
        .expect("the selected default must reach output configuration validation");
    assert!(output.default_output_config().is_ok());
    let input = choose(&host, None, true)
        .expect("the selected default must reach input configuration validation");
    assert!(input.default_input_config().is_ok());
    for input in [false, true] {
        let (device, _) = configured(&host, None, input).unwrap();
        assert_eq!(
            device.id().unwrap().id(),
            match mode.as_str() {
                "bridges" => "pipewire",
                "fallback" => "pulse",
                _ => "default",
            }
        );
    }
    let id = cpal::DeviceId::new(cpal::HostId::Alsa, "default").to_string();
    assert_eq!(
        configured(&host, Some(&id), false)
            .unwrap()
            .0
            .id()
            .unwrap()
            .id(),
        "default"
    );
    let missing = cpal::DeviceId::new(cpal::HostId::Alsa, "missing").to_string();
    assert!(
        configured(&host, Some(&missing), false).is_err(),
        "a selected device cannot silently fall back"
    );
    if mode == "bridges" {
        let listed = devices().unwrap();
        assert_eq!(
            listed[0].id,
            cpal::DeviceId::new(cpal::HostId::Alsa, "pipewire").to_string()
        );
        assert!(
            !listed
                .iter()
                .any(|device| device.id.parse::<cpal::DeviceId>().unwrap().id() == "null")
        );
        let out = cpal::DeviceId::new(cpal::HostId::Alsa, "outonly").to_string();
        assert_eq!(
            host.device_by_id(&out.parse().unwrap())
                .unwrap()
                .description()
                .unwrap()
                .direction(),
            cpal::DeviceDirection::Output
        );
        assert!(configured(&host, Some(&out), false).is_ok());
        assert!(
            configured(&host, Some(&out), true).is_err(),
            "known output-only devices cannot become microphones"
        );
    }
}
