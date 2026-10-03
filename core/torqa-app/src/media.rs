//! Controls the music app the rider listens to (R26), with what each system offers out of the
//! box: AppleScript on macOS (Spotify or Music), `playerctl` (MPRIS) on Linux and media keys on
//! Windows. Nothing to install, nothing to authorise beyond the system's own prompt.

use std::process::Command;

/// What to tell the music app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    /// Play or pause.
    PlayPause,
    /// Next track.
    Next,
    /// Previous track.
    Previous,
}

/// The program and arguments that send `command` on this system, if supported.
#[must_use]
pub fn command_line(command: MediaCommand) -> Option<(String, Vec<String>)> {
    if cfg!(target_os = "macos") {
        Some((
            "osascript".to_owned(),
            vec!["-e".to_owned(), apple_script(command)],
        ))
    } else if cfg!(windows) {
        Some((
            "powershell".to_owned(),
            vec![
                "-NoProfile".to_owned(),
                "-Command".to_owned(),
                windows_media_key(command),
            ],
        ))
    } else if cfg!(target_os = "linux") {
        let verb = match command {
            MediaCommand::PlayPause => "play-pause",
            MediaCommand::Next => "next",
            MediaCommand::Previous => "previous",
        };
        Some(("playerctl".to_owned(), vec![verb.to_owned()]))
    } else {
        None
    }
}

/// Sends `command`, waiting for the helper program.
///
/// # Errors
/// A readable message if the system has no way to control music, or no player reacted.
pub fn send(command: MediaCommand) -> Result<(), String> {
    let (program, args) =
        command_line(command).ok_or_else(|| "music control is not supported here".to_owned())?;
    let output = Command::new(&program)
        .args(&args)
        .output()
        .map_err(|e| format!("cannot control music ({program}: {e})"))?;
    if output.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(format!("no music player reacted: {}", error.trim()))
    }
}

/// Talks to Spotify if it runs, else to Apple Music; `is running` never launches an app.
fn apple_script(command: MediaCommand) -> String {
    let verb = match command {
        MediaCommand::PlayPause => "playpause",
        MediaCommand::Next => "next track",
        MediaCommand::Previous => "previous track",
    };
    format!(
        "if application \"Spotify\" is running then\n\
         tell application \"Spotify\" to {verb}\n\
         else if application \"Music\" is running then\n\
         tell application \"Music\" to {verb}\n\
         else\n\
         error \"neither Spotify nor Music is running\"\n\
         end if"
    )
}

/// Presses a media key, which the active player handles.
fn windows_media_key(command: MediaCommand) -> String {
    // Virtual-key codes of the media keys.
    let key = match command {
        MediaCommand::PlayPause => 179,
        MediaCommand::Next => 176,
        MediaCommand::Previous => 177,
    };
    format!("(New-Object -ComObject WScript.Shell).SendKeys([char]{key})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_script_prefers_spotify_and_never_launches_apps() {
        let script = apple_script(MediaCommand::Next);

        assert!(script.contains(r#"application "Spotify" is running"#));
        assert!(script.contains(r#"tell application "Spotify" to next track"#));
        assert!(script.contains(r#"tell application "Music" to next track"#));
    }

    #[test]
    fn windows_presses_the_matching_media_key() {
        assert!(windows_media_key(MediaCommand::PlayPause).ends_with("[char]179)"));
        assert!(windows_media_key(MediaCommand::Previous).ends_with("[char]177)"));
    }

    #[test]
    fn this_system_has_a_way_to_control_music() {
        // The container is Linux: MPRIS via playerctl.
        let (program, args) = command_line(MediaCommand::PlayPause).unwrap();
        if cfg!(target_os = "linux") {
            assert_eq!(program, "playerctl");
            assert_eq!(args, ["play-pause"]);
        }
    }
}
