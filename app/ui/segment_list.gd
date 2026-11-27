class_name SegmentList
extends VBoxContainer
## The rider's own segments of a course (R300) on its page: each with its length, where it
## starts and the rider's best time on it, renamed or moved with the pencil and removed with the
## bin. A new one is marked by its start and end, typed in, dragged across or clicked on the
## profile, or taken
## from a GPX track of the stretch (one exported from a segment on a cycling site, say). Times
## stay with the rides: a segment removed and marked again at the same ends keeps its best.

## The segments changed (`OrqwaApp.segments()`): the profile marks them again.
signal changed(segments: Array)
## While a segment is marked, clicks on the profile set its start, then its end ([method pick]).
signal picking_changed(picking: bool)
## The stretch being marked changed, however it was set: the profile shows it.
signal draft_changed(start_m: float, end_m: float)

## As in the core: shorter is no stretch to time, longer names do not fit the ride's panel.
const MIN_LENGTH_M: float = 50.0
const MAX_NAME_CHARS: int = 60
const KM_PER_MILE: float = 1.609344

var _orqwa: OrqwaApp
var _length_m: float = 0.0
var _imperial: bool = false
var _rows: VBoxContainer = VBoxContainer.new()
var _actions: HBoxContainer = HBoxContainer.new()
var _form: VBoxContainer = VBoxContainer.new()
var _name: LineEdit = LineEdit.new()
var _start: SpinBox = SpinBox.new()
var _end: SpinBox = SpinBox.new()
var _save: Button = Button.new()
var _file_dialog: FileDialog = FileDialog.new()
var _refused: AcceptDialog = AcceptDialog.new()
## The segment the form changes; -1 for a new one.
var _editing: int = -1
## The next click on the profile sets the end rather than the start.
var _picks_end: bool = false


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


## Lists the loaded course's segments; `length_m` is the course's length.
func show_segments(length_m: float) -> void:
	_length_m = length_m
	_imperial = _orqwa.profile().get("units", "metric") == "imperial"
	_close_form()
	_refresh()


## Hides everything while another course loads.
func clear() -> void:
	_close_form()
	for child: Node in _rows.get_children():
		child.queue_free()
	hide()


## A click on the profile at `distance_m` while marking: the start, then the end.
func pick(distance_m: float) -> void:
	if not _form.visible:
		return
	var at: float = _from_meters(clampf(distance_m, 0.0, _length_m))
	if _picks_end:
		_end.value = at
	else:
		_start.value = at
		if _to_meters(_end.value) < distance_m + MIN_LENGTH_M:
			_end.value = _from_meters(minf(distance_m + MIN_LENGTH_M, _length_m))
	_picks_end = not _picks_end
	_check()


## A drag across the profile while marking: the stretch from `start_m` to `end_m` at once. The
## next click sets the start again.
func pick_stretch(start_m: float, end_m: float) -> void:
	if not _form.visible:
		return
	_start.value = _from_meters(clampf(start_m, 0.0, _length_m))
	_end.value = _from_meters(clampf(end_m, 0.0, _length_m))
	_picks_end = false
	_check()


func _init() -> void:
	add_theme_constant_override("separation", 8)
	hide()
	add_child(UiTheme.caption(tr("Segments")))
	_rows.add_theme_constant_override("separation", 4)
	add_child(_rows)
	_actions.add_theme_constant_override("separation", 12)
	var add: Button = UiTheme.chip_action(tr("Add segment"), "plus")
	add.pressed.connect(func() -> void: _open_form(-1, "", 0.0, minf(1000.0, _length_m)))
	_actions.add_child(add)
	var gpx: Button = UiTheme.chip_action(tr("From GPX…"), "plus")
	gpx.tooltip_text = tr("A GPX track of the stretch, such as a segment exported elsewhere")
	gpx.pressed.connect(_file_dialog.popup_centered)
	_actions.add_child(gpx)
	add_child(_actions)
	_build_form()
	add_child(_form)
	_file_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	_file_dialog.access = FileDialog.ACCESS_FILESYSTEM
	_file_dialog.filters = PickedFile.open_filters(PackedStringArray(["*.gpx ; GPX"]))
	_file_dialog.use_native_dialog = true
	_file_dialog.file_selected.connect(_on_file_selected)
	add_child(_file_dialog)
	add_child(_refused)


func _build_form() -> void:
	_form.add_theme_constant_override("separation", 8)
	_name.placeholder_text = tr("Segment name")
	_name.max_length = MAX_NAME_CHARS
	_name.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_name.text_changed.connect(func(_text: String) -> void: _check())
	_form.add_child(_name)
	var ends: HBoxContainer = HBoxContainer.new()
	ends.add_theme_constant_override("separation", 10)
	for pair: Array in [[tr("From"), _start], [tr("To"), _end]]:
		var label: Label = Label.new()
		label.text = pair[0]
		ends.add_child(label)
		var spin: SpinBox = pair[1]
		spin.step = 0.01
		spin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		spin.value_changed.connect(func(_value: float) -> void: _check())
		ends.add_child(spin)
	_form.add_child(ends)
	var hint: Label = Label.new()
	hint.text = tr("Or drag across the profile, then drag its ends or middle to adjust it.")
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.add_theme_font_size_override("font_size", 13)
	hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_form.add_child(hint)
	var buttons: HBoxContainer = HBoxContainer.new()
	buttons.add_theme_constant_override("separation", 10)
	_save.text = tr("Save")
	_save.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_save.pressed.connect(_on_save)
	buttons.add_child(_save)
	var cancel: Button = Button.new()
	cancel.text = tr("Cancel")
	cancel.pressed.connect(_close_form)
	buttons.add_child(cancel)
	_form.add_child(buttons)
	_form.hide()


func _refresh() -> void:
	for child: Node in _rows.get_children():
		child.queue_free()
	var segments: Array = _orqwa.segments()
	for segment: Dictionary in segments:
		_rows.add_child(_row(segment))
	show()
	changed.emit(segments)


func _row(segment: Dictionary) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	var title: Label = Label.new()
	title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	title.text = segment["name"]
	title.add_theme_color_override("font_color", UiTheme.SEGMENT_COLOR)
	title.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(title)
	var figures: Label = Label.new()
	figures.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	var start: float = segment["start_m"]
	var length: float = segment["length_m"]
	var parts: PackedStringArray = PackedStringArray(
		[
			MomentLines.distance(length, _imperial),
			tr("from %s") % MomentLines.distance(start, _imperial),
		]
	)
	if segment["best_s"] != null:
		var best: float = segment["best_s"]
		parts.append(tr("best %s") % UiTheme.duration(best))
	figures.text = "  ·  ".join(parts)
	figures.add_theme_color_override("font_color", UiTheme.MUTED)
	row.add_child(figures)
	var id: int = segment["id"]
	var edit: Button = _icon_button("pencil", tr("Rename or move the segment"))
	var segment_name: String = segment["name"]
	var end: float = segment["end_m"]
	edit.pressed.connect(func() -> void: _open_form(id, segment_name, start, end))
	row.add_child(edit)
	var remove: Button = _icon_button(
		"bin", tr("Remove the segment; its times stay with the rides")
	)
	remove.pressed.connect(func() -> void: _remove(id))
	UiTheme.danger_button(remove)
	row.add_child(remove)
	return row


func _icon_button(icon: String, tooltip: String) -> Button:
	var button: Button = Button.new()
	button.icon = UiIcons.texture(icon)
	button.tooltip_text = tooltip
	button.focus_mode = Control.FOCUS_NONE
	return button


func _open_form(id: int, segment_name: String, start_m: float, end_m: float) -> void:
	_editing = id
	for spin: SpinBox in [_start, _end]:
		spin.max_value = _from_meters(_length_m)
		spin.suffix = "mi" if _imperial else "km"
	_name.text = segment_name
	_start.value = _from_meters(start_m)
	_end.value = _from_meters(end_m)
	_picks_end = false
	_actions.hide()
	_form.show()
	_check()
	_name.grab_focus()
	picking_changed.emit(true)


func _close_form() -> void:
	var was_open: bool = _form.visible
	_form.hide()
	_actions.show()
	_editing = -1
	if was_open:
		picking_changed.emit(false)


## Save waits for a name and a stretch long enough to time.
func _check() -> void:
	draft_changed.emit(_to_meters(_start.value), _to_meters(_end.value))
	var length: float = _to_meters(_end.value) - _to_meters(_start.value)
	_save.disabled = _name.text.strip_edges().is_empty() or length < MIN_LENGTH_M
	_save.tooltip_text = (
		tr("A segment is at least %s long.") % MomentLines.distance(MIN_LENGTH_M, _imperial)
		if length < MIN_LENGTH_M
		else ""
	)


func _on_save() -> void:
	var start: float = _to_meters(_start.value)
	var end: float = minf(_to_meters(_end.value), _length_m)
	if _orqwa.save_segment(_editing, _name.text.strip_edges(), start, end):
		_close_form()
		_refresh()


func _remove(id: int) -> void:
	if _orqwa.remove_segment(id):
		_close_form()
		_refresh()


## A GPX track becomes a new segment in the form, for the rider to check and name before saving.
func _on_file_selected(picked: String) -> void:
	var refusal: String = PickedFile.refusal(picked, PackedStringArray(["gpx"]))
	if not refusal.is_empty():
		_refused.dialog_text = refusal
		_refused.popup_centered()
		return
	# A segment's track is small: copied on this thread, unlike a course.
	var path: String = PickedFile.local_path(picked)
	var found: Dictionary = _orqwa.segment_from_gpx(path)
	PickedFile.forget(path)
	if found.is_empty():
		return
	var found_name: String = found["name"]
	var start: float = found["start_m"]
	var end: float = found["end_m"]
	_open_form(-1, found_name, start, end)


func _from_meters(meters: float) -> float:
	return meters / 1000.0 / (KM_PER_MILE if _imperial else 1.0)


func _to_meters(value: float) -> float:
	return value * 1000.0 * (KM_PER_MILE if _imperial else 1.0)
