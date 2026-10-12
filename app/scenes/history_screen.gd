class_name HistoryScreen
extends Control
## The rider's past rides (R31): a list on the left, the selected ride's figures, chart, time
## in zones and power curve over the rider's best (R62) on the right, its power records marked.
## Rides have names (R50), edited in place, their FIT files can be saved anywhere on the
## computer (R28) and they go up to the rider's training platforms (R29). Right after a ride the
## same screen is its summary (R42): that ride only, to name, keep, upload or discard.

signal closed

const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048
const CHART_POINTS: int = 600

## Inside the start page's tabs: no Back button, no own background.
var embedded: bool = false:
	set(value):
		embedded = value
		if is_node_ready():
			_apply_embedded()

var _torqa: TorqaApp
var _rides: Array = []
var _imperial: bool = false
var _list: VBoxContainer = VBoxContainer.new()
var _selected: int = -1
var _empty: Label = Label.new()
var _detail: VBoxContainer = VBoxContainer.new()
var _title: EditableTitle = EditableTitle.new(tr("Rename the ride"))
var _left: PanelContainer = PanelContainer.new()
var _summary_caption: Label = UiTheme.caption(tr("Ride summary"))
var _actions: HBoxContainer = HBoxContainer.new()
var _confirm_delete: ConfirmationDialog = ConfirmationDialog.new()
## Showing a single ride's summary after riding it, rather than the history.
var _summary: bool = false
var _back: Button = Button.new()
var _subtitle: Label = Label.new()
var _stats: GridContainer = GridContainer.new()
var _climbs: VBoxContainer = VBoxContainer.new()
## The durations over which the ride holds the rider's best power (R62).
var _power_records: HFlowContainer = HFlowContainer.new()
## What an FTP test showed (R22), and taking it as the rider's FTP.
var _ftp_row: HBoxContainer = HBoxContainer.new()
var _ftp_text: Label = Label.new()
var _ftp_button: Button = Button.new()
var _chart: RideChart = RideChart.new()
var _power_zones: ZoneBars = ZoneBars.new()
var _heart_rate_zones: ZoneBars = ZoneBars.new()
var _power_curve: PowerCurveChart = PowerCurveChart.new()
## The rider's best power over each duration among all rides, as `TorqaApp.power_curves()`
## gives it; read with the rides.
var _best_efforts: Array = []
var _delete_button: Button = Button.new()
## Saving the ride's FIT file somewhere outside the data directory (R28).
var _export_button: Button = Button.new()
var _export_dialog: FileDialog = FileDialog.new()
var _export_status: Label = Label.new()
var _exporting: bool = false
## Uploading the ride to the rider's training platforms (R29), each a menu entry.
var _upload_button: MenuButton = MenuButton.new()
var _account_dialog: AccountDialog = AccountDialog.new()
## The platforms with the active rider's accounts, as `TorqaApp.accounts()` gives them.
var _accounts: Array = []
## Uploads the rider started here and not finished yet, by `_upload_key()`: only these
## ask to connect again when the platform refuses the account.
var _started: Dictionary[String, bool] = {}
## The platform to upload the shown ride to once its account is connected.
var _upload_after_connect: String = ""
var _starting_upload: bool = false


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.failed.connect(_on_failed)
	_torqa.upload_finished.connect(_on_upload_finished)
	_torqa.upload_failed.connect(_on_upload_failed)
	_account_dialog.bind(torqa)


## The summary of the ride just saved in `path` (R42): only that ride, to name, keep or
## discard; `closed` when done.
func open_summary(path: String) -> void:
	_summary = true
	_load()
	for i: int in range(_rides.size()):
		var ride: Dictionary = _rides[i]
		if ride["path"] == path:
			_show_ride(i)


## Reloads the rides of the active rider and shows the newest.
func open() -> void:
	_summary = false
	_load()


func _load() -> void:
	_left.visible = not _summary
	_summary_caption.visible = _summary
	_actions.visible = _summary
	_delete_button.visible = not _summary
	_imperial = _torqa.profile().get("units", "metric") == "imperial"
	_rides = _torqa.history()
	var curves: Dictionary = _torqa.power_curves(0, false, false)
	_best_efforts = curves.get("period", [])
	for child: Node in _list.get_children():
		child.queue_free()
	var group: ButtonGroup = ButtonGroup.new()
	for i: int in range(_rides.size()):
		var ride: Dictionary = _rides[i]
		var entry: Button = Button.new()
		entry.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		entry.text = _list_text(ride)
		entry.alignment = HORIZONTAL_ALIGNMENT_LEFT
		entry.toggle_mode = true
		entry.button_group = group
		entry.button_pressed = i == 0
		entry.pressed.connect(_show_ride.bind(i))
		_list.add_child(entry)
	_empty.visible = _rides.is_empty()
	_detail.visible = not _rides.is_empty()
	if not _rides.is_empty():
		_show_ride(0)


func _ready() -> void:
	var columns: HBoxContainer = HBoxContainer.new()
	columns.add_theme_constant_override("separation", 20)
	(%Margin as MarginContainer).add_child(columns)

	var left: PanelContainer = _left
	left.custom_minimum_size = Vector2(400, 0)
	columns.add_child(left)
	var left_rows: VBoxContainer = VBoxContainer.new()
	left_rows.add_theme_constant_override("separation", 14)
	left.add_child(left_rows)
	var header: HBoxContainer = HBoxContainer.new()
	header.add_theme_constant_override("separation", 14)
	var back: Button = _back
	back.text = tr("← Back")
	back.pressed.connect(_close)
	header.add_child(back)
	var heading: Label = Label.new()
	heading.text = tr("Your rides")
	heading.add_theme_font_size_override("font_size", 22)
	header.add_child(heading)
	left_rows.add_child(header)
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_list.add_theme_constant_override("separation", 8)
	scroll.add_child(_list)
	left_rows.add_child(scroll)
	_empty.text = tr("No rides yet. Finished rides appear here.")
	_empty.add_theme_color_override("font_color", UiTheme.MUTED)
	left_rows.add_child(_empty)

	var right: PanelContainer = PanelContainer.new()
	right.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	columns.add_child(right)
	_detail.add_theme_constant_override("separation", 16)
	right.add_child(_detail)
	var title_row: HBoxContainer = HBoxContainer.new()
	var titles: VBoxContainer = VBoxContainer.new()
	titles.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	titles.add_child(_summary_caption)
	_title.edit_finished.connect(_rename)
	titles.add_child(_title)
	_subtitle.add_theme_color_override("font_color", UiTheme.MUTED)
	titles.add_child(_subtitle)
	# Shows a file path.
	_export_status.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_export_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_export_status.add_theme_color_override("font_color", UiTheme.MUTED)
	_export_status.hide()
	titles.add_child(_export_status)
	title_row.add_child(titles)
	_upload_button.icon = UiIcons.texture("upload")
	_upload_button.tooltip_text = tr("Upload")
	_upload_button.focus_mode = Control.FOCUS_NONE
	# Drawn like the buttons beside it, not as a flat menu title.
	_upload_button.flat = false
	_upload_button.about_to_popup.connect(_fill_upload_menu)
	var menu: PopupMenu = _upload_button.get_popup()
	# Entries name the platform; they are translated as they are made.
	menu.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	menu.id_pressed.connect(_on_upload_chosen)
	_title.add_action(_upload_button)
	_account_dialog.account_connected.connect(_on_account_connected)
	add_child(_account_dialog)
	_export_button.icon = UiIcons.texture("download")
	_export_button.tooltip_text = tr("Export FIT file")
	_export_button.focus_mode = Control.FOCUS_NONE
	_export_button.pressed.connect(_choose_export)
	_title.add_action(_export_button)
	_export_dialog.title = tr("Export the ride as a FIT file")
	_export_dialog.file_mode = FileDialog.FILE_MODE_SAVE_FILE
	_export_dialog.access = FileDialog.ACCESS_FILESYSTEM
	# Not the working directory: that is the disk's root for an app started from the Finder.
	_export_dialog.current_dir = OS.get_system_dir(OS.SYSTEM_DIR_DOWNLOADS)
	_export_dialog.filters = PackedStringArray(["*.fit ; " + tr("FIT activities")])
	_export_dialog.use_native_dialog = true
	_export_dialog.file_selected.connect(_export)
	add_child(_export_dialog)
	_delete_button.icon = UiIcons.texture("bin")
	_delete_button.tooltip_text = tr("Delete ride")
	_delete_button.focus_mode = Control.FOCUS_NONE
	_delete_button.pressed.connect(_confirm_delete.popup_centered)
	UiTheme.danger_button(_delete_button)
	# Beside the pencil, on the title's line, so the two align.
	_title.add_action(_delete_button)
	_detail.add_child(title_row)
	_confirm_delete.title = tr("Delete ride?")
	_confirm_delete.dialog_text = tr("The ride and its FIT file are deleted.")
	_confirm_delete.ok_button_text = tr("Delete")
	UiTheme.danger_button(_confirm_delete.get_ok_button())
	_confirm_delete.confirmed.connect(_on_delete_confirmed)
	add_child(_confirm_delete)

	_stats.columns = 6
	_stats.add_theme_constant_override("h_separation", 28)
	_stats.add_theme_constant_override("v_separation", 12)
	_detail.add_child(_stats)
	_climbs.add_theme_constant_override("separation", 4)
	_detail.add_child(_climbs)
	_power_records.add_theme_constant_override("h_separation", 24)
	_detail.add_child(_power_records)
	_ftp_row.add_theme_constant_override("separation", 16)
	_ftp_text.add_theme_font_size_override("font_size", 18)
	_ftp_row.add_child(_ftp_text)
	_ftp_button.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_ftp_button.pressed.connect(_use_ftp)
	_ftp_row.add_child(_ftp_button)
	_ftp_row.hide()
	_detail.add_child(_ftp_row)

	var legend: HBoxContainer = HBoxContainer.new()
	legend.add_theme_constant_override("separation", 18)
	# i18n-begin
	for entry: Array in [
		["Power", UiTheme.POWER_COLOR],
		["Heart rate", UiTheme.HEART_RATE_COLOR],
		["Elevation", Color(1, 1, 1, 0.35)],
	]:
		# i18n-end
		var key_name: String = entry[0]
		var key_color: Color = entry[1]
		var key: Label = UiTheme.caption(key_name)
		key.add_theme_color_override("font_color", key_color)
		legend.add_child(key)
	_detail.add_child(legend)
	_chart.custom_minimum_size = Vector2(0, 220)
	_detail.add_child(_chart)

	var zones: HBoxContainer = HBoxContainer.new()
	zones.add_theme_constant_override("separation", 32)
	_power_curve.custom_minimum_size = Vector2(0, 170)
	# i18n-begin
	for entry: Array in [
		["Time in power zones", _power_zones],
		["Time in heart-rate zones", _heart_rate_zones],
		["Power curve", _power_curve],
	]:
		# i18n-end
		var column: VBoxContainer = VBoxContainer.new()
		column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		column.add_theme_constant_override("separation", 8)
		var caption: String = entry[0]
		column.add_child(UiTheme.caption(caption))
		var content: Control = entry[1]
		column.add_child(content)
		zones.add_child(column)
	_detail.add_child(zones)

	# Summary only: keep (the ride is saved already) or discard it.
	var push: Control = Control.new()
	push.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_detail.add_child(push)
	_actions.add_theme_constant_override("separation", 12)
	_actions.alignment = BoxContainer.ALIGNMENT_END
	var discard: Button = Button.new()
	discard.text = tr("Discard ride")
	discard.pressed.connect(_confirm_delete.popup_centered)
	UiTheme.danger_button(discard)
	_actions.add_child(discard)
	var done: Button = Button.new()
	done.text = tr("Done")
	done.custom_minimum_size = Vector2(140, 0)
	done.pressed.connect(_keep)
	_actions.add_child(done)
	_actions.hide()
	_detail.add_child(_actions)
	_apply_embedded()


func _apply_embedded() -> void:
	_back.visible = not embedded
	($Background as ColorRect).visible = not embedded
	if embedded:
		var margin: MarginContainer = %Margin
		for side: String in ["left", "top", "right", "bottom"]:
			margin.add_theme_constant_override("margin_" + side, 0)


func _show_ride(index: int) -> void:
	_selected = index
	var ride: Dictionary = _rides[index]
	var path: String = ride["path"]
	var start: int = ride["start_unix_s"]
	var ride_name: String = ride["name"]
	_title.placeholder_text = _default_name(ride)
	_title.text = ride_name
	_subtitle.text = UiTheme.date(start)
	_export_status.hide()
	_fill_stats(ride)
	_fill_climbs(ride)
	_fill_power_records(ride)
	_show_power_curve(ride)
	_show_ftp(ride)
	var detail: Dictionary = _torqa.ride_detail(path, CHART_POINTS)
	if detail.is_empty():
		return
	var elevation: PackedVector2Array = detail["elevation_m"]
	var power: PackedVector2Array = detail["power"]
	var heart_rate: PackedVector2Array = detail["heart_rate"]
	_chart.set_series(elevation, power, heart_rate)
	var power_zones: PackedFloat64Array = detail["power_zones"]
	var heart_rate_zones: PackedFloat64Array = detail["heart_rate_zones"]
	_power_zones.set_zones(power_zones, UiTheme.POWER_ZONES)
	_heart_rate_zones.set_zones(heart_rate_zones, UiTheme.HEART_RATE_ZONES)
	# Rides without a power meter or heart-rate strap have nothing to show there.
	(_power_zones.get_parent() as Control).visible = not power.is_empty()
	(_heart_rate_zones.get_parent() as Control).visible = not heart_rate.is_empty()


func _fill_stats(ride: Dictionary) -> void:
	for child: Node in _stats.get_children():
		child.queue_free()
	var distance_km: float = ride["distance_m"] / 1000.0
	var gain_m: float = ride["elevation_gain_m"]
	var speed_kmh: float = ride["avg_speed_kmh"]
	var elapsed_s: float = ride["elapsed_s"]
	# i18n-begin
	var stats: Array[Array] = [
		["Time", _duration(elapsed_s), ""],
		[
			"Distance",
			"%.1f" % (distance_km / KM_PER_MILE if _imperial else distance_km),
			"mi" if _imperial else "km"
		],
		[
			"Climbing",
			"%d" % roundi(gain_m / METERS_PER_FOOT if _imperial else gain_m),
			"ft" if _imperial else "m"
		],
		[
			"Avg speed",
			"%.1f" % (speed_kmh / KM_PER_MILE if _imperial else speed_kmh),
			"mph" if _imperial else "km/h"
		],
		["Avg power", _number(ride["avg_power"], "%d"), "W"],
		["Normalized", _number(ride["normalized_power"], "%d"), "W"],
		["Max power", _number(ride["max_power"], "%d"), "W"],
		["Intensity", _number(ride["intensity_factor"], "%.2f"), ""],
		["TSS", _number(ride["training_stress"], "%d"), ""],
		["Work", _number(ride["work_kj"], "%d"), "kJ"],
		["Avg heart rate", _number(ride["avg_heart_rate"], "%d"), "bpm"],
		["Avg cadence", _number(ride["avg_cadence"], "%d"), "rpm"],
	]
	# i18n-end
	for stat: Array in stats:
		var cell: VBoxContainer = VBoxContainer.new()
		cell.add_theme_constant_override("separation", 0)
		var caption: String = stat[0]
		var text: String = stat[1]
		var unit: String = stat[2]
		cell.add_child(UiTheme.caption(caption))
		var value: Label = UiTheme.value(20)
		value.text = "%s %s" % [text, unit] if not unit.is_empty() else text
		cell.add_child(value)
		_stats.add_child(cell)


## What an FTP test (R22) showed, next to the rider's FTP now, with a button to take it.
func _show_ftp(ride: Dictionary) -> void:
	var estimate: Variant = ride.get("ftp_estimate_w")
	_ftp_row.visible = estimate != null
	if estimate == null:
		return
	var estimate_w: float = estimate
	var current_w: float = _torqa.profile().get("ftp_w", 0.0)
	_ftp_text.text = (
		tr("FTP test: your FTP is about %d W (now %d W).") % [roundi(estimate_w), roundi(current_w)]
	)
	_ftp_button.text = tr("Use %d W") % roundi(estimate_w)
	_ftp_button.visible = roundi(estimate_w) != roundi(current_w)
	_ftp_button.set_meta("watts", estimate_w)


func _use_ftp() -> void:
	var watts: float = _ftp_button.get_meta("watts", 0.0)
	if watts > 0.0 and _torqa.use_ftp(watts):
		_ftp_text.text = tr("Your FTP is now %d W: zones and workouts follow it.") % roundi(watts)
		_ftp_button.hide()


## The ride's times on the route's climbs, records marked.
func _fill_climbs(ride: Dictionary) -> void:
	for child: Node in _climbs.get_children():
		child.queue_free()
	var climbs: Array = ride.get("climbs", [])
	var route_time: Variant = ride.get("route_time_s")
	_climbs.visible = not climbs.is_empty() or route_time != null
	if not _climbs.visible:
		return
	_climbs.add_child(UiTheme.caption(tr("Times")))
	if route_time != null:
		var route_s: float = route_time
		var record: bool = ride["route_record"]
		_climbs.add_child(_time_row(tr("Whole route"), route_s, null, record))
	for i: int in range(climbs.size()):
		var climb: Dictionary = climbs[i]
		var start_km: float = climb["start_m"] / 1000.0
		var length_km: float = climb["length_m"] / 1000.0
		var time_s: float = climb["time_s"]
		var record: bool = climb["record"]
		var what: String = tr("Climb %d  ·  %.1f km from km %.1f") % [i + 1, length_km, start_km]
		_climbs.add_child(_time_row(what, time_s, climb["avg_power"], record))


## A ★ for each duration over which the ride holds the rider's best power (R62).
func _fill_power_records(ride: Dictionary) -> void:
	for child: Node in _power_records.get_children():
		child.queue_free()
	var records: PackedFloat64Array = ride.get("power_records", PackedFloat64Array())
	var curve: PackedVector2Array = ride.get("power_curve", PackedVector2Array())
	_power_records.visible = not records.is_empty()
	for seconds: float in records:
		var watts: float = 0.0
		for point: Vector2 in curve:
			if is_equal_approx(point.x, seconds):
				watts = point.y
		var badge: Label = Label.new()
		badge.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		badge.text = (
			tr("★ Best %s: %d W") % [PowerCurveChart.duration_label(seconds), roundi(watts)]
		)
		badge.add_theme_color_override("font_color", UiTheme.CLIMB_COLORS["Cat 3"])
		_power_records.add_child(badge)


## The ride's best power over each duration, over the rider's best of all rides (R62).
func _show_power_curve(ride: Dictionary) -> void:
	var curve: PackedVector2Array = ride.get("power_curve", PackedVector2Array())
	(_power_curve.get_parent() as Control).visible = not curve.is_empty()
	_power_curve.clear()
	_power_curve.add_curve(curve, UiTheme.POWER_COLOR)
	_power_curve.add_efforts(_best_efforts, UiTheme.MUTED)


func _time_row(what: String, seconds: float, power: Variant, record: bool) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 16)
	var name_label: Label = Label.new()
	name_label.text = what
	name_label.custom_minimum_size = Vector2(300, 0)
	row.add_child(name_label)
	var time: Label = Label.new()
	time.text = UiTheme.duration(seconds)
	time.custom_minimum_size = Vector2(70, 0)
	row.add_child(time)
	var watts: Label = Label.new()
	watts.text = _number(power, "%d") + " W" if power != null else ""
	watts.custom_minimum_size = Vector2(70, 0)
	watts.add_theme_color_override("font_color", UiTheme.MUTED)
	row.add_child(watts)
	if record:
		var badge: Label = Label.new()
		badge.text = tr("★ Personal record")
		badge.add_theme_color_override("font_color", UiTheme.CLIMB_COLORS["Cat 3"])
		row.add_child(badge)
	return row


func _on_delete_confirmed() -> void:
	if _selected < 0:
		return
	var ride: Dictionary = _rides[_selected]
	var path: String = ride["path"]
	if _torqa.delete_ride(path):
		if _summary:
			_close()
		else:
			_load()


## Asks where to save the ride's FIT file (R28), suggesting its name as the file name (R50).
func _choose_export() -> void:
	if _selected < 0:
		return
	# A name still being typed is the one to export under.
	_rename()
	var ride: Dictionary = _rides[_selected]
	var ride_name: String = ride["name"]
	var title: String = ride_name if not ride_name.is_empty() else _default_name(ride)
	_export_dialog.current_file = TorqaApp.ride_export_file_name(title)
	_export_dialog.popup_centered_ratio(0.7)


func _export(to: String) -> void:
	if _selected < 0:
		return
	var ride: Dictionary = _rides[_selected]
	var path: String = ride["path"]
	_exporting = true
	if _torqa.export_ride(path, to):
		_show_status(tr("Saved as %s") % to)
	_exporting = false


func _on_failed(message: String) -> void:
	if _exporting or _starting_upload:
		_show_status(message, UiTheme.DANGER)


## Lists the platforms: the ride's page where it went, uploading where the rider has an
## account, connecting first where not.
func _fill_upload_menu() -> void:
	var menu: PopupMenu = _upload_button.get_popup()
	menu.clear()
	if _selected < 0:
		return
	var ride: Dictionary = _rides[_selected]
	var path: String = ride["path"]
	var uploads: Dictionary = ride.get("uploads", {})
	_accounts = _torqa.accounts(_rider())
	for i: int in range(_accounts.size()):
		var account: Dictionary = _accounts[i]
		var platform: String = account["platform"]
		var platform_name: String = account["name"]
		var label: String = account["label"]
		if uploads.has(platform):
			menu.add_item(tr("View on %s") % platform_name, i)
		elif _torqa.uploading(path).has(platform):
			menu.add_item(tr("Uploading to %s…") % platform_name, i)
			menu.set_item_disabled(menu.get_item_index(i), true)
		elif label.is_empty():
			menu.add_item(tr("Connect %s and upload…") % platform_name, i)
		else:
			menu.add_item(tr("Upload to %s") % platform_name, i)


func _on_upload_chosen(index: int) -> void:
	if _selected < 0 or index >= _accounts.size():
		return
	var account: Dictionary = _accounts[index]
	var platform: String = account["platform"]
	var ride: Dictionary = _rides[_selected]
	var uploads: Dictionary = ride.get("uploads", {})
	var label: String = account["label"]
	if uploads.has(platform):
		var url: String = uploads[platform]
		OS.shell_open(url)
	elif label.is_empty():
		_upload_after_connect = platform
		_account_dialog.open(_rider(), account)
	else:
		_upload(platform)


## Sends the shown ride to `platform` under the title it shows (R50).
func _upload(platform: String) -> void:
	if _selected < 0:
		return
	# A name still being typed is the one to upload under.
	_rename()
	var path: String = _shown_path()
	_starting_upload = true
	var started: bool = _torqa.upload_ride(path, platform, _shown_title())
	_starting_upload = false
	if started:
		_started[_upload_key(path, platform)] = true
		_show_status(tr("Uploading to %s…") % _platform_name(platform))


func _on_account_connected(platform: String) -> void:
	if platform == _upload_after_connect:
		_upload_after_connect = ""
		_upload(platform)


func _on_upload_finished(path: String, platform: String, url: String) -> void:
	_started.erase(_upload_key(path, platform))
	for ride: Dictionary in _rides:
		if ride["path"] == path:
			var uploads: Dictionary = ride.get("uploads", {})
			uploads[platform] = url
			ride["uploads"] = uploads
	if _shown_path() == path:
		_show_status(tr("Uploaded to %s.") % _platform_name(platform))


func _on_upload_failed(path: String, platform: String, message: String, reconnect: bool) -> void:
	var key: String = _upload_key(path, platform)
	var started_here: bool = _started.has(key)
	_started.erase(key)
	if _shown_path() != path:
		return
	if not reconnect:
		# The platform's words; a few, e.g. Strava wanting a subscription, are translated.
		_show_status(tr(message), UiTheme.DANGER)
		return
	_show_status(
		tr("%s no longer accepts the account: connect it again.") % _platform_name(platform),
		UiTheme.DANGER
	)
	# The platform no longer lets Torqa in: connect again, then upload. Not for automatic
	# uploads, which may end while the rider is elsewhere.
	if not started_here or not is_visible_in_tree():
		return
	for account: Dictionary in _accounts:
		if account["platform"] == platform:
			_upload_after_connect = platform
			_account_dialog.open(_rider(), account, message)


func _platform_name(platform: String) -> String:
	for account: Dictionary in _accounts:
		if account["platform"] == platform:
			var platform_name: String = account["name"]
			return platform_name
	return platform


func _rider() -> String:
	var id: String = _torqa.profile().get("id", "")
	return id


## The title the shown ride goes by (R50): its name, or its route and date.
func _shown_title() -> String:
	if _selected < 0 or _selected >= _rides.size():
		return ""
	var ride: Dictionary = _rides[_selected]
	var ride_name: String = ride["name"]
	return ride_name if not ride_name.is_empty() else _default_name(ride)


func _shown_path() -> String:
	if _selected < 0 or _selected >= _rides.size():
		return ""
	var ride: Dictionary = _rides[_selected]
	return ride["path"]


static func _upload_key(path: String, platform: String) -> String:
	return path + "\n" + platform


func _show_status(text: String, color: Color = UiTheme.MUTED) -> void:
	_export_status.text = text
	_export_status.add_theme_color_override("font_color", color)
	_export_status.show()


## Saves the name typed into the title (R50); an empty name shows route and date again.
func _rename() -> void:
	if _selected < 0:
		return
	var ride: Dictionary = _rides[_selected]
	var path: String = ride["path"]
	var old_name: String = ride["name"]
	var new_name: String = _title.text.strip_edges()
	if new_name == old_name or not _torqa.rename_ride(path, new_name):
		return
	ride["name"] = new_name
	if _selected < _list.get_child_count():
		(_list.get_child(_selected) as Button).text = _list_text(ride)


## Done on the summary (R42): the rider keeps the ride, and it goes up to their accounts set
## to upload automatically (R60). Discarding goes another way.
func _keep() -> void:
	_rename()
	var path: String = _shown_path()
	if not path.is_empty():
		_torqa.keep_ride(path, _shown_title())
	_close()


func _close() -> void:
	_rename()
	_summary = false
	closed.emit()


## "Gurtenstrasse · Sat 3 Oct": the name of a ride the rider has not named (R50).
func _default_name(ride: Dictionary) -> String:
	var route: String = ride["route"]
	var start: int = ride["start_unix_s"]
	return "%s · %s" % [route, UiTheme.date(start).get_slice(",", 0).rsplit(" ", true, 1)[0]]


func _list_text(ride: Dictionary) -> String:
	var start: int = ride["start_unix_s"]
	var distance_km: float = ride["distance_m"] / 1000.0
	var elapsed_s: float = ride["elapsed_s"]
	var distance: String = (
		"%.1f mi" % (distance_km / KM_PER_MILE) if _imperial else "%.1f km" % distance_km
	)
	var ride_name: String = ride["name"]
	var title: String = ride_name if not ride_name.is_empty() else _default_name(ride)
	return "%s\n%s  ·  %s  ·  %s" % [title, UiTheme.date(start), distance, _duration(elapsed_s)]


static func _duration(seconds: float) -> String:
	var total: int = roundi(seconds)
	return "%d:%02d:%02d" % [total / 3600, total / 60 % 60, total % 60]


static func _number(value: Variant, format: String) -> String:
	if value == null:
		return "--"
	var number: float = value
	return format % (roundi(number) if format == "%d" else number)
