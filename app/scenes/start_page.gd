class_name StartPage
extends Control
## The start page (R38): tabs for Courses, Workouts (R58), Group Rides, History, Analysis (R61),
## Profile and Devices, and the cog at the top right for settings (R83). The ride view is separate;
## riding starts from a course's detail page or the Workouts tab.

## A ride has started with `options` (`RideOptions.options()`); a workout's also carry
## `workout` (`WorkoutsTab.workout()`) and `on_course`.
signal ride_started(options: Dictionary)

enum Tab { COURSES, WORKOUTS, GROUP_RIDES, HISTORY, ANALYSIS, PROFILE, DEVICES }

## Where the wordmark's letters sit in its drawing (its viewBox): their baseline is this share
## of its height above the bottom, the middle of their capitals this share above the baseline.
const WORDMARK_DESCENT: float = 21.0 / 141.0
const WORDMARK_CAP_MIDDLE: float = 50.0 / 141.0

var _orqwa: OrqwaApp
var _tabs: TabContainer = TabContainer.new()
## The tabs' captions, in the header beside the logo so the two share one row.
var _tab_bar: TabBar = TabBar.new()
var _courses_page: Control = Control.new()
var _courses: CoursesTab = CoursesTab.new()
var _detail: CourseDetail = CourseDetail.new()
var _workouts: WorkoutsTab = WorkoutsTab.new()
var _history: HistoryScreen = (
	(preload("res://scenes/history_screen.tscn") as PackedScene).instantiate() as HistoryScreen
)
var _analysis: AnalysisTab = AnalysisTab.new()
var _profile: ProfileTab = ProfileTab.new()
var _devices: DevicesTab = DevicesTab.new()
var _group: GroupRidesTab = GroupRidesTab.new()
var _group_page: ScrollContainer = ScrollContainer.new()
## This computer's graphics and the About tab (R83), behind [member _settings_button].
var _settings: SettingsDialog = SettingsDialog.new()
var _settings_button: Button = Button.new()
## The options and ghost of the ride waiting for its world.
var _pending_ride: Array = []
## The workout waiting for its course's world, and whether it has one.
var _pending_workout: Dictionary = {}
var _workout_on_course: bool = false


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_courses.bind(orqwa)
	_detail.bind(orqwa)
	_workouts.bind(orqwa)
	_history.bind(orqwa)
	_analysis.bind(orqwa)
	_profile.bind(orqwa)
	_devices.bind(orqwa)
	_group.bind(orqwa)
	_orqwa.group_started.connect(_group_started, CONNECT_DEFERRED)
	_settings.bind(orqwa)


## Back from a ride: records, history, courses and the rider's profile (R98) may have changed.
func refresh() -> void:
	_courses.refresh()
	_profile.refresh()
	_workouts.refresh()
	_detail.refresh_records()
	if _tabs.current_tab == Tab.HISTORY:
		_history.open()
	elif _tabs.current_tab == Tab.ANALYSIS:
		_analysis.refresh()


## The ride options of the course being ridden, e.g. to show them in the ride.
func ride_options() -> Dictionary:
	return _detail.ride_options()


func _ready() -> void:
	(%Rows as VBoxContainer).add_child(_tabs)
	_tabs.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_tabs.tabs_visible = false
	var gap: Control = Control.new()
	gap.custom_minimum_size = Vector2(16, 0)
	(%Header as HBoxContainer).add_child(gap)
	_tab_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_tab_bar.size_flags_vertical = Control.SIZE_SHRINK_END
	_tab_bar.clip_tabs = false
	(%Header as HBoxContainer).add_child(_tab_bar)
	_settings_button.icon = UiIcons.texture("cog")
	_settings_button.tooltip_text = tr("Settings")
	_settings_button.focus_mode = Control.FOCUS_NONE
	_settings_button.size_flags_vertical = Control.SIZE_SHRINK_END
	_settings_button.add_theme_stylebox_override("normal", UiTheme.icon_button(Color.TRANSPARENT))
	_settings_button.add_theme_stylebox_override("hover", UiTheme.icon_button(UiTheme.SURFACE))
	_settings_button.add_theme_stylebox_override(
		"pressed", UiTheme.icon_button(Color(UiTheme.ACCENT, 0.18))
	)
	_settings_button.pressed.connect(_settings.open)
	(%Header as HBoxContainer).add_child(_settings_button)
	add_child(_settings)
	# i18n-begin
	for page: Array in [
		[_courses_page, "Courses"],
		[_workouts, "Workouts"],
		[_group_page, "Group Rides"],
		[_history, "History"],
		[_analysis, "Analysis"],
		[_profile, "Profile"],
		[_devices, "Devices"],
	]:
		# i18n-end
		var node: Control = page[0]
		var title: String = page[1]
		node.name = title
		_tabs.add_child(node)
		_tab_bar.add_tab(tr(title))
	_courses.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_courses_page.add_child(_courses)
	_detail.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_detail.hide()
	_courses_page.add_child(_detail)
	_history.embedded = true
	_courses.course_opened.connect(_open_course)
	_detail.back_requested.connect(_show_gallery)
	_detail.ride_requested.connect(_on_ride_requested)
	_detail.ready_to_ride.connect(_start_ride)
	_detail.course_changed.connect(_courses.refresh)
	_workouts.start_requested.connect(_on_workout_requested)
	_workouts.ready_to_start.connect(_start_workout)
	_group.devices_requested.connect(func() -> void: _tabs.current_tab = Tab.DEVICES)
	_group.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	UiTheme.scroll_down_only(_group_page)
	_group_page.add_child(_group)
	_profile.profile_changed.connect(_courses.refresh)
	_tabs.tab_changed.connect(_on_tab_changed)
	_tab_bar.tab_changed.connect(func(tab: int) -> void: _tabs.current_tab = tab)
	# The course page sits over the gallery in its tab: a click on the Courses tab, already
	# the current one, brings the gallery back like the page's own button does (#188).
	_tab_bar.tab_clicked.connect(_on_tab_clicked)


func _notification(what: int) -> void:
	if what == NOTIFICATION_THEME_CHANGED and is_node_ready():
		_align_brand()


## The wordmark stands on the tabs' baseline and the mark centres on its capitals, so logo and
## tabs read as one line. Derived from the tab font, which only the app's theme sets.
func _align_brand() -> void:
	var font: Font = _tab_bar.get_theme_font("font")
	var baseline: float = (
		_tab_bar.get_theme_stylebox("tab_selected").get_margin(SIDE_BOTTOM)
		+ font.get_descent(_tab_bar.get_theme_font_size("font_size"))
	)
	var wordmark: TextureRect = %Wordmark
	var letters: float = wordmark.custom_minimum_size.x / wordmark.texture.get_width()
	letters *= wordmark.texture.get_height()
	# Drawn at the top of a taller rect (keep aspect), so the space below lifts the letters.
	wordmark.custom_minimum_size.y = letters + maxf(0, baseline - WORDMARK_DESCENT * letters)
	var logo: TextureRect = %Logo
	var mark: float = logo.custom_minimum_size.x / logo.texture.get_width()
	mark *= logo.texture.get_height()
	var cap_middle: float = baseline + WORDMARK_CAP_MIDDLE * letters
	logo.custom_minimum_size.y = mark + maxf(0, cap_middle - mark / 2)


func _on_tab_clicked(tab: int) -> void:
	if tab == Tab.COURSES and _detail.visible:
		_show_gallery()


func _open_course(course: Dictionary) -> void:
	if _group_active():
		_tabs.current_tab = Tab.GROUP_RIDES
		return
	_courses.hide()
	_detail.show()
	_detail.open(course)


## Back from a course to the gallery, as its back button goes (the remote's Back, Escape);
## false where there is nothing to go back from.
func go_back() -> bool:
	if not _detail.visible:
		return false
	_show_gallery()
	return true


func _show_gallery() -> void:
	_detail.hide()
	_courses.show()


func _on_tab_changed(tab: int) -> void:
	# Pages switched in code, e.g. to Devices for a missing trainer, move the caption too.
	if _tab_bar.current_tab != tab:
		_tab_bar.current_tab = tab
	if tab == Tab.HISTORY:
		_history.open()
	elif tab == Tab.ANALYSIS:
		_analysis.refresh()
	elif tab == Tab.WORKOUTS:
		# Courses and riders change on the other tabs.
		_workouts.refresh()


## Riding needs a trainer: without one, the Devices tab says what to do (R41).
func _on_ride_requested(options: Dictionary, ghost: Dictionary) -> void:
	if _group_active():
		_tabs.current_tab = Tab.GROUP_RIDES
		return
	_detail.show_status("")
	if not _devices.connect_selected():
		_detail.show_status(tr("No trainer connected — choose one under Devices."))
		_tabs.current_tab = Tab.DEVICES
		return
	_pending_ride = [options, ghost]
	_detail.build()


func _on_workout_requested(workout: Dictionary, course_path: String) -> void:
	if _group_active():
		_tabs.current_tab = Tab.GROUP_RIDES
		return
	if not _devices.connect_selected():
		_workouts.show_status(tr("No trainer connected — choose one under Devices."))
		_tabs.current_tab = Tab.DEVICES
		return
	_pending_workout = workout
	_workout_on_course = not course_path.is_empty()
	_workouts.prepare()


func _start_workout() -> void:
	if _pending_workout.is_empty():
		return
	var workout: Dictionary = _pending_workout
	_pending_workout = {}
	# On a course, its world looks as last chosen on the course page.
	var options: Dictionary = _detail.ride_options()
	var flat_descents: bool = options["flat_descents"]
	if _orqwa.start_workout(workout, _workout_on_course, flat_descents):
		options["workout"] = workout
		options["on_course"] = _workout_on_course
		options["overlay"] = _workouts.start_as_overlay()
		ride_started.emit(options)


func _start_ride() -> void:
	if _pending_ride.is_empty():
		return
	var options: Dictionary = _pending_ride[0]
	var ghost: Dictionary = _pending_ride[1]
	_pending_ride = []
	var difficulty: float = options["difficulty"]
	var flat_descents: bool = options["flat_descents"]
	if _orqwa.start_ride(difficulty, flat_descents, ghost):
		ride_started.emit(options)


func _group_active() -> bool:
	return _orqwa.group_status().get("phase", "idle") != "idle"


func _group_started(on_course: bool, workout: bool) -> void:
	var options: Dictionary = _detail.ride_options()
	options["on_course"] = on_course
	if workout:
		options["workout"] = {"kind": "plan", "id": _orqwa.group_status()["workout_id"]}
	ride_started.emit(options)
