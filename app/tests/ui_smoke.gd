extends SceneTree
## Headless checks of UI behaviour that needs no ride: the HUD editor (R51).
## Run: godot --headless --path app -s res://tests/ui_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_hud_editor()
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

	# Drag "speed" onto the upper half of the first chip of the shown list: before it.
	var shown: Array[Node] = _chips(editor, 0)
	(shown[0] as Control).size = Vector2(200, 30)
	shown[0].call("_drop_data", Vector2(10, 5), {HudEditor.DRAG_KEY: "speed"})
	_expect(editor.layout(), ["speed", "cadence", "heart_rate"], "drop before a figure")
	# Drag "cadence" onto an available chip: hidden.
	_chips(editor, 2)[0].call("_drop_data", Vector2.ZERO, {HudEditor.DRAG_KEY: "cadence"})
	_expect(editor.layout(), ["speed", "heart_rate"], "drop on the available list")

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


## The chips of a list column of the editor: 0 = shown, 2 = available.
func _chips(editor: HudEditor, column: int) -> Array[Node]:
	return editor.get_child(column).get_child(1).get_child(0).get_children()


func _expect(actual: PackedStringArray, expected: Array, what: String) -> void:
	_check(actual == PackedStringArray(expected), "%s: %s" % [what, actual])


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("UI SMOKE TEST FAILED: " + what)
		_failed = true
