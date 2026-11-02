class_name RideSettingsDialog
extends AcceptDialog
## Everything that can be changed during a ride, in one place (R48): the workout (R56), the
## ride options (camera, difficulty, descents, time of day, weather), the rider's HUD and
## profile (R98), applied as they change; the devices, to reconnect them (R69); plus finishing
## and saving, or aborting without saving after a confirmation (R49).

signal options_changed(options: Dictionary)
signal workout_changed(workout: Dictionary)
signal hud_changed(layout: PackedStringArray)
## The rider changed their profile (R98): all of it, shaped like `OrqwaApp.profile()`.
signal profile_changed(profile: Dictionary)
signal finish_requested
signal abort_requested

var _tabs: TabContainer = TabContainer.new()
var _workout: WorkoutOptions = WorkoutOptions.new()
var _options: RideOptions = RideOptions.new()
var _hud: HudEditor = HudEditor.new()
var _profile: RideProfile = RideProfile.new()
var _devices: RideDevices = RideDevices.new()
var _orqwa: OrqwaApp
## The units the HUD editor shows its figures in, to follow the rider's.
var _imperial: bool = false
var _confirm_abort: ConfirmationDialog = ConfirmationDialog.new()
var _shared_workout: Label = Label.new()


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Ride settings")
	ok_button_text = tr("Done")
	min_size = Vector2i(860, 480)
	_workout.name = tr("Workout")
	_tabs.add_child(_workout)
	_options.name = tr("Ride", "ride_tab")
	_tabs.add_child(_options)
	_hud.name = tr("HUD")
	_tabs.add_child(_hud)
	var profile_page: ScrollContainer = UiTheme.scrolling(_profile)
	profile_page.name = tr("Profile")
	_tabs.add_child(profile_page)
	_devices.name = tr("Devices")
	_tabs.add_child(_devices)
	var content: VBoxContainer = VBoxContainer.new()
	_shared_workout.text = tr(
		"Use Group Rides for the shared workout. Your intensity stays personal."
	)
	_shared_workout.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_shared_workout.hide()
	content.add_child(_shared_workout)
	_tabs.size_flags_vertical = Control.SIZE_EXPAND_FILL
	content.add_child(_tabs)
	add_child(content)
	_translate_ride_tab()
	_workout.changed.connect(_on_workout_changed)
	_options.changed.connect(func() -> void: options_changed.emit(_options.options()))
	_hud.layout_changed.connect(func(layout: PackedStringArray) -> void: hud_changed.emit(layout))
	_profile.changed.connect(_on_profile_changed)
	add_button(tr("Abort without saving"), true, "abort")
	add_button(tr("Finish & save"), false, "finish")
	custom_action.connect(_on_action)
	_confirm_abort.theme = theme
	_confirm_abort.title = tr("Abort ride?")
	_confirm_abort.dialog_text = tr("The ride ends and nothing is saved.")
	_confirm_abort.ok_button_text = tr("Abort ride")
	_confirm_abort.confirmed.connect(func() -> void: abort_requested.emit())
	add_child(_confirm_abort)


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED:
		_translate_ride_tab()


func _translate_ride_tab() -> void:
	if is_instance_valid(_options) and _options.get_parent() == _tabs:
		_tabs.set_tab_title(_options.get_index(), tr("Ride", "ride_tab"))


## Shows the devices of `orqwa` and reconnects them (R69), and shows its rider's profile.
func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_options.bind(orqwa)
	_devices.bind(orqwa)


## The rider's heart-rate zones, FTP and structured workouts, for changing a workout (see
## `WorkoutOptions`); files are imported before a ride, not during it. The workout shown
## stays, also when the rider's FTP changed while the dialog is open (R98).
func configure_workout(zones: PackedVector2Array, ftp_w: float, plans: Array) -> void:
	var shown: Dictionary = _workout.workout()
	_workout.configure(zones, ftp_w)
	_workout.set_plans(plans)
	_workout.allow_import(false)
	if visible:
		_workout.set_workout(shown)


## Opens the dialog showing the ride's current `options` and HUD `layout`; `world_options`
## false shows those of a ride along a video instead of those of the 3D world. A workout's
## options carry `workout` and `on_course` (see `StartPage.ride_started`): its workout shows,
## and a workout on its own has no ride options.
func edit(
	options: Dictionary, layout: PackedStringArray, imperial: bool, world_options: bool = true
) -> void:
	var workout: Dictionary = options.get("workout", {})
	var on_course: bool = options.get("on_course", false)
	var shared: bool = _orqwa != null and _orqwa.group_status().get("workout_shared", false)
	_shared_workout.visible = shared
	_tabs.set_tab_hidden(_workout.get_index(), workout.is_empty() or shared)
	_tabs.set_tab_hidden(_options.get_index(), not workout.is_empty() and not on_course)
	_tabs.current_tab = 0 if not workout.is_empty() else _options.get_index()
	if shared:
		_tabs.current_tab = _options.get_index() if on_course else _hud.get_index()
	if not workout.is_empty():
		_workout.set_workout(workout)
	_options.set_options(options)
	_options.show_option_groups(world_options, not world_options, workout.is_empty())
	_imperial = imperial
	_hud.edit(layout, imperial)
	if _orqwa != null:
		_profile.edit(_orqwa.profile())
	_devices.refresh()
	UiScale.popup(self, Vector2i(960, 600))


func _on_workout_changed() -> void:
	var workout: Dictionary = _workout.workout()
	workout["plan"] = _workout.plan()
	workout_changed.emit(workout)


## Other units show in the HUD editor too.
func _on_profile_changed(profile: Dictionary) -> void:
	var imperial: bool = profile.get("units", "metric") == "imperial"
	if imperial != _imperial:
		_imperial = imperial
		_hud.edit(_hud.layout(), imperial)
	profile_changed.emit(profile)


func _on_action(action: StringName) -> void:
	hide()
	if action == &"finish":
		finish_requested.emit()
	elif action == &"abort":
		_confirm_abort.popup_centered()
