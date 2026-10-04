extends SceneTree
## Headless checks of UI behaviour that needs no ride: the HUD editor (R51).
## Run: godot --headless --path app -s res://tests/ui_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_hud_editor()
	_ride_settings()
	_course_cards()
	_translations()
	if not _failed:
		print("UI SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _hud_editor() -> void:
	var editor: HudEditor = HudEditor.new()
	root.add_child(editor)
	var changes: Array[PackedStringArray] = []
	editor.layout_changed.connect(func(layout: PackedStringArray) -> void: changes.append(layout))
	editor.edit(PackedStringArray(["power", "speed", "cadence"]), false)

	editor.place("cadence", 0)
	_expect(editor.layout(), ["cadence", "power", "speed"], "move to the large figure")
	editor.place("heart_rate", 1)
	_expect(editor.layout(), ["cadence", "heart_rate", "power", "speed"], "add from available")
	editor.remove("power")
	_expect(editor.layout(), ["cadence", "heart_rate", "speed"], "remove")

	# Drag "cadence" out of the HUD onto an available figure: removed.
	_available_chips(editor)[0].call("_drop_data", Vector2.ZERO, {HudEditor.DRAG_KEY: "cadence"})
	_expect(editor.layout(), ["heart_rate", "speed"], "drop on the available list")
	editor.place("speed", 0)

	# Directly in the HUD (R54): drop "power" on the lower half of the large figure: right after it.
	var preview: HudPanel = editor.find_children("*", "HudPanel", true, false)[0]
	var large: Control = preview.get_child(0)
	large.size = Vector2(200, 80)
	var accepts: bool = large.call("_can_drop_data", Vector2(10, 70), {HudPanel.DRAG_KEY: "power"})
	_check(accepts, "the HUD accepts figures")
	large.call("_drop_data", Vector2(10, 70), {HudPanel.DRAG_KEY: "power"})
	_expect(editor.layout(), ["speed", "power", "heart_rate"], "drop into the HUD")
	# Grid cells split left/right: dropping "speed" on the right half of "heart_rate" (the last
	# figure) moves it to the end.
	preview = editor.find_children("*", "HudPanel", true, false)[0]
	var grid: Node = preview.get_child(preview.get_child_count() - 1)
	var last: Control = grid.get_child(grid.get_child_count() - 1)
	last.size = Vector2(100, 40)
	last.call("_drop_data", Vector2(90, 10), {HudPanel.DRAG_KEY: "speed"})
	_expect(editor.layout(), ["power", "heart_rate", "speed"], "move within the HUD")
	# Free space in the HUD appends.
	preview = editor.find_children("*", "HudPanel", true, false)[0]
	preview.call("_drop_data", Vector2.ZERO, {HudPanel.DRAG_KEY: "cadence"})
	_expect(editor.layout(), ["power", "heart_rate", "speed", "cadence"], "drop on free space")
	editor.remove("power")
	editor.remove("cadence")

	for id: String in ["speed", "heart_rate"]:
		editor.remove(id)
	_expect(editor.layout(), ["heart_rate"], "the last figure stays")
	for i: int in range(20):
		editor.place(str(TorqaApp.hud_metrics()[i]["id"]), 99)
	_check(editor.layout().size() == TorqaApp.hud_max_metrics(), "at most the maximum figures")
	_check(not changes.is_empty(), "changes are reported")
	editor.free()


## The ride options and the in-ride settings dialog (R48, R49).
func _ride_settings() -> void:
	var options: RideOptions = RideOptions.new()
	root.add_child(options)
	var wanted: Dictionary = {
		"camera": 2, "difficulty": 75.0, "flat_descents": true, "time": "Evening", "weather": "Rain"
	}
	options.set_options(wanted)
	_check(options.options() == wanted, "options round trip: %s" % options.options())
	options.free()

	var dialog: RideSettingsDialog = RideSettingsDialog.new()
	root.add_child(dialog)
	var events: Array[String] = []
	dialog.finish_requested.connect(func() -> void: events.append("finish"))
	dialog.abort_requested.connect(func() -> void: events.append("abort"))
	dialog.options_changed.connect(func(_options: Dictionary) -> void: events.append("options"))
	dialog.edit(wanted, PackedStringArray(["power", "speed"]), false)
	dialog.custom_action.emit(&"finish")
	_check(events == ["finish"], "finish at once: %s" % [events])
	# Aborting asks first; only the confirmation aborts.
	dialog.custom_action.emit(&"abort")
	_check(events == ["finish"], "abort needs a confirmation: %s" % [events])
	var confirm: ConfirmationDialog = (
		dialog.find_children("*", "ConfirmationDialog", true, false)[0]
	)
	confirm.confirmed.emit()
	_check(events == ["finish", "abort"], "abort once confirmed: %s" % [events])
	dialog.free()


## Course cards (R37, R39) from a course preview: figures one per row, no elevation strip.
func _course_cards() -> void:
	var course: Dictionary = {
		"path": "/tmp/x.tqc",
		"name": "Gurten",
		"length_m": 2400.0,
		"elevation_gain_m": 149.4,
		"max_grade": 17.6,
		"track": PackedVector2Array([Vector2(0, 0), Vector2(500, 800), Vector2(900, 1200)]),
		"profile": PackedVector2Array([Vector2(0, 560), Vector2(2400, 709)]),
	}
	var figures: Array[PackedStringArray] = CourseCard.figures(course, false)
	var expected: Array[PackedStringArray] = [
		PackedStringArray(["Length", "2.4 km"]),
		PackedStringArray(["Climbing", "149 m"]),
		PackedStringArray(["Steepest", "18 %"]),
	]
	_check(figures == expected, "card figures: %s" % [figures])
	var imperial: Array[PackedStringArray] = CourseCard.figures(course, true)
	_check(
		imperial[0][1] == "1.5 mi" and imperial[1][1] == "490 ft",
		"imperial card figures: %s" % [imperial]
	)
	var card: CourseCard = CourseCard.new(course, false)
	root.add_child(card)
	var opened: Array[bool] = [false]
	card.pressed.connect(func() -> void: opened[0] = true)
	var click: InputEventMouseButton = InputEventMouseButton.new()
	click.button_index = MOUSE_BUTTON_LEFT
	click.pressed = true
	card.call("_gui_input", click)
	_check(opened[0], "a click opens the course")
	_check(not _has_badge(card), "a GPX course has no video badge")
	card.free()
	course["video"] = "Gurten.MP4"
	var video_card: CourseCard = CourseCard.new(course, false)
	_check(_has_badge(video_card), "a video course is marked as one")
	video_card.free()


func _has_badge(node: Node) -> bool:
	for child: Node in node.get_children():
		var label: Label = child as Label
		if (label != null and label.text == "Video") or _has_badge(child):
			return true
	return false


func _translations() -> void:
	var before: String = TranslationServer.get_locale()
	TranslationServer.set_locale("de")
	_check(TranslationServer.translate("Power") == "Leistung", "German texts load")
	_check(
		TranslationServer.translate("Climb done in %s") % "4:12" == "Anstieg geschafft in 4:12",
		"formatted texts translate"
	)
	TranslationServer.set_locale("en")
	_check(TranslationServer.translate("Power") == "Power", "English is the source language")
	TranslationServer.set_locale(before)


## The available figures of the editor (its second column).
func _available_chips(editor: HudEditor) -> Array[Node]:
	return editor.get_child(1).get_child(1).get_child(0).get_children()


func _expect(actual: PackedStringArray, expected: Array, what: String) -> void:
	_check(actual == PackedStringArray(expected), "%s: %s" % [what, actual])


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("UI SMOKE TEST FAILED: " + what)
		_failed = true
