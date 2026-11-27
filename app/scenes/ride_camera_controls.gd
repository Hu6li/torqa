class_name RideCameraControls
extends RefCounted
## The ride's camera keys and their buttons: C the next camera, Shift + left and right round
## the rider, Shift + up and down nearer and further, or in first person looking round, up and
## down (R276); the same as small buttons just above the ride bar, which fold away with it while
## the keys keep working.

var _world: RideWorld
var _bar: RideBar
var _buttons: CameraBar = CameraBar.new()
var _tell: Callable
## Whether there is a world to look at: not on video courses nor in a workout on its own.
var _world_shown: bool = true


## Whether `key` is one of the camera's.
static func is_key(key: InputEventKey) -> bool:
	var arrow: bool = key.keycode in [KEY_LEFT, KEY_RIGHT, KEY_UP, KEY_DOWN]
	return key.keycode == KEY_C or (key.shift_pressed and arrow)


## Puts the buttons on `screen` above `bar`; what the camera does is told with `tell`.
func attach(screen: Control, bar: RideBar, tell: Callable) -> void:
	_bar = bar
	_tell = tell
	screen.add_child(_buttons)
	_buttons.camera_requested.connect(cycle)
	_buttons.orbit_requested.connect(_turn.bind(false))
	_buttons.distance_requested.connect(_turn.bind(true))
	bar.item_rect_changed.connect(_place)
	_buttons.resized.connect(_place)
	bar.folded_changed.connect(func(_folded: bool) -> void: _show())


func bind(world: RideWorld) -> void:
	_world = world


## A new ride starts, with a world to look at or not.
func begin(world_shown: bool) -> void:
	_world_shown = world_shown
	_show()


## Does what the camera key `key` asks for.
func press(key: InputEventKey) -> void:
	if key.keycode == KEY_C:
		cycle()
	elif _world.is_free_camera():
		# It looks around with Shift + arrows instead (#78).
		return
	elif key.keycode in [KEY_LEFT, KEY_RIGHT]:
		var step: int = 1 if key.keycode == KEY_RIGHT else -1
		# From the rider's eyes an arrow looks the way it points; the buttons swing the view as
		# round the rider.
		if _world.camera() == RideWorld.CameraMode.FIRST_PERSON:
			step = -step
		_turn(step, false)
	else:
		_turn(-1 if key.keycode == KEY_UP else 1, true)


## Handles an assigned ride control using the same directions as the camera bar.
func handle_control(control: String) -> bool:
	match control:
		"next_camera":
			cycle()
		"camera_left":
			_turn(-1, false)
		"camera_right":
			_turn(1, false)
		"camera_nearer":
			_turn(-1, true)
		"camera_further":
			_turn(1, true)
		_:
			return false
	return true


## The next camera (C, the camera bar, or an assigned button).
func cycle() -> void:
	if _world_shown:
		_tell.call(tr("Camera: %s") % tr(_world.cycle_camera()))


func _turn(step: int, further: bool) -> void:
	if not _world_shown:
		return
	var view: String = tr(_world.turn_camera(step, further))
	if _world.camera() == RideWorld.CameraMode.FIRST_PERSON:
		_tell.call(tr("Looking: %s") % view)
	else:
		_tell.call((tr("Camera distance: %s") if further else tr("Camera: %s")) % view)


func _show() -> void:
	_buttons.visible = _world_shown and not _bar.folded


func _place() -> void:
	_buttons.reset_size()
	_buttons.position = _bar.position - Vector2(0.0, _buttons.size.y + 6.0)
