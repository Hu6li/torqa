extends SceneTree
## Headless checks of the ride HUD: its editor (R51, R54) and the line under a large power
## figure.
## Run: godot --headless --path app -s res://tests/hud_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_hud_editor()
	_power_detail()
	if not _failed:
		print("HUD SMOKE TEST PASSED")
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


## A large power figure has the rider's W/kg, % of FTP and zone under it; without an FTP, no
## percentage. The line keeps the widest width it has had, so the HUD does not jump in width.
func _power_detail() -> void:
	var hud: HudPanel = HudPanel.new()
	root.add_child(hud)
	hud.show_layout(PackedStringArray(["power", "speed"]))
	var detail: Label = hud.get("_power_detail")
	hud.show_values({"power": 250.0, "power_percent_ftp": 104.2}, 3.3, 4)
	_check(detail.text == "3.3 W/kg · 104 % FTP · Z4 Threshold", "power line: " + detail.text)
	hud.show_values({"power": 250.0, "power_percent_ftp": null}, 3.3, 4)
	_check(detail.text == "3.3 W/kg · Z4 Threshold", "without an FTP: " + detail.text)
	hud.show_values({"power": 620.0, "power_percent_ftp": 258.0}, 8.3, 7)
	var sprint: float = detail.get_combined_minimum_size().x
	hud.show_values({"power": 250.0, "power_percent_ftp": 104.2}, 3.3, 4)
	_check(
		detail.get_combined_minimum_size().x >= sprint,
		"the line keeps its widest width: %s" % detail.get_combined_minimum_size()
	)
	hud.show_layout(PackedStringArray(["speed", "power"]))
	_check(not detail.visible, "no power line under another figure")
	hud.free()


func _available_chips(editor: HudEditor) -> Array[Node]:
	return editor.get_child(1).get_child(1).get_child(0).get_children()


func _expect(actual: PackedStringArray, expected: Array, what: String) -> void:
	_check(actual == PackedStringArray(expected), "%s: %s" % [what, actual])


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("HUD SMOKE TEST FAILED: " + what)
		_failed = true
