class_name WorkoutEditor
extends ConfirmationDialog
## The workout editor (R21): a structured workout's name, description and steps — steady,
## ramp or free, each with its duration, power in percent of FTP, cadence and a message — with
## a block of intervals added at once and the result drawn as it will be ridden. Saved as a
## ZWO file in the library; a built-in workout is saved as a copy.

## The workout was saved as `id` (`TorqaApp.workouts()`).
signal saved(id: String)
## The workout's file was deleted from the library.
signal deleted

enum Kind { STEADY, RAMP, FREE }

const ROW_SEPARATION: int = 8

var _torqa: TorqaApp
## The library file being edited, replaced on saving; "" for a new workout or a built-in.
var _replace: String = ""
var _ftp_w: float = 200.0
var _name_edit: LineEdit = LineEdit.new()
var _description: LineEdit = LineEdit.new()
var _rows: VBoxContainer = VBoxContainer.new()
var _chart: WorkoutChart = WorkoutChart.new()
var _total: Label = Label.new()
var _repeat: SpinBox = SpinBox.new()
var _on_time: LineEdit = LineEdit.new()
var _on_power: SpinBox = SpinBox.new()
var _off_time: LineEdit = LineEdit.new()
var _off_power: SpinBox = SpinBox.new()
var _confirm_delete: ConfirmationDialog = ConfirmationDialog.new()
var _delete_button: Button


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa


## Opens the editor on `plan` (a `TorqaApp.workouts()` entry), or on a new workout if empty,
## for a rider with `ftp_w`.
func edit(plan: Dictionary, ftp_w: float) -> void:
	_ftp_w = ftp_w
	var builtin: bool = plan.get("builtin", false)
	_replace = "" if plan.is_empty() or builtin else plan["id"]
	var plan_name: String = plan.get("name", "")
	if builtin:
		plan_name = tr("%s (copy)") % plan_name
	_name_edit.text = plan_name
	_description.text = plan.get("description", "")
	for row: Node in _rows.get_children():
		_rows.remove_child(row)
		row.queue_free()
	var steps: Array = plan.get("steps", [])
	for step: Dictionary in steps:
		_add_row(step)
	if steps.is_empty():
		_add_row({"duration_s": 600.0, "from_pct": 50.0, "to_pct": 75.0})
		_add_row({"duration_s": 1200.0, "from_pct": 75.0, "to_pct": 75.0})
		_add_row({"duration_s": 300.0, "from_pct": 60.0, "to_pct": 40.0})
	get_cancel_button().text = tr("Cancel")
	_delete_button.visible = not _replace.is_empty()
	_update_preview()
	popup_centered(Vector2i(1100, 680))


## The workout as `TorqaApp.save_workout()` takes it.
func workout() -> Dictionary:
	var steps: Array = []
	for row: Node in _rows.get_children():
		var step: _Step = row as _Step
		steps.append(step.value())
	var workout_name: String = _name_edit.text.strip_edges()
	return {
		"name": workout_name if not workout_name.is_empty() else tr("Workout"),
		"description": _description.text.strip_edges(),
		"steps": steps,
	}


## Adds `repeat` times a step of `on` seconds at `on_pct` of FTP and one of `off` seconds at
## `off_pct`.
func add_intervals(repeat: int, on: float, on_pct: float, off: float, off_pct: float) -> void:
	for i: int in range(repeat):
		_add_row({"duration_s": on, "from_pct": on_pct, "to_pct": on_pct})
		_add_row({"duration_s": off, "from_pct": off_pct, "to_pct": off_pct})
	_update_preview()


## Seconds as "m:ss".
static func clock(seconds: float) -> String:
	var whole: int = roundi(seconds)
	return "%d:%02d" % [whole / 60, whole % 60]


## "m:ss" (or plain seconds) as seconds; negative if it is not a time.
static func seconds_of(text: String) -> float:
	var parts: PackedStringArray = text.strip_edges().split(":")
	if parts.size() == 1 and parts[0].is_valid_int():
		return float(parts[0].to_int())
	if parts.size() == 2 and parts[0].is_valid_int() and parts[1].is_valid_int():
		var minutes: int = parts[0].to_int()
		var seconds: int = parts[1].to_int()
		if minutes >= 0 and seconds >= 0 and seconds < 60:
			return float(minutes * 60 + seconds)
	return -1.0


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Workout editor")
	ok_button_text = tr("Save")
	min_size = Vector2i(980, 560)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	var fields: GridContainer = GridContainer.new()
	fields.columns = 2
	fields.add_theme_constant_override("h_separation", 24)
	fields.add_theme_constant_override("v_separation", 8)
	# i18n-begin
	for field: Array in [["Name", _name_edit], ["Description", _description]]:
		# i18n-end
		var label: Label = Label.new()
		var caption: String = field[0]
		label.text = caption
		fields.add_child(label)
		var edit_field: LineEdit = field[1]
		edit_field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		fields.add_child(edit_field)
	rows.add_child(fields)

	rows.add_child(_Step.header())
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_rows.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_rows.add_theme_constant_override("separation", 4)
	scroll.add_child(_rows)
	rows.add_child(scroll)

	var adding: HBoxContainer = HBoxContainer.new()
	adding.add_theme_constant_override("separation", ROW_SEPARATION)
	var add_step: Button = Button.new()
	add_step.text = tr("+ Step")
	add_step.pressed.connect(
		func() -> void:
			_add_row({"duration_s": 300.0, "from_pct": 60.0, "to_pct": 60.0})
			_update_preview()
	)
	adding.add_child(add_step)
	adding.add_child(UiTheme.caption(tr("Intervals")))
	_repeat.min_value = 1.0
	_repeat.max_value = 50.0
	_repeat.value = 5.0
	_repeat.suffix = "×"
	adding.add_child(_repeat)
	for pair: Array in [
		[_on_time, "1:00", _on_power, 120.0], [_off_time, "1:00", _off_power, 50.0]
	]:
		var time: LineEdit = pair[0]
		var default_time: String = pair[1]
		time.text = default_time
		time.custom_minimum_size.x = 70.0
		adding.add_child(time)
		var power: SpinBox = pair[2]
		var default_power: float = pair[3]
		_percent(power)
		power.value = default_power
		adding.add_child(power)
	var add_intervals_button: Button = Button.new()
	add_intervals_button.text = tr("Add")
	add_intervals_button.pressed.connect(_on_add_intervals)
	adding.add_child(add_intervals_button)
	rows.add_child(adding)

	_chart.custom_minimum_size = Vector2(0, 110)
	rows.add_child(_chart)
	_total.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(_total)
	add_child(rows)

	_delete_button = add_button(tr("Delete"), true, "delete")
	custom_action.connect(_on_action)
	confirmed.connect(_save)
	_confirm_delete.theme = theme
	_confirm_delete.title = tr("Delete workout?")
	_confirm_delete.dialog_text = tr("Its file is deleted from your workouts.")
	_confirm_delete.ok_button_text = tr("Delete")
	_confirm_delete.confirmed.connect(_delete)
	add_child(_confirm_delete)


func _add_row(step: Dictionary) -> void:
	var row: _Step = _Step.new(step)
	row.changed.connect(_update_preview)
	row.move_requested.connect(_move.bind(row))
	row.remove_requested.connect(
		func() -> void:
			_rows.remove_child(row)
			row.queue_free()
			_update_preview()
	)
	_rows.add_child(row)


func _move(by: int, row: _Step) -> void:
	_rows.move_child(row, clampi(row.get_index() + by, 0, _rows.get_child_count() - 1))
	_update_preview()


func _on_add_intervals() -> void:
	var on: float = seconds_of(_on_time.text)
	var off: float = seconds_of(_off_time.text)
	if on <= 0.0 or off < 0.0:
		return
	add_intervals(roundi(_repeat.value), on, _on_power.value, off, _off_power.value)


## The chart and total time of the steps as they are now.
func _update_preview() -> void:
	var steps: Array = []
	var total: float = 0.0
	for row: Node in _rows.get_children():
		var step: Dictionary = (row as _Step).value()
		var duration: float = step["duration_s"]
		total += duration
		var free: bool = step["free"]
		var from_pct: float = step["from_pct"]
		var to_pct: float = step["to_pct"]
		(
			steps
			. append(
				{
					"duration_s": duration,
					"from_w": null if free else from_pct / 100.0 * _ftp_w,
					"to_w": null if free else to_pct / 100.0 * _ftp_w,
				}
			)
		)
	_chart.set_steps(steps, _ftp_w)
	_total.text = tr("%d steps  ·  %s") % [steps.size(), clock(total)]
	get_ok_button().disabled = steps.is_empty()


func _save() -> void:
	var id: String = _torqa.save_workout(workout(), _replace)
	if not id.is_empty():
		saved.emit(id)


func _on_action(action: StringName) -> void:
	if action == &"delete":
		_confirm_delete.popup_centered()


func _delete() -> void:
	if _torqa.delete_workout(_replace):
		hide()
		deleted.emit()


static func _percent(spin: SpinBox) -> void:
	spin.min_value = 0.0
	spin.max_value = 400.0
	spin.step = 1.0
	spin.suffix = "%"


## One step: kind, duration, power (from and, for a ramp, to), cadence and message.
class _Step:
	extends HBoxContainer

	signal changed
	signal move_requested(by: int)
	signal remove_requested

	const WIDTHS: Array[float] = [130.0, 80.0, 100.0, 100.0, 125.0, 0.0]

	var _kind: OptionButton = OptionButton.new()
	var _time: LineEdit = LineEdit.new()
	var _from: SpinBox = SpinBox.new()
	var _to: SpinBox = SpinBox.new()
	var _cadence: SpinBox = SpinBox.new()
	var _message: LineEdit = LineEdit.new()
	var _seconds: float = 300.0

	func _init(step: Dictionary) -> void:
		add_theme_constant_override("separation", ROW_SEPARATION)
		# i18n-begin
		for kind: String in ["Steady", "Ramp", "Free ride"]:
			# i18n-end
			_kind.add_item(tr(kind))
		_seconds = step.get("duration_s", 300.0)
		_time.text = WorkoutEditor.clock(_seconds)
		# Free steps have no power: `free` from the editor, `null` from the library.
		var free: bool = (
			step.get("free", false) or (step.has("from_pct") and step["from_pct"] == null)
		)
		var from_pct: float = 0.0 if step.get("from_pct") == null else step["from_pct"]
		var to_pct: float = from_pct if step.get("to_pct") == null else step["to_pct"]
		for spin: SpinBox in [_from, _to]:
			WorkoutEditor._percent(spin)
		_from.value = roundf(from_pct)
		_to.value = roundf(to_pct)
		_cadence.max_value = 150.0
		_cadence.suffix = "rpm"
		_cadence.tooltip_text = tr("The cadence to keep; 0 for none")
		_cadence.value = 0.0 if step.get("cadence") == null else step["cadence"]
		_message.text = step.get("message", "")
		_message.placeholder_text = tr("Message at the start of the step")
		var kind: int = Kind.FREE if free else (Kind.STEADY if from_pct == to_pct else Kind.RAMP)
		_kind.select(kind)
		var fields: Array[Control] = [_kind, _time, _from, _to, _cadence, _message]
		for i: int in range(fields.size()):
			var field: Control = fields[i]
			field.custom_minimum_size.x = WIDTHS[i]
			add_child(field)
		_message.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		# i18n-begin
		for action: Array in [["↑", -1, "Move up"], ["↓", 1, "Move down"]]:
			# i18n-end
			var move: Button = Button.new()
			var label: String = action[0]
			var by: int = action[1]
			var tip: String = action[2]
			move.text = label
			move.tooltip_text = tr(tip)
			move.pressed.connect(func() -> void: move_requested.emit(by))
			add_child(move)
		var remove: Button = Button.new()
		remove.text = "✕"
		remove.tooltip_text = tr("Remove the step")
		remove.pressed.connect(func() -> void: remove_requested.emit())
		add_child(remove)
		_kind.item_selected.connect(func(_index: int) -> void: _on_kind())
		_time.text_submitted.connect(func(_text: String) -> void: _on_time())
		_time.focus_exited.connect(_on_time)
		for spin: SpinBox in [_from, _to, _cadence]:
			spin.value_changed.connect(func(_value: float) -> void: _on_power())
		_show_kind()

	## The column captions, aligned with the steps' fields.
	static func header() -> HBoxContainer:
		var row: HBoxContainer = HBoxContainer.new()
		row.add_theme_constant_override("separation", ROW_SEPARATION)
		# i18n-begin
		var captions: Array[String] = ["Step", "Duration", "Power", "Ramp to", "Cadence", "Message"]
		# i18n-end
		for i: int in range(captions.size()):
			var label: Label = UiTheme.caption(captions[i])
			label.custom_minimum_size.x = WIDTHS[i]
			row.add_child(label)
		return row

	## The step as `TorqaApp.save_workout()` takes it.
	func value() -> Dictionary:
		var free: bool = _kind.selected == Kind.FREE
		var ramp: bool = _kind.selected == Kind.RAMP
		return {
			"duration_s": _seconds,
			"free": free,
			"from_pct": _from.value,
			"to_pct": _to.value if ramp else _from.value,
			"cadence": _cadence.value,
			"message": _message.text.strip_edges(),
		}

	func _on_kind() -> void:
		_show_kind()
		changed.emit()

	func _on_time() -> void:
		var seconds: float = WorkoutEditor.seconds_of(_time.text)
		if seconds > 0.0:
			_seconds = seconds
		_time.text = WorkoutEditor.clock(_seconds)
		changed.emit()

	func _on_power() -> void:
		changed.emit()

	func _show_kind() -> void:
		var kind: int = _kind.selected
		_from.editable = kind != Kind.FREE
		_to.editable = kind == Kind.RAMP
		_from.modulate.a = 0.4 if kind == Kind.FREE else 1.0
		_to.modulate.a = 1.0 if kind == Kind.RAMP else 0.4
