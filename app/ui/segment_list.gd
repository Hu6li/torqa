class_name SegmentList
extends VBoxContainer
## The rider's own segments of a course (R302) on its page: each with its length, where it
## starts and the rider's best time on it, renamed or moved with the pencil and removed with the
## bin. A new one is marked by its start and end, typed in, dragged across or clicked on the
## profile, or taken from a GPX track of the stretch (one exported from a segment on a cycling
## site, say). Times stay with the rides: a segment removed and marked again at the same ends
## keeps its best. Once the course is prepared differently (ADR 0044), a segment asks the rider
## to check its ends, and is not timed until they save it.

## The segments to draw changed ([method drawn] of `OrqwaApp.segments()`): the profile and the
## map mark them again.
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
var _check_note: Label = Label.new()
## The segment the form changes; empty for a new one.
var _editing: String = ""
## The next click on the profile sets the end rather than the start.
var _picks_end: bool = false
## The ends being marked, in metres: the fields round them to what they show, a hundredth of a
## mile or kilometre, which would move a segment on every save.
var _start_m: float = 0.0
var _end_m: float = 0.0


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
	var at: float = clampf(distance_m, 0.0, _length_m)
	if _picks_end:
		_set_ends(_start_m, at)
	else:
		_set_ends(at, maxf(_end_m, minf(at + MIN_LENGTH_M, _length_m)))
	_picks_end = not _picks_end
	_check()


## How the profile marks a segment, under the form and as the profile's tooltip.
func profile_hint() -> String:
	return (
		"%s %s"
		% [
			tr("Drag across the profile to mark a segment, or click its start and end."),
			tr("Drag either end or the middle to adjust it."),
		]
	)


## A drag across the profile while marking: the stretch from `start_m` to `end_m` at once. The
## next click sets the start again.
func pick_stretch(start_m: float, end_m: float) -> void:
	if not _form.visible:
		return
	_set_ends(clampf(start_m, 0.0, _length_m), clampf(end_m, 0.0, _length_m))
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
	add.pressed.connect(func() -> void: _open_form("", "", 0.0, minf(1000.0, _length_m)))
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
		ends.add_child(spin)
	_form.add_child(ends)
	# Typed in, the field's value is what the rider means.
	_start.value_changed.connect(
		func(value: float) -> void:
			_start_m = _to_meters(value)
			_check()
	)
	_end.value_changed.connect(
		func(value: float) -> void:
			_end_m = _to_meters(value)
			_check()
	)
	var hint: Label = Label.new()
	hint.text = profile_hint()
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.add_theme_font_size_override("font_size", 13)
	hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_form.add_child(hint)
	_check_note.text = (
		"%s %s %s"
		% [
			tr("The course is prepared differently now."),
			tr("Check that the segment starts and ends where it did, then save."),
			tr("It is timed again, and its times so far still count."),
		]
	)
	_check_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_check_note.add_theme_font_size_override("font_size", 13)
	_check_note.add_theme_color_override("font_color", UiTheme.WARNING)
	_check_note.hide()
	_form.add_child(_check_note)
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


## Of `segments` (`OrqwaApp.segments()`), those the profiles and the maps draw: not those whose
## ends the rider is to check (ADR 0044). They are not timed, and may lie elsewhere on the course
## as prepared now. The stretch being checked shows on the profile while its form is open.
static func drawn(segments: Array) -> Array:
	return segments.filter(
		func(segment: Dictionary) -> bool: return segment.get("confirmed", false)
	)


func _refresh() -> void:
	_list(_orqwa.segments())


## Lists every one of `segments`, those to check too; only those [method drawn] are marked.
func _list(segments: Array) -> void:
	for child: Node in _rows.get_children():
		child.queue_free()
	for segment: Dictionary in segments:
		_rows.add_child(_row(segment))
	show()
	changed.emit(drawn(segments))


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
	var id: String = segment["id"]
	var segment_name: String = segment["name"]
	var end: float = segment["end_m"]
	var confirmed: bool = segment.get("confirmed", true)
	var open: Callable = func() -> void: _open_form(id, segment_name, start, end, not confirmed)
	if not confirmed:
		# The course came out differently: the ends may lie elsewhere until the rider checks them.
		var check: Button = UiTheme.chip_action(tr("Check ends"), "pencil")
		check.tooltip_text = (
			"%s %s"
			% [tr("The course is prepared differently now."), tr("Not timed until checked.")]
		)
		check.add_theme_color_override("font_color", UiTheme.WARNING)
		check.pressed.connect(open)
		row.add_child(check)
	var edit: Button = _icon_button("pencil", tr("Rename or move the segment"))
	edit.pressed.connect(open)
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


func _open_form(
	id: String, segment_name: String, start_m: float, end_m: float, checking: bool = false
) -> void:
	_editing = id
	_check_note.visible = checking
	for spin: SpinBox in [_start, _end]:
		spin.max_value = _from_meters(_length_m)
		spin.suffix = "mi" if _imperial else "km"
	_name.text = segment_name
	_set_ends(start_m, end_m)
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
	_editing = ""
	if was_open:
		picking_changed.emit(false)


## Save waits for a name and a stretch long enough to time.
func _check() -> void:
	draft_changed.emit(_start_m, _end_m)
	var length: float = _end_m - _start_m
	_save.disabled = _name.text.strip_edges().is_empty() or length < MIN_LENGTH_M
	_save.tooltip_text = (
		tr("A segment is at least %s long.") % MomentLines.distance(MIN_LENGTH_M, _imperial)
		if length < MIN_LENGTH_M
		else ""
	)


func _on_save() -> void:
	var end: float = minf(_end_m, _length_m)
	if _orqwa.save_segment(_editing, _name.text.strip_edges(), _start_m, end):
		_close_form()
		_refresh()


func _remove(id: String) -> void:
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
	_open_form("", found_name, start, end)


## Shows the ends without taking the fields' rounding back.
func _set_ends(start_m: float, end_m: float) -> void:
	_start_m = start_m
	_end_m = end_m
	_start.set_value_no_signal(_from_meters(start_m))
	_end.set_value_no_signal(_from_meters(end_m))


func _from_meters(meters: float) -> float:
	return meters / 1000.0 / (KM_PER_MILE if _imperial else 1.0)


func _to_meters(value: float) -> float:
	return value * 1000.0 * (KM_PER_MILE if _imperial else 1.0)
