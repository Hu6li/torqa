class_name RideScreen
extends Control
## The live ride HUD: a compact metrics column, the map with the elevation profile below it,
## status toasts and one settings button (R48) — the rest of the screen is the ride. A workout
## (R56) adds its targets; on its own (R58) it has no map or world, but a chart of power and
## heart rate over its own backdrop.

signal closed
## The ride was saved to `path`; show its summary (R42).
signal summary_requested(path: String)
## The rider wants the overlay (R55), or the whole screen back.
signal overlay_requested(on: bool)
## The rider wants to switch between full screen and a window (#376).
signal fullscreen_requested
## The rider wants the overlay `steps` sizes larger, or smaller if negative (#124).
signal overlay_zoom_requested(steps: int)

## Keys during the ride: C camera; M play/pause music, "." next and "," previous track.
# i18n-begin
const MUSIC_KEYS: Dictionary[Key, Array] = {
	KEY_M: ["play_pause", "Music: play / pause"],
	KEY_PERIOD: ["next", "Music: next track"],
	KEY_COMMA: ["previous", "Music: previous track"],
}
# i18n-end
const TOAST_SECONDS: float = 4.0
## The frame-time budget of every preset (R43): 60 fps, with some slack; held below it this
## long, a lower preset is suggested once per ride.
const FRAME_BUDGET_S: float = 1.0 / 60.0 * 1.15
const SLOW_FOR_S: float = 10.0
## Speeds of a simulated ride (#53).
const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048
## How often the workout chart is redrawn: samples come once a second.
const CHART_EVERY_S: float = 1.0
## The workout chart spans at least this long at first, so its start is not stretched.
const CHART_MIN_S: float = 600.0

var _orqwa: OrqwaApp
var _world: RideWorld
var _settings_dialog: RideSettingsDialog = RideSettingsDialog.new()
## The pause opened the ride settings: going on closes them again.
var _settings_for_pause: bool = false
## The ride options in effect (`RideOptions.options()`).
var _options: Dictionary = {}
var _sound: RideSound = RideSound.new()
var _imperial: bool = false
var _climb_panel: PanelContainer = PanelContainer.new()
var _climb_title: Label = UiTheme.caption("")
var _climb_left: Label = UiTheme.value(20)
var _climb_time: Label = Label.new()
var _ghost_panel: PanelContainer = PanelContainer.new()
var _ghost_name: Label = UiTheme.caption("")
var _ghost_gap: Label = UiTheme.value(20)
var _finished: bool = false
var _saved: bool = false
var _toast_left: float = 0.0
## The moments of the ride (R124).
var _moment_card: MomentCard = MomentCard.new()
var _chime: MomentChime = MomentChime.new()
## Simulated rides (fake trainer): speed, jumps on map and profile, free camera (#53).
var _bar: RideBar = RideBar.new()
var _group_button: Button = Button.new()
var _group_popup: PopupPanel = PopupPanel.new()
var _group_controls: GroupRidesTab = GroupRidesTab.new()
var _simulating: bool = false
## The workout being ridden (`WorkoutsTab.workout()`), empty on a plain ride.
var _workout: Dictionary = {}
var _workout_panel: WorkoutPanel = WorkoutPanel.new()
## A workout on its own: no world behind the screen, a chart instead of map and profile.
var _backdrop: ColorRect = ColorRect.new()
var _chart_panel: PanelContainer = PanelContainer.new()
var _chart: RideChart = RideChart.new()
var _chart_left: float = 0.0
## A structured workout's steps above the chart, the part done dimmed.
var _plan_chart: WorkoutChart = WorkoutChart.new()
## The overlay (R55): only the HUD and the workout, the rest of the screen hidden meanwhile.
var _overlay_hud: OverlayHud = OverlayHud.new()
var _hidden_by_overlay: Array[Control] = []
var _time_scale: float = 1.0
## The virtual gear shown last (R9), to tell the rider of a shift; 0 before the first.
var _gear: int = 0
var _frame_time: float = 0.0
var _slow_for: float = 0.0
var _budget_noted: bool = false

@onready var _hud: HudPanel = %Metrics
@onready var _minimap: Minimap = %Minimap
@onready var _profile: ElevationProfile = %Profile
@onready var _profile_info: Label = %ProfileInfo
@onready var _toast: PanelContainer = %Toast
@onready var _toast_label: Label = %ToastLabel


func bind(orqwa: OrqwaApp, world: RideWorld) -> void:
	_orqwa = orqwa
	_world = world
	_orqwa.device_connected.connect(_on_device_connected)
	_orqwa.device_disconnected.connect(_on_device_disconnected)
	_orqwa.remembered_missing.connect(_on_remembered_missing)
	_settings_dialog.bind(_orqwa)
	# Deferred: the handler calls back into ORQWA, which is still busy emitting the signal.
	_orqwa.ride_finished.connect(_on_ride_finished, CONNECT_DEFERRED)
	_orqwa.ride_saved.connect(_on_ride_saved)
	_orqwa.moment.connect(_on_moment)
	_orqwa.failed.connect(_on_failed)
	_orqwa.control_requested.connect(_on_control_requested)
	_group_controls.bind(orqwa)


## Prepares the screen for a new ride on the loaded route with `options` in effect.
func begin(options: Dictionary) -> void:
	_options = options
	_apply_sound_options(options)
	_finished = false
	_saved = false
	_gear = 0
	_moment_card.clear()
	_bar.show_settings(true)
	_bar.show_back(false)
	_bar.show_pause(true)
	_show_paused(false)
	_bar.folded = _orqwa.ride_bar_folded()
	_workout = options.get("workout", {})
	var on_its_own: bool = not _workout.is_empty() and not options.get("on_course", false)
	_backdrop.visible = on_its_own
	_chart_panel.visible = on_its_own
	_chart.set_series(PackedVector2Array(), PackedVector2Array(), PackedVector2Array())
	_chart_left = 0.0
	# Heart-rate zones span half the maximum to the maximum: the chart shows all of them.
	var max_heart_rate: float = _orqwa.profile().get("max_heart_rate_bpm", 185.0)
	_chart.heart_rate_range = Vector2(max_heart_rate * 0.5, max_heart_rate)
	(get_node("Attribution") as Control).visible = not on_its_own
	for panel: Control in [$RightColumn/MapPanel, $RightColumn/ProfilePanel]:
		panel.visible = not on_its_own
	_workout_panel.visible = not _workout.is_empty()
	_workout_panel.show_workout(_workout)
	var rider: Dictionary = _orqwa.profile()
	var ftp: float = rider.get("ftp_w", 200.0)
	_settings_dialog.configure_workout(_orqwa.heart_rate_zones(), ftp, _orqwa.workouts())
	_show_plan(ftp)
	_minimap.set_track(_orqwa.track(2000))
	_minimap.set_map(_orqwa.minimap_mesh())
	_profile.set_profile(_orqwa.elevation_profile(600))
	var climb_info: Dictionary = _orqwa.climbs()
	var climbs: Array = climb_info.get("climbs", [])
	_profile.set_climbs(climbs)
	_climb_panel.hide()
	var profile: Dictionary = _orqwa.profile()
	_imperial = profile.get("units", "metric") == "imperial"
	_hud.imperial = _imperial
	_hud.show_layout(_orqwa.hud_layout())
	if not _orqwa.trainer_connected():
		_show_toast(tr("Waiting for the trainer…"))
	# Workouts are not sped up: a simulated heart beats in real time.
	var simulating: bool = _orqwa.simulating() and _workout.is_empty()
	_simulating = simulating
	_bar.set_simulating(simulating)
	_minimap.jumpable = simulating
	_profile.jumpable = simulating
	_set_time_scale(1.0)
	_frame_time = 0.0
	_slow_for = 0.0
	_budget_noted = false


func _ready() -> void:
	add_child(_sound)
	# The map panel clips the map to its rounded shape; it must be opaque, as the clip mask
	# also takes its transparency.
	var map_panel: Panel = _minimap.get_parent() as Panel
	var opaque: StyleBoxFlat = UiTheme.panel()
	opaque.bg_color = Color(0.1, 0.11, 0.12)
	map_panel.add_theme_stylebox_override("panel", opaque)
	_build_backdrop()
	_build_workout_panel()
	_build_climb_panel()
	_build_ghost_panel()
	_minimap.jump_requested.connect(
		func(position_m: Vector2) -> void: _orqwa.jump_near(position_m.x, position_m.y)
	)
	_profile.jump_requested.connect(
		func(distance_m: float) -> void: _orqwa.jump_to_distance(distance_m)
	)
	add_child(_settings_dialog)
	_settings_dialog.options_changed.connect(_on_options_changed)
	_settings_dialog.workout_changed.connect(_on_workout_changed)
	_settings_dialog.hud_changed.connect(_on_hud_changed)
	_settings_dialog.profile_changed.connect(_on_profile_changed)
	_settings_dialog.finish_requested.connect(_on_finish_requested)
	_settings_dialog.abort_requested.connect(_on_abort_requested)
	# The controls in one bar at the bottom left (#189); the keys work with it folded too.
	_bar.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_LEFT, Control.PRESET_MODE_MINSIZE, 24)
	_bar.grow_horizontal = Control.GROW_DIRECTION_END
	_bar.grow_vertical = Control.GROW_DIRECTION_BEGIN
	_bar.settings_requested.connect(_open_settings)
	_bar.overlay_requested.connect(func() -> void: overlay_requested.emit(true))
	_bar.fullscreen_requested.connect(fullscreen_requested.emit)
	_bar.back_requested.connect(closed.emit)
	_bar.speed_chosen.connect(_set_time_scale)
	_bar.folded_changed.connect(func(folded: bool) -> void: _orqwa.set_ride_bar_folded(folded))
	add_child(_bar)
	_group_button.text = tr("Group Rides")
	_group_button.set_anchors_and_offsets_preset(
		Control.PRESET_BOTTOM_RIGHT, Control.PRESET_MODE_MINSIZE, 24
	)
	_group_button.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	_group_button.grow_vertical = Control.GROW_DIRECTION_BEGIN
	_group_button.pressed.connect(func() -> void: _group_popup.popup_centered(Vector2i(640, 480)))
	add_child(_group_button)
	_group_controls.compact = true
	var group_scroll: ScrollContainer = ScrollContainer.new()
	group_scroll.custom_minimum_size = Vector2(320, 240)
	_group_controls.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	group_scroll.add_child(_group_controls)
	_group_popup.add_child(group_scroll)
	add_child(_group_popup)
	_overlay_hud.hide()
	_overlay_hud.leave_requested.connect(func() -> void: overlay_requested.emit(false))
	_overlay_hud.zoom_requested.connect(overlay_zoom_requested.emit)
	_overlay_hud.pause_requested.connect(_toggle_pause)
	_bar.pause_requested.connect(_toggle_pause)
	add_child(_moment_card)
	_moment_card.shown.connect(func(_text: String) -> void: _play_chime())
	add_child(_chime)
	add_child(_overlay_hud)


func _unhandled_input(event: InputEvent) -> void:
	var key: InputEventKey = event as InputEventKey
	if not visible or key == null or not key.pressed or key.echo:
		return
	if is_overlay():
		# The overlay is too small for dialogs; it only goes back or changes its size.
		if key.keycode in [KEY_O, KEY_ESCAPE]:
			overlay_requested.emit(false)
		elif key.keycode in [KEY_EQUAL, KEY_PLUS, KEY_KP_ADD]:
			overlay_zoom_requested.emit(1)
		elif key.keycode in [KEY_MINUS, KEY_KP_SUBTRACT]:
			overlay_zoom_requested.emit(-1)
		elif key.keycode in [KEY_P, KEY_SPACE]:
			_toggle_pause()
		else:
			return
		get_viewport().set_input_as_handled()
		return
	if key.keycode in [KEY_UP, KEY_DOWN] and not _world.is_free_camera():
		# Virtual gears (R9); with a cassette the rider shifts on the bike.
		_orqwa.shift(1 if key.keycode == KEY_UP else -1)
	elif key.keycode == KEY_O and not _finished:
		overlay_requested.emit(true)
	elif key.keycode in [KEY_P, KEY_SPACE] and not _finished:
		_toggle_pause()
	elif key.keycode == KEY_C:
		_cycle_camera()
	elif key.keycode == KEY_S and not _finished:
		_open_settings()
	elif key.keycode in [KEY_H, KEY_E]:
		_step_difficulty(1 if key.keycode == KEY_H else -1)
	elif key.keycode in [KEY_I, KEY_L]:
		_step_intensity(1 if key.keycode == KEY_I else -1)
	elif key.keycode == KEY_N:
		_skip_step()
	elif _simulating and key.keycode in [KEY_EQUAL, KEY_PLUS, KEY_KP_ADD]:
		_step_time_scale(1)
	elif _simulating and key.keycode in [KEY_MINUS, KEY_KP_SUBTRACT]:
		_step_time_scale(-1)
	elif MUSIC_KEYS.has(key.keycode):
		_control_music(MUSIC_KEYS[key.keycode])
	else:
		return
	get_viewport().set_input_as_handled()


func _process(delta: float) -> void:
	if _orqwa != null:
		_group_button.visible = (
			not is_overlay() and _orqwa.group_status().get("phase", "idle") != "idle"
		)
	if _orqwa != null:
		_sound.update(_orqwa, visible and not _finished)
	if _toast_left > 0.0:
		_toast_left -= delta
		if _toast_left <= 0.0:
			_toast.hide()
	if not visible or _finished:
		return
	_watch_frame_time(delta)
	var state: Dictionary = _orqwa.ride_state()
	if state.is_empty():
		return
	if is_overlay():
		_overlay_hud.show_state(state)
	_show_gear(state["gear"])
	var metrics: Dictionary = state["metrics"]
	_hud.show_values(metrics, state["watts_per_kg"], state["power_zone"])
	var zones: Dictionary = state["metric_zones"]
	_hud.show_zones(zones)
	_chart.heart_rate_target = _workout_panel.show_state(state["workout"], state["heart_rate"])
	if _chart_panel.visible:
		_chart_left -= delta
		if _chart_left <= 0.0:
			_chart_left = CHART_EVERY_S
			_plan_chart.progress_s = _workout_elapsed(state)
			var chart: Dictionary = _orqwa.ride_chart(600)
			var power: PackedVector2Array = chart["power"]
			var heart_rate: PackedVector2Array = chart["heart_rate"]
			_chart.set_series(PackedVector2Array(), power, heart_rate)
	if not state.has("x"):
		return
	var grade: float = state["grade"]
	var elevation: float = state["elevation_m"]
	var distance_m: float = state["distance_m"]
	var x_m: float = state["x"]
	var y_m: float = state["y"]
	var heading: float = state["heading"]
	if _imperial:
		_profile_info.text = "%d ft  ·  %+.1f %%" % [roundi(elevation / METERS_PER_FOOT), grade]
	else:
		_profile_info.text = "%d m  ·  %+.1f %%" % [roundi(elevation), grade]
	_minimap.set_rider(Vector2(x_m, y_m), heading)
	_show_climb(state["climb"])
	_show_ghost(state["ghost"])
	_profile.set_rider_distance(distance_m)


## Whether the screen shows as the overlay.
func is_overlay() -> bool:
	return _overlay_hud.visible


## Shows only the HUD and the workout, for the overlay window (R55), or the whole screen again.
func set_overlay(on: bool) -> void:
	if on == is_overlay():
		return
	if on:
		_moment_card.clear()
		_overlay_hud.begin(_orqwa.hud_layout(), _imperial, _workout)
		_hidden_by_overlay.clear()
		for child: Node in get_children():
			var control: Control = child as Control
			if control != null and control != _overlay_hud and control.visible:
				control.hide()
				_hidden_by_overlay.append(control)
		_overlay_hud.show()
	else:
		_overlay_hud.hide()
		for control: Control in _hidden_by_overlay:
			# A message whose time ran out meanwhile stays gone.
			if control != _toast or _toast_left > 0.0:
				control.show()
		_hidden_by_overlay.clear()


## The size the overlay's content needs, in interface units.
func overlay_content_size() -> Vector2:
	return _overlay_hud.content_size()


## The outline of the overlay's content in window pixels.
func overlay_outline() -> PackedVector2Array:
	return _overlay_hud.clickable_outline()


## Tells the rider of a shift (R9).
func _show_gear(gear: Variant) -> void:
	if gear == null:
		return
	var info: Dictionary = gear
	var number: int = info["number"]
	var of: int = info["of"]
	if _gear != 0 and number != _gear:
		_show_toast(tr("Gear %d of %d") % [number, of])
	_gear = number


func _open_settings() -> void:
	_options["camera"] = _world.camera()
	var world: bool = not _orqwa.riding_along_video()
	_settings_dialog.edit(_options, _orqwa.hud_layout(), _imperial, world)


## Applies changed ride options at once: camera, conditions and sound in the world, difficulty
## and descents on the trainer.
func _on_options_changed(options: Dictionary) -> void:
	for key: String in ["workout", "on_course"]:
		if _options.has(key):
			options[key] = _options[key]
	_options = options
	_world.apply_options(options)
	var difficulty: float = options["difficulty"]
	var flat_descents: bool = options["flat_descents"]
	_orqwa.adjust_ride(difficulty, flat_descents)
	var video_sound: bool = options.get("video_sound", true)
	_orqwa.set_video_sound(video_sound)
	_apply_sound_options(options)


func _apply_sound_options(options: Dictionary) -> void:
	var wind: float = options.get("wind_volume", 50.0)
	var bike: float = options.get("bike_volume", 50.0)
	var weather: String = options.get("weather", "Clear")
	_orqwa.set_ambient_sound(wind, bike, weather)


## A changed workout reaches the trainer at once; the ride keeps the name it started with.
func _on_workout_changed(workout: Dictionary) -> void:
	workout["name"] = _workout.get("name", "")
	_workout = workout
	_options["workout"] = workout
	_orqwa.change_workout(workout)
	_workout_panel.show_workout(workout)
	var ftp: float = _orqwa.profile().get("ftp_w", 200.0)
	_show_plan(ftp)


## A structured workout's steps over the chart, its time axis as long as the workout.
func _show_plan(ftp: float) -> void:
	var plan: Dictionary = _workout.get("plan", {})
	_plan_chart.visible = not plan.is_empty()
	_chart.min_duration = CHART_MIN_S
	_chart.power_range = Vector2.ZERO
	if plan.is_empty():
		return
	var steps: Array = plan["steps"]
	_plan_chart.set_steps(steps, ftp)
	var duration: float = plan["duration_s"]
	_chart.min_duration = duration
	_chart.power_range = Vector2(0.0, _plan_chart.top_w())


## Time into a structured workout: ahead of the ride's once steps were skipped (#139).
func _workout_elapsed(state: Dictionary) -> float:
	var workout: Variant = state["workout"]
	if workout is Dictionary:
		var progress: Variant = workout["progress"]
		if progress is Dictionary:
			return progress["elapsed_s"]
	return state["elapsed_s"]


func _on_hud_changed(layout: PackedStringArray) -> void:
	var saved: PackedStringArray = _orqwa.set_hud_layout(layout)
	if not saved.is_empty():
		_hud.show_layout(saved)


## The rider changed their profile in the ride settings (R98): it is saved, the core's ride
## follows the weight, drivetrain and FTP, and the screen the rest. A heart-rate zone held
## follows the new zones; a structured workout's chart, the new FTP.
func _on_profile_changed(profile: Dictionary) -> void:
	var before: Dictionary = _orqwa.profile()
	var id: String = before.get("id", "")
	if _orqwa.save_profile(id, profile).is_empty():
		return
	var language: String = profile.get("language", "")
	ProfileTab.apply_language(language)
	_imperial = profile.get("units", "metric") == "imperial"
	_hud.imperial = _imperial
	_hud.show_layout(_orqwa.hud_layout())
	if profile.get("map_style") != before.get("map_style"):
		_minimap.set_map(_orqwa.minimap_mesh())
	var max_heart_rate: float = profile.get("max_heart_rate_bpm", 185.0)
	_chart.heart_rate_range = Vector2(max_heart_rate * 0.5, max_heart_rate)
	var ftp: float = profile.get("ftp_w", 200.0)
	var plans: Array = _orqwa.workouts()
	_settings_dialog.configure_workout(_orqwa.heart_rate_zones(), ftp, plans)
	if _workout.get("kind", "") == "zone":
		_orqwa.change_workout(_workout)
	elif _workout.get("kind", "") == "plan":
		for plan: Dictionary in plans:
			if plan["id"] == _workout.get("id", ""):
				_workout["plan"] = plan
	_show_plan(ftp)


func _on_finish_requested() -> void:
	_finish_ride()


func _on_abort_requested() -> void:
	_orqwa.abort_ride()
	_finished = true
	_bar.show_pause(false)
	closed.emit()


func _build_backdrop() -> void:
	_backdrop.color = Color(0.07, 0.08, 0.1)
	_backdrop.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_backdrop.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_backdrop.hide()
	add_child(_backdrop)
	move_child(_backdrop, 0)
	# Between the HUD and the workout panel, from the top down to the buttons.
	_chart_panel.anchor_right = 1.0
	_chart_panel.anchor_bottom = 1.0
	_chart_panel.offset_left = 308.0
	_chart_panel.offset_right = -348.0
	_chart_panel.offset_top = 24.0
	_chart_panel.offset_bottom = -92.0
	_chart.min_duration = CHART_MIN_S
	_chart_panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var rows: VBoxContainer = VBoxContainer.new()
	var legend: HBoxContainer = HBoxContainer.new()
	legend.add_theme_constant_override("separation", 16)
	# i18n-begin
	for entry: Array in [["Power", UiTheme.POWER_COLOR], ["Heart rate", UiTheme.HEART_RATE_COLOR]]:
		# i18n-end
		var text: String = entry[0]
		var color: Color = entry[1]
		var caption: Label = UiTheme.caption(text)
		caption.add_theme_color_override("font_color", color)
		legend.add_child(caption)
	rows.add_child(legend)
	# A structured workout's steps lie behind the power and heart rate, on the same scales.
	var area: Control = Control.new()
	area.size_flags_vertical = Control.SIZE_EXPAND_FILL
	for chart: Control in [_plan_chart, _chart]:
		chart.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
		area.add_child(chart)
	_plan_chart.modulate = Color(1, 1, 1, 0.4)
	_plan_chart.hide()
	rows.add_child(area)
	_chart_panel.add_child(rows)
	_chart_panel.hide()
	add_child(_chart_panel)
	move_child(_chart_panel, 1)


func _build_workout_panel() -> void:
	_workout_panel.hide()
	var column: VBoxContainer = $RightColumn
	column.add_child(_workout_panel)
	column.move_child(_workout_panel, 0)


func _build_ghost_panel() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	_ghost_name.add_theme_color_override("font_color", UiTheme.GHOST_COLOR)
	rows.add_child(_ghost_name)
	rows.add_child(_ghost_gap)
	_ghost_panel.add_child(rows)
	_ghost_panel.hide()
	($RightColumn as VBoxContainer).add_child(_ghost_panel)


## The ghost (`ride_state()["ghost"]`) on the map and profile, and the time gap to it.
func _show_ghost(ghost: Variant) -> void:
	if ghost == null:
		_ghost_panel.hide()
		_minimap.set_ghost(Vector2.ZERO, false)
		_profile.set_ghost_distance(-1.0)
		return
	var info: Dictionary = ghost
	var x_m: float = info["x"]
	var y_m: float = info["y"]
	var distance_m: float = info["distance_m"]
	var ghost_name: String = info["name"]
	_minimap.set_ghost(Vector2(x_m, y_m), true)
	_profile.set_ghost_distance(distance_m)
	_ghost_name.text = ghost_name.to_upper()
	if info["gap_s"] == null:
		_ghost_gap.text = tr("Finished")
		_ghost_gap.remove_theme_color_override("font_color")
	else:
		var gap: float = info["gap_s"]
		var behind: bool = gap > 0.0
		_ghost_gap.text = (
			(tr("%s behind") if behind else tr("%s ahead")) % UiTheme.duration(absf(gap))
		)
		_ghost_gap.add_theme_color_override(
			"font_color", UiTheme.HEART_RATE_COLOR if behind else UiTheme.CLIMB_COLORS["Cat 4"]
		)
	_ghost_panel.show()


func _build_climb_panel() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	rows.add_child(_climb_title)
	rows.add_child(_climb_left)
	_climb_time.add_theme_font_size_override("font_size", 14)
	_climb_time.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(_climb_time)
	_climb_panel.add_child(rows)
	_climb_panel.hide()
	($RightColumn as VBoxContainer).add_child(_climb_panel)


## Progress on the climb the rider is on (`ride_state()["climb"]`), hidden between climbs.
func _show_climb(climb: Variant) -> void:
	if climb == null:
		_climb_panel.hide()
		return
	var info: Dictionary = climb
	var index: int = info["index"]
	var count: int = info["count"]
	var category: String = info["category"]
	var left_m: float = info["length_m"] - info["ridden_m"]
	var grade: float = info["grade"]
	var elapsed: float = info["elapsed_s"]
	_climb_title.text = (tr("%s  ·  climb %d of %d") % [tr(category).to_upper(), index + 1, count])
	var color: Color = UiTheme.CLIMB_COLORS.get(category, UiTheme.MUTED)
	_climb_title.add_theme_color_override("font_color", color)
	var left: String = (
		"%.2f mi" % (left_m / 1000.0 / KM_PER_MILE)
		if _imperial
		else ("%.1f km" % (left_m / 1000.0) if left_m >= 1000.0 else "%d m" % roundi(left_m))
	)
	_climb_left.text = tr("%s to go  ·  %.1f %%") % [left, grade]
	var time: String = UiTheme.duration(elapsed)
	if info["best_s"] != null:
		var best: float = info["best_s"]
		time += "  ·  " + tr("best %s") % UiTheme.duration(best)
	_climb_time.text = time
	_climb_panel.show()


## Tells of a moment of the ride (R124) in the rider's voice: on a card, or in the overlay's
## bar; not at all with the voice off.
func _on_moment(moment: Dictionary) -> void:
	if not visible or _finished:
		return
	var voice: String = _orqwa.profile().get("voice", "casual")
	if voice == "off":
		return
	var line: String = MomentLines.line(moment, voice, _imperial)
	if is_overlay():
		_overlay_hud.tell(line)
		_play_chime()
	else:
		_moment_card.tell(MomentLines.figure(moment, _imperial), line)


## The chime of a moment, unless the app's sounds are off (R125).
func _play_chime() -> void:
	if _orqwa.sounds():
		_chime.play()


## Suggests a lower graphics preset when the ride stays below 60 fps (R43). Simulated rides
## are left alone: they are for trying courses out.
func _watch_frame_time(delta: float) -> void:
	if _budget_noted or _simulating or _orqwa.riding_along_video() or _backdrop.visible:
		return
	_frame_time = lerpf(_frame_time if _frame_time > 0.0 else delta, delta, 0.05)
	_slow_for = _slow_for + delta if _frame_time > FRAME_BUDGET_S else 0.0
	if _slow_for > SLOW_FOR_S and _orqwa.graphics_quality() != "low":
		_budget_noted = true
		_show_toast(
			tr("Below 60 fps: a lower graphics quality (Settings → Graphics) runs smoother.")
		)


func _set_time_scale(scale: float) -> void:
	_time_scale = _orqwa.set_time_scale(scale) if _orqwa != null else 1.0
	_bar.show_speed(_time_scale)


func _step_time_scale(step: int) -> void:
	var index: int = RideBar.TIME_SCALES.find(_time_scale)
	_set_time_scale(RideBar.TIME_SCALES[clampi(index + step, 0, RideBar.TIME_SCALES.size() - 1)])
	_show_toast(tr("Simulation: %d×") % roundi(_time_scale))


## A shifter's button asked for a control (#139): done as its key does.
func _on_control_requested(control: String) -> void:
	if not visible:
		return
	if control == "next_camera":
		# The overlay shows no world to look at.
		if not is_overlay():
			_cycle_camera()
	elif control == "overlay":
		if not _finished:
			overlay_requested.emit(not is_overlay())
	elif control == "pause":
		_toggle_pause()
	elif control in ["difficulty_up", "difficulty_down"]:
		_step_difficulty(1 if control == "difficulty_up" else -1)
	elif control in ["intensity_up", "intensity_down"]:
		_step_intensity(1 if control == "intensity_up" else -1)
	elif control == "skip_step":
		_skip_step()
	elif control == "finish_ride":
		# As the ride settings' Finish & save: saved, then the summary.
		if not _finished:
			_on_finish_requested()
	else:
		for music: Array in MUSIC_KEYS.values():
			if music[0] == control:
				_control_music(music)


## Makes the trainer difficulty a step harder or easier (#139, H / E), as the ride settings
## would.
func _step_difficulty(step: int) -> void:
	if _finished:
		return
	var difficulty: float = _orqwa.step_difficulty(step)
	if difficulty < 0.0:
		return
	_options["difficulty"] = difficulty
	_show_toast(tr("Trainer difficulty: %d %%") % roundi(difficulty))


## Makes the workout's powers a step harder or easier (#139, I / L); other rides keep theirs.
func _step_intensity(step: int) -> void:
	if _finished:
		return
	var intensity: float = _orqwa.step_intensity(step)
	if intensity >= 0.0:
		_show_toast(tr("Workout intensity: %d %%") % roundi(intensity))


## On with the next step of a structured workout (#139, N).
func _skip_step() -> void:
	if _orqwa.skip_step():
		_show_toast(tr("Workout: next step"))


## Plays, pauses or skips the music: `music` is a command and its message, as in MUSIC_KEYS.
func _control_music(music: Array) -> void:
	var command: String = music[0]
	var message: String = music[1]
	_orqwa.control_music(command)
	_show_toast(tr(message))


func _cycle_camera() -> void:
	if _orqwa.riding_along_video() or _backdrop.visible:
		return
	_show_toast(tr("Camera: %s") % tr(_world.cycle_camera()))


## Pauses the ride, or goes on with it: the clock, the road and the trainer wait (P).
func _toggle_pause() -> void:
	if _finished:
		return
	_show_paused(_orqwa.set_paused(not _orqwa.is_paused()))


## Paused, the ride settings open, to change what the pause was for (R98); the overlay has no
## room for them.
func _show_paused(paused: bool) -> void:
	_bar.show_paused(paused)
	_overlay_hud.show_paused(paused)
	if paused:
		_show_toast(tr("Paused"))
		_toast_left = INF
		if not is_overlay() and not _settings_dialog.visible:
			_open_settings()
			_settings_for_pause = true
		return
	if _toast_left == INF:
		_toast_left = 0.0
		_toast.hide()
	if _settings_for_pause:
		_settings_for_pause = false
		_settings_dialog.hide()


func _show_toast(message: String) -> void:
	_toast_label.text = message
	# The overlay has no room for messages.
	if not is_overlay():
		_toast.show()
	_toast_left = TOAST_SECONDS


## Devices are called by their own names; the phone remote by the core's, translated.
func _on_device_connected(device_name: String) -> void:
	_show_toast(tr("%s connected") % tr(device_name))


func _on_device_disconnected(device_name: String) -> void:
	_show_toast(tr("%s disconnected — reconnecting…") % tr(device_name))


## A reconnect from the ride settings (R69) did not find some devices.
func _on_remembered_missing(names: PackedStringArray) -> void:
	if visible:
		_show_toast(tr("%s not found") % ", ".join(names))


func _on_ride_finished() -> void:
	overlay_requested.emit(false)
	_finish_ride()


func _finish_ride() -> void:
	_finished = true
	_moment_card.clear()
	_settings_dialog.hide()
	_bar.show_pause(false)
	_bar.show_settings(false)
	_orqwa.finish_ride()
	if not _saved:
		# Nothing to save (e.g. the trainer never connected): offer the way back.
		_show_toast(tr("Nothing recorded."))
		_bar.show_back(true)


func _on_ride_saved(path: String) -> void:
	_finished = true
	_bar.show_pause(false)
	_saved = true
	_bar.show_settings(false)
	summary_requested.emit(path)


func _on_failed(message: String) -> void:
	if visible:
		_show_toast(message)
