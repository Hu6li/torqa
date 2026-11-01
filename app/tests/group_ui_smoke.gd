extends SceneTree
## Group setup and activity preview behavior (R169).

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	await _group_connections()
	await _group_activity()
	await _group_motion()
	await _microphone_permissions()
	if not _failed:
		print("GROUP UI SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## A permission result must never start capture after the rider's action has returned.
func _microphone_permissions() -> void:
	var app: OrqwaApp = OrqwaApp.new()
	root.add_child(app)
	var voice: GroupVoiceControls = GroupVoiceControls.new()
	voice.bind(app)
	root.add_child(voice)
	await process_frame
	var requests: Array[bool] = []
	voice._permission = func(request: bool) -> String:
		requests.append(request)
		return "pending"
	voice._on_test()
	_check(requests == [true], "testing explicitly requests microphone permission")
	_check(_microphone_off(app), "pending permission opens no capture")
	_check(voice._permission_waiting, "the OS prompt has visible pending guidance")
	voice._permission = func(request: bool) -> String:
		requests.append(request)
		return "allowed"
	voice._settings.hide()
	voice.update_status({})
	_check(requests == [true, false], "waiting polls authorization without requesting again")
	_check(_microphone_off(app), "late approval never starts a test or unmutes")
	_check(not voice._permission_waiting, "approval ends polling and asks the rider to retry")
	for access: String in ["denied", "restricted", "unavailable"]:
		voice._permission = func(_request: bool) -> String: return access
		voice._mic.set_pressed_no_signal(true)
		voice._voice()
		_check(not voice._mic.button_pressed, access + " keeps the microphone off")
		voice._on_test()
		_check(_microphone_off(app), access + " cannot start a test")
		_check(voice._settings_permission_hint.visible, access + " explains how to recover")
	requests.clear()
	voice._permission = func(request: bool) -> String:
		requests.append(request)
		return "allowed"
	voice._on_listen(false)
	voice.update_status({"speaker_test": true})
	_check(requests.is_empty(), "listening and presenting speaker checks request no microphone")
	_check(voice._microphone_allowed(), "authorized microphone action may continue")
	_check(not voice._permission_hint.visible, "retry clears the permission guidance")
	if not OS.has_feature("macos"):
		_check(
			OrqwaCore.microphone_permission(false) == "allowed", "other platforms keep their flow"
		)
	voice.free()
	app.free()
	await process_frame


func _microphone_off(app: OrqwaApp) -> bool:
	var testing: bool = app.group_status().get("microphone_test", false)
	var mic: bool = app.group_status().get("microphone", false)
	return not testing and not mic


## Cancelling setup grants no network access; joining inspects the code before connecting.
func _group_connections() -> void:
	var host: OrqwaApp = OrqwaApp.new()
	root.add_child(host)
	var page: GroupRidesTab = GroupRidesTab.new()
	page.bind(host)
	root.add_child(page)
	await process_frame
	var connection: GroupConnectionDialog = page._connection
	connection.ask_create()
	_check(
		connection.visible and str(host.group_status()["phase"]) == "idle",
		"create asks before starting networking"
	)
	_check(connection._mode == "internet", "online rides have a clear default")
	connection.get_cancel_button().pressed.emit()
	await process_frame
	_check(
		not connection.visible and str(host.group_status()["phase"]) == "idle",
		"cancel creates no lobby"
	)
	connection.ask_create()
	connection._more.pressed.emit()
	connection._lookup.button_pressed = false
	connection._cancel()
	connection.ask_create()
	_check(
		not connection._lookup.button_pressed,
		"opening setup again preserves an explicit lookup opt-out"
	)
	connection._choices["home"].button_pressed = true
	_check(
		not connection._lookup.button_pressed and not connection._more.visible,
		"same-home rides need no public address lookup"
	)
	connection._confirm()
	var until: int = Time.get_ticks_msec() + 5000
	while str(host.group_status()["invite"]).is_empty() and Time.get_ticks_msec() < until:
		await process_frame
	var invitation: String = host.group_status()["invite"]
	_check(not invitation.is_empty(), "confirmation opens the home-network lobby")
	var peer: OrqwaApp = OrqwaApp.new()
	root.add_child(peer)
	var joining: GroupRidesTab = GroupRidesTab.new()
	joining.bind(peer)
	root.add_child(joining)
	await process_frame
	_check(
		not peer.group_connect(invitation, "internet", true),
		"home-network permission is checked before creating a connection owner"
	)
	_check(
		str(peer.group_status()["phase"]) == "idle", "incompatible invitation starts no networking"
	)
	joining._show_exchange("join")
	joining._paste.text = invitation
	joining._submit_exchange()
	_check(
		joining._connection.visible and str(peer.group_status()["phase"]) == "idle",
		"pasted invitation opens connection choices offline"
	)
	_check(
		joining._connection._mode.is_empty(), "invitation cannot silently grant home-network access"
	)
	joining._connection.get_cancel_button().pressed.emit()
	await process_frame
	_check(
		joining._exchange.visible and joining._paste.text == invitation,
		"Back preserves the invitation"
	)
	joining._submit_exchange()
	joining._connection._choices["home"].button_pressed = true
	joining._connection._confirm()
	until = Time.get_ticks_msec() + 5000
	while str(peer.group_status()["reply"]).is_empty() and Time.get_ticks_msec() < until:
		await process_frame
	_check(not str(peer.group_status()["reply"]).is_empty(), "join starts only after confirmation")
	peer.group_leave()
	host.group_leave()
	joining.free()
	page.free()
	peer.queue_free()
	host.queue_free()
	await process_frame


## Group choices preview before sharing, survive a refresh, and audio devices stay usable.
func _group_activity() -> void:
	var app: OrqwaApp = OrqwaApp.new()
	root.add_child(app)
	var group: GroupRidesTab = GroupRidesTab.new()
	group.bind(app)
	root.add_child(group)
	await process_frame
	group.set_process(false)
	var picker: GroupActivityPicker = group._picker
	_check(picker._course.item_count > 1, "group picker offers the bundled course")
	_check(picker._workout.item_count > 1, "group picker offers structured workouts")
	picker._course.select(1)
	picker._workout.select(1)
	picker._preview()
	_check(picker._map.visible and picker._elevation.visible, "course map and elevation preview")
	_check(picker._chart.visible and picker._duration.visible, "workout power and duration preview")
	var course: String = picker.course()
	var workout: String = picker.workout()
	picker.refresh()
	_check(picker.course() == course and picker.workout() == workout, "refresh preserves choices")
	_check(not picker._apply.disabled, "a preview is not shared until confirmed")
	picker.show_selection(true, course, workout)
	_check(picker._apply.disabled, "shared activity has an acknowledgement")
	picker.show_selection(false, course, workout)
	_check(
		picker._course.disabled and not picker._apply.visible, "joiners preview the shared choice"
	)
	group._show_status({"host": true, "phase": "waiting", "drafting": true, "keep_together": true})
	_check(
		not group._drafting.disabled and not group._together.disabled, "organizer sets both rules"
	)
	group._show_status({"host": false, "phase": "ready", "drafting": false, "keep_together": true})
	_check(
		group._drafting.disabled and group._together.disabled, "joiners cannot change shared rules"
	)
	_check(
		not group._drafting.button_pressed and group._together.button_pressed,
		"rules are independent"
	)
	group._show_status(
		{"host": true, "phase": "countdown", "drafting": false, "keep_together": true}
	)
	_check(group._drafting.disabled and group._together.disabled, "rules freeze at countdown")
	_check(group._rule_summary.visible, "riders see the frozen rules during the ride")
	group._show_status(
		{"host": false, "phase": "waiting", "name": "Recovery 30", "can_retry": true}
	)
	_check(group._retry.visible, "activity retry remains available beside standard error history")
	_check(not group._invitation_panel.visible, "joiners have no empty host invitation card")
	group._show_status(
		{"host": false, "phase": "ready", "late": true, "prepared": true, "can_enter": true}
	)
	_check(
		group._enter.visible and not group._ready_button.visible,
		"late joiners get one explicit entry action"
	)
	_check(not group._picker._apply.visible, "late joiners cannot change the running selection")
	group.compact = true
	group._show_status({"host": true, "phase": "riding", "invite": "private"})
	_check(group._invitation_panel.visible, "organizers invite late friends during the ride")
	group._show_status(
		{"host": true, "phase": "waiting", "invite": "private", "reachability": "strict"}
	)
	_check(
		group._reach.visible and not group._reach_text.text.is_empty(),
		"a host behind strict NAT learns that friends at other homes probably cannot join"
	)
	for quiet: String in ["direct", "mapped", "home", ""]:
		group._show_status(
			{"host": true, "phase": "waiting", "invite": "private", "reachability": quiet}
		)
		_check(not group._reach.visible, "no reachability warning for " + quiet)
	_check(not group._enter.visible, "existing riders cannot restart through entry")
	group._show_status(
		{
			"phase": "riding",
			"handover": "offered",
			"handover_offer": true,
			"can_cancel_handover": true
		}
	)
	_check(
		group._organizer_accept.visible and group._organizer_decline.visible,
		"successor gives explicit consent"
	)
	_check(not group._organizer_reach.visible, "no warning without discovery evidence")
	group._show_status(
		{
			"phase": "riding",
			"handover": "offered",
			"handover_offer": true,
			"can_cancel_handover": true,
			"reachability": "strict"
		}
	)
	_check(
		group._organizer_reach.visible,
		"an offered organizer behind strict NAT is warned before accepting"
	)
	group._show_status({"phase": "riding", "handover": "preparing", "can_cancel_handover": true})
	_check(
		not group._organizer_accept.visible and group._organizer_cancel.visible,
		"preparation can be cancelled before commit"
	)
	group._show_status(
		{
			"host": true,
			"phase": "riding",
			"workout_shared": true,
			"workout_paused": true,
			"can_control_workout": true
		}
	)
	_check(
		group._workout_panel.visible and not group._workout_pause.disabled,
		"organizer controls the shared workout"
	)
	_check(
		group._workout_pause.text == tr("Resume workout for everyone"),
		"paused workout has a clear group resume action"
	)
	group._show_status({"host": false, "phase": "riding", "workout_shared": true})
	_check(
		(
			group._workout_panel.visible
			and not group._workout_pause.visible
			and not group._workout_next.visible
		),
		"joiners see workout status without organizer buttons"
	)
	var voice: GroupVoiceControls = group._voice_controls
	voice.update_status({"voice": true, "microphone": false, "devices": []})
	_check(
		not voice._inputs.disabled and not voice._outputs.disabled,
		"devices can change during voice"
	)
	_check(not voice._mic.button_pressed, "listening does not imply microphone consent")
	voice.update_status({"voice_pending": true})
	_check(
		not voice._inputs.disabled and not voice._outputs.disabled,
		"devices can change during startup"
	)
	voice.update_status(
		{
			"voice": true,
			"input_device": "headset",
			"output_device": "headset",
			"voice_volume": 0.4,
			"devices": [{"id": "headset", "name": "Headset", "input": true, "output": true}]
		}
	)
	var input: String = voice._inputs.get_selected_metadata()
	var output: String = voice._outputs.get_selected_metadata()
	_check(input == "headset", "selected microphone follows the session")
	_check(output == "headset", "selected speakers follow the session")
	_check(is_equal_approx(voice._volume.value, 40.0), "volume follows the session")
	voice.update_status({"voice": true, "deafened": true, "microphone": false})
	_check(voice._mic.disabled, "mute all sound prevents microphone activation")
	voice.update_status(
		{"microphone_test": true, "microphone_level": 0.65, "echo_prevention": true}
	)
	_check(voice._listen.disabled and voice._mic.disabled, "local testing cannot transmit voice")
	_check(is_equal_approx(voice._level.value, 0.65), "local microphone meter follows Rust status")
	_check(voice._echo.button_pressed, "speaker echo prevention starts enabled")
	voice.update_status(
		{
			"microphone_test": true,
			"microphone_voice_detected": true,
			"microphone_voice_heard": true,
			"microphone_checked": true
		}
	)
	_check(voice._speech.text == tr("Voice detected"), "speech has a visible detection indicator")
	_check(voice._speaker_test.disabled, "microphone and speaker tests are exclusive")
	voice.update_status({"microphone_checked": true, "microphone_voice_heard": true})
	_check(
		voice._speech.text == tr("Voice detected"),
		"the completed microphone result remains visible"
	)
	voice.update_status({"microphone_checked": true})
	_check(voice._speech.text == tr("No voice detected"), "a quiet test has an explicit result")
	voice.update_status({"speaker_test": true})
	_check(
		voice._test.disabled and voice._listen.disabled,
		"speaker testing keeps capture and voice disabled"
	)
	_check(voice._speaker_test.text == tr("Stop speaker test"), "the speaker check can be stopped")
	voice.update_status({"echo_prevention": false})
	_check(
		not voice._echo.button_pressed and not voice._listen.disabled,
		"headphone mode and stopped test follow status"
	)
	group.free()
	app.free()


func _group_motion() -> void:
	var avatars: GroupRidersWorld = GroupRidersWorld.new()
	avatars.set_process(false)
	root.add_child(avatars)
	var panel: GroupRidersPanel = GroupRidersPanel.new()
	root.add_child(panel)
	await process_frame
	var peer: Dictionary = {
		"id": 1,
		"generation": "1",
		"name": "Alex <rider>",
		"gap_m": 12.0,
		"rider_x": 0.0,
		"rider_y": 12.0,
		"elevation_m": 500.0,
		"heading": 0.0,
		"grade": 8.0,
		"speed_kmh": 20.0,
		"cadence": 85.0,
		"effort": 4.0,
		"curvature": 0.0,
	}
	avatars.show_riders([peer], 1.0 / 60.0)
	panel.show_riders([peer], false)
	var first: GroupRiderAvatar = avatars.get_child(0)
	_check(
		first.generation == "1" and panel.visible, "fresh membership appears on the road and HUD"
	)
	_check(panel._labels[1].text == "12 m ahead", "relative gap uses metric distance")
	peer["gap_m"] = -12.0
	panel.show_riders([peer], true)
	_check(panel._labels[1].text == "39 ft behind", "gap follows direction and rider units")
	peer["generation"] = "2"
	avatars.show_riders([peer], 1.0 / 60.0)
	var replacement: GroupRiderAvatar = avatars.get_child(0)
	_check(
		first != replacement and replacement.generation == "2",
		"reused slots discard the old avatar motion"
	)
	avatars.show_riders([], 1.0 / 60.0)
	panel.show_riders([], false)
	_check(
		avatars.get_child_count() == 0 and not panel.visible, "stale or departed peers disappear"
	)
	avatars.free()
	panel.free()


func _check(condition: bool, message: String) -> void:
	if not condition:
		_failed = true
		push_error(message)
