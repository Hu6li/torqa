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
	await _ride_window_columns()
	_ride_bar()
	_offer_card()
	await _organizer_offer()
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
	_check(not voice.microphone_allowed(), "a pending prompt does not open the microphone")
	_check(requests == [true], "turning the microphone on explicitly requests permission")
	_check(_microphone_off(app), "pending permission opens no capture")
	_check(voice._permission_waiting, "the OS prompt has visible pending guidance")
	voice._permission = func(request: bool) -> String:
		requests.append(request)
		return "allowed"
	voice.update_status({})
	_check(requests == [true, false], "waiting polls authorization without requesting again")
	_check(_microphone_off(app), "late approval never starts a check or unmutes")
	_check(not voice._permission_waiting, "approval ends polling and asks the rider to retry")
	var dialog: GroupVoiceDialog = voice._dialog
	for access: String in ["denied", "restricted", "unavailable"]:
		voice._permission = func(_request: bool) -> String: return access
		voice._set_microphone(true)
		_check(_microphone_off(app), access + " keeps the microphone off")
		dialog._microphone_on.button_pressed = true
		_check(not dialog._microphone_on.button_pressed, access + " keeps the join window's off")
		_check(_microphone_off(app), access + " cannot start a check")
		_check(dialog.permission_hint.visible, access + " explains how to recover")
	requests.clear()
	voice._permission = func(request: bool) -> String:
		requests.append(request)
		return "allowed"
	dialog._play_sound()
	voice.update_status({"speaker_test": true})
	_check(requests.is_empty(), "listening and speaker checks request no microphone")
	_check(voice.microphone_allowed(), "authorized microphone action may continue")
	_check(not voice._permission_hint.visible, "retry clears the permission guidance")
	if not OS.has_feature("macos"):
		_check(
			OrqwaCore.microphone_permission(false) == "allowed", "other platforms keep their flow"
		)
	voice.free()
	app.free()
	await process_frame


## One main button follows the rider's next step; a line says what the group waits for (R196).
func _next_steps(step: GroupNextStep) -> void:
	var preparing: Dictionary = {"phase": "ready", "host": true, "prepared": true}
	step.show_status(preparing, false, "Me", 0, true)
	_check(step._main.text == "Connect your trainer", "without a trainer, connecting comes first")
	step.show_status(preparing, false, "Me", 0, false)
	_check(not step._main.visible, "unless the trainer cannot be connected from here")
	step.show_status(preparing, true, "Me", 0, true)
	_check(
		step._main.text == "I'm ready" and not step._main.disabled and not step._undo.visible,
		"then the rider says they are ready"
	)
	var waiting: Dictionary = preparing.duplicate()
	waiting["ready"] = true
	waiting["roster"] = [
		{"name": "Me", "ready": true, "me": true},
		{"name": "Mia", "ready": false, "me": false},
	]
	step.show_status(waiting, true, "Me", 0, true)
	_check(
		step._main.text == "Start group ride" and step._main.disabled and step._undo.visible,
		"the organizer can start once everyone is ready, or take readiness back"
	)
	_check(step._line.text == "Waiting for Mia", "and sees who the group waits for")
	waiting["can_start"] = true
	waiting["roster"][1]["ready"] = true
	step.show_status(waiting, true, "Me", 0, true)
	_check(
		not step._main.disabled and step._line.text == "Everyone is ready", "then starts the ride"
	)
	var friend: Dictionary = {"phase": "ready", "host": false, "prepared": true, "ready": true}
	step.show_status(friend, true, "Alex", 0, true)
	_check(
		not step._main.visible and step._undo.visible, "a ready friend only takes readiness back"
	)
	_check(step._line.text == "You're ready. Alex starts the ride.", "and knows who starts it")
	var late: Dictionary = {"phase": "ready", "late": true, "prepared": true, "can_enter": true}
	step.show_status(late, true, "Alex", 0, true)
	_check(step._main.text == "Join ride now", "friends joining late enter explicitly")
	await process_frame


## In the ride's window, voice and the workout make a second column beside the riders (R196).
func _ride_window_columns() -> void:
	var app: OrqwaApp = OrqwaApp.new()
	root.add_child(app)
	var group: GroupRidesTab = GroupRidesTab.new()
	group.compact = true
	group.bind(app)
	root.add_child(group)
	await process_frame
	_check(
		(
			group._side_right.visible
			and group._voice_controls.get_parent().get_parent().get_parent() == group._side_right
		),
		"voice sits in the second column"
	)
	_check(group._leave == null, "the window has its own way to leave")
	group.free()
	app.free()


func _riders(app: OrqwaApp) -> int:
	var roster: Array = app.group_status()["roster"]
	return roster.size()


func _microphone_off(app: OrqwaApp) -> bool:
	var testing: bool = app.group_status().get("microphone_test", false)
	var mic: bool = app.group_status().get("microphone", false)
	return not testing and not mic


## Riders may name their own lookup servers (R192); mistakes are shown, never asked.
func _lookup_servers(host: OrqwaApp, connection: GroupConnectionDialog) -> void:
	_check(not connection._servers.editable, "servers are named only for a lookup")
	_check(
		connection._servers.text.is_empty() and "stun." in connection._servers.placeholder_text,
		"the public servers are shown until others are named"
	)
	connection._lookup.button_pressed = true
	connection._servers.text = "stun.example.org:0"
	connection._servers.text_changed.emit(connection._servers.text)
	_check(
		connection._servers_help.get_theme_color("font_color") == UiTheme.DANGER,
		"a server that cannot be asked is pointed out while typing"
	)
	connection._advanced.hide()
	connection._confirm()
	await process_frame
	_check(
		(
			connection.visible
			and connection._advanced.visible
			and str(host.group_status()["phase"]) == "idle"
		),
		"confirming shows the mistake in folded options and starts no networking"
	)
	connection._cancel()
	_check(host.set_stun_servers("stun:stun.example.org 192.0.2.1:3479"), "servers named")
	connection.ask_create()
	var named: String = "stun.example.org:3478, 192.0.2.1:3479"
	_check(connection._servers.text == named, "the named servers are shown as a lookup asks them")
	connection._servers.text = "typo:x"
	connection._cancel()
	connection.ask_create()
	_check(connection._servers.text == named, "edits not confirmed are dropped")
	_check(host.set_stun_servers(""), "the public servers again")
	connection._lookup.button_pressed = false
	connection._cancel()
	connection.ask_create()


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
	await _lookup_servers(host, connection)
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
	# Pasted into the home page's field, the reply instead, or the message it came in (R195).
	var join: GroupJoinCard = joining._join
	join._field.text = "orqwa:nothing"
	join._submit()
	_check(join._error.visible and not joining._connection.visible, "a wrong paste says so")
	join._field.text = "Ride with me tonight: " + invitation + " See you!"
	join._submit()
	_check(
		joining._connection.visible and str(peer.group_status()["phase"]) == "idle",
		"pasted invitation opens connection choices offline"
	)
	_check(not joining._connection.organizer().is_empty(), "the organizer is named from it")
	_check(
		joining._connection._mode.is_empty(), "invitation cannot silently grant home-network access"
	)
	joining._connection.get_cancel_button().pressed.emit()
	await process_frame
	_check(
		join._field.text.contains(invitation) and not joining._connection.visible,
		"Back keeps the invitation in the field"
	)
	join._submit()
	joining._connection._choices["home"].button_pressed = true
	joining._connection._confirm()
	until = Time.get_ticks_msec() + 5000
	while str(peer.group_status()["reply"]).is_empty() and Time.get_ticks_msec() < until:
		await process_frame
	_check(not str(peer.group_status()["reply"]).is_empty(), "join starts only after confirmation")
	_check(join._field.text.is_empty(), "the field empties once joining has started")
	joining._show_status(peer.group_status())
	_check(
		(
			joining._connecting.visible
			and joining._joining._code._field.text == str(peer.group_status()["reply"])
		),
		"the joiner sees the reply to send"
	)
	# The organizer adds the reply from their own field, here pasted with its message.
	var organizer: GroupRidesTab = GroupRidesTab.new()
	organizer.bind(host)
	root.add_child(organizer)
	await process_frame
	organizer._show_status(host.group_status())
	_check(organizer._add.visible, "the organizer has a seat to fill")
	organizer._add._field.text = invitation
	organizer._add._submit()
	_check(
		(
			organizer._add._error.text
			== tr("That's your invitation. Paste the reply your friend sent back.")
		),
		"their own invitation is told apart from a reply"
	)
	var reply: String = peer.group_status()["reply"]
	organizer._add._field.text = "Here: " + reply
	organizer._add._edited(organizer._add._field.text)
	_check(not organizer._add._error.visible, "editing clears the message")
	organizer._add._submit()
	until = Time.get_ticks_msec() + 10000
	while _riders(host) < 2 and Time.get_ticks_msec() < until:
		await process_frame
	_check(_riders(host) == 2, "the friend joins")
	organizer._add._field.text = reply
	organizer._add._submit()
	until = Time.get_ticks_msec() + 3000
	while str(host.group_status()["refusal"]).is_empty() and Time.get_ticks_msec() < until:
		await process_frame
	organizer._show_status(host.group_status())
	_check(
		organizer._add._error.text == tr("That reply was added already."),
		"the same reply twice says why it adds nobody"
	)
	organizer.free()
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
	picker.set_organizer("Alex", "Ride")
	picker.show_selection(false, course, workout)
	_check(
		(
			not picker._course.visible
			and picker._course_name.visible
			and not picker._apply.visible
			and picker._hint.text == "Chosen by Alex"
		),
		"friends see the shared choice by name, never the pickers (R196)"
	)
	# Friends see what the organizer is choosing before it is shared (#418).
	var browsed: Array = []
	picker.browsed.connect(func(c: String, w: String) -> void: browsed.append([c, w]))
	picker.show_selection(true, "", "")
	picker._course.select(1)
	picker._course.item_selected.emit(1)
	_check(browsed == [[picker.course(), ""]], "the organizer's choice goes to friends")
	picker.show_selection(false, "", "")
	_check(browsed.size() == 1, "a friend's picker sends nothing")
	_check(
		picker._waiting.visible and picker._waiting_text.text == "Alex is choosing the ride",
		"friends see who is choosing"
	)
	var listed: Dictionary = app.courses()[0]
	var shown_course: Dictionary = listed.duplicate()
	var path: String = listed["path"]
	shown_course["picture"] = app.course_preview(path)
	shown_course.erase("path")
	var shown_workout: Dictionary = app.workouts()[0]
	var shown_name: String = shown_course["name"]
	picker.show_preview({"course": shown_course, "workout": shown_workout})
	_check(
		picker.previewing() and not picker._course.visible and picker._course_name.visible,
		"a friend sees the course the organizer is choosing, by name"
	)
	_check(
		picker._course_name.text == shown_name,
		"named as the organizer has it: %s" % picker._course_name.text
	)
	_check(
		picker._map.visible and picker._elevation.visible and picker._chart.visible,
		"with its map, elevation and the workout's power"
	)
	_check(
		picker._course_caption.text == "Course preview" and not picker._hint.visible,
		"plainly called a preview"
	)
	picker.show_preview({"course": {}, "workout": shown_workout})
	_check(
		not picker._course_preview.visible and picker._workout_preview.visible,
		"a workout alone previews without a course"
	)
	picker.show_preview({})
	_check(
		not picker.previewing() and picker._waiting.visible and not picker._course.visible,
		"friends wait for the organizer again once the preview is gone"
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
	var step: GroupNextStep = group._next_step
	_check(step._working.visible, "it turns while this rider's ride is prepared")
	_check(
		step._progress.visible and is_equal_approx(step._progress.value, 55.0),
		"and shows how far the world is built"
	)
	group._show_status({"host": false, "phase": "ready", "name": "Ride", "roster": roster})
	_check(not step._working.visible, "nothing turns once it is prepared")
	_check(group._activity_panel.visible, "friends have the activity column too (R196)")
	await _next_steps(step)
	# Who is speaking (R190), this rider too, and who is muted here (R194): the voice buttons
	# follow without rebuilding the rows.
	var voice_status: Dictionary = {
		"host": false, "phase": "ready", "name": "Ride", "roster": roster, "voice": true
	}
	voice_status["microphone"] = true
	group._show_status(voice_status)
	var rows: Array[Node] = group._roster.get_children()
	roster[1]["speaking"] = true
	roster[2]["speaking"] = true
	roster[3]["muted"] = true
	group._show_status(voice_status)
	_check(group._roster.get_children() == rows, "speaking does not rebuild the riders' rows")
	var buttons: Dictionary[int, Button] = group._voice_buttons
	_check(
		buttons[1].icon == UiIcons.texture("speaker", 20, UiTheme.SUCCESS),
		"a friend heard speaking lights their speaker"
	)
	_check(
		buttons[0].icon == UiIcons.texture("speaker", 20, UiTheme.MUTED),
		"a quiet friend's speaker stays grey"
	)
	_check(
		buttons[2].icon == UiIcons.texture("microphone", 20, UiTheme.SUCCESS),
		"this rider's microphone lights while it sends speech"
	)
	_check(
		buttons[3].icon == UiIcons.texture("speaker_off", 20, UiTheme.DANGER),
		"a friend muted here is struck through"
	)
	_check(buttons[3].tooltip_text == tr("Unmute %s") % "Mia", "its tooltip says what it does")
	roster[1]["speaking"] = false
	voice_status["microphone"] = false
	group._show_status(voice_status)
	_check(
		buttons[1].icon == UiIcons.texture("speaker", 20, UiTheme.MUTED),
		"the speaker goes grey when they stop"
	)
	_check(
		buttons[2].icon == UiIcons.texture("microphone_off", 20, UiTheme.DANGER),
		"this rider's microphone, off, is struck through"
	)
	group._show_status({"host": true, "phase": "waiting", "roster": roster})
	var bins: int = 0
	for row: Node in group._roster.get_children():
		var last: Button = row.get_child(row.get_child_count() - 1) as Button
		if last != null and last.icon == UiIcons.texture("bin") and not last.disabled:
			bins += 1
	_check(bins == 3, "the organizer may remove each friend, not themselves")
	roster[3]["muted"] = false
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
	_check(
		group._rules_text.visible and not group._rules.visible,
		"riders read the frozen rules during the ride"
	)
	group._show_status(
		{"host": false, "phase": "waiting", "name": "Recovery 30", "can_retry": true}
	)
	_check(
		group._next_step._retry.visible,
		"activity retry remains available beside standard error history"
	)
	# The organizer sees who stands by and may choose another (#426).
	var standing: Array = [
		{"id": 0, "name": "Me", "ready": true, "me": true, "organizer": true},
		{"id": 1, "name": "Sam", "ready": true, "me": false, "fallback": true},
	]
	group._show_status(
		{"host": true, "phase": "riding", "can_choose_fallback": true, "roster": standing}
	)
	var tagged: Label = group._roster.get_child(1).get_child(1).get_child(1)
	_check(tagged.text.ends_with("Fallback organizer"), "the fallback is tagged: %s" % tagged.text)
	_check(
		group._fallback_change.visible and not group._fallback_change.disabled,
		"the organizer can choose another fallback"
	)
	group._show_status(
		{"host": false, "phase": "riding", "standby_offer": true, "roster": standing}
	)
	_check(
		group._organizer_panel.visible and group._organizer_accept.text == "Stand in as organizer",
		"a request to stand in shows in the panel"
	)
	# A rider whose connection was lost keeps trying by themselves (#434).
	var away: Array = [
		{"id": 0, "name": "Host", "ready": true, "me": false},
		{"id": 1, "name": "Sam", "ready": true, "me": false, "away": true},
	]
	group._show_status({"host": false, "phase": "riding", "reconnect_s": 272.4, "roster": away})
	_check(
		group._next_step._reconnect.visible and group._next_step._working.visible,
		"reconnecting turns and offers to try now"
	)
	var line: String = group._next_step._line.text
	_check(line.contains("4:32"), "and says how long: %s" % line)
	var sam: Label = group._roster.get_child(1).get_child(1).get_child(1)
	_check(sam.text == "Reconnecting…", "a friend whose seat is held: %s" % sam.text)
	group._show_status({"host": false, "phase": "riding", "roster": away})
	_check(not group._next_step._reconnect.visible, "a connected rider has nothing to retry")
	_check(not group._invitation_panel.visible, "joiners have no empty host invitation card")
	group._show_status(
		{"host": false, "phase": "ready", "late": true, "prepared": true, "can_enter": true}
	)
	_check(
		group._next_step._action == "devices" and not group._next_step._undo.visible,
		"late joiners connect their trainer before their one explicit entry"
	)
	_check(not group._picker._apply.visible, "late joiners cannot change the running selection")
	group.compact = true
	group._show_status({"host": true, "phase": "riding", "invite": "private"})
	_check(group._invitation_panel.visible, "organizers invite late friends during the ride")
	group._show_status(
		{"host": true, "phase": "waiting", "invite": "private", "reachability": "strict"}
	)
	_check(
		group._invite._reach.visible and not group._invite._reach_text.text.is_empty(),
		"a host behind strict NAT learns that friends at other homes probably cannot join"
	)
	for quiet: String in ["direct", "mapped", "home", ""]:
		group._show_status(
			{"host": true, "phase": "waiting", "invite": "private", "reachability": quiet}
		)
		_check(not group._invite._reach.visible, "no reachability warning for " + quiet)
	_check(group._next_step._action != "enter", "existing riders cannot restart through entry")
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
	voice.update_status({"voice": false})
	_check(voice._join.visible and not voice._joined.visible, "voice starts with a way to join")
	voice.update_status(
		{"voice": true, "microphone": false, "echo_prevention": false, "voice_volume": 0.4}
	)
	_check(not voice._join.visible and voice._joined.visible, "in voice, its controls show")
	_check(not voice._ask.visible, "how the rider listens was asked when joining")
	voice.update_status({"voice": true, "output_name": "AirPods Pro", "ask_listening": true})
	_check(
		voice._ask.visible and voice._ask_title.text.contains("AirPods Pro"),
		"voice moving to another output asks again, naming it (R194)"
	)
	voice.update_status(
		{"voice": true, "microphone": false, "echo_prevention": false, "voice_volume": 0.4}
	)
	var labels: Array[String] = []
	for button: Node in voice._joined.find_children("*", "Button", true, false):
		labels.append((button as Button).text)
	_check(
		not labels.has(tr("Microphone on")) and not labels.has(tr("Microphone off")),
		"the microphone is switched beside the rider's name, not in the card"
	)
	_check(is_equal_approx(voice._volume.value, 40.0), "volume follows the session")
	voice.update_status({"voice": true, "deafened": true, "microphone": false})
	var asked: Array[bool] = []
	voice._permission = func(request: bool) -> String:
		asked.append(request)
		return "allowed"
	voice.toggle_microphone()
	_check(asked.is_empty(), "mute all sound prevents microphone activation")
	voice.update_status({"speaker_test": true})
	_check(voice._joined.visible, "a sound check pauses voice without leaving it")
	voice.update_status({})
	_check(voice._join.visible, "out of voice, joining is offered again")
	var dialog: GroupVoiceDialog = voice._dialog
	dialog.ask_join()
	_check(
		dialog._choice.visible and not dialog._check.visible,
		"joining first asks how the rider listens"
	)
	dialog._listen_with(false)
	_check(
		dialog._check.visible and dialog._output_caption.text == tr("Headphones"),
		"then checks the sound of the headphones chosen"
	)
	_check(not dialog._inputs.visible, "the microphone starts off, its picker hidden")
	dialog.update_status(
		{
			"input_device": "headset",
			"output_device": "headset",
			"microphone_level": 0.65,
			"microphone_voice_detected": true,
			"devices": [{"id": "headset", "name": "Headset", "input": true, "output": true}]
		}
	)
	var input: String = dialog._inputs.get_selected_metadata()
	var output: String = dialog._outputs.get_selected_metadata()
	_check(input == "headset", "selected microphone follows the session")
	_check(output == "headset", "selected speakers follow the session")
	_check(is_equal_approx(dialog._level.value, 0.65), "the meter follows Rust status")
	_check(dialog._heard.text == tr("Voice detected"), "speech has a visible indicator")
	dialog.update_status({"speaker_test": true})
	_check(dialog._play.disabled, "one sound check at a time")
	dialog._on_action(&"back")
	_check(dialog._choice.visible, "back returns to how the rider listens")
	dialog.hide()
	dialog.update_status({"microphone": true, "echo_prevention": true})
	dialog.ask_devices()
	_check(
		dialog._choice.visible and not dialog._check.visible,
		"changing devices asks first how the rider listens"
	)
	dialog._listen_with(true)
	_check(
		dialog._microphone_on.button_pressed and dialog._inputs.visible,
		"the window shows the microphone as it is"
	)
	_check(dialog._output_caption.text == tr("Speakers"), "and how the rider listens")
	dialog.hide()
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
	var speaker: TextureRect = panel._speaking[1]
	_check(speaker.modulate.a == 0.0, "no speaker beside a quiet rider")
	peer["speaking"] = true
	panel.show_riders([peer], false)
	_check(
		panel._speaking[1] == speaker and speaker.modulate.a == 1.0,
		"a speaker beside the name of a rider heard speaking"
	)
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
	var listen: GroupListenCard = GroupListenCard.new()
	root.add_child(listen)
	var answers: Array[bool] = []
	listen.answered.connect(func(speakers: bool) -> void: answers.append(speakers))
	listen.ask("AirPods Pro")
	_check(
		listen.visible and listen._title.text.contains("AirPods Pro"),
		"over the ride, the card names the new output"
	)
	(listen.get_child(0).get_child(2).get_child(0) as Button).pressed.emit()
	_check(answers == [false] and not listen.visible, "headphones answer it and close it")
	listen.free()
	var microphone: Button = bar._microphone
	_check(not microphone.is_visible_in_tree(), "no microphone button outside voice")
	bar.show_microphone(true, false)
	_check(
		(
			microphone.is_visible_in_tree()
			and microphone.tooltip_text == tr("Turn your microphone on")
		),
		"in voice, the bar switches the microphone"
	)
	var switched: Array[bool] = []
	bar.microphone_requested.connect(func() -> void: switched.append(true))
	microphone.pressed.emit()
	_check(switched == [true], "pressing it asks for the switch")
	bar.folded = true
	_check(not group.is_visible_in_tree(), "it folds away with the other controls")
	_check(not microphone.is_visible_in_tree(), "the microphone button too")
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
	# Standing in should the organizer be lost (#426) says so, and is answered the same way.
	card.show_offer("Alex", "", true)
	_check(
		"stand in" in card._title.text and card._accept.text == "Stand in as organizer",
		"a standby request: %s / %s" % [card._title.text, card._accept.text]
	)
	_check("about a minute" in card._text.text, "it says when standing in would happen")
	card._accept.pressed.emit()
	_check(answers == [true, false, true], "standing in is accepted like organizing")
	card.show_offer("Alex", "")
	_check(card._accept.text == "Accept organizer duties", "an organizer request is itself again")
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
	ride._show_group({"phase": "riding", "roster": roster, "standby_offer": true})
	_check(
		card.visible and card._accept.text == "Stand in as organizer",
		"a request to stand in comes over the ride too"
	)
	main.free()


func _check(condition: bool, message: String) -> void:
	if not condition:
		_failed = true
		push_error(message)
