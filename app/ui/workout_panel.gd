class_name WorkoutPanel
extends PanelContainer
## What a workout asks for (R56): its kind, the power the trainer holds and, for a heart-rate
## hold, the heart rate it aims at next to the current one. Shown in the ride screen and in
## the overlay (R55).

var _title: Label = UiTheme.caption("")
var _target: Label = UiTheme.value(28)
var _heart: Label = Label.new()


func _init() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	rows.add_child(_title)
	rows.add_child(_target)
	_heart.add_theme_font_size_override("font_size", 14)
	_heart.add_theme_color_override("font_color", UiTheme.MUTED)
	_heart.hide()
	rows.add_child(_heart)
	add_child(rows)


## Shows the kind of `workout` (`WorkoutsTab.workout()`) as the title.
func show_workout(workout: Dictionary) -> void:
	var kind: String = workout.get("kind", "power")
	var title: String = tr("Constant power")
	if kind == "zone":
		var zone: int = workout.get("zone", 1)
		title = tr("Heart-rate zone %d") % zone
	elif kind == "bpm":
		title = tr("Heart rate")
	_title.text = title.to_upper()


## Shows the targets (`ride_state()["workout"]`) and the `heart_rate` now (or null); returns
## the heart rate held, or 0 at constant power.
func show_state(workout: Variant, heart_rate: Variant) -> float:
	if workout == null:
		return 0.0
	var info: Dictionary = workout
	var target: float = info["target_power_w"]
	_target.text = "%d W" % roundi(target)
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
