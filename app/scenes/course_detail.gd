class_name CourseDetail
extends HBoxContainer
## One course (R40): name (renamable), key figures, path card, elevation profile with climbs and
## the rider's records on it, next to the ride options (R48) and who to race (R20) → Ride.
## Opening the page loads only the route; the 3D world is built once Ride is pressed.

signal back_requested
## Ride was pressed with `options` (`RideOptions.options()`) against `ghost` (`GhostPicker`).
signal ride_requested(options: Dictionary, ghost: Dictionary)
## The world asked for with `build()` is ready: the ride can start.
signal ready_to_ride
## The course was renamed or deleted: the gallery is out of date.
signal course_changed

var _torqa: TorqaApp
var _course: Dictionary = {}
var _path: String = ""
## Waiting for this course's route, or for its world after Ride.
var _loading_route: bool = false
var _building: bool = false
var _title: EditableTitle = EditableTitle.new(tr("Rename the course"))
var _figures: VBoxContainer = VBoxContainer.new()
var _path_card: PathCard = PathCard.new()
var _profile: ElevationProfile = ElevationProfile.new()
var _records: Label = Label.new()
var _options: RideOptions = RideOptions.new()
var _ghost: GhostPicker = GhostPicker.new()
var _ride_button: Button = Button.new()
var _loading_bar: ProgressBar = ProgressBar.new()
var _status: Label = Label.new()
var _confirm_delete: ConfirmationDialog = ConfirmationDialog.new()


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.route_loaded.connect(_on_route_loaded)
	_torqa.loading_progress.connect(_on_loading_progress)
	_torqa.world_ready.connect(_on_world_ready)
	_torqa.failed.connect(_on_failed)


## Shows `course` (from `TorqaApp.courses()`), loading its route unless it is loaded already.
func open(course: Dictionary) -> void:
	_course = course
	_path = course["path"]
	_building = false
	_title.text = course["name"]
	_show_figures()
	var track: PackedVector2Array = course.get("track", PackedVector2Array())
	var profile: PackedVector2Array = course.get("profile", PackedVector2Array())
	_path_card.set_track(track)
	_profile.set_profile(profile)
	_profile.set_climbs([])
	_records.text = ""
	_status.text = ""
	_loading_bar.hide()
	_ride_button.disabled = true
	_loading_route = true
	if _torqa.loaded_course() != _path:
		_torqa.open_course(_path)
	elif _torqa.has_route():
		_show_route()


## After a ride on it: the rider's records may have changed.
func refresh_records() -> void:
	if not _path.is_empty() and _torqa.loaded_course() == _path and _torqa.has_route():
		_show_route()


## The ride options chosen here, as `RideOptions.options()` returns them.
func ride_options() -> Dictionary:
	return _options.options()


## Builds the course's 3D world for riding; `ready_to_ride` follows, at once if it is built.
func build() -> void:
	if _torqa.build_world():
		ready_to_ride.emit()
		return
	_building = true
	_ride_button.disabled = true
	_loading_bar.value = 0.0
	_loading_bar.show()
	_status.text = tr("Building 3D world") + " …"


## Shows why riding is not possible (e.g. no trainer); empty clears it.
func show_status(message: String) -> void:
	_status.text = message


func _init() -> void:
	add_theme_constant_override("separation", 24)
	var left: VBoxContainer = VBoxContainer.new()
	left.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left.add_theme_constant_override("separation", 14)
	var top: HBoxContainer = HBoxContainer.new()
	var back: Button = Button.new()
	back.text = tr("← Courses")
	back.pressed.connect(func() -> void: back_requested.emit())
	top.add_child(back)
	var push: Control = Control.new()
	push.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	top.add_child(push)
	var delete: Button = Button.new()
	delete.text = tr("Delete course")
	delete.pressed.connect(_confirm_delete.popup_centered)
	top.add_child(delete)
	left.add_child(top)
	_title.edit_finished.connect(_rename)
	left.add_child(_title)
	var overview: HBoxContainer = HBoxContainer.new()
	overview.add_theme_constant_override("separation", 24)
	_path_card.custom_minimum_size = Vector2(0, 260)
	_path_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	overview.add_child(_path_card)
	_figures.custom_minimum_size = Vector2(220, 0)
	overview.add_child(_figures)
	overview.size_flags_vertical = Control.SIZE_EXPAND_FILL
	left.add_child(overview)
	left.add_child(UiTheme.caption(tr("Elevation")))
	_profile.custom_minimum_size = Vector2(0, 120)
	left.add_child(_profile)
	_records.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	left.add_child(_records)
	add_child(left)

	var panel: PanelContainer = PanelContainer.new()
	panel.custom_minimum_size = Vector2(520, 0)
	var right: VBoxContainer = VBoxContainer.new()
	right.add_theme_constant_override("separation", 14)
	right.add_child(UiTheme.caption(tr("Ride options")))
	right.add_child(_options)
	right.add_child(UiTheme.caption(tr("Race against")))
	right.add_child(_ghost)
	var fill: Control = Control.new()
	fill.size_flags_vertical = Control.SIZE_EXPAND_FILL
	right.add_child(fill)
	_loading_bar.max_value = 1.0
	_loading_bar.show_percentage = false
	_loading_bar.custom_minimum_size = Vector2(0, 8)
	right.add_child(_loading_bar)
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.add_theme_color_override("font_color", UiTheme.MUTED)
	right.add_child(_status)
	_ride_button.text = tr("Ride")
	_ride_button.custom_minimum_size = Vector2(0, 56)
	_ride_button.add_theme_font_size_override("font_size", 22)
	_ride_button.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_ride_button.pressed.connect(
		func() -> void: ride_requested.emit(_options.options(), _ghost.choice())
	)
	right.add_child(_ride_button)
	panel.add_child(right)
	add_child(panel)

	_confirm_delete.title = tr("Delete course?")
	_confirm_delete.dialog_text = tr("Its file is deleted; your rides on it are kept.")
	_confirm_delete.ok_button_text = tr("Delete")
	_confirm_delete.confirmed.connect(_delete)
	add_child(_confirm_delete)


func _show_figures() -> void:
	for child: Node in _figures.get_children():
		_figures.remove_child(child)
		child.queue_free()
	var imperial: bool = _torqa.profile().get("units", "metric") == "imperial"
	_figures.add_child(CourseCard.figure_rows(_course, imperial))


func _rename() -> void:
	var old_name: String = _course.get("name", "")
	var new_name: String = _title.text.strip_edges()
	if new_name.is_empty() or new_name == old_name or not _torqa.rename_course(_path, new_name):
		_title.text = old_name
		return
	_course["name"] = new_name
	_title.text = new_name
	course_changed.emit()


func _delete() -> void:
	if _torqa.delete_course(_path):
		_path = ""
		course_changed.emit()
		back_requested.emit()


func _on_route_loaded(_route: Dictionary) -> void:
	if _loading_route and _torqa.loaded_course() == _path:
		_show_route()


## The route is loaded: climbs, records and riding.
func _show_route() -> void:
	_loading_route = false
	_ride_button.disabled = false
	var climbs: Dictionary = _torqa.climbs()
	var climb_list: Array = climbs.get("climbs", [])
	_profile.set_climbs(climb_list)
	_profile.set_profile(_torqa.elevation_profile(600))
	var lines: PackedStringArray = PackedStringArray()
	if climbs.get("route_best_s") != null:
		var best: float = climbs["route_best_s"]
		lines.append(tr("Your best time on this route: %s") % UiTheme.duration(best))
	for climb: Dictionary in climb_list:
		var category: String = climb["category"]
		var length_km: float = climb["length_m"] / 1000.0
		var grade: float = climb["grade"]
		var line: String = tr("%s %.1f km at %.1f %%") % [tr(category), length_km, grade]
		if climb["best_s"] != null:
			var climb_best: float = climb["best_s"]
			line += " " + tr("(best %s)") % UiTheme.duration(climb_best)
		lines.append(line)
	_records.text = "\n".join(lines)
	var ftp: float = _torqa.profile().get("ftp_w", 200.0)
	_ghost.configure(_torqa.has_personal_best(), ftp)


func _on_loading_progress(step: String, _unit: String, done: int, total: int) -> void:
	if not _building:
		return
	_loading_bar.value = float(done) / float(maxi(total, 1))
	_status.text = tr(step) + " …"


func _on_world_ready(_info: Dictionary) -> void:
	if not _building:
		return
	_building = false
	_loading_bar.hide()
	_status.text = ""
	_ride_button.disabled = false
	ready_to_ride.emit()


func _on_failed(message: String) -> void:
	if visible and (_loading_route or _building):
		_loading_route = false
		_building = false
		_loading_bar.hide()
		_status.text = message
