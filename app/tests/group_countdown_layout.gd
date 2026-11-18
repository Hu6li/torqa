class_name GroupCountdownLayout
extends RefCounted
## The shared countdown and live pings keep the activity preview still (R204, R235).


static func check(tree: SceneTree, verify: Callable) -> void:
	var main: Control = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	tree.root.add_child(main)
	await tree.process_frame
	var start: StartPage = main.get_node("StartPage")
	var app: OrqwaApp = main.get_node("Orqwa")
	verify.call(app.connect_fake_trainer(180.0, 85.0), "countdown's trainer connects")
	var until: int = Time.get_ticks_msec() + 5000
	while not app.trainer_connected() and Time.get_ticks_msec() < until:
		await tree.process_frame
	verify.call(app.trainer_connected(), "countdown has a connected trainer")
	var group: GroupRidesTab = start._group
	start._tabs.current_tab = StartPage.Tab.GROUP_RIDES
	group.set_process(false)
	var course: String = app.courses()[0]["path"]
	var workout: String = app.workouts()[0]["id"]
	var original_size: Vector2i = tree.root.size
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for room: Vector2 in [Vector2(1600, 900), Vector2(1024, 640)]:
			tree.root.size = Vector2i((room * tree.root.content_scale_factor).round())
			for host: bool in [true, false]:
				for activity: Array in [[course, ""], [course, workout], ["", workout]]:
					await _countdown(tree, group, host, activity, verify)
	main.free()
	tree.root.size = original_size
	TranslationServer.set_locale("en")
	await tree.process_frame


static func _countdown(
	tree: SceneTree, group: GroupRidesTab, host: bool, activity: Array, verify: Callable
) -> void:
	var course: String = activity[0]
	var workout: String = activity[1]
	var status: Dictionary = {
		"phase": "ready",
		"host": host,
		"prepared": true,
		"ready": true,
		"can_start": host,
		"course_path": course,
		"workout_id": workout,
		"name": "Group ride",
		"roster":
		[
			{"id": 0, "name": "Alex", "ready": true, "me": host, "organizer": true},
			{"id": 1, "name": "Mia", "ready": true, "me": not host},
		],
	}
	var friend: Dictionary = status["roster"][1 if host else 0]
	friend["ping_ms"] = 45
	group._show_status(status)
	var picker: GroupActivityPicker = group._picker
	# The organizer's draft may differ from the shared choice. The shared choice is shown
	# before measuring the countdown, as when they press Share with the group.
	if host:
		picker._select(picker._course, course)
		picker._select(picker._workout, workout)
		picker._preview()
	await _settle(tree)
	var map: Rect2 = picker._map.get_global_rect()
	var elevation: Rect2 = picker._elevation.get_global_rect()
	var chart: Rect2 = picker._chart.get_global_rect()
	for seconds: int in [5, 3, 1]:
		friend["ping_ms"] = 60000 if seconds == 3 else 45
		status["phase"] = "countdown"
		status["countdown_s"] = seconds
		group._show_status(status)
		await _settle(tree)
		var context: String = "%s host=%s %s" % [TranslationServer.get_locale(), host, activity]
		if not course.is_empty():
			verify.call(
				picker._map.get_global_rect().is_equal_approx(map),
				(
					"countdown keeps the map's position and size: %s %s -> %s"
					% [context, map, picker._map.get_global_rect()]
				)
			)
			verify.call(
				picker._elevation.get_global_rect().is_equal_approx(elevation),
				"countdown keeps the elevation's position and size: " + context
			)
		if not workout.is_empty():
			verify.call(
				picker._chart.get_global_rect().is_equal_approx(chart),
				"countdown keeps the workout's position and size: " + context
			)
		verify.call(
			picker._course.disabled and picker._workout.disabled and picker._apply.disabled,
			"countdown freezes activity changes"
		)
		verify.call(
			group._drafting.disabled and group._together.disabled, "countdown freezes shared rules"
		)
		verify.call(
			(
				group._next_step._line.text
				== TranslationServer.translate("Starting in %s seconds…") % seconds
			),
			"countdown replaces the readiness message"
		)


static func _settle(tree: SceneTree) -> void:
	for frame: int in range(8):
		await tree.process_frame
