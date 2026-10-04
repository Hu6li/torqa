//! Incyclist's route video format (`gpx-import`), used by free route-video libraries such as
//! Fred Van Gestel's: an XML control file naming the video and a GPX file recorded with it. The
//! GPX's timestamps, counted from its first point, are the video's timeline.

use quick_xml::Reader;
use quick_xml::events::Event;

/// Reading a control file failed.
#[derive(Debug, thiserror::Error)]
pub enum IncyclistError {
    /// The file is not valid XML.
    #[error("invalid route video file: {0}")]
    Xml(String),
    /// The root element is not `gpx-import` (e.g. commercial RLV/KWT formats).
    #[error("not an Incyclist route video (gpx-import) file")]
    Unsupported,
    /// A required element is missing.
    #[error("route video file lacks <{0}>")]
    Missing(&'static str),
}

/// What an Incyclist control file says.
#[derive(Debug, Clone, PartialEq)]
pub struct IncyclistRoute {
    /// Display name.
    pub title: String,
    /// The video, relative to the control file.
    pub video_file: String,
    /// The GPX track, relative to the control file.
    pub gpx_file: String,
    /// Frames per second.
    pub frame_rate: f64,
    /// The video frame the GPX starts at (0 or 1 for the first).
    pub start_frame: u64,
}

impl IncyclistRoute {
    /// Where the GPX's first point sits in the video.
    #[must_use]
    pub fn video_offset(&self) -> std::time::Duration {
        #[allow(clippy::cast_precision_loss)] // frame numbers far below 2^52
        let frames = self.start_frame.saturating_sub(1) as f64;
        std::time::Duration::from_secs_f64(frames / self.frame_rate.max(1.0))
    }
}

/// Reads an Incyclist `gpx-import` control file.
///
/// # Errors
/// [`IncyclistError`] if it is not such a file or lacks the video, GPX or frame rate.
pub fn parse(xml: &str) -> Result<IncyclistRoute, IncyclistError> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| IncyclistError::Xml(e.to_string()))?;
        match event {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if path.is_empty() && name != "gpx-import" {
                    return Err(IncyclistError::Unsupported);
                }
                path.push(name);
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => {
                let name = r.to_string();
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(c) = entity(&name) {
                    text.push(c);
                }
            }
            Event::End(_) => {
                if path.len() == 2 {
                    fields.push((path[1].clone(), text.trim().to_owned()));
                }
                path.pop();
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, value)| key == name && !value.is_empty())
            .map(|(_, value)| value.clone())
    };
    if fields.is_empty() {
        return Err(IncyclistError::Unsupported);
    }
    let video_file = field("video-file-path").ok_or(IncyclistError::Missing("video-file-path"))?;
    Ok(IncyclistRoute {
        title: field("title")
            .or_else(|| field("name"))
            .unwrap_or_else(|| video_file.clone()),
        gpx_file: field("gpx-file-path").ok_or(IncyclistError::Missing("gpx-file-path"))?,
        video_file,
        frame_rate: field("framerate")
            .and_then(|f| f.parse().ok())
            .ok_or(IncyclistError::Missing("framerate"))?,
        start_frame: field("start-frame")
            .and_then(|f| f.parse().ok())
            .unwrap_or(0),
    })
}

/// HTML entities these files use in titles (they are not XML entities); unknown ones are
/// dropped.
fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "auml" => 'ä',
        "ouml" => 'ö',
        "uuml" => 'ü',
        "Auml" => 'Ä',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        "szlig" => 'ß',
        "eacute" => 'é',
        "egrave" => 'è',
        "ecirc" => 'ê',
        "Eacute" => 'É',
        "aacute" => 'á',
        "agrave" => 'à',
        "acirc" => 'â',
        "iacute" => 'í',
        "icirc" => 'î',
        "oacute" => 'ó',
        "ocirc" => 'ô',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "ccedil" => 'ç',
        "ntilde" => 'ñ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROL: &str = r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<gpx-import>
    <name>FR_Example</name>
    <id>e60beaa7-1238-41c9-adbb-6be022de73ca</id>
    <country>FR</country>
    <title>Source Dr&ocirc;me 1 - Col de Pr&eacute;mol</title>
    <video-file-path>FR_Example.mp4</video-file-path>
    <gpx-file-path>FR_Example.gpx</gpx-file-path>
    <framerate>30.0</framerate>
    <start-frame>1</start-frame>
    <end-frame>125527</end-frame>
    <informations>
        <information distance="120" en="Part 1 &amp; more" />
    </informations>
</gpx-import>"#;

    #[test]
    fn reads_a_control_file() {
        let route = parse(CONTROL).unwrap();

        assert_eq!(route.title, "Source Drôme 1 - Col de Prémol");
        assert_eq!(route.video_file, "FR_Example.mp4");
        assert_eq!(route.gpx_file, "FR_Example.gpx");
        assert!((route.frame_rate - 30.0).abs() < 1e-9);
        // Start frame 1 is the first frame: no offset.
        assert_eq!(route.video_offset(), std::time::Duration::ZERO);
    }

    #[test]
    fn a_later_start_frame_shifts_the_track_into_the_video() {
        let late = CONTROL.replace(
            "<start-frame>1</start-frame>",
            "<start-frame>61</start-frame>",
        );

        assert_eq!(
            parse(&late).unwrap().video_offset(),
            std::time::Duration::from_secs(2)
        );
    }

    #[test]
    fn other_formats_are_refused() {
        assert!(matches!(
            parse("<kwt><name>x</name></kwt>"),
            Err(IncyclistError::Unsupported)
        ));
        assert!(matches!(
            parse(&CONTROL.replace("<gpx-file-path>FR_Example.gpx</gpx-file-path>", "")),
            Err(IncyclistError::Missing("gpx-file-path"))
        ));
    }
}
