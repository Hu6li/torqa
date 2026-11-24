extends SceneTree
## Headless checks of the ride's cameras (R276): chase and drone look at the rider from eight
## sides and three distances, chosen with Shift + arrows or the small buttons above the ride
## bar; first person looks round, up and down with them, and the free camera keeps them for
## looking around (#78).
## Run: godot --headless --path app -s res://tests/camera_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_sides()
	_first_person()
	await _keys_and_buttons()
	if not _failed:
		print("CAMERA SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## Each side is where its name says, 45° from the next, and looks at the rider; near, normal
## and far stand off further each.
func _sides() -> void:
	# A rider at the origin riding north (−z), their right towards +x.
	var rider: Transform3D = Transform3D.IDENTITY
	for mode: int in [RideWorld.CameraMode.CHASE, RideWorld.CameraMode.DRONE]:
		var view: RideCamera = RideCamera.new()
		var behind: Vector3 = view.target(mode, rider, INF).origin
		_check(behind.z > 1.0 and absf(behind.x) < 0.01, "behind the rider: %s" % behind)
		var sides: Array[Vector3] = []
		for angle: int in range(RideCamera.ANGLES.size()):
			view.angle = angle
			var camera: Transform3D = view.target(mode, rider, INF)
			sides.append(Vector3(camera.origin.x, 0.0, camera.origin.z).normalized())
			# Every side has the rider well in view; from the sides and the front, in the middle.
			var to_rider: Vector3 = (rider.origin - camera.origin).normalized()
			var off: float = rad_to_deg((-camera.basis.z).angle_to(to_rider))
			var name: String = RideCamera.ANGLES[angle]
			_check(off < 30.0, "%s sees the rider, %.0f° off" % [name, off])
			if angle in [2, 4, 6]:
				_check(off < 15.0, "%s looks at the rider, %.0f° off" % [name, off])
		_check(sides[2].x > 0.99 and sides[6].x < -0.99, "right and left: %s" % [sides])
		_check(sides[4].z < -0.99, "the front is ahead of the rider: %s" % sides[4])
		for i: int in range(sides.size()):
			var apart: float = rad_to_deg(sides[i].angle_to(sides[(i + 1) % sides.size()]))
			_check(absf(apart - 45.0) < 0.01, "45° apart: %f" % apart)
		view.angle = 8
		_check(view.angle == 0, "round to behind again")
		var reach: Array[float] = []
		for distance: int in range(RideCamera.DISTANCES.size()):
			view.distance = distance
			reach.append(view.target(mode, rider, INF).origin.length())
		_check(reach[0] < reach[1] and reach[1] < reach[2], "near, normal, far: %s" % [reach])
		view.distance = 5
		_check(view.distance == 2, "no further than far")
		# A new side is reached round the rider, never through them.
		view.distance = 1
		view.angle = 0
		view.target(mode, rider, INF)
		view.angle = 4
		for frame: int in range(120):
			var camera: Vector3 = view.target(mode, rider, 1.0 / 60.0).origin
			var level: float = Vector2(camera.x, camera.z).length()
			if level < 6.0:
				_check(false, "the camera passes the rider at %s" % camera)
				break


## From the rider's eyes the head turns round in the same steps, and looks up and down.
func _first_person() -> void:
	var rider: Transform3D = Transform3D.IDENTITY
	var eyes: RideCamera = RideCamera.new()
	var mode: int = RideWorld.CameraMode.FIRST_PERSON
	var ahead: Transform3D = eyes.target(mode, rider, INF)
	_check((-ahead.basis.z).z < -0.99, "first person looks down the road")
	# As round the rider, left swings the view clockwise: the head turns right.
	_check(eyes.turn(mode, -2, false) == "Right", "two steps left look right")
	var right: Vector3 = -eyes.target(mode, rider, INF).basis.z
	_check(right.x > 0.99, "looks to the right: %s" % right)
	_check(eyes.turn(mode, -2, false) == "Behind", "and on, behind")
	var behind: Vector3 = -eyes.target(mode, rider, INF).basis.z
	_check(behind.z > 0.99, "looks back: %s" % behind)
	_check(eyes.turn(mode, 3, false) == "Ahead right", "back round the other way")
	eyes.face_ahead()
	var level: float = (-eyes.target(mode, rider, INF).basis.z).y
	_check(eyes.turn(mode, -1, true) == "Up", "Shift + up looks up")
	var up: float = (-eyes.target(mode, rider, INF).basis.z).y
	_check(eyes.turn(mode, 2, true) == "Down", "Shift + down looks down")
	var down: float = (-eyes.target(mode, rider, INF).basis.z).y
	_check(down < level and level < up, "down, level, up: %f %f %f" % [down, level, up])
	_check(eyes.angle == 0 and eyes.distance == 1, "chase and drone keep their side")


## Shift + arrows and the buttons through the running app, and the free camera's own use of them.
func _keys_and_buttons() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	(main.get_node("StartPage") as Control).hide()
	var ride: RideScreen = main.get_node("RideScreen")
	ride.show()
	var world: RideWorld = main.get_node("World")
	world.show()
	var view: RideCamera = world.get("_view")
	world.set_camera(RideWorld.CameraMode.FIRST_PERSON)
	await _press(KEY_RIGHT)
	_check(world.camera() == RideWorld.CameraMode.FIRST_PERSON, "first person stays")
	_check(view.look == 7 and world.camera_angle() == 0, "and turns the view as round the rider")
	world.set_camera(RideWorld.CameraMode.CHASE)
	_check(view.look == 0, "it looks ahead again next time")
	await _press(KEY_RIGHT)
	_check(world.camera_angle() == 1, "Shift + right goes round to the right")
	await _press(KEY_LEFT)
	await _press(KEY_LEFT)
	_check(world.camera_angle() == 7, "Shift + left goes round past behind")
	await _press(KEY_UP)
	_check(view.distance == 0, "Shift + up brings it nearer")
	await _press(KEY_DOWN)
	_check(view.distance == 1, "Shift + down takes it further")
	world.set("_free", true)
	await _press(KEY_RIGHT)
	_check(world.camera_angle() == 7 and world.is_free_camera(), "the free camera looks instead")
	world.set_camera(RideWorld.CameraMode.CHASE)

	var cameras: RideCameraControls = ride.get("_cameras")
	var buttons: CameraBar = cameras.get("_buttons")
	var bar: RideBar = ride.get("_bar")
	await process_frame
	_check(
		buttons.visible and buttons.get_rect().end.y <= bar.position.y,
		"just above the ride bar: %s, %s" % [buttons.get_rect(), bar.get_rect()]
	)
	_check(buttons.size.y < bar.size.y, "smaller than the bar: %s" % buttons.size)
	var row: Container = buttons.get_child(0)
	for button: Button in row.get_children():
		_check(
			button.icon != null and not button.tooltip_text.is_empty(),
			"an icon with a tooltip: %s" % button.tooltip_text
		)
	(row.get_child(2) as Button).pressed.emit()
	_check(world.camera_angle() == 0, "the button goes round to the right")
	(row.get_child(4) as Button).pressed.emit()
	_check(view.distance == 2, "the button takes it further")
	bar.folded = true
	bar.folded_changed.emit(true)
	_check(not buttons.visible, "folded away with the bar")
	await _press(KEY_LEFT)
	_check(world.camera_angle() == 7, "the keys work with the bar folded")
	bar.folded = false
	bar.folded_changed.emit(false)
	cameras.begin(false)
	_check(not buttons.visible, "no buttons without a world to look at")
	await _press(KEY_RIGHT)
	_check(world.camera_angle() == 7, "nor keys")
	main.free()


## Presses Shift + `key` and lets go, as the keyboard would.
func _press(key: Key) -> void:
	for pressed: bool in [true, false]:
		for held: Key in [KEY_SHIFT, key]:
			var event: InputEventKey = InputEventKey.new()
			event.keycode = held
			event.physical_keycode = held
			event.pressed = pressed
			event.shift_pressed = pressed
			Input.parse_input_event(event)
		for frame: int in range(3):
			await process_frame


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("CAMERA SMOKE TEST FAILED: " + what)
		_failed = true
