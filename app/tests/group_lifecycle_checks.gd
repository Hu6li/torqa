class_name GroupLifecycleChecks
extends RefCounted
## Ending a group activity returns to the lobby and permits a fresh start (R236).

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
