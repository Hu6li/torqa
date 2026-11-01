//! Locale-independent failures carried to presentation (#261).
use std::fmt;

/// Stable classification of a failure, independent of its technical cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Could not complete the group ride action.
    GroupRide,
    /// Could not prepare the shared activity.
    GroupActivity,
    /// Voice is unavailable. Check your audio devices and microphone permission.
    GroupVoice,
    /// Could not save the rider profile.
    SaveProfile,
    /// Could not delete the rider profile.
    DeleteProfile,
    /// Could not open %s.
    OpenFile,
    /// Could not read %s.
    ReadFile,
    /// Could not import %s.
    ImportFile,
    /// Could not save the course.
    SaveCourse,
    /// Could not save %s.
    SaveFile,
    /// Could not save the HUD layout.
    SaveHud,
    /// Could not rename the activity.
    RenameRide,
    /// Could not delete %s.
    DeleteFile,
    /// Could not export to %s.
    ExportFile,
    /// Video %s was not found. Put it next to the course file.
    MissingVideo,
    /// %s was not found. It holds the course's slopes: put it next to the RLV file.
    MissingSlopes,
    /// Could not save the video association.
    SaveVideo,
    /// Could not play the video.
    PlayVideo,
    /// Course preparation stopped unexpectedly.
    PreparationStopped,
    /// No route is loaded.
    NoRoute,
    /// No trainer is connected.
    NoTrainer,
    /// The selected device is not available.
    UnknownDevice,
    /// The course is not ready yet.
    CourseNotReady,
    /// The rider profile was not found.
    UnknownProfile,
    /// ORQWA could not start.
    Runtime,
    /// The selected platform is not supported.
    UnknownPlatform,
    /// Connect %s first.
    ConnectFirst,
    /// %s does not follow this route.
    GhostRoute,
    /// %s has no timestamps for a ghost rider.
    GhostTimes,
    /// %s has no GPS track. Import its GPX route, then add the video to the course.
    VideoGps,
    /// Set a start and an end point for the video on the route.
    VideoEndpoints,
    /// The route must end within the video.
    VideoEnd,
    /// Could not control the music player.
    MusicControl,
    /// No music player responded.
    MusicResponse,
    /// An error occurred with the trainer.
    Trainer,
    /// Could not read the connected accounts.
    ReadAccounts,
    /// Communication with the heart-rate sensor failed.
    HeartRate,
    /// Communication with the power meter failed.
    PowerMeter,
    /// Communication with the cadence sensor failed.
    Cadence,
    /// Open a course first.
    OpenCourse,
    /// This course already has a video.
    AlreadyVideo,
    /// This course has no video.
    NoVideo,
    /// This course needs its video because it has no geographic route.
    VideoRequired,
    /// No video course is loaded.
    NoVideoCourse,
    /// This video follows its GPS data and does not need manual synchronization.
    VideoAuto,
    /// Enter a name for the course.
    CourseName,
    /// Could not search for devices.
    Scan,
    /// Could not reach %s. Check your internet connection.
    Unreachable,
    /// Could not upload the activity to %s: %s
    Upload,
    /// Could not connect the account with %s: %s
    ConnectAccount,
    /// Could not save the account connection.
    SaveAccount,
    /// Could not load the activity.
    LoadRide,
    /// Could not import the workout.
    ImportWorkout,
    /// Could not save the workout.
    SaveWorkout,
    /// Could not delete the workout.
    DeleteWorkout,
    /// Could not update the course.
    UpdateCourse,
    /// Could not load the ghost rider.
    LoadGhost,
    /// Could not prepare the course.
    PrepareCourse,
    /// Could not read the video.
    ReadVideo,
    /// The video and the route do not match.
    VideoMismatch,
    /// Could not save the setting.
    SaveSetting,
    /// Could not load the workout.
    LoadWorkout,
    /// An unexpected error occurred.
    Unexpected,
    /// The only rider cannot be deleted (shared with the delete button's tooltip).
    LastProfile,
    /// Points must follow each other on the route and in the video.
    VideoOrder,
    /// %s no longer accepts the account: connect it again.
    Unauthorized,
    /// The service requires the rider to reactivate their API application.
    InactiveApplication,
    /// Could not read the workouts planned on %s: %s
    PlannedWorkouts,
    /// Could not start the remote on port %s.
    StartRemote,
    /// Could not look for a newer version of ORQWA.
    CheckUpdate,
    /// Could not download the update.
    DownloadUpdate,
    /// Could not start the update's installer.
    InstallUpdate,
    /// Could not load the regions to download.
    LoadRegions,
    /// Could not download the map detail of %s.
    DownloadRegion,
    /// Could not prepare the map detail of %s.
    PrepareRegion,
}
impl Kind {
    /// Unformatted English gettext key; translate before substituting arguments.
    #[must_use]
    pub fn message(self) -> &'static str {
        // i18n-begin
        match self {
            Self::GroupRide => "Could not complete the group ride action.",
            Self::GroupActivity => "Could not prepare the shared activity.",
            Self::GroupVoice => {
                "Voice is unavailable. Check your audio devices and microphone permission."
            }
            Self::SaveProfile => "Could not save the rider profile.",
            Self::DeleteProfile => "Could not delete the rider profile.",
            Self::OpenFile => "Could not open %s.",
            Self::ReadFile => "Could not read %s.",
            Self::ImportFile => "Could not import %s.",
            Self::SaveCourse => "Could not save the course.",
            Self::SaveFile => "Could not save %s.",
            Self::SaveHud => "Could not save the HUD layout.",
            Self::RenameRide => "Could not rename the activity.",
            Self::DeleteFile => "Could not delete %s.",
            Self::ExportFile => "Could not export to %s.",
            Self::MissingVideo => "Video %s was not found. Put it next to the course file.",
            Self::MissingSlopes => {
                "%s was not found. It holds the course's slopes: put it next to the RLV file."
            }
            Self::SaveVideo => "Could not save the video association.",
            Self::PlayVideo => "Could not play the video.",
            Self::PreparationStopped => "Course preparation stopped unexpectedly.",
            Self::NoRoute => "No route is loaded.",
            Self::NoTrainer => "No trainer is connected.",
            Self::UnknownDevice => "The selected device is not available.",
            Self::CourseNotReady => "The course is not ready yet.",
            Self::UnknownProfile => "The rider profile was not found.",
            Self::Runtime => "ORQWA could not start.",
            Self::UnknownPlatform => "The selected platform is not supported.",
            Self::ConnectFirst => "Connect %s first.",
            Self::GhostRoute => "%s does not follow this route.",
            Self::GhostTimes => "%s has no timestamps for a ghost rider.",
            Self::VideoGps => {
                "%s has no GPS track. Import its GPX route, then add the video to the course."
            }
            Self::VideoEndpoints => "Set a start and an end point for the video on the route.",
            Self::VideoEnd => "The route must end within the video.",
            Self::MusicControl => "Could not control the music player.",
            Self::MusicResponse => "No music player responded.",
            Self::Trainer => "An error occurred with the trainer.",
            Self::ReadAccounts => "Could not read the connected accounts.",
            Self::HeartRate => "Communication with the heart-rate sensor failed.",
            Self::PowerMeter => "Communication with the power meter failed.",
            Self::Cadence => "Communication with the cadence sensor failed.",
            Self::OpenCourse => "Open a course first.",
            Self::AlreadyVideo => "This course already has a video.",
            Self::NoVideo => "This course has no video.",
            Self::VideoRequired => {
                "This course needs its video because it has no geographic route."
            }
            Self::NoVideoCourse => "No video course is loaded.",
            Self::VideoAuto => {
                "This video follows its GPS data and does not need manual synchronization."
            }
            Self::CourseName => "Enter a name for the course.",
            Self::Scan => "Could not search for devices.",
            Self::Unreachable => "Could not reach %s. Check your internet connection.",
            Self::Upload => "Could not upload the activity to %s: %s",
            Self::ConnectAccount => "Could not connect the account with %s: %s",
            Self::SaveAccount => "Could not save the account connection.",
            Self::LoadRide => "Could not load the activity.",
            Self::ImportWorkout => "Could not import the workout.",
            Self::SaveWorkout => "Could not save the workout.",
            Self::DeleteWorkout => "Could not delete the workout.",
            Self::UpdateCourse => "Could not update the course.",
            Self::LoadGhost => "Could not load the ghost rider.",
            Self::PrepareCourse => "Could not prepare the course.",
            Self::ReadVideo => "Could not read the video.",
            Self::VideoMismatch => "The video and the route do not match.",
            Self::SaveSetting => "Could not save the setting.",
            Self::LoadWorkout => "Could not load the workout.",
            Self::Unexpected => "An unexpected error occurred.",
            Self::LastProfile => "The only rider cannot be deleted",
            Self::VideoOrder => "Points must follow each other on the route and in the video.",
            Self::Unauthorized => "%s no longer accepts the account: connect it again.",
            Self::InactiveApplication => {
                "Strava has switched off your API application: the account that owns it needs a Strava subscription. Subscribe, reactivate the application in Strava's API settings, then upload again."
            }
            Self::PlannedWorkouts => "Could not read the workouts planned on %s: %s",
            Self::StartRemote => {
                "Could not start the remote on port %s. Another program may use it: choose another port."
            }
            Self::CheckUpdate => "Could not look for a newer version of ORQWA.",
            Self::DownloadUpdate => "Could not download the update.",
            Self::InstallUpdate => "Could not start the update's installer.",
            Self::LoadRegions => "Could not load the regions to download.",
            Self::DownloadRegion => "Could not download the map detail of %s.",
            Self::PrepareRegion => "Could not prepare the map detail of %s.",
        }
        // i18n-end
    }
}

/// A message key and untouched values, with diagnostics kept separate from rider text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The operation or reason to explain to the rider.
    pub kind: Kind,
    /// Values such as filenames and device names; never translate them.
    pub args: Vec<String>,
    /// Original diagnostic, suitable for troubleshooting and logs.
    pub details: String,
}
impl Failure {
    /// Attaches an operation to its original cause without interpreting the cause's words.
    pub fn new(kind: Kind, cause: impl fmt::Display) -> Self {
        Self {
            kind,
            args: Vec::new(),
            details: cause.to_string(),
        }
    }
    /// Supplies a value for the next template placeholder.
    #[must_use]
    pub fn arg(mut self, value: impl fmt::Display) -> Self {
        self.args.push(value.to_string());
        self
    }
    /// Original diagnostic substring, for callers inspecting a failed recording.
    #[must_use]
    pub fn contains(&self, value: &str) -> bool {
        self.details.contains(value)
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.details)
    }
}
impl std::error::Error for Failure {}
impl From<crate::AppError> for Failure {
    fn from(error: crate::AppError) -> Self {
        use crate::AppError;
        let kind = match &error {
            AppError::Reported(failure) => return failure.clone(),
            AppError::Runtime(_) => Kind::Runtime,
            AppError::NoRoute => Kind::NoRoute,
            AppError::NoTrainer => Kind::NoTrainer,
            AppError::UnknownDevice => Kind::UnknownDevice,
            AppError::CourseNotReady => Kind::CourseNotReady,
            AppError::UnknownProfile => Kind::UnknownProfile,
            AppError::LastProfile => Kind::LastProfile,
            AppError::GhostUnavailable(_) => Kind::LoadGhost,
            AppError::UnknownPlatform => Kind::UnknownPlatform,
            AppError::NotConnected(platform) => {
                return Self::new(Kind::ConnectFirst, &error).arg(platform);
            }
        };
        Self::new(kind, error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_does_not_depend_on_the_diagnostics_language() {
        let native = "Zugriff verweigert: /Räder/100%/%s.fit";
        let failure = Failure::new(Kind::ReadFile, native).arg("/Räder/100%/%s.fit");
        assert_eq!(failure.kind.message(), "Could not read %s.");
        assert_eq!(failure.args, ["/Räder/100%/%s.fit"]);
        assert_eq!(failure.details, native);
        assert_eq!(
            Failure::from(crate::AppError::Reported(failure.clone())),
            failure
        );
    }

    #[test]
    fn unreported_errors_keep_their_cause_as_details() {
        let failure = Failure::from(crate::AppError::GhostUnavailable("native cause".to_owned()));
        assert_eq!(failure.kind, Kind::LoadGhost);
        assert_eq!(failure.details, "native cause");
    }
}
