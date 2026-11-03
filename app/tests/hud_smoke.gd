extends SceneTree
## Headless checks of the ride HUD: its editor (R51, R54), the line under a large power
## figure, the zone bars beside the figures and the minimap.
## Run: godot --headless --path app -s res://tests/hud_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_hud_editor()
	_power_detail()
	_zone_bars()
	_calories()
	await _rider_hud_scrolling()
	await _minimap()
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
		editor.place(str(OrqwaApp.hud_metrics()[i]["id"]), 99)
	_check(editor.layout().size() == OrqwaApp.hud_max_metrics(), "at most the maximum figures")
	_check(not changes.is_empty(), "changes are reported")
	editor.free()


## A large power figure has the rider's W/kg, % of FTP and zone under it; without an FTP, no
## percentage. The line keeps the widest width it has had, so the HUD does not jump in width.
## A large heart rate has its % of the maximum and zone under it, in the zone's colour.
func _power_detail() -> void:
	var hud: HudPanel = HudPanel.new()
	root.add_child(hud)
	hud.show_layout(PackedStringArray(["heart_rate", "speed"]))
	var detail: Label = hud.get("_detail")
	var heart: Dictionary = {"heart_rate_percent_max": 80.0, "heart_rate_zone": 3.0}
	hud.show_values(heart, 3.3, 4)
	var moderate: Color = UiTheme.HEART_RATE_ZONES[2][1]
	_check(detail.visible and detail.text == "80 % max · Z3 Moderate", "heart line: " + detail.text)
	_check(detail.get_theme_color("font_color") == moderate, "in the heart-rate zone's colour")
	hud.show_values({"heart_rate_percent_max": null, "heart_rate_zone": 3.0}, 3.3, 4)
	_check(detail.text.is_empty(), "no heart line without a maximum: " + detail.text)
	hud.show_layout(PackedStringArray(["power", "speed"]))
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


## Each figure under the large one with a zone has a bar in that zone's colour (#432): power
## figures in the power zones' colours, heart-rate figures in the heart-rate zones'; none
## without a zone, e.g. for a rider without an FTP. The large figure has none: its power line
## is coloured already. The editor's example values show theirs.
func _zone_bars() -> void:
	var hud: HudPanel = HudPanel.new()
	root.add_child(hud)
	hud.show_layout(PackedStringArray(["speed", "power", "heart_rate", "cadence", "avg_power"]))
	hud.show_zones({"power": 5, "heart_rate": 2, "cadence": null, "avg_power": 3})
	var vo2max: Color = UiTheme.POWER_ZONES[4][1]
	var tempo: Color = UiTheme.POWER_ZONES[2][1]
	var light: Color = UiTheme.HEART_RATE_ZONES[1][1]
	_check(_zone_bar(hud, "power") == vo2max, "power in its zone's colour")
	_check(_zone_bar(hud, "avg_power") == tempo, "the average in its own")
	_check(_zone_bar(hud, "heart_rate") == light, "heart rate in its zones' colours")
	_check(_zone_bar(hud, "cadence").a == 0.0, "no bar beside a figure without zones")
	hud.show_zones({"power": null, "heart_rate": 2})
	_check(_zone_bar(hud, "power").a == 0.0, "no bar without a zone")
	hud.show_layout(PackedStringArray(["power", "heart_rate"]))
	hud.show_zones({"power": 5, "heart_rate": 2})
	_check(_zone_bar(hud, "power").a == 0.0, "none beside the large figure")
	_check(_zone_bar(hud, "heart_rate") == light, "but under it")
	hud.free()
	var editor: HudEditor = HudEditor.new()
	root.add_child(editor)
	editor.edit(PackedStringArray(["speed", "power", "heart_rate"]), false)
	var preview: HudPanel = editor.find_children("*", "HudPanel", true, false)[0]
	for id: String in ["power", "heart_rate"]:
		_check(_zone_bar(preview, id).a > 0.0, "an example bar for " + id)
	editor.free()


## Calories keeps its short caption and kcal, with unavailable values and method details.
func _calories() -> void:
	var hud: HudPanel = HudPanel.new()
	root.add_child(hud)
	for imperial: bool in [false, true]:
		hud.imperial = imperial
		hud.show_layout(PackedStringArray(["calories", "work"]))
		var metric: Dictionary = hud.catalogue["calories"]
		_check(
			str(metric["caption"]) == "Calories" and str(metric["unit"]) == "kcal",
			"short calorie title and unit"
		)
		hud.show_values({"calories": null}, null, null)
		var values: Dictionary = hud.get("_values")
		var figure: Label = values["calories"]
		_check(figure.text == "--", "no estimate without enough information")
		hud.show_values(
			{"calories": 717.02, "work": 720.0}, null, null, {"calories": "Based on power"}
		)
		_check(figure.text == "717", "calories do not follow distance units")
		var tile: Control = figure.get_parent().get_parent()
		_check(tile.tooltip_text == "Based on power", "method in tooltip, not the title")
		var work: Label = values["work"]
		_check(work.text == "720", "mechanical work remains separate")
	hud.free()
	var editor: HudEditor = HudEditor.new()
	root.add_child(editor)
	editor.edit(PackedStringArray(["power"]), false)
	editor.place("calories", 1)
	_expect(editor.layout(), ["power", "calories"], "calories are selectable per rider")
	editor.free()


## The colour of the bar beside the figure `id`, transparent for none or the large figure.
func _zone_bar(hud: HudPanel, id: String) -> Color:
	var cells: Dictionary = hud.get("_cells")
	if not cells.has(id):
		return Color.TRANSPARENT
	var cell: Control = cells[id]
	var color: Color = cell.get("_zone")
	return color


func _available_chips(editor: HudEditor) -> Array[Node]:
	return editor.get_child(1).get_child(1).get_child(0).get_children()


## The rider's available figures scroll with the pointer over a figure or its + button.
func _rider_hud_scrolling() -> void:
	root.size = Vector2i(1024, 640)
	var scene: PackedScene = load("res://scenes/main.tscn")
	var main: Control = scene.instantiate()
	root.add_child(main)
	var dialog: ProfileDialog = main.find_children("*", "ProfileDialog", true, false)[0]
	dialog.edit({"id": "r", "name": "Rider"}, PackedStringArray(["power"]))
	var tabs: TabContainer = dialog.find_children("*", "TabContainer", true, false)[0]
	var editor: HudEditor = dialog.get("_hud")
	tabs.current_tab = editor.get_index()
	for frame: int in range(6):
		await process_frame
	var scroll: ScrollContainer = editor.get_child(1).get_child(1)
	var chip: Control = _available_chips(editor)[0]
	for control: Control in [chip, chip.get_child(0).get_child(1)]:
		scroll.scroll_vertical = 0
		await process_frame
		await process_frame
		var point: Vector2 = control.get_global_rect().get_center()
		_pointer_scroll(dialog, point, false)
		_check(
			scroll.scroll_vertical > 0,
			"wheel over an available HUD %s scrolls" % control.get_class()
		)
		scroll.scroll_vertical = 0
		_pointer_scroll(dialog, point, true)
		_check(
			scroll.scroll_vertical > 0,
			"trackpad gesture over an available HUD %s scrolls" % control.get_class()
		)
	var fixed_point: Vector2 = scroll.get_global_rect().position + Vector2(80, 60)
	for step: int in range(60):
		_pointer_scroll(dialog, fixed_point, false)
		await process_frame
	var last: Control = _available_chips(editor).back()
	_check(
		scroll.get_global_rect().encloses(last.get_global_rect()),
		"mouse wheel reaches the last available figure"
	)
	var add: Button = last.get_child(0).get_child(1)
	var motion: InputEventMouseMotion = InputEventMouseMotion.new()
	motion.position = Vector2(dialog.position) + add.get_global_rect().get_center()
	Input.parse_input_event(motion)
	Input.flush_buffered_events()
	var click: InputEventMouseButton = InputEventMouseButton.new()
	click.position = motion.position
	click.button_index = MOUSE_BUTTON_LEFT
	click.pressed = true
	click.button_mask = MOUSE_BUTTON_MASK_LEFT
	Input.parse_input_event(click)
	Input.flush_buffered_events()
	click = click.duplicate()
	click.pressed = false
	click.button_mask = 0
	Input.parse_input_event(click)
	Input.flush_buffered_events()
	_check(editor.layout().size() == 2, "a figure reached by scrolling can be added with +")
	main.free()


func _pointer_scroll(dialog: Window, point: Vector2, pan_gesture: bool) -> void:
	var motion: InputEventMouseMotion = InputEventMouseMotion.new()
	motion.position = point
	dialog.push_input(motion)
	if pan_gesture:
		var pan: InputEventPanGesture = InputEventPanGesture.new()
		pan.position = point
		pan.delta = Vector2(0, 3)
		dialog.push_input(pan)
	else:
		var wheel: InputEventMouseButton = InputEventMouseButton.new()
		wheel.position = point
		wheel.button_index = MOUSE_BUTTON_WHEEL_DOWN
		wheel.pressed = true
		dialog.push_input(wheel)
		wheel = wheel.duplicate()
		wheel.pressed = false
		dialog.push_input(wheel)


func _expect(actual: PackedStringArray, expected: Array, what: String) -> void:
	_check(actual == PackedStringArray(expected), "%s: %s" % [what, actual])


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("HUD SMOKE TEST FAILED: " + what)
		_failed = true


## The minimap draws its map once and only moves and turns it as the rider goes: a city's
## thousands of lines drawn anew every frame held whole courses to ten frames a second. Every
## point of the map still lies where the view puts it, close up and over the whole route.
func _minimap() -> void:
	var map: Minimap = Minimap.new()
	map.size = Vector2(300, 300)
	root.add_child(map)
	var line: Dictionary = {
		"points": PackedVector2Array([Vector2(-200, 50), Vector2(300, 400)]),
		"width": 2.0,
		"color": Color.WHITE,
	}
	var no_area: Dictionary = {"vertices": PackedVector2Array(), "colors": PackedColorArray()}
	map.set_map({"background": Color.BLACK, "land": no_area, "lines": [line], "corridor": no_area})
	map.set_track(PackedVector2Array([Vector2.ZERO, Vector2(800, 600), Vector2(1500, -200)]))
	var layer: Node2D = map.get("_map")
	var drawn: Array[int] = [0]
	layer.draw.connect(func() -> void: drawn[0] += 1)
	await process_frame
	var first: int = drawn[0]
	_check(first > 0, "the map is drawn")
	for step: int in range(5):
		map.set_rider(Vector2(step * 40.0, step * 25.0), step * 0.4)
		await process_frame
	_check(drawn[0] == first, "riding moves and turns the map without drawing it again")
	_check(_map_where_viewed(map), "close up, the map lies where the view puts it")
	map.set("_follow", false)
	map.call("_redraw_map")
	await process_frame
	_check(drawn[0] == first + 1, "the whole route is drawn once more, at its own scale")
	_check(_map_where_viewed(map), "over the whole route, the map lies where the view puts it")
	map.free()


## Whether the minimap's drawn map, placed by its layer, matches its view, and the layer only
## moves and turns it (so lines keep their widths in pixels).
func _map_where_viewed(map: Minimap) -> bool:
	var layer: Node2D = map.get("_map")
	var drawn_at: Transform2D = map.get("_map_scale")
	var view: Transform2D = map.call("_view")
	for point: Vector2 in [Vector2(-200, 50), Vector2(300, 400), Vector2(1500, -200)]:
		if (layer.transform * (drawn_at * point)).distance_to(view * point) > 0.01:
			return false
	return is_equal_approx(absf(layer.transform.determinant()), 1.0)
