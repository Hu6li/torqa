class_name CourseDetail
extends HBoxContainer
## One course (R40): its path card, elevation profile with climbs, key figures and the rider's
## records on it, next to the ride options (R48) and who to race (R20) → Ride.

signal back_requested
## Start riding with `options` (`RideOptions.options()`) against `ghost` (`GhostPicker`).
signal ride_requested(options: Dictionary, ghost: Dictionary)

var _torqa: TorqaApp
var _path: String = ""
var _ready_to_ride: bool = false
var _title: Label = Label.new()
var _figures: Label = Label.new()
var _path_card: PathCard = PathCard.new()
var _profile: ElevationProfile = ElevationProfile.new()
var _records: Label = Label.new()
var _options: RideOptions = RideOptions.new()
var _ghost: GhostPicker = GhostPicker.new()
var _ride_button: Button = Button.new()
var _loading_bar: ProgressBar = ProgressBar.new()
var _status: Label = Label.new()


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.loading_progress.connect(_on_loading_progress)
	_torqa.world_ready.connect(_on_world_ready)
	_torqa.failed.connect(_on_failed)


## Shows `course` (from `TorqaApp.courses()`) and loads it unless it is `loaded` already.
func open(course: Dictionary, loaded: bool) -> void:
	_path = course["path"]
	var imperial: bool = _torqa.profile().get("units", "metric") == "imperial"
	_title.text = course["name"]
	_figures.text = CourseCard.figures_text(course, imperial)
	var track: PackedVector2Array = course.get("track", PackedVector2Array())
	var profile: PackedVector2Array = course.get("profile", PackedVector2Array())
	_path_card.set_preview(track, profile)
	_profile.set_profile(profile)
	_profile.set_climbs([])
	_records.text = ""
	_status.text = ""
	if loaded:
		_on_world_ready({})
	else:
		_ready_to_ride = false
		_ride_button.disabled = true
		_loading_bar.value = 0.0
		_loading_bar.show()
		_torqa.open_course(_path)


## After a ride on it: the rider's records may have changed.
func refresh_records() -> void:
	if _ready_to_ride:
		_on_world_ready({})


## The ride options chosen here, as `RideOptions.options()` returns them.
func ride_options() -> Dictionary:
	return _options.options()


func _init() -> void:
	add_theme_constant_override("separation", 24)
	var left: VBoxContainer = VBoxContainer.new()
	left.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left.add_theme_constant_override("separation", 12)
	var back: Button = Button.new()
	back.text = tr("← Courses")
	back.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	back.pressed.connect(func() -> void: back_requested.emit())
	left.add_child(back)
	_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_title.add_theme_font_size_override("font_size", 28)
	left.add_child(_title)
	_figures.add_theme_color_override("font_color", UiTheme.MUTED)
	left.add_child(_figures)
	_path_card.custom_minimum_size = Vector2(0, 300)
	_path_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	left.add_child(_path_card)
	left.add_child(UiTheme.caption(tr("Elevation")))
	_profile.custom_minimum_size = Vector2(0, 110)
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
	var push: Control = Control.new()
	push.size_flags_vertical = Control.SIZE_EXPAND_FILL
	right.add_child(push)
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


## Shows why riding is not possible (e.g. no trainer); empty clears it.
func show_status(message: String) -> void:
	_status.text = message


func _on_loading_progress(step: String, _unit: String, done: int, total: int) -> void:
	if not visible or _ready_to_ride:
		return
	_loading_bar.value = float(done) / float(maxi(total, 1))
	_status.text = tr(step) + " …"


## The course is ready: records (from the loaded route) and riding.
func _on_world_ready(_info: Dictionary) -> void:
	if not visible and _path.is_empty():
		return
	_ready_to_ride = true
	_loading_bar.hide()
	_status.text = ""
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


func _on_failed(message: String) -> void:
	if visible and not _ready_to_ride:
		_loading_bar.hide()
		_status.text = message
