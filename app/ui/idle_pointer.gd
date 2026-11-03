class_name IdlePointer
extends Node
## Keeps an idle pointer out of course views, visible in dialogs (#477, R200–R201, R205).

const DELAY_SECONDS: float = 3.0

## Seconds without mouse activity before the pointer hides; tests shorten it to run quickly.
var delay_seconds: float = DELAY_SECONDS

## Enabled only while a ride shows its 3D world or video.
var enabled: bool = false:
	set(value):
		if enabled == value:
			return
		enabled = value
		if not is_node_ready():
			return
		if enabled:
			_restart()
		else:
			_suspend()

var _timer: Timer = Timer.new()
var _inside: bool = false
var _hidden: bool = false
var _subwindows: Array[Window] = []


func _ready() -> void:
	_timer.one_shot = true
	_timer.timeout.connect(_hide_pointer)
	add_child(_timer)
	var window: Window = get_window()
	_inside = window.get_visible_rect().has_point(window.get_mouse_position())
	window.mouse_entered.connect(_entered)
	window.mouse_exited.connect(_left)
	window.focus_entered.connect(_restart)
	window.focus_exited.connect(_suspend)
	get_tree().node_added.connect(_watch_window)
	for node: Node in get_tree().root.find_children("*", "Window", true, false):
		_watch_window(node)
	_restart()


## Embedded dialogs can retain main-window focus and receive mouse input in its place.
func _watch_window(node: Node) -> void:
	if not node is Window or node == get_window():
		return
	var window: Window = node as Window
	_subwindows.append(window)
	window.visibility_changed.connect(_restart)
	window.tree_exiting.connect(_forget_window.bind(window), CONNECT_ONE_SHOT)
	_restart()


func _forget_window(window: Window) -> void:
	_subwindows.erase(window)
	_restart()


func _dialog_open() -> bool:
	return _subwindows.any(func(window: Window) -> bool: return window.visible)


func _input(event: InputEvent) -> void:
	if not enabled:
		return
	var motion: InputEventMouseMotion = event as InputEventMouseMotion
	if motion != null:
		# Godot also sends motion events when the pointer has not moved.
		if motion.relative.is_zero_approx():
			return
	elif not event is InputEventMouseButton:
		return
	_restart()


func _exit_tree() -> void:
	_suspend()


func _entered() -> void:
	_inside = true
	_restart()


func _left() -> void:
	_inside = false
	_suspend()


func _restart() -> void:
	_timer.stop()
	_show_pointer()
	# Closing an embedded dialog can restore focus while its parent scene is leaving.
	var window: Window = get_window()
	if enabled and window != null and _inside and window.has_focus() and not _dialog_open():
		_timer.start(delay_seconds)


func _suspend() -> void:
	_timer.stop()
	_show_pointer()


func _hide_pointer() -> void:
	var window: Window = get_window()
	if (
		not enabled
		or window == null
		or not _inside
		or not window.has_focus()
		or _dialog_open()
		or Input.get_mouse_button_mask() != 0
	):
		return
	if Input.mouse_mode == Input.MOUSE_MODE_VISIBLE:
		Input.mouse_mode = Input.MOUSE_MODE_HIDDEN
		_hidden = true


func _show_pointer() -> void:
	if _hidden:
		if Input.mouse_mode == Input.MOUSE_MODE_HIDDEN:
			Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
		_hidden = false
