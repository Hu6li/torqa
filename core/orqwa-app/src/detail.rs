//! Map detail (ADR 0033) on the settings' Courses tab: the regions this computer downloads
//! from Geofabrik for the road surfaces the map tiles leave out. Off until turned on; a region
//! is downloaded only when the rider asks for it, and courses prepared afterwards use it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use orqwa_osm::detail::{self, Geofabrik, Offer, Stage};
use orqwa_storage::computer;
use tokio::task::AbortHandle;
use tracing::warn;

use crate::failure::{Failure, Kind};
use crate::{App, AppError, AppEvent, JobResult};

/// What the app knows of map detail.
pub(crate) struct DetailState {
    /// Where Geofabrik lists its regions: its server, except in tests.
    pub(crate) index: String,
    /// The download or preparation under way.
    job: Option<Job>,
    /// The number of the last job started: what a job sends carries its own, so that a job
    /// stopped since is not taken for the one under way.
    serial: u64,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            index: detail::INDEX.to_owned(),
            job: None,
            serial: 0,
        }
    }
}

/// A region being downloaded or prepared, and how to stop it.
struct Job {
    serial: u64,
    region: Option<String>,
    task: AbortHandle,
    stop: Arc<AtomicBool>,
}

/// A region on this computer, as the Courses tab lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRegion {
    /// Geofabrik's name for it, to update or delete it by.
    pub id: String,
    /// Its name to show.
    pub name: String,
    /// When its data was current (Unix seconds), once ready, if the download says.
    pub data_time: Option<i64>,
    /// The bytes it takes: its download and its detail.
    pub size: u64,
    /// Whether courses prepared now use it.
    pub ready: bool,
    /// Whether it is being downloaded or prepared.
    pub busy: bool,
}

/// A region Geofabrik offers, to choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionOffer {
    /// Geofabrik's name for it, to download it by.
    pub id: String,
    /// Its name to show.
    pub name: String,
    /// The region it is a part of; `None` for a continent, offered only by its parts.
    pub parent: Option<String>,
}

/// What a map detail job is doing, for its progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailStep {
    /// Downloading the region (bytes).
    Downloading,
    /// Reading its download, twice over (bytes of both readings).
    Reading,
    /// Writing its detail (cells).
    Writing,
}

/// Background work on map detail, done.
pub(crate) enum Done {
    Offers(Result<Vec<Offer>, Failure>),
    Size(String, Option<u64>),
    Progress(String, DetailStep, u64, u64),
    /// A region is ready, or was prepared.
    Ready,
    UpToDate(String),
    Failed(Failure),
    /// Stopped as the rider asked.
    Stopped,
}

impl App {
    fn osm_dir(&self) -> PathBuf {
        self.cache_dir.join("osm")
    }

    /// Whether this computer downloads regions' map detail for its courses (ADR 0033); no
    /// until turned on.
    #[must_use]
    pub fn map_detail(&self) -> bool {
        computer::map_detail(&self.computer)
    }

    /// Turns downloading regions' map detail on or off for this computer. On, downloads
    /// already in the cache (put there by hand or cut short in their preparation) are
    /// prepared; off, every region downloaded is deleted.
    ///
    /// # Errors
    /// [`AppError::Reported`] if it cannot be saved, or a region cannot be deleted.
    pub fn set_map_detail(&mut self, on: bool) -> Result<(), AppError> {
        computer::set_map_detail(&self.computer, on)
            .map_err(|e| AppError::Reported(Failure::new(Kind::SaveSetting, e)))?;
        if on {
            self.prepare_downloaded_regions();
            return Ok(());
        }
        self.stop_region_download();
        for region in detail::kept(&self.osm_dir()) {
            self.delete_region(&region.id)?;
        }
        Ok(())
    }

    /// The regions on this computer, by id; none while it downloads none, as turning that off
    /// deletes them.
    #[must_use]
    pub fn map_detail_regions(&self) -> Vec<DetailRegion> {
        if !self.map_detail() {
            return Vec::new();
        }
        let osm = self.osm_dir();
        let offers = detail::cached_offers(&osm);
        let busy = self.detail.job.as_ref().and_then(|job| job.region.clone());
        detail::kept(&osm)
            .into_iter()
            .map(|kept| DetailRegion {
                name: kept
                    .ready
                    .as_ref()
                    .map_or_else(|| name_of(&kept.id, &offers), |region| region.name.clone()),
                data_time: kept.ready.as_ref().and_then(|region| region.data_time),
                size: kept.size,
                ready: kept.ready.is_some(),
                busy: busy
                    .as_deref()
                    .is_some_and(|id| file_id(id) == file_id(&kept.id)),
                id: kept.id,
            })
            .collect()
    }

    /// How many bytes the regions on this computer take.
    #[must_use]
    pub fn map_detail_size(&self) -> u64 {
        self.map_detail_regions().iter().map(|r| r.size).sum()
    }

    /// Whether a region is being downloaded or prepared.
    #[must_use]
    pub fn map_detail_busy(&self) -> bool {
        self.detail.job.is_some()
    }

    /// Asks Geofabrik for the regions it offers; reports [`AppEvent::RegionOffers`], or
    /// [`AppEvent::MapDetailFailed`].
    pub fn load_region_offers(&mut self) {
        let geofabrik = Geofabrik::at(self.detail.index.clone());
        let osm = self.osm_dir();
        let tx = self.jobs_tx.clone();
        self.runtime.spawn(async move {
            let offers = geofabrik
                .offers(&osm)
                .await
                .map_err(|e| Failure::new(Kind::LoadRegions, e));
            let _ = tx.send(JobResult::Detail(0, Done::Offers(offers)));
        });
    }

    /// Asks Geofabrik how large region `id`'s download is; reports [`AppEvent::RegionSize`].
    pub fn load_region_size(&mut self, id: &str) {
        let Some(offer) = self.offer(id) else {
            return;
        };
        let geofabrik = Geofabrik::at(self.detail.index.clone());
        let tx = self.jobs_tx.clone();
        self.runtime.spawn(async move {
            let size = geofabrik
                .latest(&offer)
                .await
                .ok()
                .map(|latest| latest.size);
            let _ = tx.send(JobResult::Detail(0, Done::Size(offer.id, size)));
        });
    }

    /// Downloads region `id` and prepares its detail in the background, or tells it up to
    /// date if its latest download is ready already; whether it starts. Only while map detail
    /// is on, one region at a time. Reports [`AppEvent::MapDetailProgress`], then
    /// [`AppEvent::MapDetailChanged`], [`AppEvent::MapDetailUpToDate`] or
    /// [`AppEvent::MapDetailFailed`].
    pub fn download_region(&mut self, id: &str) -> bool {
        if self.detail.job.is_some() || !self.map_detail() {
            return false;
        }
        let Some(offer) = self.offer(id) else {
            return false;
        };
        let geofabrik = Geofabrik::at(self.detail.index.clone());
        let osm = self.osm_dir();
        let sink = self.detail_sink();
        let serial = sink.serial;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let region = offer.id.clone();
        let task = self.runtime.spawn(async move {
            let done = match fetch(&geofabrik, &osm, &offer, &sink, stopping).await {
                Ok(done) => done,
                Err(failure) => Done::Failed(failure),
            };
            sink.send(done);
        });
        self.detail.job = Some(Job {
            serial,
            region: Some(region),
            task: task.abort_handle(),
            stop,
        });
        true
    }

    /// Prepares, in the background, the regions downloaded but not ready: a download put in
    /// the cache by hand, or one whose preparation was stopped. Only while map detail is on
    /// and nothing else is under way.
    pub fn prepare_downloaded_regions(&mut self) {
        if self.detail.job.is_some() || !self.map_detail() {
            return;
        }
        let osm = self.osm_dir();
        let offers = detail::cached_offers(&osm);
        let waiting: Vec<(String, String)> = detail::kept(&osm)
            .into_iter()
            .filter(|kept| kept.downloaded && kept.ready.is_none())
            .map(|kept| (name_of(&kept.id, &offers), kept.id))
            .collect();
        if waiting.is_empty() {
            return;
        }
        let sink = self.detail_sink();
        let serial = sink.serial;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let task = self.runtime.spawn(async move {
            for (name, id) in waiting {
                let source = detail::download_source(&osm, &id);
                if let Err(done) = prepare(&osm, (&id, &name, source), &sink, &stopping).await {
                    sink.send(done);
                    return;
                }
            }
            sink.send(Done::Ready);
        });
        self.detail.job = Some(Job {
            serial,
            region: None,
            task: task.abort_handle(),
            stop,
        });
    }

    /// Stops the download or preparation under way. What was downloaded stays, to go on
    /// with; a region ready before stays ready.
    pub fn stop_region_download(&mut self) {
        if let Some(job) = self.detail.job.take() {
            job.stop.store(true, Ordering::Relaxed);
            job.task.abort();
        }
    }

    /// Deletes region `id`: its download and its detail. Courses prepared with it carry what
    /// they used; one already gone is no error.
    ///
    /// # Errors
    /// [`AppError::Reported`] if it cannot be deleted.
    pub fn delete_region(&mut self, id: &str) -> Result<(), AppError> {
        let busy = self
            .detail
            .job
            .as_ref()
            .and_then(|job| job.region.as_deref());
        if busy.is_some_and(|busy| file_id(busy) == file_id(id)) {
            self.stop_region_download();
        }
        detail::remove(&self.osm_dir(), id)
            .map_err(|e| AppError::Reported(Failure::new(Kind::DeleteFile, e).arg(id.to_owned())))
    }

    /// Where the next job sends what it did.
    fn detail_sink(&mut self) -> Sink {
        self.detail.serial += 1;
        Sink {
            tx: self.jobs_tx.clone(),
            serial: self.detail.serial,
        }
    }

    pub(crate) fn detail_done(&mut self, serial: u64, done: Done) -> Option<AppEvent> {
        let current = self.detail.job.as_ref().map(|job| job.serial);
        if serial != 0 && current != Some(serial) {
            // From a job stopped since: the one under way, if any, tells its own.
            return None;
        }
        match done {
            Done::Offers(Ok(offers)) => Some(AppEvent::RegionOffers(
                offers
                    .into_iter()
                    .map(|offer| RegionOffer {
                        id: offer.id,
                        name: offer.name,
                        parent: offer.parent,
                    })
                    .collect(),
            )),
            Done::Offers(Err(failure)) => Some(AppEvent::MapDetailFailed(failure)),
            Done::Size(id, size) => Some(AppEvent::RegionSize { id, size }),
            Done::Progress(id, step, done, total) => Some(AppEvent::MapDetailProgress {
                id,
                step,
                done,
                total,
            }),
            // Stopped as asked, the regions are as they were or more ready.
            Done::Ready | Done::Stopped => {
                self.detail.job = None;
                Some(AppEvent::MapDetailChanged)
            }
            Done::UpToDate(name) => {
                self.detail.job = None;
                Some(AppEvent::MapDetailUpToDate(name))
            }
            Done::Failed(failure) => {
                self.detail.job = None;
                warn!(?failure, "map detail failed");
                Some(AppEvent::MapDetailFailed(failure))
            }
        }
    }

    /// The region `id` Geofabrik offers, as last listed.
    fn offer(&self, id: &str) -> Option<Offer> {
        detail::cached_offers(&self.osm_dir())
            .into_iter()
            .find(|offer| offer.id == id || file_id(&offer.id) == file_id(id))
    }
}

/// Downloads `offer` unless its latest download is there already, and prepares its detail.
async fn fetch(
    geofabrik: &Geofabrik,
    osm: &std::path::Path,
    offer: &Offer,
    sink: &Sink,
    stop: Arc<AtomicBool>,
) -> Result<Done, Failure> {
    let failed = |e| Failure::new(Kind::DownloadRegion, e).arg(offer.name.clone());
    let latest = geofabrik.latest(offer).await.map_err(failed)?;
    let ready = detail::installed(osm)
        .into_iter()
        .find(|region| file_id(&region.id) == file_id(&offer.id));
    if ready.is_some_and(|region| region.source.as_deref() == Some(latest.name.as_str())) {
        return Ok(Done::UpToDate(offer.name.clone()));
    }
    if detail::download_source(osm, &offer.id).as_deref() != Some(latest.name.as_str()) {
        let mut reporter = Percent::new(sink, &offer.id, DetailStep::Downloading);
        geofabrik
            .download(osm, &offer.id, &latest, &mut |done, total| {
                reporter.report(done, total);
            })
            .await
            .map_err(failed)?;
    }
    let source = Some(latest.name);
    match prepare(osm, (&offer.id, &offer.name, source), sink, &stop).await {
        Ok(()) => Ok(Done::Ready),
        Err(done) => Ok(done),
    }
}

/// Makes the detail of region `id` from its download, reporting progress.
async fn prepare(
    osm: &std::path::Path,
    (id, name, source): (&str, &str, Option<String>),
    sink: &Sink,
    stop: &Arc<AtomicBool>,
) -> Result<(), Done> {
    let (osm, id_owned, name_owned) = (osm.to_owned(), id.to_owned(), name.to_owned());
    let (sink, stop) = (sink.clone(), Arc::clone(stop));
    let result = tokio::task::spawn_blocking(move || {
        let mut reading = Percent::new(&sink, &id_owned, DetailStep::Reading);
        let mut writing = Percent::new(&sink, &id_owned, DetailStep::Writing);
        let mut progress = |stage, done, total| match stage {
            Stage::Ways => reading.report(done, 2 * total),
            Stage::Nodes => reading.report(total + done, 2 * total),
            Stage::Cells => writing.report(done, total),
        };
        detail::install(
            &osm,
            (&id_owned, &name_owned, source.as_deref()),
            &mut progress,
            &stop,
        )
    })
    .await;
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(orqwa_osm::OsmError::Stopped)) => Err(Done::Stopped),
        Ok(Err(error)) => Err(Done::Failed(
            Failure::new(Kind::PrepareRegion, error).arg(name.to_owned()),
        )),
        Err(error) => Err(Done::Failed(
            Failure::new(Kind::PrepareRegion, error).arg(name.to_owned()),
        )),
    }
}

/// Where a job sends what it did, under its number.
#[derive(Clone)]
struct Sink {
    tx: std::sync::mpsc::Sender<JobResult>,
    serial: u64,
}

impl Sink {
    fn send(&self, done: Done) {
        let _ = self.tx.send(JobResult::Detail(self.serial, done));
    }
}

/// Sends a job's progress on at most once per percent.
struct Percent<'a> {
    sink: &'a Sink,
    id: &'a str,
    step: DetailStep,
    last: Option<u64>,
}

impl<'a> Percent<'a> {
    fn new(sink: &'a Sink, id: &'a str, step: DetailStep) -> Self {
        Self {
            sink,
            id,
            step,
            last: None,
        }
    }

    fn report(&mut self, done: u64, total: u64) {
        let percent = (done * 100).checked_div(total).unwrap_or(100);
        if self.last == Some(percent) {
            return;
        }
        self.last = Some(percent);
        let progress = Done::Progress(self.id.to_owned(), self.step, done, total);
        self.sink.send(progress);
    }
}

/// A region's id as its files are named: `us/california` is `us-california`.
fn file_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// A region's name: as Geofabrik last listed it, or made from its id.
fn name_of(id: &str, offers: &[Offer]) -> String {
    if let Some(offer) = offers.iter().find(|o| file_id(&o.id) == file_id(id)) {
        return offer.name.clone();
    }
    id.rsplit('/')
        .next()
        .unwrap_or(id)
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::tests::temp_dir;

    const BERN: &[u8] = include_bytes!("../../orqwa-osm/tests/data/bern.osm.pbf");

    /// A stand-in for Geofabrik offering Bern, its latest download `bern-261024.osm.pbf`.
    fn geofabrik() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let base = site.clone();
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let mut socket = socket.unwrap();
                let mut request = [0; 4096];
                let read = socket.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..read]).to_lowercase();
                let line = request.lines().next().unwrap_or_default().to_owned();
                let (status, extra, body): (&str, String, Vec<u8>) = if line.contains("index") {
                    let index = format!(
                        r#"{{"features":[{{"properties":{{"id":"europe","name":"Europe","urls":{{"pbf":"{base}/europe-latest.osm.pbf"}}}}}},{{"properties":{{"id":"bern","parent":"europe","name":"Bern","urls":{{"pbf":"{base}/europe/bern-latest.osm.pbf"}}}}}}]}}"#
                    );
                    ("200 OK", String::new(), index.into_bytes())
                } else if line.contains("latest") {
                    let to = format!("Location: {base}/europe/bern-261024.osm.pbf\r\n");
                    ("307 Temporary Redirect", to, Vec::new())
                } else {
                    ("200 OK", String::new(), BERN.to_vec())
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes());
                if !line.starts_with("head") {
                    let _ = socket.write_all(&body);
                }
            }
        });
        format!("{site}/index-v1-nogeom.json")
    }

    /// The events until `done` says so.
    fn until(app: &mut App, mut done: impl FnMut(&AppEvent) -> bool) -> Vec<AppEvent> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut events = Vec::new();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            for event in app.update(Duration::from_millis(10)) {
                let stop = done(&event);
                events.push(event);
                if stop {
                    return events;
                }
            }
        }
        panic!("no end: {events:?}");
    }

    fn put(cache: &Path, id: &str) {
        let download = detail::download_path(&cache.join("osm"), id);
        std::fs::create_dir_all(download.parent().unwrap()).unwrap();
        std::fs::write(download, BERN).unwrap();
    }

    #[test]
    fn turned_on_downloads_put_in_the_cache_by_hand_are_prepared_and_listed() {
        let dir = temp_dir("detail-adopt");
        let cache = dir.join("cache");
        put(&cache, "switzerland");
        let mut app = App::new(dir.join("data"), cache.clone()).unwrap();
        assert!(!app.map_detail(), "off for a new computer");
        assert_eq!(app.map_detail_regions(), [], "listed only once on");

        app.set_map_detail(true).unwrap();
        until(&mut app, |e| matches!(e, AppEvent::MapDetailChanged));

        let regions = app.map_detail_regions();
        assert_eq!(regions.len(), 1);
        let switzerland = &regions[0];
        assert_eq!(switzerland.name, "Switzerland", "named from its file");
        assert!(switzerland.ready && !switzerland.busy);
        assert_eq!(switzerland.data_time, Some(1_792_875_600));
        assert!(
            switzerland.size > BERN.len() as u64,
            "the download and its detail"
        );
        assert_eq!(app.map_detail_size(), switzerland.size);
        let ready = detail::installed(&cache.join("osm"));
        assert_eq!(
            ready[0].extract,
            format!("switzerland-2026-10-24-c{}", orqwa_osm::detail::CONTENT)
        );
        app.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_region_is_downloaded_prepared_then_up_to_date_and_turned_off_deleted() {
        let dir = temp_dir("detail-download");
        let cache = dir.join("cache");
        let mut app = App::new(dir.join("data"), cache.clone()).unwrap();
        app.detail.index = geofabrik();
        assert!(!app.download_region("bern"), "not while off");
        app.set_map_detail(true).unwrap();

        app.load_region_offers();
        let events = until(&mut app, |e| matches!(e, AppEvent::RegionOffers(_)));
        let Some(AppEvent::RegionOffers(offers)) = events.last() else {
            unreachable!()
        };
        let bern = RegionOffer {
            id: "bern".to_owned(),
            name: "Bern".to_owned(),
            parent: Some("europe".to_owned()),
        };
        assert!(offers.contains(&bern));
        app.load_region_size("bern");
        let events = until(&mut app, |e| matches!(e, AppEvent::RegionSize { .. }));
        let size = BERN.len() as u64;
        assert_eq!(
            events.last(),
            Some(&AppEvent::RegionSize {
                id: "bern".to_owned(),
                size: Some(size)
            })
        );

        assert!(app.download_region("bern"));
        assert!(!app.download_region("bern"), "one at a time");
        assert!(app.map_detail_busy());
        let events = until(&mut app, |e| matches!(e, AppEvent::MapDetailChanged));
        let steps: Vec<DetailStep> = events
            .iter()
            .filter_map(|e| match e {
                AppEvent::MapDetailProgress { step, .. } => Some(*step),
                _ => None,
            })
            .collect();
        for step in [
            DetailStep::Downloading,
            DetailStep::Reading,
            DetailStep::Writing,
        ] {
            assert!(steps.contains(&step), "{step:?} in {steps:?}");
        }
        let regions = app.map_detail_regions();
        assert_eq!(
            regions
                .iter()
                .map(|r| (r.name.as_str(), r.ready))
                .collect::<Vec<_>>(),
            [("Bern", true)]
        );
        assert!(!app.map_detail_busy());

        assert!(app.download_region("bern"));
        let events = until(&mut app, |e| matches!(e, AppEvent::MapDetailUpToDate(_)));
        assert_eq!(
            events.last(),
            Some(&AppEvent::MapDetailUpToDate("Bern".to_owned()))
        );

        app.set_map_detail(false).unwrap();
        assert_eq!(detail::kept(&cache.join("osm")), [], "all deleted");
        app.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_a_job_stopped_since_sends_is_not_taken_for_the_next() {
        let dir = temp_dir("detail-serial");
        let cache = dir.join("cache");
        put(&cache, "bern");
        let mut app = App::new(dir.join("data"), cache).unwrap();
        app.set_map_detail(true).unwrap();
        let first = app.detail.serial;
        app.stop_region_download();
        put_job(&mut app);

        let late = app.detail_done(first, Done::Ready);

        assert_eq!(late, None);
        assert!(app.map_detail_busy(), "the job under way goes on");
        let current = app.detail.serial;
        assert_eq!(
            app.detail_done(current, Done::Ready),
            Some(AppEvent::MapDetailChanged)
        );
        assert!(!app.map_detail_busy());
        app.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A job under way that does nothing, standing for a download.
    fn put_job(app: &mut App) {
        let sink = app.detail_sink();
        let task = app.runtime.spawn(std::future::pending::<()>());
        app.detail.job = Some(Job {
            serial: sink.serial,
            region: Some("zurich".to_owned()),
            task: task.abort_handle(),
            stop: Arc::new(AtomicBool::new(false)),
        });
    }

    #[test]
    fn a_region_deleted_leaves_the_others() {
        let dir = temp_dir("detail-delete");
        let cache = dir.join("cache");
        put(&cache, "bern");
        put(&cache, "us/california");
        let mut app = App::new(dir.join("data"), cache).unwrap();
        app.set_map_detail(true).unwrap();
        until(&mut app, |e| matches!(e, AppEvent::MapDetailChanged));
        let names: Vec<_> = app
            .map_detail_regions()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["Bern", "Us California"]);

        app.delete_region("bern").unwrap();
        app.delete_region("bern")
            .expect("a list shown before it went may delete it again");

        let names: Vec<_> = app
            .map_detail_regions()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["Us California"]);
        app.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
