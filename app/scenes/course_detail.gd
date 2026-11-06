class_name CourseDetail
extends HBoxContainer
## One course (R40): name, key figures with the rider's estimated time (R66), path card,
## elevation profile with climbs and the rider's records on it, next to the ride options (R48)
## and who to race (R20) → Ride. The pencil beside the name renames the course and adds, aligns
## or removes its video (R17); the bin beside it deletes the course. Opening the page loads
## only the route, a spinner turning until it is in; the 3D world is built once Ride is pressed.

signal back_requested
## Ride was pressed with `options` (`RideOptions.options()`) against `ghost` (`GhostPicker`).
signal ride_requested(options: Dictionary, ghost: Dictionary)
## The world data asked for with `build()` is ready; scene preparation follows before riding.
signal ready_to_ride
## The course was renamed, deleted or given a new map picture: the gallery is out of date.
signal course_changed

## The course's side of the page takes this many times the ride options' share of its width.
const COURSE_SHARE: float = 1.5
const ANDROID: StringName = &"OrqwaAndroid"
## The map stands beside the figures only where it gets at least this width, else above them.
const MAP_ROOM: float = 280.0

var _orqwa: OrqwaApp
var _course: Dictionary = {}
var _path: String = ""
## Waiting for this course's route, or for its world after Ride.
var _loading_route: bool = false
var _building: bool = false
var _preparing_scene: bool = false
## Building the world again in place of one kept from an older ORQWA (R101), not to ride yet.
var _regenerating: bool = false
var _regenerate: Button = Button.new()
var _title: EditableTitle = EditableTitle.new(tr("Edit the course's name and video"), false)
var _figures: VBoxContainer = VBoxContainer.new()
var _path_card: PathCard = PathCard.new()
var _profile: ElevationProfile = ElevationProfile.new()
var _records: Label = Label.new()
var _options: RideOptions = RideOptions.new()
var _ghost: GhostPicker = GhostPicker.new()
var _ride_button: Button = Button.new()
var _continuation: VBoxContainer = VBoxContainer.new()
var _progress_label: Label = Label.new()
var _progress_bar: ProgressBar = ProgressBar.new()
var _continue_button: Button = Button.new()
var _from_start: Button = Button.new()
var _reset_progress: Button = Button.new()
var _continuing: bool = false
var _loading_bar: ProgressBar = ProgressBar.new()
var _log: PreparationLog = PreparationLog.new()
var _status: Label = Label.new()
## Asks Android to let ORQWA read a synced course video (R149), where it may not yet.
var _videos_button: Button = Button.new()
## Turns beside the status while the route is prepared: climbs, records, the estimated time
## and Ride wait for it.
var _spinner: Spinner = Spinner.new()
var _delete_button: Button = Button.new()
var _confirm_delete: ConfirmationDialog = ConfirmationDialog.new()
var _edit_dialog: CourseEditDialog = CourseEditDialog.new()
## Courses with a video are ridden along it or in 3D, as chosen when riding (#44).
var _view_dialog: AcceptDialog = AcceptDialog.new()
var _along_video: bool = false
## The descents the estimated time was worked out for: of the options, only they change it.
var _estimated_flat_descents: bool = false


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_options.bind(orqwa)
	_orqwa.route_loaded.connect(_on_route_loaded)
	_orqwa.loading_progress.connect(_on_loading_progress)
	_orqwa.world_ready.connect(_on_world_ready)
	_orqwa.failed.connect(_on_failed)


## Shows `course` (from `OrqwaApp.courses()`), loading its route unless it is loaded already.
func open(course: Dictionary) -> void:
	_options.set_season(_orqwa.season())
	_course = course
	_path = course["path"]
	_building = false
	_preparing_scene = false
	_regenerating = false
	_regenerate.hide()
	_title.text = course["name"]
	_show_figures()
	var track: PackedVector2Array = course.get("track", PackedVector2Array())
	var profile: PackedVector2Array = course.get("profile", PackedVector2Array())
	_path_card.set_track(track)
	_path_card.set_map(_orqwa.course_preview(_path))
	_profile.set_profile(profile)
	_profile.set_climbs([])
	_records.text = ""
	_status.text = ""
	# The rider's own default difficulty (#175); the ride's settings may change it later.
	var rider: Dictionary = _orqwa.profile()
	var difficulty: float = rider.get("default_difficulty_pct", 50.0)
	_options.set_difficulty(difficulty)
	_loading_bar.hide()
	_continuation.hide()
	_ride_button.show()
	_log.clear()
	_ride_button.disabled = true
	_loading_route = true
	_spinner.show()
	_status.text = tr("Loading the course") + " …"
	_videos_button.hide()
	if _orqwa.loaded_course() == _path and _orqwa.has_route():
		_show_route()
	else:
		# Also when it is the loaded course but has no route: it failed to open, perhaps for a
		# missing video the rider has put back since.
		_orqwa.open_course(_path)


## After a ride on it: the rider's records may have changed.
func refresh_records() -> void:
	_options.set_season(_orqwa.season())
	if not _path.is_empty() and _orqwa.loaded_course() == _path and _orqwa.has_route():
		_show_route()


## The ride options chosen here, as `RideOptions.options()` returns them.
func ride_options() -> Dictionary:
	return _options.options()


## Builds the course's world data; `ready_to_ride` requests the final scene preparation.
func build() -> void:
	_orqwa.ride_along_video(_along_video)
	if _along_video:
		ready_to_ride.emit()
		return
	_building = true
	_show_building()
	if _orqwa.build_world():
		_on_world_ready({})


## World data is built; keep the loading page until its scene has also been prepared (R223).
func show_scene_progress(done: int, total: int) -> void:
	_preparing_scene = true
	_ride_button.disabled = true
	_set_continuation_disabled(true)
	_loading_bar.value = float(done) / float(maxi(total, 1))
	_loading_bar.show()
	_status.text = tr("Preparing the 3D scene") + " …"
	_log.step("Preparing the 3D scene", "chunks", done, total)


## The scene is ready, or its pending start was abandoned.
func finish_preparation() -> void:
	_preparing_scene = false
	_loading_bar.hide()
	if _log.visible:
		_log.finish()
	_status.text = ""
	_ride_button.disabled = _loading_route or _regenerating
	_set_continuation_disabled(_ride_button.disabled)


## Back to the gallery must not enter a ride when preparation finishes later.
func cancel_preparation() -> void:
	if _building or _preparing_scene:
		_building = false
		finish_preparation()


## Builds the world kept from an older ORQWA again with this one (R101); the rider rides after.
func _regenerate_world() -> void:
	if not _orqwa.regenerate_world():
		return
	_regenerating = true
	_regenerate.hide()
	_show_building()


func _show_building() -> void:
	_ride_button.disabled = true
	_set_continuation_disabled(true)
	_loading_bar.value = 0.0
	_loading_bar.show()
	_log.start()
	_status.text = tr("Building 3D world") + " …"


## Shows why riding is not possible (e.g. no trainer); empty clears it.
func show_status(message: String) -> void:
	_status.text = message


func _init() -> void:
	add_theme_constant_override("separation", 24)
	var left: VBoxContainer = VBoxContainer.new()
	left.size_flags_vertical = Control.SIZE_EXPAND_FILL
	left.add_theme_constant_override("separation", 14)
	var back: Button = Button.new()
	back.icon = UiIcons.texture("back")
	back.tooltip_text = tr("Back")
	back.focus_mode = Control.FOCUS_NONE
	back.pressed.connect(func() -> void: back_requested.emit())
	_title.add_leading(back)
	_title.edit_requested.connect(_open_editor)
	_delete_button.icon = UiIcons.texture("bin")
	_delete_button.tooltip_text = tr("Delete course")
	_delete_button.focus_mode = Control.FOCUS_NONE
	_delete_button.pressed.connect(_confirm_delete.popup_centered)
	UiTheme.danger_button(_delete_button)
	# Beside the pencil, as on a ride in the history (#190).
	_title.add_action(_delete_button)
	left.add_child(_title)
	# The figures beside the map, or under it in a small window (R94).
	var overview: ReflowGrid = ReflowGrid.new()
	overview.expand_room = MAP_ROOM
	overview.add_theme_constant_override("h_separation", 24)
	overview.add_theme_constant_override("v_separation", 14)
	_path_card.custom_minimum_size = Vector2(0, 200)
	_path_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_path_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
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
	var left_scroll: ScrollContainer = UiTheme.scrolling(left)
	left_scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left_scroll.size_flags_stretch_ratio = COURSE_SHARE
	add_child(left_scroll)

	var panel: PanelContainer = PanelContainer.new()
	panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var panel_content: VBoxContainer = VBoxContainer.new()
	panel_content.add_theme_constant_override("separation", 14)
	var right: VBoxContainer = VBoxContainer.new()
	right.size_flags_vertical = Control.SIZE_EXPAND_FILL
	right.add_theme_constant_override("separation", 14)
	right.add_child(UiTheme.caption(tr("Ride options")))
	_options.changed.connect(_on_options_changed)
	right.add_child(_options)
	right.add_child(UiTheme.caption(tr("Race against")))
	right.add_child(_ghost)
	var fill: Control = Control.new()
	fill.size_flags_vertical = Control.SIZE_EXPAND_FILL
	right.add_child(fill)
	_loading_bar.max_value = 1.0
	_loading_bar.show_percentage = false
	_loading_bar.custom_minimum_size = Vector2(0, 8)
	# A world kept from an older ORQWA is ridden as it is until the rider has it built again.
	_regenerate.text = tr("Regenerate world")
	_regenerate.tooltip_text = tr(
		"Its 3D world was kept from an older ORQWA: build it with this one"
	)
	_regenerate.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	_regenerate.pressed.connect(_regenerate_world)
	_regenerate.hide()
	right.add_child(_regenerate)
	right.add_child(_log)
	right.add_child(_loading_bar)
	var status_row: HBoxContainer = HBoxContainer.new()
	status_row.add_theme_constant_override("separation", 10)
	_spinner.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_spinner.hide()
	status_row.add_child(_spinner)
	_status.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.add_theme_color_override("font_color", UiTheme.MUTED)
	status_row.add_child(_status)
	_videos_button.text = tr("Allow videos")
	_videos_button.hide()
	_videos_button.pressed.connect(_on_videos_pressed)
	status_row.add_child(_videos_button)
	right.add_child(status_row)
	_ride_button.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_ride_button.text = tr("Ride", "ride_action")
	_ride_button.custom_minimum_size = Vector2(0, 56)
	_ride_button.add_theme_font_size_override("font_size", 22)
	_ride_button.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_ride_button.pressed.connect(_on_ride_pressed)
	_ride_button.add_to_group(RemoteNavigation.FIRST)
	# The build log grows while preparing a world; Ride keeps its place on a short screen.
	var right_scroll: ScrollContainer = UiTheme.scrolling(right)
	right_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	panel_content.add_child(right_scroll)
	panel_content.add_child(_ride_button)
	_continuation.add_theme_constant_override("separation", 12)
	_progress_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_continuation.add_child(_progress_label)
	_progress_bar.max_value = 1.0
	_progress_bar.show_percentage = false
	_progress_bar.custom_minimum_size.y = 4.0
	_continuation.add_child(_progress_bar)
	var actions: HBoxContainer = HBoxContainer.new()
	actions.add_theme_constant_override("separation", 10)
	_continue_button.text = tr("Continue ride")
	_from_start.text = tr("From start")
	_reset_progress.text = tr("Reset progress")
	_continue_button.add_theme_stylebox_override("normal", UiTheme.accent_button())
	UiTheme.danger_button(_reset_progress)
	for button: Button in [_continue_button, _from_start, _reset_progress]:
		button.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		button.custom_minimum_size.y = 48.0
		actions.add_child(button)
	# Uniform minimum widths keep translated labels from changing the relative button sizes.
	var action_width: float = 0.0
	for button: Button in [_continue_button, _from_start, _reset_progress]:
		action_width = maxf(action_width, button.get_combined_minimum_size().x)
	for button: Button in [_continue_button, _from_start, _reset_progress]:
		button.custom_minimum_size.x = action_width
	_continue_button.pressed.connect(func() -> void: _choose_ride(true))
	_from_start.pressed.connect(func() -> void: _choose_ride(false))
	_reset_progress.pressed.connect(_on_reset_progress)
	_continuation.add_child(actions)
	_continuation.hide()
	panel_content.add_child(_continuation)
	panel.add_child(panel_content)
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
	var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
	var grid: GridContainer = CourseCard.figure_rows(_course, imperial)
	_add_estimate(grid)
	_figures.add_child(grid)


## The time the rider would take at the planned share of their FTP (R66), once the route is
## loaded.
func _add_estimate(grid: GridContainer) -> void:
	if _loading_route or _orqwa.loaded_course() != _path or not _orqwa.has_route():
		return
	_estimated_flat_descents = _options.options()["flat_descents"]
	var estimate: Dictionary = _orqwa.estimated_time(_estimated_flat_descents)
	if estimate["time_s"] == null:
		return
	var seconds: float = estimate["time_s"]
	var share: float = estimate["ftp_share_pct"]
	var caption: String = tr("Time at %d %% FTP") % roundi(share)
	CourseCard.add_figure(grid, caption, CourseCard.estimate(seconds))


func _on_options_changed() -> void:
	if _options.options()["flat_descents"] != _estimated_flat_descents:
		_show_figures()


## The video is known once the route is loaded; until then only the name can change.
func _open_editor() -> void:
	var loaded: bool = not _loading_route and _orqwa.loaded_course() == _path and _orqwa.has_route()
	var video: Dictionary = _orqwa.video() if loaded else {}
	var course_name: String = _course.get("name", "")
	_edit_dialog.edit(_orqwa, course_name, video, loaded)


func _rename(new_name: String) -> void:
	if not _orqwa.rename_course(_path, new_name):
		return
	_course["name"] = new_name
	_title.text = new_name
	course_changed.emit()


func _delete() -> void:
	if _orqwa.delete_course(_path):
		_path = ""
		course_changed.emit()
		back_requested.emit()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_instance_valid(_ride_button):
		_ride_button.text = tr("Ride", "ride_action")
		_size_continuation_actions.call_deferred()


func _size_continuation_actions() -> void:
	var width: float = 0.0
	for button: Button in [_continue_button, _from_start, _reset_progress]:
		button.custom_minimum_size.x = 0.0
		width = maxf(width, button.get_minimum_size().x)
	for button: Button in [_continue_button, _from_start, _reset_progress]:
		button.custom_minimum_size.x = width


func _on_route_loaded(_route: Dictionary) -> void:
	if _loading_route and _orqwa.loaded_course() == _path:
		_show_route()


## The route is loaded: climbs, records and riding.
func _show_route() -> void:
	if _loading_route:
		_status.text = ""
	_loading_route = false
	_spinner.hide()
	_ride_button.disabled = _regenerating
	_regenerate.visible = not _regenerating and _orqwa.kept_world_outdated(_path)
	_show_option_groups()
	_show_figures()
	var climbs: Dictionary = _orqwa.climbs()
	var climb_list: Array = climbs.get("climbs", [])
	_profile.set_climbs(climb_list)
	_profile.set_profile(_orqwa.elevation_profile(600))
	_show_continuation()
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
	var ftp: float = _orqwa.profile().get("ftp_w", 200.0)
	_ghost.configure(_orqwa.has_personal_best(), ftp)
	_keep_preview()


func _on_loading_progress(step: String, unit: String, done: int, total: int) -> void:
	if not (_building or _regenerating):
		return
	_loading_bar.value = float(done) / float(maxi(total, 1))
	_log.step(step, unit, done, total)
	_status.text = tr(step) + " …"


func _on_world_ready(_info: Dictionary) -> void:
	if not (_building or _regenerating):
		return
	var riding: bool = _building
	_building = false
	_regenerating = false
	if riding:
		ready_to_ride.emit()
	else:
		finish_preparation()


## A course without a map of its whole land, from before #203, gets one from the map loaded
## with it.
func _keep_preview() -> void:
	if _path.is_empty() or not _orqwa.course_preview_outdated(_path):
		return
	var png: PackedByteArray = await MapPreview.capture(
		get_tree(), _orqwa.course_map(_path), _orqwa.track(2000)
	)
	if not png.is_empty():
		_orqwa.set_course_preview(_path, png)
		_path_card.set_map(png)
		course_changed.emit()


func _on_failed(message: String) -> void:
	if visible and (_loading_route or _building or _regenerating or _preparing_scene):
		_loading_route = false
		_building = false
		_preparing_scene = false
		_regenerating = false
		_spinner.hide()
		_loading_bar.hide()
		_status.text = message
		_videos_button.visible = _video_refused()


## Whether opening failed because Android lets ORQWA read only the videos it wrote itself in
## its folder (Android 16, R149): a sync app put this one there.
func _video_refused() -> bool:
	if not Engine.has_singleton(ANDROID):
		return false
	var failure: Dictionary = _orqwa.last_failure()
	return (
		failure.get("message", "") == "Could not open %s."
		and not Engine.get_singleton(ANDROID).call(&"canReadVideos")
	)


func _on_videos_pressed() -> void:
	var android: Object = Engine.get_singleton(ANDROID)
	if not android.is_connected("videos_allowed", _on_videos_allowed):
		android.connect("videos_allowed", _on_videos_allowed)
	android.call(&"askToReadVideos")


func _on_videos_allowed(allowed: bool) -> void:
	if not allowed:
		_status.text = tr(
			"Not allowed. In Android's settings for ORQWA, set Photos and videos to Allow all."
		)
		return
	_videos_button.hide()
	_loading_route = true
	_spinner.show()
	_status.text = tr("Loading the course") + " …"
	_orqwa.open_course(_path)


func _show_option_groups() -> void:
	var video: Dictionary = _orqwa.video()
	# A video course is ridden either way, unless it has no place (Tacx RLV): then only along
	# its video.
	var located: bool = video.get("located", true)
	_options.show_option_groups(located, not video.is_empty())


func _on_ride_pressed() -> void:
	_choose_ride(false)


func _choose_ride(continuing: bool) -> void:
	_continuing = continuing
	var video: Dictionary = _orqwa.video()
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
	var options: Dictionary = _options.options()
	options["continue_course"] = _continuing
	ride_requested.emit(options, _ghost.choice())


func _show_continuation() -> void:
	var point: Dictionary = _orqwa.continuation_point()
	_show_continuation_point(point)


func _show_continuation_point(point: Dictionary) -> void:
	_continuation.visible = not point.is_empty()
	_ride_button.visible = point.is_empty()
	_set_continuation_disabled(_building or _regenerating or _preparing_scene)
	if point.is_empty():
		_profile.set_rider_distance(0.0)
		_path_card.set_marker({})
		return
	var distance: float = point["distance_m"]
	var remaining: float = point["remaining_m"]
	var length: float = point["length_m"]
	var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
	var divisor: float = 1609.344 if imperial else 1000.0
	var unit: String = "mi" if imperial else "km"
	_progress_label.text = (
		tr("Saved at %s · %s remaining")
		% ["%.1f %s" % [distance / divisor, unit], "%.1f %s" % [remaining / divisor, unit]]
	)
	_progress_bar.value = distance / length
	_profile.set_rider_distance(distance)
	_path_card.set_marker(_orqwa.course_position(distance))


func _set_continuation_disabled(disabled: bool) -> void:
	for button: Button in [_continue_button, _from_start, _reset_progress]:
		button.disabled = disabled


func _on_reset_progress() -> void:
	if _orqwa.reset_continuation():
		_show_continuation()
		course_changed.emit()


func _add_video(video: String, marks: PackedVector2Array) -> void:
	if _orqwa.add_video(video, marks):
		_status.text = tr("Video added: this course is ridden along it now.")
		_show_option_groups()
		course_changed.emit()


func _align_video(marks: PackedVector2Array) -> void:
	if _orqwa.align_video(marks):
		_status.text = tr("Video aligned with the route.")


## Back to riding the course in 3D only.
func _remove_video() -> void:
	if _orqwa.remove_video():
		_status.text = tr("Video removed: this course is ridden in 3D.")
		_show_option_groups()
		course_changed.emit()
