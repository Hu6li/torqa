extends SceneTree
## Group setup and activity preview behavior (R169).

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	await _group_connections()
	await _group_activity()
	await _group_motion()
	_ride_bar()
	_offer_card()
	await _organizer_offer()
	if not _failed:
		print("GROUP UI SMOKE TEST PASSED")
	quit(1 if _failed else 0)


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
	# How far everyone has got (#419), and something turning while this rider waits (#420).
	var roster: Array = [
		{"id": 0, "name": "Host", "ready": false, "me": false, "stage": "preparing", "percent": 70},
		{
			"id": 1,
			"name": "Sam",
			"ready": false,
			"me": false,
			"stage": "downloading",
			"percent": 40
		},
		{"id": 2, "name": "Me", "ready": false, "me": true, "stage": "preparing", "percent": 55},
		{"id": 3, "name": "Mia", "ready": true, "me": false, "stage": "", "percent": 0},
	]
	group._show_status({"host": false, "phase": "preparing", "name": "Ride", "roster": roster})
	var states: Array[String] = []
	for row: Node in group._roster.get_children():
		states.append((row.get_child(1).get_child(1) as Label).text)
	_check(states[0] == "Building the world · 70 %", "a friend building: %s" % states[0])
	_check(states[1] == "Downloading · 40 %", "a friend downloading: %s" % states[1])
	_check(states[3] == "Ready", "a ready friend: %s" % states[3])
	_check(group._working.visible, "it turns while this rider's ride is prepared")
	_check(
		group._progress.visible and is_equal_approx(group._progress.value, 55.0),
		"and shows how far the world is built"
	)
	group._show_status({"host": false, "phase": "ready", "name": "Ride", "roster": roster})
	_check(not group._working.visible, "nothing turns once it is prepared")
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
	# A rider whose connection was lost keeps trying by themselves (#434).
	var away: Array = [
		{"id": 0, "name": "Host", "ready": true, "me": false},
		{"id": 1, "name": "Sam", "ready": true, "me": false, "away": true},
	]
	group._show_status({"host": false, "phase": "riding", "reconnect_s": 272.4, "roster": away})
	_check(
		group._reconnect.visible and group._working.visible,
		"reconnecting turns and offers to try now"
	)
	_check(group._next.text.contains("4:32"), "and says how long: %s" % group._next.text)
	var sam: Label = group._roster.get_child(1).get_child(1).get_child(1)
	_check(sam.text == "Reconnecting…", "a friend whose seat is held: %s" % sam.text)
	group._show_status({"host": false, "phase": "riding", "roster": away})
	_check(not group._reconnect.visible, "a connected rider has nothing to retry")
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
		"power_w": 252.0,
		"watts_per_kg": 3.36,
		"avatar": "male",
	}
	avatars.show_riders([peer], 1.0 / 60.0)
	panel.show_riders([peer], false)
	var first: GroupRiderAvatar = avatars.get_child(0)
	_check(
		first.generation == "1" and panel.visible, "fresh membership appears on the road and HUD"
	)
	_check(panel._labels[1].text == "12 m ahead", "relative gap uses metric distance")
	# Others see the rider as they chose to look (#423) and their effort (#422).
	_check(first._avatar.rider == "male", "the rider's own avatar")
	_check(panel._efforts[1].text == "252 W · 3.4 W/kg", "power and W/kg beside the name")
	_check("252 W · 3.4 W/kg" in first._name.text, "and under it on the road")
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


## The group ride opens from the ride's control bar (#431), and only while in a group.
func _ride_bar() -> void:
	var bar: RideBar = RideBar.new()
	root.add_child(bar)
	var group: Button = bar._group
	_check(not group.is_visible_in_tree(), "no group button outside a group")
	bar.show_group(true)
	var asked: Array[bool] = []
	bar.group_requested.connect(func() -> void: asked.append(true))
	group.pressed.emit()
	_check(group.is_visible_in_tree() and asked == [true], "in a group, the bar opens it")
	bar.folded = true
	_check(not group.is_visible_in_tree(), "it folds away with the other controls")
	bar.free()


## The request to organize says who asks and what it means, warns only when the rider's own
## discovery suggests friends could not reach them, and goes once answered (#424).
func _offer_card() -> void:
	var card: GroupOfferCard = GroupOfferCard.new()
	root.add_child(card)
	card.show_offer("Alex", "")
	_check(card.visible and "Alex" in card._title.text, "the request says who asks")
	_check(not card._warning.visible, "no warning without a reason")
	var answers: Array[bool] = []
	card.answered.connect(func(accept: bool) -> void: answers.append(accept))
	card._accept.pressed.emit()
	_check(answers == [true] and not card.visible, "accepting answers and closes it")
	card.show_offer("Alex", "Friends could not reach you.")
	_check(card._warning.visible, "a reason to doubt reaching the rider is shown")
	card._decline.pressed.emit()
	_check(answers == [true, false] and not card.visible, "declining answers too")
	card.free()


## Over the ride, the request shows when it comes and goes when the organizer withdraws it;
## the group's button shows only in a group (#431).
func _organizer_offer() -> void:
	var main: Control = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var ride: RideScreen = main.get_node("RideScreen")
	var card: GroupOfferCard = ride._offer
	var group: Button = ride._bar._group
	var roster: Array = [{"name": "Alex", "organizer": true}, {"name": "Sam", "me": true}]
	ride._show_group({"phase": "idle"})
	_check(not group.visible and not card.visible, "nothing of the group outside one")
	ride._show_group({"phase": "riding", "roster": roster})
	_check(group.visible and not card.visible, "in a group its button, and no request")
	var offer: Dictionary = {
		"phase": "riding", "roster": roster, "handover_offer": true, "reachability": "strict"
	}
	ride._show_group(offer)
	_check(card.visible and "Alex" in card._title.text, "the request comes over the ride")
	ride.show()
	for frame: int in range(3):
		await process_frame
	var bottom: float = ride.size.y - RideScreen.OFFER_BOTTOM
	_check(
		card.size.y < 360 and absf(card.get_rect().end.y - bottom) < 1.0,
		"as tall as its lines, above the controls: %s" % card.get_rect()
	)
	_check(card._warning.visible, "with the warning about reaching the rider")
	ride._show_group({"phase": "riding", "roster": roster})
	_check(not card.visible, "a withdrawn request goes")
	main.free()


func _check(condition: bool, message: String) -> void:
	if not condition:
		_failed = true
		push_error(message)
