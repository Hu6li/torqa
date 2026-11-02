class_name RideSummaryDialog
extends AcceptDialog
## A quick summary over the finished ride (R113), with a line on its place in the rider's week
## (R124); charts and uploads stay in its details.

signal closed
signal details_requested(path: String)

const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048

var _orqwa: OrqwaApp
var _path: String = ""
var _name: String = ""
var _title: EditableTitle = EditableTitle.new(tr("Rename the ride"))
var _stats: ReflowGrid = ReflowGrid.new()
var _power_records: PowerRecordBadges = PowerRecordBadges.new()
var _assisted: Label = Label.new()
var _recap: RideRecap = RideRecap.new()
var _confirm_discard: ConfirmationDialog = ConfirmationDialog.new()


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Ride summary")
	ok_button_text = tr("Done")
	min_size = Vector2i(560, 300)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 24)
	rows.add_child(_title)
	_assisted.text = tr("Assisted group ride · speed records excluded")
	_assisted.add_theme_color_override("font_color", UiTheme.MUTED)
	_assisted.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_assisted.custom_minimum_size.x = 480
	rows.add_child(_assisted)
	_title.edit_finished.connect(_rename)
	rows.add_child(_recap)
	_stats.max_columns = 3
	_stats.add_theme_constant_override("h_separation", 32)
	_stats.add_theme_constant_override("v_separation", 20)
	rows.add_child(_stats)
	rows.add_child(_power_records)
	add_child(rows)
	var discard: Button = add_button(tr("Discard ride"), true, "discard")
	UiTheme.danger_button(discard)
	add_button(tr("View details"), false, "details")
	custom_action.connect(_on_action)
	confirmed.connect(_keep)
	canceled.connect(_cancel)
	_confirm_discard.theme = theme
	_confirm_discard.title = tr("Discard ride?")
	_confirm_discard.dialog_text = tr("The ride and its FIT file are deleted.")
	_confirm_discard.ok_button_text = tr("Discard ride")
	UiTheme.danger_button(_confirm_discard.get_ok_button())
	_confirm_discard.confirmed.connect(_discard)
	add_child(_confirm_discard)


## Opens the saved ride in the rider's units, omitting sensor figures it did not record.
func open(path: String) -> void:
	_path = path
	for ride: Dictionary in _orqwa.history():
		if ride["path"] != path:
			continue
		_show_ride(ride)
		return


func _show_ride(ride: Dictionary) -> void:
	var ride_name: String = ride["name"]
	_name = ride_name
	_title.text = ride_name
	var start: int = ride["start_unix_s"]
	_title.placeholder_text = (
		"%s · %s" % [ride["route"], UiTheme.date(start).get_slice(",", 0).rsplit(" ", true, 1)[0]]
	)
	_recap.show_ride(_orqwa, _path)
	_fill_stats(ride)
	_power_records.show_ride(ride)
	_assisted.visible = ride.get("assisted", false)
	var height: int = 420 if _power_records.visible else 340
	UiScale.popup(
		self,
		Vector2i(640, height + (40 if _recap.visible else 0) + (40 if _assisted.visible else 0))
	)
	get_ok_button().grab_focus()


func _fill_stats(ride: Dictionary) -> void:
	for child: Node in _stats.get_children():
		_stats.remove_child(child)
		child.queue_free()
	var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
	var distance_km: float = ride["distance_m"] / 1000.0
	var gain_m: float = ride["elevation_gain_m"]
	var elapsed_s: float = ride["elapsed_s"]
	_stat(tr("Time"), UiTheme.duration(elapsed_s))
	_stat(
		tr("Distance"),
		("%.1f mi" % (distance_km / KM_PER_MILE)) if imperial else "%.1f km" % distance_km
	)
	_stat(
		tr("Climbing"),
		("%d ft" % roundi(gain_m / METERS_PER_FOOT)) if imperial else "%d m" % roundi(gain_m)
	)
	if ride["avg_power"] != null:
		var watts: float = ride["avg_power"]
		_stat(tr("Avg power"), "%d W" % roundi(watts))
	if ride["avg_heart_rate"] != null:
		var bpm: float = ride["avg_heart_rate"]
		_stat(tr("Avg heart rate"), "%d bpm" % roundi(bpm))
	if ride["avg_cadence"] != null:
		var rpm: float = ride["avg_cadence"]
		_stat(tr("Avg cadence"), "%d rpm" % roundi(rpm))


func _stat(caption: String, text: String) -> void:
	var cell: VBoxContainer = VBoxContainer.new()
	cell.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	cell.add_child(UiTheme.caption(caption))
	var value: Label = UiTheme.value(28)
	value.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	value.text = text
	cell.add_child(value)
	_stats.add_child(cell)


func _rename() -> void:
	var new_name: String = _title.text.strip_edges()
	if new_name != _name and _orqwa.rename_ride(_path, new_name):
		_name = new_name


func _on_action(action: StringName) -> void:
	if action == &"discard":
		_confirm_discard.popup_centered()
	elif action == &"details":
		_rename()
		hide()
		details_requested.emit(_path)


func _keep() -> void:
	_rename()
	var shown: String = _name if not _name.is_empty() else _title.placeholder_text
	_orqwa.keep_ride(_path, shown)
	_close()


func _discard() -> void:
	if _orqwa.delete_ride(_path):
		_close()


func _close() -> void:
	hide()
	closed.emit()


func _cancel() -> void:
	_rename()
	_close()
