//! Group view dictionaries; membership, readiness and lifecycle remain in the app core.
use godot::prelude::*;
use orqwa_app::{App, group::GroupView};
use std::time::Duration;

pub(super) fn riders(app: &App, view: &GroupView) -> VarArray {
    let mut peers = VarArray::new();
    let state = app.ride_state();
    let local = view.distance.map_or_else(
        || state.as_ref().map_or(0.0, |s| s.distance.0),
        |distance| distance.0,
    );
    let route = state
        .filter(|s| s.position.is_some())
        .and_then(|_| app.route());
    let projection = route.map(orqwa_routes::LocalProjection::for_route);
    for peer in &view.riders {
        let generation = peer.generation.to_string();
        let mut dict = vdict! {
            "id" => i64::from(peer.id), "generation" => generation.as_str(), "name" => peer.name.as_str(),
            "distance_m" => peer.distance.0, "gap_m" => peer.distance.0-local,
            "speed_kmh" => peer.speed.as_kilometers_per_hour(), "power_w" => peer.power.0,
            "cadence" => peer.cadence.0, "speaking" => peer.speaking,
            // Effort drives the avatar's motion only, from the rider-and-bike mass.
            "effort" => peer.power.0 / peer.mass.0.max(1.0),
            // The roster keeps weights within its bounds; never a division by zero.
            "watts_per_kg" => peer.power.0 / peer.weight.0.max(1.0),
            "avatar" => match peer.avatar {
                orqwa_domain::profile::Avatar::Female => "female",
                orqwa_domain::profile::Avatar::Male => "male",
            },
        };
        if let (Some(route), Some(projection)) = (route, &projection) {
            let at = route.position(peer.distance);
            let (x, y) = projection.project(at.lat, at.lon);
            let (rider_x, rider_y) = app.rider_position(peer.distance, true).unwrap_or((x, y));
            dict.extend_dictionary(
                &vdict! {
                    "x" => x, "y" => y, "rider_x" => rider_x, "rider_y" => rider_y,
                    "elevation_m" => at.elevation.0, "heading" => at.heading,
                    "curvature" => at.curvature, "grade" => at.grade.0,
                },
                true,
            );
        }
        peers.push(&dict.to_variant());
    }
    peers
}

pub(super) fn status(app: &App) -> VarDictionary {
    let status = app.group();
    let view = app.group_view();
    let mut roster = VarArray::new();
    let mut devices = VarArray::new();
    for (id, name, input, output) in &status.devices {
        devices.push(
            &vdict! {"id"=>id.as_str(),"name"=>name.as_str(),"input"=>*input,"output"=>*output}
                .to_variant(),
        );
    }
    let local = view.distance.map_or(0.0, |distance| distance.0);
    for (id, rider) in status.roster.iter().enumerate() {
        let Some(rider) = rider else {
            continue;
        };
        let sample = status.states[id].filter(|(_, at)| at.elapsed() < Duration::from_secs(2));
        let peer = view.riders.iter().find(|peer| usize::from(peer.id) == id);
        roster.push(&vdict!{"id"=>i64::try_from(id).unwrap_or(0),"name"=>rider.name.as_str(),"organizer"=>id==usize::from(status.organizer),"ready"=>status.ready[id],"me"=>id==usize::from(status.me),"muted"=>status.muted[id],"present"=>peer.is_some(),"gap_m"=>peer.map_or(0.0,|peer|peer.distance.0-local),"power_w"=>sample.map_or(0.0,|(s,_)|s.power.0),"watts_per_kg"=>sample.map_or(0.0,|(s,_)|s.power.0/rider.weight.0.max(1.0)),
            // How far a rider not yet ready has got preparing the activity (#419).
            "stage"=>match status.preparation[id] {Some((orqwa_app::group::Stage::Downloading,_))=>"downloading",Some((orqwa_app::group::Stage::Preparing,_))=>"preparing",None=>""},
            "percent"=>status.preparation[id].map_or(0,|(_,percent)|i64::from(percent)),
            // Their connection was lost and their seat is held while they reconnect (#434).
            "away"=>status.away[id],
            "fallback_eligible"=>status.fallback_eligible(id),
            "ping_ms"=>status.ping(id).map_or(-1,|rtt|i64::try_from(rtt.as_millis()).unwrap_or(i64::MAX).max(1)),
            // Heard speaking now; this rider too, while their microphone sends speech.
            "speaking"=>status.speaking[id],
            // Standing by to take over if the organizer is lost (#426).
            "fallback"=>status.fallback==u8::try_from(id).ok()}.to_variant());
    }
    let successor_name = status
        .successor
        .and_then(|id| status.roster[usize::from(id)].as_ref())
        .map_or("", |rider| rider.name.as_str());
    let preferred_fallback = status
        .preferred_fallback
        .and_then(|id| status.roster[usize::from(id)].as_ref())
        .map_or("", |rider| rider.name.as_str());
    vdict! {"handover"=>status.handover.key(),"successor_name"=>successor_name,"handover_offer"=>status.handover==orqwa_app::group::Handover::Offered && status.successor==Some(status.me),"can_handover"=>status.can_handover(),"can_cancel_handover"=>status.handover!=orqwa_app::group::Handover::Idle && status.handover!=orqwa_app::group::Handover::Committing && (status.host || status.successor==Some(status.me)),"phase"=>status.phase.key(),"host"=>status.host,"invite"=>status.invite.as_str(),"reply"=>status.reply.as_str(),"reply_s"=>status.reply_until.map_or(0.0,|until|until.saturating_duration_since(std::time::Instant::now()).as_secs_f64()),"reply_expired"=>status.reply_expired,"admitting"=>i64::from(status.admitting),"refusal"=>status.refusal.key(),"refusals"=>i64::from(status.refusals),"reachability"=>status.reachability.map_or("",|reach| reach.key()),"prepared"=>status.prepared,"received"=>i64::try_from(status.received).unwrap_or(0),"total"=>i64::try_from(status.total).unwrap_or(0),"ready"=>status.ready[usize::from(status.me)],"can_start"=>status.can_start(),"workout_shared"=>status.plan.is_some() && matches!(status.phase,orqwa_app::group::Phase::Countdown | orqwa_app::group::Phase::Riding),"workout_paused"=>status.workout_clock.is_some_and(|clock|clock.paused),"local_paused"=>app.is_paused(),"can_control_workout"=>status.can_control_workout(),"workout_complete"=>status.workout_finished(),"late"=>status.late,"can_enter"=>status.can_enter(),"entering"=>status.entering,"name"=>status.selection.as_ref().map_or("",|s|s.name.as_str()),"error"=>status.error.as_str(),"can_retry"=>status.can_retry(),"activity_failed"=>status.activity_failed,"roster"=>&roster,"devices"=>&devices,"input_device"=>status.input_device.as_str(),"output_device"=>status.output_device.as_str(),"voice_volume"=>status.voice_volume,"echo_prevention"=>status.echo_prevention,"output_name"=>status.output_name.as_str(),"ask_listening"=>status.ask_listening,"microphone_test"=>status.microphone_test,"microphone_level"=>status.microphone_level,"speaker_test"=>status.speaker_test,"microphone_voice_detected"=>status.microphone_voice_detected,"microphone_voice_heard"=>status.microphone_voice_heard,"microphone_checked"=>status.microphone_checked,"voice"=>status.voice,"voice_pending"=>status.voice_pending,"microphone"=>status.microphone,"deafened"=>status.deafened,"countdown_s"=>status.start.map_or(0.0,|at|at.saturating_duration_since(std::time::Instant::now()).as_secs_f64()),"drafting"=>status.assistance.drafting,"keep_together"=>status.assistance.keep_together,"workout_id"=>status.workout_id.as_str(),"course_path"=>status.course.as_ref().map_or(String::new(),|p|p.display().to_string()),
    // Asked to stand in as organizer if the organizer is lost; the organizer may pick another.
    "standby_offer"=>status.standby_offer,"can_choose_fallback"=>status.can_choose_fallback(),"preferred_fallback"=>preferred_fallback,"preferred_fallback_accepted"=>status.preferred_fallback_accepted,"fallback_consent"=>status.fallback_consent.map_or(String::new(),|epoch|epoch.to_string()),
    // How long this rider keeps trying to reconnect, 0 while connected (#434).
    "reconnect_s"=>status.reconnecting.map_or(0.0,|until|until.saturating_duration_since(std::time::Instant::now()).as_secs_f64()),
    // Joining again with the same invitation, while a ride may go on alone (#536).
    "can_rejoin"=>app.group_can_rejoin(),"rides_alone"=>app.group_rides_alone(),"waits_for_ride"=>app.group_waits_for_ride(),"invitation"=>app.group_joined_with(),
    // Names the organizer's preview, "" without one (#418); `group_preview_data()` has it.
    "preview"=>status.preview.as_ref().map_or_else(String::new,|shown|shown.id.basename())}
}

/// `shown` as `OrqwaApp.group_preview_data()` describes it (#418).
pub(super) fn preview(app: &App, shown: &orqwa_app::group::Previewed) -> VarDictionary {
    let preview = &shown.preview;
    let course = preview
        .course
        .as_ref()
        .map_or_else(VarDictionary::new, |course| {
            vdict! {
                "name" => course.name.as_str(),
                "length_m" => course.length.0,
                "elevation_gain_m" => course.climbing.0,
                "max_grade" => course.steepest,
                "track" => &super::points(&course.track),
                "profile" => &super::points(&course.profile),
                "picture" => &PackedByteArray::from(course.picture.as_slice()),
            }
        });
    let workout = preview
        .workout
        .as_ref()
        .map_or_else(VarDictionary::new, |plan| {
            vdict! {
                "name" => plan.name.as_str(),
                "description" => plan.description.as_str(),
                "duration_s" => plan.duration().as_secs_f64(),
                "steps" => &super::plan_steps(plan, app.profile().profile.ftp, false),
            }
        });
    vdict! {"course" => &course, "workout" => &workout}
}
