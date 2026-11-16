extends SceneTree
## Scratch probe: does the organizer's map stay while a share is under way?


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var main: Control = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var start: StartPage = main.get_node("StartPage")
	var app: OrqwaApp = main.get_node("Orqwa")
	var group: GroupRidesTab = start._group
	start._tabs.current_tab = StartPage.Tab.GROUP_RIDES
	group.set_process(false)
	var course: String = app.courses()[0]["path"]
	print("course ", course, " picture bytes ", app.course_preview(course).size())
	for room: Vector2 in [Vector2(1600, 900), Vector2(1024, 640), Vector2(800, 1280)]:
		root.size = Vector2i((room * root.content_scale_factor).round())
		await _sequence(group, course, str(room))
	quit(0)


func _sequence(group: GroupRidesTab, course: String, room: String) -> void:
	var picker: GroupActivityPicker = group._picker
	var roster: Array = [
		{"id": 0, "name": "Alex", "ready": false, "me": true, "organizer": true},
		{"id": 1, "name": "Mia", "ready": false, "me": false},
	]
	var status: Dictionary = {"phase": "waiting", "host": true, "roster": roster}
	group._activity_key = ""
	group._show_status(status)
	picker._select(picker._course, course)
	picker._browse()
	await _report(picker, room + " browsed")
	# Share pressed: the picker resets the key, as _select_activity does.
	group._activity_key = ""
	status["phase"] = "downloading"
	group._show_status(status)
	await _report(picker, room + " downloading (host hashing)")
	status["phase"] = "preparing"
	status["course_path"] = course
	status["name"] = "Shared"
	roster[1]["stage"] = "downloading"
	roster[1]["percent"] = 40
	group._show_status(status)
	await _report(picker, room + " preparing, friend downloading")
	roster[1]["stage"] = "preparing"
	roster[1]["percent"] = 100
	status["phase"] = "ready"
	status["prepared"] = true
	group._show_status(status)
	await _report(picker, room + " ready, friend done")
	status.erase("course_path")
	status["phase"] = "waiting"
	status["prepared"] = false
	var friend: Dictionary = roster[1]
	friend.erase("stage")
	group._show_status(status)


func _report(picker: GroupActivityPicker, label: String) -> void:
	for _i: int in range(4):
		await process_frame
	print(
		"%-40s map visible=%s in_tree_visible=%s rect=%s texture=%s selected=%s" % [
			label,
			picker._map.visible,
			picker._map.is_visible_in_tree(),
			picker._map.get_global_rect(),
			picker._map._map.texture != null,
			picker.course(),
		]
	)
