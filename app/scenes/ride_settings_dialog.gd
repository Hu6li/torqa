class_name RideSettingsDialog
extends AcceptDialog
## Everything that can be changed during a ride, in one place (R48): the workout (R56), the
## ride options (camera, difficulty, descents, time of day, weather) and the rider's HUD,
## applied as they change; plus finishing and saving, or aborting without saving after a
## confirmation (R49).

signal options_changed(options: Dictionary)
signal workout_changed(workout: Dictionary)
signal hud_changed(layout: PackedStringArray)
signal finish_requested
signal abort_requested

var _tabs: TabContainer = TabContainer.new()
var _workout: WorkoutOptions = WorkoutOptions.new()
var _options: RideOptions = RideOptions.new()
var _hud: HudEditor = HudEditor.new()
var _confirm_abort: ConfirmationDialog = ConfirmationDialog.new()


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Ride settings")
	ok_button_text = tr("Done")
	min_size = Vector2i(860, 480)
	_workout.name = tr("Workout")
	_tabs.add_child(_workout)
	_options.name = tr("Ride")
	_tabs.add_child(_options)
	_hud.name = tr("HUD")
	_tabs.add_child(_hud)
	add_child(_tabs)
	_workout.changed.connect(_on_workout_changed)
	_options.changed.connect(func() -> void: options_changed.emit(_options.options()))
	_hud.layout_changed.connect(func(layout: PackedStringArray) -> void: hud_changed.emit(layout))
	add_button(tr("Abort without saving"), true, "abort")
	add_button(tr("Finish & save"), false, "finish")
	custom_action.connect(_on_action)
	_confirm_abort.theme = theme
	_confirm_abort.title = tr("Abort ride?")
	_confirm_abort.dialog_text = tr("The ride ends and nothing is saved.")
	_confirm_abort.ok_button_text = tr("Abort ride")
	_confirm_abort.confirmed.connect(func() -> void: abort_requested.emit())
	add_child(_confirm_abort)


## The rider's heart-rate zones, FTP and structured workouts, for changing a workout (see
## `WorkoutOptions`); files are imported before a ride, not during it.
func configure_workout(zones: PackedVector2Array, ftp_w: float, plans: Array) -> void:
	_workout.configure(zones, ftp_w)
	_workout.set_plans(plans)
	_workout.allow_import(false)


## Opens the dialog showing the ride's current `options` and HUD `layout`; `world_options`
## false shows those of a ride along a video instead of those of the 3D world. A workout's
## options carry `workout` and `on_course` (see `StartPage.ride_started`): its workout shows,
## and a workout on its own has no ride options.
func edit(
	options: Dictionary, layout: PackedStringArray, imperial: bool, world_options: bool = true
) -> void:
	var workout: Dictionary = options.get("workout", {})
	var on_course: bool = options.get("on_course", false)
	_tabs.set_tab_hidden(_workout.get_index(), workout.is_empty())
	_tabs.set_tab_hidden(_options.get_index(), not workout.is_empty() and not on_course)
	_tabs.current_tab = 0 if not workout.is_empty() else _options.get_index()
	if not workout.is_empty():
		_workout.set_workout(workout)
	_options.set_options(options)
	_options.show_option_groups(world_options, not world_options, workout.is_empty())
	_hud.edit(layout, imperial)
	popup_centered(Vector2i(960, 600))


func _on_workout_changed() -> void:
	var workout: Dictionary = _workout.workout()
	workout["plan"] = _workout.plan()
	workout_changed.emit(workout)


func _on_action(action: StringName) -> void:
	hide()
	if action == &"finish":
		finish_requested.emit()
	elif action == &"abort":
		_confirm_abort.popup_centered()
