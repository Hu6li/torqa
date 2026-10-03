extends SceneTree
## Headless checks of UI behaviour that needs no ride: the HUD editor (R51).
## Run: godot --headless --path app -s res://tests/ui_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_hud_editor()
	_ambience()
	# Freed players release their playbacks on the audio server a frame later.
	for i: int in range(3):
		await process_frame
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

	for id: String in ["speed", "heart_rate"]:
		editor.remove(id)
	_expect(editor.layout(), ["heart_rate"], "the last figure stays")
	for i: int in range(20):
		editor.place(str(TorqaApp.hud_metrics()[i]["id"]), 99)
	_check(editor.layout().size() == TorqaApp.hud_max_metrics(), "at most the maximum figures")
	_check(not changes.is_empty(), "changes are reported")
	editor.free()


func _ambience() -> void:
	var ambience: Ambience = Ambience.new()
	root.add_child(ambience)
	var players: Array[Node] = ambience.get_children()
	var wind: AudioStreamPlayer = players[0]
	var rain: AudioStreamPlayer = players[2]
	var stream: AudioStreamWAV = wind.stream
	_check(stream.data.size() == Ambience.MIX_RATE * 2 * 2, "two seconds of 16-bit noise")

	ambience.set_conditions("Midday", "Clear")
	for i: int in range(60):
		ambience.update(0.05, 5.0, {"forest": 1.0, "water": 0.0, "town": 0.0})
	var slow: float = wind.volume_db
	for i: int in range(60):
		ambience.update(0.05, 45.0, {"forest": 1.0, "water": 0.0, "town": 0.0})
	_check(wind.volume_db > slow + 6.0, "wind grows with speed: %s → %s" % [slow, wind.volume_db])
	_check(rain.volume_db < -40.0, "no rain in clear weather: %s" % rain.volume_db)
	ambience.set_conditions("Midday", "Rain")
	for i: int in range(60):
		ambience.update(0.05, 45.0, null)
	_check(rain.volume_db > -12.0, "rain when raining: %s" % rain.volume_db)
	ambience.silence()
	_check(not wind.playing and wind.volume_db <= -80.0, "silenced")

	# For listening: the synthesised sounds as WAV files next to the screenshots, if asked for.
	var out_dir: String = OS.get_environment("AMBIENCE_DIR")
	if not out_dir.is_empty():
		for i: int in range(players.size()):
			var player: AudioStreamPlayer = players[i]
			if player.stream is AudioStreamWAV:
				(player.stream as AudioStreamWAV).save_to_wav(
					out_dir.path_join("ambience-%s.wav" % player.bus.trim_prefix("Ambience"))
				)
	ambience.free()


## The chips of a list column of the editor: 0 = shown, 2 = available.
func _chips(editor: HudEditor, column: int) -> Array[Node]:
	return editor.get_child(column).get_child(1).get_child(0).get_children()


func _expect(actual: PackedStringArray, expected: Array, what: String) -> void:
	_check(actual == PackedStringArray(expected), "%s: %s" % [what, actual])


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("UI SMOKE TEST FAILED: " + what)
		_failed = true
