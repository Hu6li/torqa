class_name CourseDetail
extends HBoxContainer
## One course (R40): name, key figures, path card, elevation profile with climbs and the
## rider's records on it, next to the ride options (R48) and who to race (R20) → Ride. The
## pencil beside the name renames the course and adds, aligns or removes its video (R17); the
## bin beside it deletes the course. Opening the page loads only the route; the 3D world is
## built once Ride is pressed.

signal back_requested
## Ride was pressed with `options` (`RideOptions.options()`) against `ghost` (`GhostPicker`).
signal ride_requested(options: Dictionary, ghost: Dictionary)
## The world asked for with `build()` is ready: the ride can start.
signal ready_to_ride
## The course was renamed, deleted or given a new map picture: the gallery is out of date.
signal course_changed

var _torqa: TorqaApp
var _course: Dictionary = {}
var _path: String = ""
## Waiting for this course's route, or for its world after Ride.
var _loading_route: bool = false
var _building: bool = false
var _title: EditableTitle = EditableTitle.new(tr("Edit the course's name and video"), false)
var _figures: VBoxContainer = VBoxContainer.new()
var _path_card: PathCard = PathCard.new()
var _profile: ElevationProfile = ElevationProfile.new()
var _records: Label = Label.new()
var _options: RideOptions = RideOptions.new()
var _ghost: GhostPicker = GhostPicker.new()
var _ride_button: Button = Button.new()
var _loading_bar: ProgressBar = ProgressBar.new()
var _status: Label = Label.new()
var _delete_button: Button = Button.new()
var _confirm_delete: ConfirmationDialog = ConfirmationDialog.new()
var _edit_dialog: CourseEditDialog = CourseEditDialog.new()
## Courses with a video are ridden along it or in 3D, as chosen when riding (#44).
var _view_dialog: AcceptDialog = AcceptDialog.new()
var _along_video: bool = false


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
	_path_card.set_map(_torqa.course_preview(_path))
	_profile.set_profile(profile)
	_profile.set_climbs([])
	_records.text = ""
	_status.text = ""
	# The rider's own default difficulty (#175); the ride's settings may change it later.
	var rider: Dictionary = _torqa.profile()
	var difficulty: float = rider.get("default_difficulty_pct", 50.0)
	_options.set_difficulty(difficulty)
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
	_torqa.ride_along_video(_along_video)
	if _along_video or _torqa.build_world():
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
	left.add_child(top)
	_title.edit_requested.connect(_open_editor)
	_delete_button.icon = UiIcons.texture("bin")
	_delete_button.tooltip_text = tr("Delete course")
	_delete_button.focus_mode = Control.FOCUS_NONE
	_delete_button.pressed.connect(_confirm_delete.popup_centered)
	UiTheme.danger_button(_delete_button)
	# Beside the pencil, as on a ride in the history (#190).
	_title.add_action(_delete_button)
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
	_ride_button.pressed.connect(_on_ride_pressed)
	right.add_child(_ride_button)
	panel.add_child(right)
	add_child(panel)

	_confirm_delete.title = tr("Delete course?")
	_confirm_delete.dialog_text = tr("Its file is deleted; your rides on it are kept.")
	_confirm_delete.ok_button_text = tr("Delete")
	UiTheme.danger_button(_confirm_delete.get_ok_button())
	_confirm_delete.confirmed.connect(_delete)
	add_child(_confirm_delete)
	_view_dialog.title = tr("How do you want to ride?")
	_view_dialog.dialog_text = tr("This course has a video. Ride along it, or in the 3D world?")
	_view_dialog.ok_button_text = tr("Along the video")
	_view_dialog.add_button(tr("In 3D"), false, "world")
	_view_dialog.confirmed.connect(func() -> void: _ride(true))
	_view_dialog.custom_action.connect(_on_view_action)
	add_child(_view_dialog)
	_edit_dialog.course_renamed.connect(_rename)
	_edit_dialog.video_removed.connect(_remove_video)
	_edit_dialog.video_added.connect(_add_video)
	_edit_dialog.video_aligned.connect(_align_video)
	add_child(_edit_dialog)


func _show_figures() -> void:
	for child: Node in _figures.get_children():
		_figures.remove_child(child)
		child.queue_free()
	var imperial: bool = _torqa.profile().get("units", "metric") == "imperial"
	_figures.add_child(CourseCard.figure_rows(_course, imperial))


## The video is known once the route is loaded; until then only the name can change.
func _open_editor() -> void:
	var loaded: bool = not _loading_route and _torqa.loaded_course() == _path and _torqa.has_route()
	var video: Dictionary = _torqa.video() if loaded else {}
	var course_name: String = _course.get("name", "")
	_edit_dialog.edit(_torqa, course_name, video, loaded)


func _rename(new_name: String) -> void:
	if not _torqa.rename_course(_path, new_name):
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
	_show_option_groups()
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
	_keep_preview()


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


## A course without a map of its whole land, from before #203, gets one from the map loaded
## with it.
func _keep_preview() -> void:
	if _path.is_empty() or not _torqa.course_preview_outdated(_path):
		return
	var png: PackedByteArray = await MapPreview.capture(
		get_tree(), _torqa.course_map(_path), _torqa.track(2000)
	)
	if not png.is_empty():
		_torqa.set_course_preview(_path, png)
		_path_card.set_map(png)
		course_changed.emit()


func _on_failed(message: String) -> void:
	if visible and (_loading_route or _building):
		_loading_route = false
		_building = false
		_loading_bar.hide()
		_status.text = message


func _show_option_groups() -> void:
	var video: Dictionary = _torqa.video()
	# A video course is ridden either way, unless it has no place (Tacx RLV): then only along
	# its video.
	var located: bool = video.get("located", true)
	_options.show_option_groups(located, not video.is_empty())


func _on_ride_pressed() -> void:
	var video: Dictionary = _torqa.video()
	if video.is_empty():
		_ride(false)
	elif not video.get("located", true):
		_ride(true)
	else:
		_view_dialog.popup_centered()


func _on_view_action(action: StringName) -> void:
	_view_dialog.hide()
	if action == &"world":
		_ride(false)


func _ride(along_video: bool) -> void:
	_along_video = along_video
	ride_requested.emit(_options.options(), _ghost.choice())


func _add_video(video: String, marks: PackedVector2Array) -> void:
	if _torqa.add_video(video, marks):
		_status.text = tr("Video added: this course is ridden along it now.")
		_show_option_groups()
		course_changed.emit()


func _align_video(marks: PackedVector2Array) -> void:
	if _torqa.align_video(marks):
		_status.text = tr("Video aligned with the route.")


## Back to riding the course in 3D only.
func _remove_video() -> void:
	if _torqa.remove_video():
		_status.text = tr("Video removed: this course is ridden in 3D.")
		_show_option_groups()
		course_changed.emit()
