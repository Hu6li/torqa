class_name WorkoutOptions
extends GridContainer
## What a workout asks of the rider (R56): a constant power, or a heart rate (a zone's middle or
## a bpm) held between a lowest and highest power. The same control serves the Workouts tab and
## the in-ride settings, so a workout is set the same way before and during it.

## The rider changed the workout; read it with `workout()`.
signal changed

enum Kind { POWER, ZONE, BPM }

const KINDS: Array[String] = ["power", "zone", "bpm"]

var _kind: OptionButton = OptionButton.new()
var _power: SpinBox = SpinBox.new()
var _zone: OptionButton = OptionButton.new()
var _bpm: SpinBox = SpinBox.new()
var _min_power: SpinBox = SpinBox.new()
var _max_power: SpinBox = SpinBox.new()
## Each row's controls and the kinds of workout it belongs to (empty: all).
var _rows: Array[Array] = []
## The FTP the starting values were chosen for; another rider gets their own.
var _ftp_w: float = -1.0


func _init() -> void:
	columns = 3
	add_theme_constant_override("h_separation", 24)
	add_theme_constant_override("v_separation", 8)
	# i18n-begin
	for kind: String in ["Constant power", "Heart-rate zone", "Heart rate"]:
		# i18n-end
		_kind.add_item(tr(kind))
	_watts(_power, 30.0)
	_watts(_min_power, 0.0)
	_watts(_max_power, 30.0)
	_bpm.min_value = 60.0
	_bpm.max_value = 220.0
	_bpm.suffix = "bpm"
	_kind.item_selected.connect(func(_index: int) -> void: _changed())
	_zone.item_selected.connect(func(_index: int) -> void: _changed())
	for spin: SpinBox in [_power, _bpm, _min_power, _max_power]:
		spin.value_changed.connect(func(_value: float) -> void: _changed())
	# i18n-begin
	_row("Workout", _kind, [])
	_row("Power", _power, [Kind.POWER])
	_row("Zone", _zone, [Kind.ZONE])
	_row("Heart rate", _bpm, [Kind.BPM])
	_row("Lowest power", _min_power, [Kind.ZONE, Kind.BPM])
	_row("Highest power", _max_power, [Kind.ZONE, Kind.BPM])
	# i18n-end
	_show_rows()


## Sets the rider's heart-rate zones to choose from (`TorqaApp.heart_rate_zones()`) and, for a
## rider not seen before, starting values that suit their `ftp_w`.
func configure(zones: PackedVector2Array, ftp_w: float) -> void:
	var selected: int = maxi(_zone.selected, 1)
	_zone.clear()
	for i: int in range(zones.size()):
		_zone.add_item(
			tr("Zone %d  ·  %d–%d bpm") % [i + 1, roundi(zones[i].x), roundi(zones[i].y)]
		)
	_zone.select(mini(selected, _zone.item_count - 1))
	if is_equal_approx(ftp_w, _ftp_w):
		return
	_ftp_w = ftp_w
	# Endurance pace, with room either way for the heart-rate hold.
	_power.set_value_no_signal(_round_watts(ftp_w * 0.7))
	_min_power.set_value_no_signal(_round_watts(ftp_w * 0.4))
	_max_power.set_value_no_signal(_round_watts(ftp_w * 0.9))
	if zones.size() >= 2:
		_bpm.set_value_no_signal(roundf((zones[1].x + zones[1].y) / 2.0))


## The workout as `TorqaApp.start_workout()` takes it, without its name.
func workout() -> Dictionary:
	return {
		"kind": KINDS[_kind.selected],
		"power_w": _power.value,
		"zone": _zone.selected + 1,
		"bpm": _bpm.value,
		"min_w": minf(_min_power.value, _max_power.value),
		"max_w": maxf(_min_power.value, _max_power.value),
	}


## Shows `workout` (as `workout()` returns it) without emitting `changed`.
func set_workout(workout: Dictionary) -> void:
	var kind: String = workout.get("kind", "power")
	_kind.select(maxi(KINDS.find(kind), 0))
	var zone: int = workout.get("zone", 2)
	if _zone.item_count > 0:
		_zone.select(clampi(zone - 1, 0, _zone.item_count - 1))
	for pair: Array in [
		[_power, "power_w"], [_bpm, "bpm"], [_min_power, "min_w"], [_max_power, "max_w"]
	]:
		var spin: SpinBox = pair[0]
		var key: String = pair[1]
		if workout.has(key):
			var value: float = workout[key]
			spin.set_value_no_signal(value)
	_show_rows()


## A name for the history in the interface language, e.g. "Heart-rate zone 3".
func title() -> String:
	match _kind.selected:
		Kind.ZONE:
			return tr("Heart-rate zone %d") % (_zone.selected + 1)
		Kind.BPM:
			return tr("Heart rate %d bpm") % roundi(_bpm.value)
		_:
			return tr("Constant power %d W") % roundi(_power.value)


func _changed() -> void:
	_show_rows()
	changed.emit()


func _show_rows() -> void:
	for row: Array in _rows:
		var kinds: Array[Kind] = row[1]
		for control: Control in row[0]:
			control.visible = kinds.is_empty() or _kind.selected in kinds


func _row(caption: String, field: Control, kinds: Array[Kind]) -> void:
	var label: Label = Label.new()
	label.text = caption
	label.custom_minimum_size = Vector2(RideOptions.CAPTION_WIDTH, 0)
	add_child(label)
	field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(field)
	var third: Control = Control.new()
	third.custom_minimum_size.x = RideOptions.EXTRA_WIDTH
	add_child(third)
	_rows.append([[label, field, third], kinds])


static func _watts(spin: SpinBox, low: float) -> void:
	spin.min_value = low
	spin.max_value = 1500.0
	spin.step = 5.0
	spin.suffix = "W"


static func _round_watts(watts: float) -> float:
	return roundf(watts / 5.0) * 5.0
