//! The course library as the Courses tab shows it (R67): searched by name, filtered by type,
//! terrain, length and saved progress, sorted and grouped.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use orqwa_domain::profile::UnitSystem;
use orqwa_domain::units::Meters;
pub use orqwa_storage::course::CourseCountry;

use crate::CourseEntry;

/// Metres in a mile.
const MILE: f64 = 1_609.344;
/// The country chip and group of the courses whose countries are not known: ISO codes are
/// two capital letters, so it cannot name a country.
pub const NO_COUNTRY: &str = "none";

/// Gives an enum `ALL`, `name` and `from_name`: the names the front end and the settings file
/// know its values by.
macro_rules! named {
    ($type:ident { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $type {
            /// Every value, in the order they are listed.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The name the front end and the settings use.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            /// The value called `name`, if any.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.name() == name)
            }
        }
    };
}

/// Whether a course is ridden in the 3D world or along a video (R17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CourseKind {
    /// Ridden in the 3D world.
    ThreeD,
    /// Ridden along a video, with or without a place (Tacx RLV).
    Video,
}

named!(CourseKind {
    ThreeD => "3d",
    Video => "video",
});

/// How hilly a course is, by how much it climbs per kilometre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Terrain {
    /// Under 5 m per km.
    Flat,
    /// 5 to 10 m per km.
    Rolling,
    /// 10 to 20 m per km.
    Hilly,
    /// 20 m per km and more.
    Mountainous,
}

named!(Terrain {
    Flat => "flat",
    Rolling => "rolling",
    Hilly => "hilly",
    Mountainous => "mountainous",
});

impl Terrain {
    /// Climbing per kilometre, in metres, from which each terrain after flat begins.
    const FROM: [f64; 3] = [5.0, 10.0, 20.0];

    /// The terrain of a course `length` long that climbs `gain` in all.
    #[must_use]
    pub fn of(length: Meters, gain: Meters) -> Self {
        let per_km = if length.0 > 0.0 {
            gain.0 / (length.0 / 1000.0)
        } else {
            0.0
        };
        match Self::FROM.iter().filter(|&&from| per_km >= from).count() {
            0 => Self::Flat,
            1 => Self::Rolling,
            2 => Self::Hilly,
            _ => Self::Mountainous,
        }
    }
}

/// How long a course is, in four bands with round edges in the rider's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LengthBand {
    /// Up to the first edge.
    Short,
    /// Between the first and second edge.
    Medium,
    /// Between the second and third edge.
    Long,
    /// From the third edge.
    VeryLong,
}

named!(LengthBand {
    Short => "short",
    Medium => "medium",
    Long => "long",
    VeryLong => "very_long",
});

impl LengthBand {
    /// Where the bands meet: 25, 50 and 100 km, or 15, 30 and 60 miles, rather than kilometres
    /// converted to odd miles.
    #[must_use]
    pub fn edges(units: UnitSystem) -> [Meters; 3] {
        match units {
            UnitSystem::Metric => [25_000.0, 50_000.0, 100_000.0].map(Meters),
            UnitSystem::Imperial => [15.0, 30.0, 60.0].map(|miles| Meters(miles * MILE)),
        }
    }

    /// The band of a course `length` long; an edge belongs to the band above it.
    #[must_use]
    pub fn of(length: Meters, units: UnitSystem) -> Self {
        let passed = Self::edges(units)
            .iter()
            .filter(|edge| length.0 >= edge.0)
            .count();
        Self::ALL[passed]
    }
}

/// What the gallery is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// The course's name.
    #[default]
    Name,
    /// Its length.
    Length,
    /// Its total climbing.
    Climbing,
    /// The time the rider is estimated to take (R66).
    Time,
    /// When it was prepared or imported.
    Added,
    /// When the rider last rode it.
    Ridden,
}

named!(SortKey {
    Name => "name",
    Length => "length",
    Climbing => "climbing",
    Time => "time",
    Added => "added",
    Ridden => "ridden",
});

impl SortKey {
    /// Whether this order is wanted from the top down when first chosen: the latest courses
    /// and rides are the ones looked for, the rest from A, short, flat and quick.
    #[must_use]
    pub fn natural_descending(self) -> bool {
        matches!(self, Self::Added | Self::Ridden)
    }
}

/// What the gallery is grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupKey {
    /// One gallery, no groups.
    #[default]
    None,
    /// 3D and video courses.
    Kind,
    /// Flat to mountainous.
    Terrain,
    /// Length bands.
    Length,
    /// The countries passed (#230); a course passing several is in each of their groups.
    Country,
}

named!(GroupKey {
    None => "none",
    Kind => "type",
    Terrain => "terrain",
    Length => "length",
    Country => "country",
});

/// How the gallery is ordered and grouped, remembered between starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct View {
    /// What it is sorted by.
    pub sort: SortKey,
    /// Whether from the largest, latest or last letter down.
    pub descending: bool,
    /// What it is grouped by.
    pub group: GroupKey,
}

/// What the rider asks the gallery for: the view, a search and the chips chosen. An empty
/// list of chips allows every value; chips of one kind add to each other, kinds narrow down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CourseQuery {
    /// Order and groups.
    pub view: View,
    /// Words the course's name contains, of any case; empty for all.
    pub search: String,
    /// The kinds of course shown.
    pub kinds: Vec<CourseKind>,
    /// The terrains shown.
    pub terrains: Vec<Terrain>,
    /// The length bands shown.
    pub lengths: Vec<LengthBand>,
    /// The countries shown, by code, [`NO_COUNTRY`] for courses whose countries are not
    /// known; a course passing any of them is shown.
    pub countries: Vec<String>,
    /// Show only courses with a continuation point belonging to the active rider.
    pub in_progress: bool,
    /// The language to name countries in, e.g. `de` or `de_CH`; their names are searched
    /// and sorted in it.
    pub locale: String,
}

/// A course with what the gallery sorts and filters it by beyond its manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryCourse {
    /// The course file and its manifest.
    pub entry: CourseEntry,
    /// How long the rider would take at the planned share of their FTP (R66), estimated from
    /// the profile the course file keeps; `None` if that power would not get them there.
    pub estimated: Option<Duration>,
    /// When the rider last started a ride on it; `None` if never.
    pub last_ridden: Option<SystemTime>,
    /// Whether the active rider has a continuation point for this course.
    pub in_progress: bool,
}

impl LibraryCourse {
    /// Whether it is ridden in the 3D world or along a video.
    #[must_use]
    pub fn kind(&self) -> CourseKind {
        if self.entry.manifest.video.is_some() {
            CourseKind::Video
        } else {
            CourseKind::ThreeD
        }
    }

    /// How hilly it is.
    #[must_use]
    pub fn terrain(&self) -> Terrain {
        Terrain::of(
            Meters(self.entry.manifest.length_m),
            Meters(self.entry.manifest.elevation_gain_m),
        )
    }

    /// Its length band in `units`.
    #[must_use]
    pub fn length_band(&self, units: UnitSystem) -> LengthBand {
        LengthBand::of(Meters(self.entry.manifest.length_m), units)
    }

    /// The countries it passes, as far as known.
    #[must_use]
    pub fn countries(&self) -> &[CourseCountry] {
        self.entry.manifest.countries.as_deref().unwrap_or_default()
    }
}

/// The name of `country` in `locale` (e.g. `de_CH`, then `de`), else in English, else in its
/// own language, else its code.
#[must_use]
pub fn country_name<'a>(country: &'a CourseCountry, locale: &str) -> &'a str {
    let language = locale.split(['_', '-']).next().unwrap_or_default();
    [locale, language, "en", ""]
        .iter()
        .find_map(|key| country.names.get(*key))
        .map_or(country.code.as_str(), String::as_str)
}

/// A country to choose among the library's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountryChoice {
    /// Its code, or [`NO_COUNTRY`].
    pub code: String,
    /// Its name in the query's language; empty for [`NO_COUNTRY`].
    pub name: String,
}

/// One group of the gallery.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Every course shown, when not grouped.
    All,
    /// The courses of one kind.
    Kind(CourseKind),
    /// The courses of one terrain.
    Terrain(Terrain),
    /// The courses of one length band.
    Length(LengthBand),
    /// The courses passing one country, by code; `None` for those whose countries are not
    /// known.
    Country(Option<String>),
}

impl Group {
    /// The name of the kind, terrain or band it holds, the country's code or [`NO_COUNTRY`];
    /// empty for [`Group::All`].
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::All => "",
            Self::Kind(kind) => kind.name(),
            Self::Terrain(terrain) => terrain.name(),
            Self::Length(band) => band.name(),
            Self::Country(code) => code.as_deref().unwrap_or(NO_COUNTRY),
        }
    }
}

/// A group of the gallery with its courses in order.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseGroup {
    /// Which group.
    pub group: Group,
    /// The country's name in the query's language, for a country's group; else empty.
    pub label: String,
    /// Its courses, sorted as asked.
    pub courses: Vec<LibraryCourse>,
}

/// The gallery as asked for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gallery {
    /// How many courses the library holds, shown or not.
    pub total: usize,
    /// The groups with courses shown, in their own order (3D before video, flat to
    /// mountainous, short to long, countries by name and those not known last).
    pub groups: Vec<CourseGroup>,
    /// The countries the library's courses pass, shown or not, by name, then
    /// [`NO_COUNTRY`] if some courses' countries are not known.
    pub countries: Vec<CountryChoice>,
}

/// The gallery of `courses` that `query` asks for, length bands in `units`.
#[must_use]
pub fn gallery(courses: Vec<LibraryCourse>, query: &CourseQuery, units: UnitSystem) -> Gallery {
    let total = courses.len();
    let countries = country_choices(&courses, &query.locale);
    let search = query.search.trim().to_lowercase();
    let mut groups: BTreeMap<Group, Vec<LibraryCourse>> = BTreeMap::new();
    for course in courses {
        let shown = found(&course, &search, &query.locale)
            && (!query.in_progress || course.in_progress)
            && allows(&query.kinds, &course.kind())
            && allows(&query.terrains, &course.terrain())
            && allows(&query.lengths, &course.length_band(units))
            && passes(&course, &query.countries);
        if !shown {
            continue;
        }
        let group = match query.view.group {
            GroupKey::None => Group::All,
            GroupKey::Kind => Group::Kind(course.kind()),
            GroupKey::Terrain => Group::Terrain(course.terrain()),
            GroupKey::Length => Group::Length(course.length_band(units)),
            GroupKey::Country => {
                for country in course.countries() {
                    let group = Group::Country(Some(country.code.clone()));
                    groups.entry(group).or_default().push(course.clone());
                }
                if !course.countries().is_empty() {
                    continue;
                }
                Group::Country(None)
            }
        };
        groups.entry(group).or_default().push(course);
    }
    let mut groups: Vec<CourseGroup> = groups
        .into_iter()
        .map(|(group, mut courses)| {
            courses.sort_by(|a, b| order(a, b, query.view));
            let label = match &group {
                Group::Country(Some(code)) => countries
                    .iter()
                    .find(|c| &c.code == code)
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            CourseGroup {
                group,
                label,
                courses,
            }
        })
        .collect();
    // Countries by their names in the rider's language, those not known last.
    if query.view.group == GroupKey::Country {
        groups.sort_by_cached_key(|g| (g.group == Group::Country(None), g.label.to_lowercase()));
    }
    Gallery {
        total,
        groups,
        countries,
    }
}

/// Whether the search finds `course`: in its name, or in the name of a country it passes in
/// the rider's language, in English or in the country's own.
fn found(course: &LibraryCourse, search: &str, locale: &str) -> bool {
    course.entry.manifest.name.to_lowercase().contains(search)
        || course.countries().iter().any(|country| {
            let in_english = country.names.get("en").map(String::as_str);
            let own = country.names.get("").map(String::as_str);
            [Some(country_name(country, locale)), in_english, own]
                .into_iter()
                .flatten()
                .any(|name| name.to_lowercase().contains(search))
        })
}

/// Whether `course` passes one of the countries `chosen` (codes or [`NO_COUNTRY`]); all
/// courses do when none is chosen.
fn passes(course: &LibraryCourse, chosen: &[String]) -> bool {
    chosen.is_empty()
        || (course.countries().is_empty() && chosen.iter().any(|c| c == NO_COUNTRY))
        || course
            .countries()
            .iter()
            .any(|country| chosen.contains(&country.code))
}

/// The countries `courses` pass, by name in `locale`, then [`NO_COUNTRY`] if some courses'
/// countries are not known.
fn country_choices(courses: &[LibraryCourse], locale: &str) -> Vec<CountryChoice> {
    let mut choices: Vec<CountryChoice> = Vec::new();
    for country in courses.iter().flat_map(LibraryCourse::countries) {
        if !choices.iter().any(|c| c.code == country.code) {
            choices.push(CountryChoice {
                code: country.code.clone(),
                name: country_name(country, locale).to_owned(),
            });
        }
    }
    choices.sort_by_cached_key(|c| c.name.to_lowercase());
    if courses.iter().any(|c| c.countries().is_empty()) {
        choices.push(CountryChoice {
            code: NO_COUNTRY.to_owned(),
            name: String::new(),
        });
    }
    choices
}

/// Whether chips `chosen` let `value` through: all of them do when none is chosen.
fn allows<T: PartialEq>(chosen: &[T], value: &T) -> bool {
    chosen.is_empty() || chosen.contains(value)
}

/// The order of two courses in `view`, by name where they tie.
fn order(a: &LibraryCourse, b: &LibraryCourse, view: View) -> Ordering {
    let (left, right) = (&a.entry.manifest, &b.entry.manifest);
    let directed = |ordering: Ordering| {
        if view.descending {
            ordering.reverse()
        } else {
            ordering
        }
    };
    let first = match view.sort {
        SortKey::Name => directed(by_name(a, b)),
        SortKey::Length => directed(left.length_m.total_cmp(&right.length_m)),
        SortKey::Climbing => directed(left.elevation_gain_m.total_cmp(&right.elevation_gain_m)),
        SortKey::Added => directed(left.created_unix_s.cmp(&right.created_unix_s)),
        SortKey::Time => known_first(a.estimated, b.estimated, directed),
        SortKey::Ridden => known_first(a.last_ridden, b.last_ridden, directed),
    };
    first.then_with(|| by_name(a, b))
}

/// Known values in `directed` order, then the courses without one, whichever the direction:
/// a course never ridden is not the one ridden longest ago.
fn known_first<T: Ord>(
    a: Option<T>,
    b: Option<T>,
    directed: impl Fn(Ordering) -> Ordering,
) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => directed(a.cmp(&b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Alphabetical of any case, then by file so that the order is always the same.
fn by_name(a: &LibraryCourse, b: &LibraryCourse) -> Ordering {
    a.entry
        .manifest
        .name
        .to_lowercase()
        .cmp(&b.entry.manifest.name.to_lowercase())
        .then_with(|| a.entry.path.cmp(&b.entry.path))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use orqwa_storage::course::{Manifest, VideoReference};

    use super::*;

    const KM: f64 = 1000.0;

    fn course(name: &str, km: f64, gain_m: f64) -> LibraryCourse {
        LibraryCourse {
            entry: CourseEntry {
                path: PathBuf::from(format!("/courses/{name}.oqc")),
                manifest: Manifest {
                    format: 1,
                    generator: "ORQWA test".to_owned(),
                    name: name.to_owned(),
                    length_m: km * KM,
                    elevation_gain_m: gain_m,
                    max_grade_percent: 8.0,
                    created_unix_s: 0,
                    attribution: Vec::new(),
                    route_key: None,
                    track: Vec::new(),
                    profile: Vec::new(),
                    video: None,
                    countries: None,
                    map_detail: Vec::new(),
                },
            },
            estimated: None,
            last_ridden: None,
            in_progress: false,
        }
    }

    fn video(mut course: LibraryCourse) -> LibraryCourse {
        course.entry.manifest.video = Some(VideoReference {
            path: "/videos/ride.mp4".to_owned(),
            file_name: "ride.mp4".to_owned(),
            size: 1,
            offset_s: 0.0,
            end_s: None,
            marks: Vec::new(),
            located: true,
        });
        course
    }

    fn country(code: &str, names: &[(&str, &str)]) -> CourseCountry {
        CourseCountry {
            code: code.to_owned(),
            names: names
                .iter()
                .map(|&(language, name)| (language.to_owned(), name.to_owned()))
                .collect(),
        }
    }

    fn switzerland() -> CourseCountry {
        country(
            "CH",
            &[
                ("", "Schweiz/Suisse/Svizzera/Svizra"),
                ("de", "Schweiz"),
                ("en", "Switzerland"),
                ("fr", "Suisse"),
            ],
        )
    }

    fn passing(mut course: LibraryCourse, countries: Option<Vec<CourseCountry>>) -> LibraryCourse {
        course.entry.manifest.countries = countries;
        course
    }

    /// Bern, a ride from Basel into France and Germany, and two whose countries are not
    /// known: one looked up, one not yet.
    fn countries_library() -> Vec<LibraryCourse> {
        let france = country("FR", &[("de", "Frankreich"), ("en", "France")]);
        let germany = country("DE", &[("de", "Deutschland"), ("en", "Germany")]);
        vec![
            passing(
                course("Gurten loop", 31.0, 380.0),
                Some(vec![switzerland()]),
            ),
            passing(
                course("Dreiländer loop", 54.0, 420.0),
                Some(vec![switzerland(), france, germany]),
            ),
            passing(course("Somewhere", 20.0, 100.0), Some(Vec::new())),
            passing(course("Old course", 25.0, 100.0), None),
        ]
    }

    fn query_in(locale: &str) -> CourseQuery {
        CourseQuery {
            locale: locale.to_owned(),
            ..CourseQuery::default()
        }
    }

    fn added(mut course: LibraryCourse, unix_s: u64) -> LibraryCourse {
        course.entry.manifest.created_unix_s = unix_s;
        course
    }

    fn timed(mut course: LibraryCourse, minutes: Option<u64>) -> LibraryCourse {
        course.estimated = minutes.map(Duration::from_mins);
        course
    }

    fn ridden(mut course: LibraryCourse, days_ago: Option<u64>) -> LibraryCourse {
        let now = SystemTime::UNIX_EPOCH + Duration::from_hours(24 * 365);
        course.last_ridden = days_ago.map(|days| now - Duration::from_hours(24 * days));
        course
    }

    fn library() -> Vec<LibraryCourse> {
        vec![
            course("Gurten loop", 31.0, 380.0),
            course("aare flats", 42.0, 60.0),
            video(course("Jura ridge", 48.0, 720.0)),
            course("Grimsel", 120.0, 2_600.0),
        ]
    }

    fn names(gallery: &Gallery) -> Vec<Vec<&str>> {
        gallery
            .groups
            .iter()
            .map(|group| {
                group
                    .courses
                    .iter()
                    .map(|c| c.entry.manifest.name.as_str())
                    .collect()
            })
            .collect()
    }

    fn sorted(courses: Vec<LibraryCourse>, sort: SortKey, descending: bool) -> Vec<String> {
        let query = CourseQuery {
            view: View {
                sort,
                descending,
                group: GroupKey::None,
            },
            ..CourseQuery::default()
        };
        gallery(courses, &query, UnitSystem::Metric).groups[0]
            .courses
            .iter()
            .map(|c| c.entry.manifest.name.clone())
            .collect()
    }

    #[test]
    fn terrain_follows_the_climbing_per_kilometre() {
        let terrain = |gain| Terrain::of(Meters(10.0 * KM), Meters(gain));

        assert_eq!(terrain(49.0), Terrain::Flat);
        assert_eq!(terrain(50.0), Terrain::Rolling);
        assert_eq!(terrain(99.0), Terrain::Rolling);
        assert_eq!(terrain(100.0), Terrain::Hilly);
        assert_eq!(terrain(200.0), Terrain::Mountainous);
        assert_eq!(Terrain::of(Meters(0.0), Meters(10.0)), Terrain::Flat);
    }

    #[test]
    fn length_bands_have_round_edges_in_the_riders_units() {
        let metric = |km: f64| LengthBand::of(Meters(km * KM), UnitSystem::Metric);
        let imperial = |miles: f64| LengthBand::of(Meters(miles * MILE), UnitSystem::Imperial);

        assert_eq!(metric(24.9), LengthBand::Short);
        assert_eq!(metric(25.0), LengthBand::Medium);
        assert_eq!(metric(50.0), LengthBand::Long);
        assert_eq!(metric(100.0), LengthBand::VeryLong);
        assert_eq!(imperial(14.9), LengthBand::Short);
        assert_eq!(imperial(15.0), LengthBand::Medium);
        assert_eq!(imperial(59.9), LengthBand::Long);
        assert_eq!(imperial(60.0), LengthBand::VeryLong);
    }

    #[test]
    fn the_search_finds_names_of_any_case() {
        let query = CourseQuery {
            search: "  GURTEN ".to_owned(),
            ..CourseQuery::default()
        };

        let found = gallery(library(), &query, UnitSystem::Metric);

        assert_eq!(names(&found), [["Gurten loop"]]);
        assert_eq!(found.total, 4, "the library still holds all four");
    }

    #[test]
    fn in_progress_requires_a_saved_point_and_combines_with_other_filters() {
        let courses = || {
            let mut world = passing(course("Jura world", 30.0, 400.0), Some(vec![switzerland()]));
            world.in_progress = true;
            let mut film = video(passing(
                course("Jura video", 30.0, 400.0),
                Some(vec![switzerland()]),
            ));
            film.in_progress = true;
            vec![
                world,
                film,
                ridden(course("Jura finished", 30.0, 400.0), Some(1)),
            ]
        };
        let progress = CourseQuery {
            in_progress: true,
            ..CourseQuery::default()
        };
        let found = gallery(courses(), &progress, UnitSystem::Metric);
        assert_eq!(found.total, 3);
        assert_eq!(names(&found), [["Jura video", "Jura world"]]);
        let combined = CourseQuery {
            search: "jura".into(),
            kinds: vec![CourseKind::Video],
            terrains: vec![Terrain::Hilly],
            lengths: vec![LengthBand::Medium],
            countries: vec!["CH".into()],
            ..progress
        };
        assert_eq!(
            names(&gallery(courses(), &combined, UnitSystem::Metric)),
            [["Jura video"]]
        );
        let no_match = CourseQuery {
            search: "missing".into(),
            ..combined
        };
        assert_eq!(gallery(courses(), &no_match, UnitSystem::Metric).groups, []);
        assert_eq!(
            names(&gallery(
                courses(),
                &CourseQuery::default(),
                UnitSystem::Metric
            )),
            [["Jura finished", "Jura video", "Jura world"]]
        );
    }

    #[test]
    fn chips_of_one_kind_add_up_and_kinds_narrow_down() {
        let both_kinds = CourseQuery {
            kinds: vec![CourseKind::ThreeD, CourseKind::Video],
            ..CourseQuery::default()
        };
        assert_eq!(
            gallery(library(), &both_kinds, UnitSystem::Metric).groups[0]
                .courses
                .len(),
            4
        );

        let hilly_and_medium = CourseQuery {
            terrains: vec![Terrain::Hilly, Terrain::Mountainous],
            lengths: vec![LengthBand::Medium],
            ..CourseQuery::default()
        };
        assert_eq!(
            names(&gallery(library(), &hilly_and_medium, UnitSystem::Metric)),
            [["Gurten loop", "Jura ridge"]]
        );

        let nothing = CourseQuery {
            kinds: vec![CourseKind::Video],
            terrains: vec![Terrain::Flat],
            ..CourseQuery::default()
        };
        let empty = gallery(library(), &nothing, UnitSystem::Metric);
        assert_eq!(empty.groups, []);
        assert_eq!(empty.total, 4);
    }

    #[test]
    fn names_sort_alphabetically_of_any_case() {
        assert_eq!(
            sorted(library(), SortKey::Name, false),
            ["aare flats", "Grimsel", "Gurten loop", "Jura ridge"]
        );
        assert_eq!(
            sorted(library(), SortKey::Name, true),
            ["Jura ridge", "Gurten loop", "Grimsel", "aare flats"]
        );
    }

    #[test]
    fn length_and_climbing_sort_by_their_figures() {
        assert_eq!(
            sorted(library(), SortKey::Length, false),
            ["Gurten loop", "aare flats", "Jura ridge", "Grimsel"]
        );
        assert_eq!(
            sorted(library(), SortKey::Climbing, true),
            ["Grimsel", "Jura ridge", "Gurten loop", "aare flats"]
        );
    }

    #[test]
    fn the_latest_courses_come_first() {
        assert!(SortKey::Added.natural_descending());
        let courses = vec![
            added(course("Old", 30.0, 0.0), 100),
            added(course("New", 30.0, 0.0), 300),
            added(course("Middle", 30.0, 0.0), 200),
        ];

        assert_eq!(
            sorted(courses, SortKey::Added, true),
            ["New", "Middle", "Old"]
        );
    }

    #[test]
    fn courses_without_an_estimate_come_last_either_way() {
        let courses = || {
            vec![
                timed(course("Wall", 5.0, 900.0), None),
                timed(course("Long", 90.0, 900.0), Some(200)),
                timed(course("Quick", 20.0, 100.0), Some(40)),
            ]
        };

        assert_eq!(
            sorted(courses(), SortKey::Time, false),
            ["Quick", "Long", "Wall"]
        );
        assert_eq!(
            sorted(courses(), SortKey::Time, true),
            ["Long", "Quick", "Wall"]
        );
    }

    #[test]
    fn the_courses_ridden_last_come_first_and_those_never_ridden_last() {
        assert!(SortKey::Ridden.natural_descending());
        assert!(!SortKey::Time.natural_descending());
        let courses = || {
            vec![
                ridden(course("Never", 30.0, 0.0), None),
                ridden(course("Last week", 30.0, 0.0), Some(7)),
                ridden(course("Yesterday", 30.0, 0.0), Some(1)),
            ]
        };

        assert_eq!(
            sorted(courses(), SortKey::Ridden, true),
            ["Yesterday", "Last week", "Never"]
        );
        assert_eq!(
            sorted(courses(), SortKey::Ridden, false),
            ["Last week", "Yesterday", "Never"]
        );
    }

    #[test]
    fn groups_come_in_their_own_order_and_only_with_courses() {
        let by = |group| CourseQuery {
            view: View {
                group,
                ..View::default()
            },
            ..CourseQuery::default()
        };

        let terrains = gallery(library(), &by(GroupKey::Terrain), UnitSystem::Metric);
        assert_eq!(
            terrains
                .groups
                .iter()
                .map(|g| g.group.clone())
                .collect::<Vec<_>>(),
            [
                Group::Terrain(Terrain::Flat),
                Group::Terrain(Terrain::Hilly),
                Group::Terrain(Terrain::Mountainous),
            ]
        );
        assert_eq!(
            names(&terrains),
            [
                vec!["aare flats"],
                vec!["Gurten loop", "Jura ridge"],
                vec!["Grimsel"]
            ]
        );

        let kinds = gallery(library(), &by(GroupKey::Kind), UnitSystem::Metric);
        assert_eq!(
            names(&kinds),
            [
                vec!["aare flats", "Grimsel", "Gurten loop"],
                vec!["Jura ridge"]
            ]
        );

        let lengths = gallery(library(), &by(GroupKey::Length), UnitSystem::Metric);
        assert_eq!(
            lengths
                .groups
                .iter()
                .map(|g| g.group.name())
                .collect::<Vec<_>>(),
            ["medium", "very_long"]
        );
    }

    #[test]
    fn countries_are_named_in_the_riders_language_else_in_english_or_their_own() {
        let swiss = switzerland();
        assert_eq!(country_name(&swiss, "de_CH"), "Schweiz");
        assert_eq!(country_name(&swiss, "fr"), "Suisse");
        assert_eq!(country_name(&swiss, "it"), "Switzerland");
        let local_only = country("LI", &[("", "Liechtenstein")]);
        assert_eq!(country_name(&local_only, "de"), "Liechtenstein");
        assert_eq!(country_name(&country("XK", &[]), "de"), "XK");
    }

    #[test]
    fn a_course_is_grouped_under_every_country_it_passes() {
        let query = CourseQuery {
            view: View {
                group: GroupKey::Country,
                ..View::default()
            },
            ..query_in("de")
        };

        let grouped = gallery(countries_library(), &query, UnitSystem::Metric);

        let labels: Vec<&str> = grouped.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["Deutschland", "Frankreich", "Schweiz", ""]);
        assert_eq!(grouped.groups[3].group, Group::Country(None));
        assert_eq!(grouped.groups[3].group.name(), NO_COUNTRY);
        assert_eq!(
            names(&grouped),
            [
                vec!["Dreiländer loop"],
                vec!["Dreiländer loop"],
                vec!["Dreiländer loop", "Gurten loop"],
                vec!["Old course", "Somewhere"],
            ]
        );
        // In English the order follows the English names.
        let english = CourseQuery {
            locale: "en".to_owned(),
            ..query
        };
        let english = gallery(countries_library(), &english, UnitSystem::Metric);
        let labels: Vec<&str> = english.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["France", "Germany", "Switzerland", ""]);
    }

    #[test]
    fn country_chips_show_the_courses_passing_them() {
        let chosen = |codes: &[&str]| CourseQuery {
            countries: codes.iter().map(|&c| c.to_owned()).collect(),
            ..query_in("de")
        };
        let shown = |codes: &[&str]| {
            names(&gallery(
                countries_library(),
                &chosen(codes),
                UnitSystem::Metric,
            ))
            .concat()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<String>>()
        };

        assert_eq!(shown(&["FR"]), ["Dreiländer loop"]);
        assert_eq!(shown(&["FR", "CH"]), ["Dreiländer loop", "Gurten loop"]);
        assert_eq!(shown(&[NO_COUNTRY]), ["Old course", "Somewhere"]);
        // The library's countries to choose from, by name, then those not known.
        let choices = gallery(countries_library(), &chosen(&[]), UnitSystem::Metric).countries;
        let codes: Vec<&str> = choices.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["DE", "FR", "CH", NO_COUNTRY]);
    }

    #[test]
    fn the_search_finds_countries_by_name() {
        let searching = |search: &str| CourseQuery {
            search: search.to_owned(),
            ..query_in("de")
        };
        let found = |search: &str| {
            names(&gallery(
                countries_library(),
                &searching(search),
                UnitSystem::Metric,
            ))
            .concat()
            .len()
        };

        assert_eq!(found("schweiz"), 2);
        assert_eq!(found("Switzerland"), 2, "in English too");
        assert_eq!(found("frankreich"), 1);
        assert_eq!(found("suisse"), 2, "in its own languages");
    }

    #[test]
    fn every_choice_is_known_by_its_name() {
        for &key in SortKey::ALL {
            assert_eq!(SortKey::from_name(key.name()), Some(key));
        }
        for &key in GroupKey::ALL {
            assert_eq!(GroupKey::from_name(key.name()), Some(key));
        }
        for &terrain in Terrain::ALL {
            assert_eq!(Terrain::from_name(terrain.name()), Some(terrain));
        }
        assert_eq!(CourseKind::from_name("3d"), Some(CourseKind::ThreeD));
        assert_eq!(
            LengthBand::from_name("very_long"),
            Some(LengthBand::VeryLong)
        );
        assert_eq!(SortKey::from_name("colour"), None);
    }
}
