extends SceneTree
## Display checks for the idle mouse pointer (#477, R200).
## Run: xvfb-run -a godot --path app --audio-driver Dummy -s res://tests/idle_pointer_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	if DisplayServer.get_name() == "headless":
		push_error("The idle pointer check needs a display")
		quit(1)
		return
	await _idle_pointer()
	if not _failed:
		print("IDLE POINTER SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## The pointer hides on a real display (headless has no mouse mode), without swallowing
## movement over controls or clicks, and returns when another window needs it (R200).
func _idle_pointer() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	var button: Button = Button.new()
	button.text = "Pointer input"
	button.position = Vector2(20, 20)
	button.size = Vector2(200, 60)
	main.add_child(button)
	var clicked: Array[bool] = [false]
	button.pressed.connect(func() -> void: clicked[0] = true)
	root.grab_focus()
	await _settle()
	root.mouse_entered.emit()
	_pointer_motion(Vector2(30, 30), Vector2.ONE)
	await create_timer(1.6).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "the pointer has a three-second grace")
	_pointer_motion(Vector2(40, 30), Vector2(10, 0))
	await create_timer(1.6).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "movement restarts the idle delay")
	await create_timer(1.6).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "three idle seconds hide the pointer")
	_pointer_motion(Vector2(40, 30), Vector2.ZERO)
	_press(KEY_SHIFT)
	_check(Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "zero motion and keys keep it hidden")
	_pointer_motion(Vector2(50, 30), Vector2(10, 0))
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "movement over a control shows it")
	var held: InputEventMouseButton = InputEventMouseButton.new()
	held.position = Vector2(50, 30)
	held.button_index = MOUSE_BUTTON_LEFT
	held.pressed = true
	Input.parse_input_event(held)
	await _settle()
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "holding a mouse button keeps it visible")
	held.pressed = false
	Input.parse_input_event(held)
	await _settle()
	clicked[0] = false
	root.mode = Window.MODE_FULLSCREEN
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "the pointer also hides in full screen")
	for pressed: bool in [true, false]:
		var event: InputEventMouseButton = InputEventMouseButton.new()
		event.position = Vector2(50, 30)
		event.button_index = MOUSE_BUTTON_LEFT
		event.pressed = pressed
		root.push_input(event)
		_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "a click restores the pointer")
	_check(clicked[0], "restoring the pointer does not consume the click")
	await create_timer(3.2).timeout
	var dialog: AcceptDialog = AcceptDialog.new()
	main.add_child(dialog)
	dialog.popup_centered(Vector2i(300, 200))
	await _settle()
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "a dialog restores the pointer")
	dialog.hide()
	root.grab_focus()
	await _settle()
	root.mouse_entered.emit()
	_pointer_motion(Vector2(50, 30), Vector2.ONE)
	await create_timer(3.2).timeout
	root.mouse_exited.emit()
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "leaving the window restores the pointer")
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "outside the window it stays visible")
	root.mouse_entered.emit()
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "returning to the window restarts idling")
	root.focus_exited.emit()
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "losing focus restores the pointer")
	root.focus_entered.emit()
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "regaining focus restarts idling")
	root.remove_child(main)
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "leaving the scene restores the pointer")
	root.focus_entered.emit()
	root.mouse_entered.emit()
	main.free()
	root.mode = Window.MODE_WINDOWED
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "closing the view restores the pointer")


func _pointer_motion(position: Vector2, relative: Vector2) -> void:
	var event: InputEventMouseMotion = InputEventMouseMotion.new()
	event.position = position
	event.relative = relative
	root.push_input(event)


func _settle() -> void:
	for i: int in range(4):
		await process_frame


func _press(key: Key) -> void:
	for pressed: bool in [true, false]:
		var event: InputEventKey = InputEventKey.new()
		event.keycode = key
		event.pressed = pressed
		root.push_input(event)


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("IDLE POINTER SMOKE TEST FAILED: " + what)
		_failed = true
