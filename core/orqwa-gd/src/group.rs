//! Group view dictionaries; membership, readiness and lifecycle remain in the app core.
use godot::prelude::*;
use orqwa_app::App;
use std::time::Duration;

pub(super) fn riders(app: &App) -> VarArray {
    let mut peers = VarArray::new();
    let state = app.ride_state();
    let local = state.as_ref().map_or(0.0, |s| s.distance.0);
    let route = state
        .filter(|s| s.position.is_some())
        .and_then(|_| app.route());
    let projection = route.map(orqwa_routes::LocalProjection::for_route);
    for peer in app.group_riders() {
        let generation = peer.generation.to_string();
        let mut dict = vdict! {
            "id" => i64::from(peer.id), "generation" => generation.as_str(), "name" => peer.name.as_str(),
            "distance_m" => peer.distance.0, "gap_m" => peer.distance.0-local,
            "speed_kmh" => peer.speed.as_kilometers_per_hour(), "power_w" => peer.power.0,
            "cadence" => peer.cadence.0,
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
    let mut roster = VarArray::new();
    let mut devices = VarArray::new();
    for (id, name, input, output) in &status.devices {
        devices.push(
            &vdict! {"id"=>id.as_str(),"name"=>name.as_str(),"input"=>*input,"output"=>*output}
                .to_variant(),
        );
    }
    let local = app.ride_state().map_or(0.0, |s| s.distance.0);
    for (id, rider) in status.roster.iter().enumerate() {
        let Some(rider) = rider else {
            continue;
        };
        let sample = status.states[id].filter(|(_, at)| at.elapsed() < Duration::from_secs(2));
        roster.push(&vdict!{"id"=>i64::try_from(id).unwrap_or(0),"name"=>rider.name.as_str(),"organizer"=>id==usize::from(status.organizer),"ready"=>status.ready[id],"me"=>id==usize::from(status.me),"muted"=>status.muted[id],"present"=>sample.is_some(),"gap_m"=>sample.map_or(0.0,|(s,_)|s.distance.0-local),"power_w"=>sample.map_or(0.0,|(s,_)|s.power.0),"watts_per_kg"=>sample.map_or(0.0,|(s,_)|s.power.0/rider.weight.0.max(1.0)),
            // How far a rider not yet ready has got preparing the activity (#419).
            "stage"=>match status.preparation[id] {Some((orqwa_app::group::Stage::Downloading,_))=>"downloading",Some((orqwa_app::group::Stage::Preparing,_))=>"preparing",None=>""},
            "percent"=>status.preparation[id].map_or(0,|(_,percent)|i64::from(percent)),
            // Their connection was lost and their seat is held while they reconnect (#434).
            "away"=>status.away[id],
            // Standing by to take over if the organizer is lost (#426).
            "fallback"=>status.fallback==u8::try_from(id).ok()}.to_variant());
    }
    let successor_name = status
        .successor
        .and_then(|id| status.roster[usize::from(id)].as_ref())
        .map_or("", |rider| rider.name.as_str());
    vdict! {"handover"=>status.handover.key(),"successor_name"=>successor_name,"handover_offer"=>status.handover==orqwa_app::group::Handover::Offered && status.successor==Some(status.me),"can_handover"=>status.can_handover(),"can_cancel_handover"=>status.handover!=orqwa_app::group::Handover::Idle && status.handover!=orqwa_app::group::Handover::Committing && (status.host || status.successor==Some(status.me)),"phase"=>status.phase.key(),"host"=>status.host,"invite"=>status.invite.as_str(),"reply"=>status.reply.as_str(),"reachability"=>status.reachability.map_or("",|reach| reach.key()),"prepared"=>status.prepared,"received"=>i64::try_from(status.received).unwrap_or(0),"total"=>i64::try_from(status.total).unwrap_or(0),"ready"=>status.ready[usize::from(status.me)],"can_start"=>status.can_start(),"workout_shared"=>status.plan.is_some() && matches!(status.phase,orqwa_app::group::Phase::Countdown | orqwa_app::group::Phase::Riding),"workout_paused"=>status.workout_clock.is_some_and(|clock|clock.paused),"local_paused"=>app.is_paused(),"can_control_workout"=>status.can_control_workout(),"workout_complete"=>status.workout_finished(),"late"=>status.late,"can_enter"=>status.can_enter(),"entering"=>status.entering,"name"=>status.selection.as_ref().map_or("",|s|s.name.as_str()),"error"=>status.error.as_str(),"can_retry"=>status.can_retry(),"activity_failed"=>status.activity_failed,"roster"=>&roster,"devices"=>&devices,"input_device"=>status.input_device.as_str(),"output_device"=>status.output_device.as_str(),"voice_volume"=>status.voice_volume,"voice"=>status.voice,"voice_pending"=>status.voice_pending,"microphone"=>status.microphone,"deafened"=>status.deafened,"countdown_s"=>status.start.map_or(0.0,|at|at.saturating_duration_since(std::time::Instant::now()).as_secs_f64()),"drafting"=>status.assistance.drafting,"keep_together"=>status.assistance.keep_together,"workout_id"=>status.workout_id.as_str(),"course_path"=>status.course.as_ref().map_or(String::new(),|p|p.display().to_string()),
    // Asked to stand in as organizer if the organizer is lost; the organizer may pick another.
    "standby_offer"=>status.standby_offer,"can_choose_fallback"=>status.host && status.phase==orqwa_app::group::Phase::Riding && status.handover==orqwa_app::group::Handover::Idle,
    // How long this rider keeps trying to reconnect, 0 while connected (#434).
    "reconnect_s"=>status.reconnecting.map_or(0.0,|until|until.saturating_duration_since(std::time::Instant::now()).as_secs_f64())}
}
