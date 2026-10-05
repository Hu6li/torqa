//! The colour palette of the stylized look (ADR 0011). It lives in one file,
//! `app/assets/palette.json`, read here for vertex colours and by the app for its shaders, so a
//! colour changes in one place.

use std::sync::LazyLock;

use serde_json::Value;

const SOURCE: &str = include_str!("../../../app/assets/palette.json");

static PALETTE: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(SOURCE).expect("app/assets/palette.json is valid JSON"));

/// The colour `section.name` (e.g. `"ground.meadow"`) as sRGB, with `alpha` free for flags the
/// shaders read.
///
/// # Panics
/// If the palette has no such colour: names are fixed in the code, and the tests use them all.
pub(crate) fn srgb(path: &str, alpha: f32) -> [f32; 4] {
    let [r, g, b] = hex(entry(path).as_str().unwrap_or_default())
        .unwrap_or_else(|| panic!("palette colour {path} is not #rrggbb"));
    [r, g, b, alpha]
}

/// The `index`-th colour of the list `section.name`, wrapping around.
///
/// # Panics
/// If the palette has no such list.
pub(crate) fn pick(path: &str, index: usize, alpha: f32) -> [f32; 4] {
    let list = entry(path)
        .as_array()
        .filter(|list| !list.is_empty())
        .unwrap_or_else(|| panic!("palette entry {path} is not a list of colours"));
    let [r, g, b] = hex(list[index % list.len()].as_str().unwrap_or_default())
        .unwrap_or_else(|| panic!("palette list {path} holds a colour that is not #rrggbb"));
    [r, g, b, alpha]
}

fn entry(path: &str) -> &'static Value {
    let (section, name) = path.split_once('.').unwrap_or((path, ""));
    PALETTE
        .get(section)
        .and_then(|section| section.get(name))
        .unwrap_or_else(|| panic!("palette has no colour {path}"))
}

/// `#rrggbb` as three channels from 0 to 1.
fn hex(text: &str) -> Option<[f32; 3]> {
    let digits = text.strip_prefix('#').filter(|digits| digits.len() == 6)?;
    let channel = |at: usize| {
        u8::from_str_radix(&digits[at..at + 2], 16)
            .ok()
            .map(|value| f32::from(value) / 255.0)
    };
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_a_colour_or_a_list_of_colours() {
        let sections = PALETTE.as_object().expect("palette is an object");
        for (section, entries) in sections {
            let Some(entries) = entries.as_object() else {
                continue;
            };
            for (name, value) in entries {
                let colours: Vec<&Value> = match value {
                    Value::Array(list) => list.iter().collect(),
                    other => vec![other],
                };
                for colour in colours {
                    assert!(
                        colour.as_str().and_then(hex).is_some(),
                        "{section}.{name} holds {colour}, not #rrggbb"
                    );
                }
            }
        }
    }

    #[test]
    fn colours_are_read_as_srgb_channels() {
        assert_eq!(hex("#ff8000"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(hex("ff8000"), None);
        assert!((srgb("ground.meadow", 0.5)[3] - 0.5).abs() < f32::EPSILON);
        let first = pick("plants.flowers", 0, 1.0);
        assert_eq!(pick("plants.flowers", 4, 1.0), first, "lists wrap around");
    }
}
