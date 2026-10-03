class_name StartPage
extends Control
## The start page (R38): tabs for Courses, History, Profile and Devices & Settings. The ride
## view is separate; riding starts from a course's detail page.

## A ride has started with `options` (`RideOptions.options()`).
signal ride_started(options: Dictionary)

enum Tab { COURSES, HISTORY, PROFILE, DEVICES }

var _torqa: TorqaApp
var _tabs: TabContainer = TabContainer.new()
var _courses_page: Control = Control.new()
var _courses: CoursesTab = CoursesTab.new()
var _detail: CourseDetail = CourseDetail.new()
var _history: HistoryScreen = (
	(preload("res://scenes/history_screen.tscn") as PackedScene).instantiate() as HistoryScreen
)
var _profile: ProfileTab = ProfileTab.new()
var _devices: DevicesTab = DevicesTab.new()


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_courses.bind(torqa)
	_detail.bind(torqa)
	_history.bind(torqa)
	_profile.bind(torqa)
	_devices.bind(torqa)


## Back from a ride: records, history and courses may have changed.
func refresh() -> void:
	_courses.refresh()
	_detail.refresh_records()
	if _tabs.current_tab == Tab.HISTORY:
		_history.open()


## The ride options of the course being ridden, e.g. to show them in the ride.
func ride_options() -> Dictionary:
	return _detail.ride_options()


func _ready() -> void:
	(%Rows as VBoxContainer).add_child(_tabs)
	_tabs.size_flags_vertical = Control.SIZE_EXPAND_FILL
	# i18n-begin
	for page: Array in [
		[_courses_page, "Courses"],
		[_history, "History"],
		[_profile, "Profile"],
		[_devices, "Devices & Settings"],
	]:
		# i18n-end
		var node: Control = page[0]
		var title: String = page[1]
		node.name = title
		_tabs.add_child(node)
		_tabs.set_tab_title(_tabs.get_tab_count() - 1, tr(title))
	_courses.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_courses_page.add_child(_courses)
	_detail.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_detail.hide()
	_courses_page.add_child(_detail)
	_history.embedded = true
	_courses.course_opened.connect(_open_course)
	_detail.back_requested.connect(_show_gallery)
	_detail.ride_requested.connect(_on_ride_requested)
	_profile.profile_changed.connect(_courses.refresh)
	_tabs.tab_changed.connect(_on_tab_changed)


func _open_course(course: Dictionary, loaded: bool) -> void:
	_courses.hide()
	_detail.show()
	_detail.open(course, loaded)


func _show_gallery() -> void:
	_detail.hide()
	_courses.show()


func _on_tab_changed(tab: int) -> void:
	if tab == Tab.HISTORY:
		_history.open()


## Riding needs a trainer: without one, the Devices tab says what to do (R41).
func _on_ride_requested(options: Dictionary, ghost: Dictionary) -> void:
	_detail.show_status("")
	if not _devices.connect_selected():
		_detail.show_status(tr("No trainer connected — choose one under Devices & Settings."))
		_tabs.current_tab = Tab.DEVICES
		return
	var difficulty: float = options["difficulty"]
	var flat_descents: bool = options["flat_descents"]
	if _torqa.start_ride(difficulty, flat_descents, ghost):
		ride_started.emit(options)
