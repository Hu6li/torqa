class_name WorkoutPanel
extends PanelContainer
## What a workout asks for (R56, R21): its name, the power the trainer holds and, for a
## heart-rate hold, the heart rate it aims at next to the current one; for a structured
## workout the step under way, the next one and its messages. Shown in the ride screen and in
## the overlay (R55).

var _title: Label = UiTheme.caption("")
var _target: Label = UiTheme.value(28)
var _heart: Label = Label.new()
var _step: Label = Label.new()
var _cue: Label = Label.new()


func _init() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	rows.add_child(_title)
	rows.add_child(_target)
	for label: Label in [_heart, _step]:
		label.add_theme_font_size_override("font_size", 14)
		label.add_theme_color_override("font_color", UiTheme.MUTED)
		label.hide()
		rows.add_child(label)
	_cue.add_theme_font_size_override("font_size", 15)
	_cue.add_theme_color_override("font_color", UiTheme.ACCENT)
	_cue.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_cue.custom_minimum_size.x = 220.0
	_cue.hide()
	rows.add_child(_cue)
	add_child(rows)


## Shows the kind of `workout` (`WorkoutsTab.workout()`) as the title; a structured workout's
## name.
func show_workout(workout: Dictionary) -> void:
	var kind: String = workout.get("kind", "power")
	var title: String = tr("Constant power")
	if kind == "zone":
		var zone: int = workout.get("zone", 1)
		title = tr("Heart-rate zone %d") % zone
	elif kind == "bpm":
		title = tr("Heart rate")
	elif kind == "plan":
		var plan: Dictionary = workout.get("plan", {})
		title = plan.get("name", tr("Structured workout"))
	_title.text = title.to_upper()


## Shows the targets (`ride_state()["workout"]`) and the `heart_rate` now (or null); returns
## the heart rate held, or 0 if none.
func show_state(workout: Variant, heart_rate: Variant) -> float:
	if workout == null:
		return 0.0
	var info: Dictionary = workout
	if info["target_power_w"] == null:
		_target.text = tr("Free ride")
	else:
		var target: float = info["target_power_w"]
		_target.text = "%d W" % roundi(target)
	_show_progress(info["progress"])
	if info["target_heart_rate"] == null:
		_heart.hide()
		return 0.0
	var target_bpm: float = info["target_heart_rate"]
	var now: String = "--"
	if heart_rate != null:
		var bpm: float = heart_rate
		now = str(roundi(bpm))
	_heart.text = tr("for %d bpm  ·  now %s") % [roundi(target_bpm), now]
	_heart.show()
	return target_bpm


## A structured workout's step, what comes next and its message, from `progress` (see
## `TorqaApp.ride_state()`), or nothing.
func _show_progress(progress: Variant) -> void:
	if progress == null:
		_step.hide()
		_cue.hide()
		return
	var info: Dictionary = progress
	var step: int = info["step"]
	var steps: int = info["steps"]
	var step_left: float = info["step_left_s"]
	var line: String = (
		tr("Step %d of %d  ·  %s left") % [step + 1, steps, UiTheme.duration(step_left)]
	)
	if info["cadence"] != null:
		var cadence: float = info["cadence"]
		line += "  ·  %d rpm" % roundi(cadence)
	if info["next"] != null:
		var next: Dictionary = info["next"]
		var next_s: float = next["duration_s"]
		if next["power_w"] == null:
			line += "\n" + tr("Next: free ride for %s") % UiTheme.duration(next_s)
		else:
			var next_w: float = next["power_w"]
			line += "\n" + tr("Next: %d W for %s") % [roundi(next_w), UiTheme.duration(next_s)]
	_step.text = line
	_step.show()
	var cue: String = info["cue"]
	_cue.text = cue
	_cue.visible = not cue.is_empty()
