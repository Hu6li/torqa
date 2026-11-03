class_name IdlePointer
extends Node
## Keeps an idle pointer out of the view without capturing it (#477, R200).

const DELAY_SECONDS: float = 3.0

var _timer: Timer = Timer.new()
var _inside: bool = false
var _hidden: bool = false


func _ready() -> void:
	_timer.wait_time = DELAY_SECONDS
	_timer.one_shot = true
	_timer.timeout.connect(_hide_pointer)
	add_child(_timer)
	var window: Window = get_window()
	_inside = window.get_visible_rect().has_point(window.get_mouse_position())
	window.mouse_entered.connect(_entered)
	window.mouse_exited.connect(_left)
	window.focus_entered.connect(_restart)
	window.focus_exited.connect(_suspend)
	_restart()


func _input(event: InputEvent) -> void:
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
	_show_pointer()
	# Closing an embedded dialog can restore focus while its parent scene is leaving.
	var window: Window = get_window()
	if window != null and _inside and window.has_focus():
		_timer.start()


func _suspend() -> void:
	_timer.stop()
	_show_pointer()


func _hide_pointer() -> void:
	var window: Window = get_window()
	if (
		window == null
		or not _inside
		or not window.has_focus()
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
