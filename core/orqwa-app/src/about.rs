//! What the About tab shows (R83): ORQWA's licence and those of everything built into the app,
//! each notice with its licence's full text, so the notices those licences ask for ship with
//! every copy (ADR 0018).

use std::fmt::Write as _;

use crate::ATTRIBUTION;

/// Where ORQWA's source code is, as the GPL offers it to everyone who has the app.
pub const SOURCE: &str = "https://github.com/ORQWA/ORQWA";

/// Between the licences in a notice, as in the crates' notices (`core/about.hbs`).
const RULE: &str = "────────────────────────────────────────";

/// The crates' notices, written by `scripts/notices.sh` with cargo-about.
const CRATES: &str = include_str!("../notices/crates.txt");
/// FFmpeg's licence, from its release's `COPYING.LGPLv2.1`.
const LGPL: &str = include_str!("../notices/LGPL-2.1.txt");
const GPL: &str = include_str!("../../../LICENSE");

/// A part of the app that a notice covers, in the order the About tab lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// ORQWA itself.
    Orqwa,
    /// The Godot engine and the components it is built from.
    Godot,
    /// FFmpeg's libraries, which decode the course videos (ADR 0010).
    Ffmpeg,
    /// The Rust crates built into the app.
    Crates,
    /// Models and data from others.
    Assets,
}

impl Part {
    /// Its name, by which the front end titles it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Part::Orqwa => "orqwa",
            Part::Godot => "godot",
            Part::Ffmpeg => "ffmpeg",
            Part::Crates => "crates",
            Part::Assets => "assets",
        }
    }
}

/// The copyright and licence of a part, then the licence's full text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// What it covers.
    pub part: Part,
    /// The notice as shown.
    pub text: String,
}

/// What the Godot engine reports of its licence and of the components it is built from; only
/// the running engine knows (`Engine.get_license_text`, `get_copyright_info`,
/// `get_license_info`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineCredits {
    /// Godot's own licence, with its copyright.
    pub licence: String,
    /// The components' files' copyrights, a component once for each licence it comes under.
    pub components: Vec<Component>,
    /// The texts of the components' licences, by the licences' names.
    pub licences: Vec<(String, String)>,
}

/// Part of a component of the Godot engine: its copyright holders and its licence's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// The component, e.g. `FreeType`.
    pub name: String,
    /// Each holder's copyright line.
    pub copyright: Vec<String>,
    /// The name of its licence, as in [`EngineCredits::licences`].
    pub licence: String,
}

/// The notices of ORQWA and of everything built into it, in the order they are listed;
/// `engine` is what the Godot engine reports of itself.
#[must_use]
pub fn notices(engine: &EngineCredits) -> Vec<Notice> {
    vec![
        Notice {
            part: Part::Orqwa,
            text: format!(
                "ORQWA {}\n\nORQWA is free software under the GNU General Public License, \
                 version 3. Its source code: {SOURCE}\n\n{GPL}",
                orqwa_domain::version()
            ),
        },
        Notice {
            part: Part::Godot,
            text: godot(engine),
        },
        Notice {
            part: Part::Ffmpeg,
            text: format!(
                "FFmpeg's libavcodec, libavformat, libavutil and libswscale, © the FFmpeg \
                 developers (https://ffmpeg.org).\n\nBuilt without FFmpeg's optional GPL parts, \
                 under the GNU Lesser General Public License, version 2.1 or later. Its source \
                 code: https://ffmpeg.org/download.html; ORQWA builds it with \
                 scripts/build-ffmpeg.sh in ORQWA's source code.\n\n{LGPL}"
            ),
        },
        Notice {
            part: Part::Crates,
            text: CRATES.to_owned(),
        },
        Notice {
            part: Part::Assets,
            text: assets(),
        },
    ]
}

/// Godot's licence, then its components with their copyrights and licences, then the texts of
/// those licences, each once.
fn godot(engine: &EngineCredits) -> String {
    let mut text = engine.licence.trim_end().to_owned();
    text.push_str("\n\nGodot is built from these components:\n");
    for component in &engine.components {
        text.push('\n');
        text.push_str(&component.name);
        text.push('\n');
        for holder in &component.copyright {
            let _ = writeln!(text, "© {holder}");
        }
        let _ = writeln!(text, "Licence: {}", component.licence);
    }
    for (name, licence) in &engine.licences {
        let _ = writeln!(text, "\n{RULE}\n\n{name}\n\n{}", licence.trim_end());
    }
    text
}

/// Others' models and data in the app (docs/CREDITS.md).
fn assets() -> String {
    let territory_license = include_str!("../../orqwa-osm/data/country-coder-LICENSE.md");
    let mut text = String::from(
        "Rider bodies: derived from the \"primitive\" bodies of Human Base Meshes v1.4.1 by \
         Blender Studio and contributors, CC0 1.0 (https://www.blender.org/download/demo-files/). \
         Buildings, vegetation, clouds, kits, helmets and bikes are ORQWA's own work, under \
         ORQWA's licence.\n\nMap and terrain data, credited on every course built from it:\n",
    );
    for line in ATTRIBUTION {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(
        "\nBuilding textures (CC0 1.0, https://creativecommons.org/publicdomain/zero/1.0/):\n\
         Painted Plaster Wall by Amal Kumar (https://polyhaven.com/a/painted_plaster_wall);\n\
         Roof Tiles by Stephan Seeliger (https://polyhaven.com/a/roof_tiles).\n\
         Albedo neutralized for regional colours; normal and roughness maps converted by ORQWA.\n",
    );
    text.push_str(
        "\nRide audio textures (CC0 1.0, https://creativecommons.org/publicdomain/zero/1.0/):\n\
         Wind and Leaves.wav by ecfike (https://freesound.org/people/ecfike/sounds/183496/);\n\
         Light Rain by soundrecorder7 (https://freesound.org/people/soundrecorder7/sounds/167034/);\n\
         bicycle forward movement by dawidwmika (https://freesound.org/people/dawidwmika/sounds/380288/);\n\
         bike-wheel-free-looped.flac by yurkobb (https://freesound.org/people/yurkobb/sounds/274481/);\n\
         bike_gear_shift.wav by ChemiCatz (https://freesound.org/people/ChemiCatz/sounds/267910/);\n\
         changingGears.wav by stib (https://freesound.org/people/stib/sounds/494322/).\n\
         Excerpts mixed to mono, filtered, resampled and level adjusted by ORQWA.\n",
    );
    text.push_str(
        "\nCountry codes (ISO 3166-1): from Debian's iso-codes \
         (https://salsa.debian.org/iso-codes-team/iso-codes), under the GNU Lesser General \
         Public License, version 2.1 or later (the text is with FFmpeg's notice).\n",
    );
    let _ = write!(
        text,
        "\n\nTraffic territory outlines: country-coder contributors, ISC. \
        https://github.com/rapideditor/country-coder\n\n{territory_license}"
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(notices: &[Notice], part: Part) -> &str {
        &notices
            .iter()
            .find(|notice| notice.part == part)
            .unwrap()
            .text
    }

    #[test]
    fn lists_orqwa_then_what_it_is_built_from() {
        let parts: Vec<Part> = notices(&EngineCredits::default())
            .iter()
            .map(|notice| notice.part)
            .collect();
        assert_eq!(
            parts,
            [
                Part::Orqwa,
                Part::Godot,
                Part::Ffmpeg,
                Part::Crates,
                Part::Assets
            ]
        );
    }

    #[test]
    fn orqwa_comes_with_the_gpl_and_where_its_source_is() {
        let notices = notices(&EngineCredits::default());
        let orqwa = text_of(&notices, Part::Orqwa);
        assert!(orqwa.starts_with(&format!("ORQWA {}", orqwa_domain::version())));
        assert!(orqwa.contains("GNU GENERAL PUBLIC LICENSE"));
        assert!(orqwa.contains("Version 3, 29 June 2007"));
        assert!(orqwa.contains(SOURCE));
    }

    #[test]
    fn ffmpeg_comes_with_the_lgpl() {
        let notices = notices(&EngineCredits::default());
        let ffmpeg = text_of(&notices, Part::Ffmpeg);
        assert!(ffmpeg.contains("GNU LESSER GENERAL PUBLIC LICENSE"));
        assert!(ffmpeg.contains("Version 2.1, February 1999"));
    }

    #[test]
    fn crates_are_third_party_with_their_licence_texts() {
        let notices = notices(&EngineCredits::default());
        let crates = text_of(&notices, Part::Crates);
        // The Godot bindings are linked into the app; ORQWA's own crates are under its licence.
        let mut names = crates.split([',', '\n']).map(str::trim_start);
        assert!(names.any(|entry| entry.starts_with("godot ")));
        assert!(!crates.contains("orqwa-"));
        assert!(crates.contains("Permission is hereby granted, free of charge"));
    }

    #[test]
    fn assets_credit_the_rider_bodies_and_every_course_data_source() {
        let notices = notices(&EngineCredits::default());
        let assets = text_of(&notices, Part::Assets);
        assert!(assets.contains("Human Base Meshes v1.4.1"));
        assert!(assets.contains("CC0 1.0"));
        assert!(assets.contains(include_str!(
            "../../orqwa-osm/data/country-coder-LICENSE.md"
        )));
        for line in ATTRIBUTION {
            assert!(assets.contains(line), "{line}");
        }
    }

    #[test]
    fn godot_credits_each_component_and_carries_each_licence_text_once() {
        let engine = EngineCredits {
            licence: "Copyright (c) Godot Engine contributors.\n".to_owned(),
            components: vec![
                Component {
                    name: "FreeType".to_owned(),
                    copyright: vec!["1996-2023 David Turner".to_owned()],
                    licence: "FTL".to_owned(),
                },
                Component {
                    name: "HarfBuzz".to_owned(),
                    copyright: vec!["2010 Red Hat".to_owned(), "2012 Google".to_owned()],
                    licence: "Expat".to_owned(),
                },
                Component {
                    name: "libpng".to_owned(),
                    copyright: vec!["1995-2024 The PNG Reference Library Authors".to_owned()],
                    licence: "Expat".to_owned(),
                },
            ],
            licences: vec![
                ("FTL".to_owned(), "The FreeType Project LICENSE".to_owned()),
                (
                    "Expat".to_owned(),
                    "Permission is hereby granted".to_owned(),
                ),
            ],
        };
        let notices = notices(&engine);
        let godot = text_of(&notices, Part::Godot);
        assert!(godot.starts_with("Copyright (c) Godot Engine contributors."));
        assert!(godot.contains("FreeType\n© 1996-2023 David Turner\nLicence: FTL\n"));
        assert!(godot.contains("HarfBuzz\n© 2010 Red Hat\n© 2012 Google\nLicence: Expat\n"));
        assert!(godot.contains("libpng\n"));
        assert_eq!(godot.matches("The FreeType Project LICENSE").count(), 1);
        assert_eq!(godot.matches("Permission is hereby granted").count(), 1);
    }
}
