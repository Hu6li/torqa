extends SceneTree
## Display checks for the idle mouse pointer in course views and dialogs (R200–R201, R205).
## Run: xvfb-run -a godot --path app --audio-driver Dummy -s res://tests/idle_pointer_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"
const COURSE_OPTIONS: Dictionary = {"time": "Midday", "weather": "Clear", "camera": 0}

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
	await _inactive_views(main)
	var start: StartPage = main.get_node("StartPage")
	start.ride_started.emit(COURSE_OPTIONS.duplicate())
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
	await _dialogs(main)
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
	await _course_views(main)
	root.remove_child(main)
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "leaving the scene restores the pointer")
	root.focus_entered.emit()
	root.mouse_entered.emit()
	main.free()
	root.mode = Window.MODE_WINDOWED
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "closing the view restores the pointer")


## Embedded windows can keep the main window focused and receive all its mouse input.
func _dialogs(main: Control) -> void:
	var ride: RideScreen = main.get_node("RideScreen")
	var dialog: AcceptDialog = ride._group_window
	ride._bar.group_requested.emit()
	await _settle()
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "the group dialog restores the pointer")
	root.focus_entered.emit()
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "an idle group dialog keeps the pointer")
	var nested: AcceptDialog = AcceptDialog.new()
	dialog.add_child(nested)
	nested.popup_centered(Vector2i(300, 200))
	await _settle()
	var motion: InputEventMouseMotion = InputEventMouseMotion.new()
	motion.relative = Vector2.ONE
	nested.push_input(motion)
	root.focus_entered.emit()
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "nested dialog input keeps it visible")
	nested.free()
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "the parent dialog still needs a pointer")
	dialog.hide()
	await _settle()
	var popup: PopupPanel = PopupPanel.new()
	main.add_child(popup)
	popup.popup(Rect2i(200, 200, 200, 100))
	await _settle()
	_check(popup.visible, "the popup remains open for the idle check")
	await create_timer(3.2).timeout
	_check(popup.visible, "the popup remains open beyond the idle delay")
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "popups also keep the pointer visible")
	popup.free()


## The normal start/ride signals cover the views that must never hide the pointer (R201).
func _inactive_views(main: Control) -> void:
	root.mouse_entered.emit()
	_pointer_motion(Vector2(30, 30), Vector2.ONE)
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "the start page keeps its pointer")
	var start: StartPage = main.get_node("StartPage")
	start.ride_started.emit({"workout": {"kind": "power", "power_w": 180.0}, "on_course": false})
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "a plain workout keeps its pointer")
	var ride: RideScreen = main.get_node("RideScreen")
	ride.closed.emit()


## A course workout hides it over 3D or video, with immediate restoration on leaving (R201).
func _course_views(main: Control) -> void:
	var ride: RideScreen = main.get_node("RideScreen")
	ride.closed.emit()
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "leaving a course restores the pointer")
	await create_timer(3.2).timeout
	_check(
		Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "the start page stays visible after a ride"
	)
	var start: StartPage = main.get_node("StartPage")
	var options: Dictionary = COURSE_OPTIONS.duplicate()
	options.merge({"workout": {"kind": "power", "power_w": 180.0}, "on_course": true})
	start.ride_started.emit(options)
	await _settle()
	root.mouse_entered.emit()
	_pointer_motion(Vector2(30, 30), Vector2.ONE)
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "a workout with 3D hides the idle pointer")
	ride.overlay_requested.emit(true)
	await _settle()
	_check(
		Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "entering the overlay restores the pointer"
	)
	await create_timer(3.2).timeout
	_check(Input.mouse_mode == Input.MOUSE_MODE_VISIBLE, "the HUD-only overlay keeps its pointer")
	ride.overlay_requested.emit(false)
	await _settle()
	# Video visibility is a presentation fixture; no decoder or course download is needed.
	var world: RideWorld = main.get_node("World")
	world.hide()
	var video: VideoView = main.get("_video")
	video.show()
	root.grab_focus()
	await _settle()
	root.mouse_entered.emit()
	_pointer_motion(Vector2(30, 30), Vector2.ONE)
	await create_timer(3.2).timeout
	_check(
		Input.mouse_mode == Input.MOUSE_MODE_HIDDEN, "a workout with video hides the idle pointer"
	)


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
