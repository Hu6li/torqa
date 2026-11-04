class_name GroupJoinActivityChecks
extends RefCounted
## Joining shows the shared activity before local files arrive, at the organizer's size (#508).


static func check(tree: SceneTree, verify: Callable) -> void:
	var scene: PackedScene = load("res://scenes/main.tscn")
	var main: Control = scene.instantiate()
	tree.root.add_child(main)
	var start: StartPage = main.get_node("StartPage")
	var app: OrqwaApp = main.get_node("Orqwa")
	var group: GroupRidesTab = start._group
	start._tabs.current_tab = StartPage.Tab.GROUP_RIDES
	group.set_process(false)
	var picker: GroupActivityPicker = group._picker
	var course: String = app.courses()[0]["path"]
	var workout: String = app.workouts()[0]["id"]
	var original_size: Vector2i = tree.root.size
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for late: bool in [false, true]:
			await _receiving(tree, group, late, course, workout, verify)
		for room: Vector2i in [
			Vector2i(1600, 900), Vector2i(2560, 1440), Vector2i(1024, 640), Vector2i(720, 900)
		]:
			tree.root.size = Vector2i((Vector2(room) * tree.root.content_scale_factor).round())
			for plan: String in ["", workout]:
				var status: Dictionary = _status(false)
				status.merge({"host": true, "course_path": course, "workout_id": plan}, true)
				group._show_status(status)
				picker._select(picker._course, course)
				picker._select(picker._workout, plan)
				picker._preview()
				await _settle(tree)
				var voice_card: Control = group._voice_controls.get_parent()
				var organizer_map: Vector2 = picker._map.size
				status["host"] = false
				group._show_status(status)
				await _settle(tree)
				verify.call(
					(
						picker._map.size.x > 0.0
						and is_equal_approx(
							picker._map.size.y / picker._map.size.x, CourseCard.MAP_ASPECT
						)
					),
					"the bounded image remains visible and keeps its proportions"
				)
				verify.call(
					(
						picker._map.size.x <= organizer_map.x + 1.0
						and picker._map.size.y <= organizer_map.y + 1.0
					),
					(
						"a joiner's course image is no larger than the organizer's: %s %s"
						% [locale, room]
					)
				)
				if not group._body.vertical:
					verify.call(
						(
							picker._map.get_global_rect().end.y
							<= voice_card.get_global_rect().end.y + 1.0
						),
						"the course image ends above the voice card's bottom"
					)
	main.free()
	tree.root.size = original_size
	TranslationServer.set_locale("en")
	await tree.process_frame


static func _receiving(
	tree: SceneTree,
	group: GroupRidesTab,
	late: bool,
	course: String,
	workout: String,
	verify: Callable
) -> void:
	var picker: GroupActivityPicker = group._picker
	var status: Dictionary = _status(late)
	status["phase"] = "waiting"
	status["name"] = ""
	group._show_status(status)
	verify.call(picker._waiting.visible, "an unselected lobby still says the organizer is choosing")
	for activity: Array in [[course, ""], ["", workout], [course, workout]]:
		status["name"] = "Evening group ride"
		status["phase"] = "downloading"
		status["course_path"] = ""
		status["workout_id"] = ""
		group._show_status(status)
		await _settle(tree)
		verify.call(
			not picker._waiting.visible and picker._course_name.is_visible_in_tree(),
			"the shared activity replaces choosing while its files are downloading"
		)
		verify.call(
			picker._course_name.text == "Evening group ride" and picker._hint.is_visible_in_tree(),
			"the received selection names the activity and who chose it before a local copy exists"
		)
		verify.call(not picker._apply.visible, "a joiner cannot change the shared activity")
		# A stopped transfer still has a frozen choice; it must not look like a new selection.
		status["activity_failed"] = true
		group._show_status(status)
		verify.call(not picker._waiting.visible, "a download failure retains the shared name")
		status["activity_failed"] = false
		status["phase"] = "preparing"
		status["course_path"] = activity[0]
		status["workout_id"] = activity[1]
		group._show_status(status)
		await _settle(tree)
		verify.call(not picker._waiting.visible, "local preparation keeps the shared choice")
		verify.call(
			picker._map.is_visible_in_tree() == (not str(activity[0]).is_empty()),
			"the downloaded course shows its map"
		)
		verify.call(
			picker._chart.is_visible_in_tree() == (not str(activity[1]).is_empty()),
			"the downloaded workout shows its power chart"
		)
		status["phase"] = "ready"
		status["prepared"] = true
		group._show_status(status)
		verify.call(not picker._waiting.visible, "a ready joiner still sees the selected activity")
	status["name"] = ""
	status["course_path"] = ""
	status["workout_id"] = ""
	group._show_status(status)
	verify.call(picker._waiting.visible, "a lobby without a selection returns to choosing")


static func _status(late: bool) -> Dictionary:
	return {
		"host": false,
		"late": late,
		"phase": "ready",
		"name": "Evening group ride",
		"roster":
		[
			{"id": 0, "name": "Alex", "ready": true, "organizer": true, "me": false},
			{"id": 1, "name": "Rider", "ready": false, "me": true},
		]
	}


static func _settle(tree: SceneTree) -> void:
	for frame: int in range(8):
		await tree.process_frame
