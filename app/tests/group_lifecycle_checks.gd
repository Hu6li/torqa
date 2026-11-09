class_name GroupLifecycleChecks
extends RefCounted
## Ending a group activity returns to the lobby and permits a fresh start (R237).

const MAIN_SCENE: PackedScene = preload("res://scenes/main.tscn")


static func check(tree: SceneTree, verify: Callable) -> void:
	var main: Control = MAIN_SCENE.instantiate()
	tree.root.add_child(main)
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var ride: RideScreen = main.get_node("RideScreen")
	app.group_started.emit(false, true)
	for frame: int in range(4):
		await tree.process_frame
	ride._group_window.popup_centered(Vector2i(980, 600))
	app.group_returned.emit()
	for frame: int in range(4):
		await tree.process_frame
	verify.call(start.visible and not ride.visible, "ending the activity returns to the start page")
	verify.call(start._tabs.current_tab == StartPage.Tab.GROUP_RIDES, "the group lobby is selected")
	verify.call(not ride._group_window.visible, "the previous ride's group window closes")
	verify.call(ride._finished, "ride input and presentation stop in the lobby")
	app.group_started.emit(false, true)
	for frame: int in range(4):
		await tree.process_frame
	verify.call(ride.visible and not ride._finished, "a new group start opens a fresh ride")
	var group: GroupRidesTab = ride._group_controls
	group._show_status({"phase": "riding", "host": true})
	verify.call(group._end_activity.visible, "the organizer can end the activity for everyone")
	group._show_status({"phase": "riding", "host": false})
	verify.call(not group._end_activity.visible, "friends have no organizer end control")
	main.free()
	await tree.process_frame
	await _lobby_controls(tree, verify)
	await _early_consent(tree, verify)


static func _early_consent(tree: SceneTree, verify: Callable) -> void:
	var dialog: GroupFallbackConsentDialog = GroupFallbackConsentDialog.new()
	tree.root.add_child(dialog)
	var status: Dictionary = {
		"host": false,
		"phase": "waiting",
		"fallback_consent": "42",
		"standby_offer": true,
		"roster": [{"name": "Organizer", "organizer": true}]
	}
	dialog.show_status(status)
	await _settle(tree)
	verify.call(dialog.visible, "the friend sees the consent request in the lobby")
	verify.call("Organizer" in dialog.title, "the request names the organizer")
	status["phase"] = "riding"
	dialog.show_status(status)
	verify.call(dialog.visible, "an unanswered lobby request remains visible after Start")
	dialog._answered = "42"
	dialog.show_status(status)
	verify.call(not dialog.visible, "an answered request is not displayed again")
	status["fallback_consent"] = ""
	status["standby_offer"] = false
	dialog.show_status(status)
	verify.call(not dialog.visible, "a withdrawn request disappears")
	dialog.free()
	await tree.process_frame


static func _lobby_controls(tree: SceneTree, verify: Callable) -> void:
	var main: Control = MAIN_SCENE.instantiate()
	tree.root.add_child(main)
	var app: OrqwaApp = main.get_node("Orqwa")
	app.connect_fake_trainer(180.0, 85.0)
	var until: int = Time.get_ticks_msec() + 5000
	while not app.trainer_connected() and Time.get_ticks_msec() < until:
		await tree.process_frame
	var start: StartPage = main.get_node("StartPage")
	start._tabs.current_tab = StartPage.Tab.GROUP_RIDES
	var group: GroupRidesTab = start._group
	group.set_process(false)
	var status: Dictionary = {
		"phase": "waiting",
		"host": true,
		"roster": [{"id": 0, "name": "Organizer", "me": true, "ready": false}]
	}
	group._show_status(status)
	await _settle(tree)
	verify.call(group._organizer_panel.visible, "organizer controls are visible before Start")
	verify.call(
		group._fallback_change.visible and group._fallback_change.disabled,
		"a solo lobby shows the unavailable fallback choice"
	)
	verify.call(
		not group._organizer_status.text.is_empty(),
		"the solo lobby explains why a fallback cannot be chosen"
	)
	var roster: Array = status["roster"]
	roster.append(
		{"id": 1, "name": "Friend", "me": false, "ready": false, "fallback_eligible": true}
	)
	status["can_choose_fallback"] = true
	group._show_status(status)
	verify.call(
		not group._fallback_change.disabled, "a connected friend can be chosen before readiness"
	)
	group._choose_organizer(true)
	await _settle(tree)
	var choices: VBoxContainer = group._organizer_picker._choices
	verify.call(
		choices.get_child_count() == 2, "the fallback picker includes the connected, unready friend"
	)
	group._organizer_picker.hide()
	status.merge(
		{
			"phase": "ready",
			"ready": true,
			"prepared": true,
			"can_start": true,
			"preferred_fallback": "Friend",
			"preferred_fallback_accepted": true
		},
		true
	)
	group._show_status(status)
	await _settle(tree)
	var step: GroupNextStep = group._next_step
	var start_rect: Rect2 = step._main.get_global_rect()
	verify.call(
		step._main.text == TranslationServer.translate("Start group ride"),
		"the prepared organizer has the Start action"
	)
	status["phase"] = "countdown"
	group._show_status(status)
	await _settle(tree)
	verify.call(
		step._cancel.visible and not step._main.visible,
		"Cancel replaces Start in the lobby countdown"
	)
	verify.call(
		step._cancel.text == TranslationServer.translate("Cancel"),
		"the countdown action says Cancel"
	)
	verify.call(
		step._cancel.get_global_rect().is_equal_approx(start_rect),
		"Cancel keeps the Start button's place and size"
	)
	verify.call(
		not group._end_activity.visible, "the lobby has no separate end-for-everyone button"
	)
	verify.call(
		"Friend" in group._organizer_status.text,
		"the lobby names the consenting fallback before starting"
	)
	status["host"] = false
	group._show_status(status)
	verify.call(not step._cancel.visible, "friends cannot cancel the organizer's countdown")
	main.free()
	await tree.process_frame


static func _settle(tree: SceneTree) -> void:
	for frame: int in range(8):
		await tree.process_frame


static func capture(
	tree: SceneTree, start: StartPage, group: GroupRidesTab, app: OrqwaApp, out: String
) -> void:
	var scroll: ScrollContainer = start._group_page
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		var solo: Dictionary = {
			"host": true,
			"phase": "waiting",
			"roster": [{"id": 0, "name": "Rider", "me": true, "ready": false}]
		}
		group._show_status(solo)
		await _capture(tree, scroll, out, "group-fallback-alone-" + locale)
		var chosen: Dictionary = GroupReadinessLayout.status(app, true, "ready")
		chosen["can_choose_fallback"] = true
		chosen["preferred_fallback"] = "Mia"
		chosen["fallback_consent"] = "42"
		for rider: Dictionary in chosen["roster"]:
			rider["fallback_eligible"] = not rider["me"]
		group._show_status(chosen)
		await _capture(tree, scroll, out, "group-fallback-auto-lobby-" + locale)
		chosen["fallback_consent"] = ""
		chosen["preferred_fallback_accepted"] = true
		group._show_status(chosen)
		await _capture(tree, scroll, out, "group-fallback-lobby-" + locale)
		group._choose_organizer(true)
		await _capture(tree, scroll, out, "group-fallback-picker-" + locale)
		group._organizer_picker.hide()
		var consent: GroupFallbackConsentDialog = GroupFallbackConsentDialog.new()
		tree.root.add_child(consent)
		var friend: Dictionary = GroupReadinessLayout.status(app, false, "ready")
		friend.merge({"fallback_consent": "42", "standby_offer": true})
		group._show_status(friend)
		consent.show_status(friend)
		await _capture(tree, scroll, out, "group-fallback-consent-" + locale, false)
		consent.free()
		chosen["phase"] = "countdown"
		group._show_status(chosen)
		await _capture(tree, scroll, out, "group-cancel-" + locale, false)
		await _capture_ride_window(tree, start, chosen, out, locale)


static func _capture_ride_window(
	tree: SceneTree, start: StartPage, status: Dictionary, out: String, locale: String
) -> void:
	var ride: RideScreen = start.get_parent().get_node("RideScreen")
	status.merge(
		{
			"phase": "riding",
			"workout_shared": true,
			"workout_paused": true,
			"can_control_workout": true
		},
		true
	)
	start.hide()
	ride.show()
	ride.set_process(false)
	ride._group_controls.set_process(false)
	UiScale.popup(ride._group_window, Vector2i(980, 600))
	ride._group_controls._show_status(status)
	await _settle(tree)
	var scroll: ScrollContainer = ride._group_controls.get_parent()
	scroll.scroll_vertical = 100000
	await _settle(tree)
	await tree.create_timer(0.15).timeout
	tree.root.get_texture().get_image().save_png(
		out.path_join("group-organizer-window-" + locale + ".png")
	)
	ride._group_window.hide()
	ride.hide()
	ride.set_process(true)
	start.show()


static func _capture(
	tree: SceneTree, scroll: ScrollContainer, out: String, name: String, bottom: bool = true
) -> void:
	await _settle(tree)
	scroll.scroll_vertical = 100000 if bottom else 0
	await _settle(tree)
	await tree.create_timer(0.15).timeout
	tree.root.get_texture().get_image().save_png(out.path_join(name + ".png"))
	print("saved " + name)


## A fresh group ride must show the difficulty the engine takes from the active profile.
static func check_difficulty(tree: SceneTree, verify: Callable) -> void:
	var setup: OrqwaApp = OrqwaApp.new()
	tree.root.add_child(setup)
	var previous: String = setup.profile()["id"]
	var rider: String = setup.save_profile(
		"", {"name": "Group difficulty check", "default_difficulty_pct": 100.0}
	)
	setup.free()
	var main: Control = MAIN_SCENE.instantiate()
	tree.root.add_child(main)
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var ride: RideScreen = main.get_node("RideScreen")
	# No course page has initialized its options since this app instance opened.
	app.group_started.emit(true, false)
	for i: int in range(4):
		await tree.process_frame
	var difficulty: float = ride._options["difficulty"]
	verify.call(
		is_equal_approx(difficulty, 100.0), "the first group ride uses the saved 100% default"
	)
	ride._bar.settings_requested.emit()
	var shown: float = ride._settings_dialog._options.options()["difficulty"]
	verify.call(is_equal_approx(shown, 100.0), "the ride settings show the saved difficulty too")
	ride._settings_dialog.hide()
	# A previously visited course must not override the profile on a later group start either.
	start._detail._options.set_difficulty(25.0)
	var profile: Dictionary = app.profile()
	profile["default_difficulty_pct"] = 75.0
	app.save_profile(rider, profile)
	app.group_started.emit(true, false)
	for i: int in range(4):
		await tree.process_frame
	difficulty = ride._options["difficulty"]
	verify.call(is_equal_approx(difficulty, 75.0), "a group start ignores stale course difficulty")
	app.select_profile(previous)
	app.delete_profile(rider)
	main.free()
	await tree.process_frame
