class_name RideJumps
extends RefCounted
## Jumps along a simulated ride, from the minimap or the elevation profile. The scene keeps only
## the surroundings of the rider, or of the free camera while it flies (ADR 0040): after a jump,
## or when the camera comes back to the rider from afar, the clock and the road wait for the
## rider's surroundings, as a pause does, and go on by themselves. A rider riding alone who
## catches up with the world still being built waits the same way; a group rides on.

var _orqwa: OrqwaApp
var _world: RideWorld
var _say: Callable
var _waiting: bool = false
var _was_free: bool = false


## `say` shows a message for a few seconds.
func listen(minimap: Minimap, profile: ElevationProfile, say: Callable) -> void:
	_say = say
	minimap.jump_requested.connect(
		func(position_m: Vector2) -> void: _jumped(_orqwa.jump_near(position_m.x, position_m.y))
	)
	profile.jump_requested.connect(
		func(distance_m: float) -> void: _jumped(_orqwa.jump_to_distance(distance_m))
	)


func bind(orqwa: OrqwaApp, world: RideWorld) -> void:
	_orqwa = orqwa
	_world = world


## Goes on once the rider's surroundings are uploaded; until then, says why it waits.
func update() -> void:
	var free: bool = _world.is_free_camera()
	if _was_free and not free:
		_wait()
	_was_free = free
	if not _waiting and _world.waiting_for_world() and _riding_alone():
		_wait()
	if not _waiting:
		return
	if _world.surroundings_ready():
		_waiting = false
		_orqwa.set_paused(false)
	else:
		_say.call(_why())


## A new ride began: no longer waiting.
func cancel() -> void:
	_waiting = false


## The rider pressed pause while the ride waited: true when the wait becomes the rider's own
## pause, which stays on, rather than resuming into a place not yet loaded.
func take_pause() -> bool:
	var waited: bool = _waiting
	_waiting = false
	return waited


func _jumped(jumped: bool) -> void:
	if jumped:
		_wait()


func _wait() -> void:
	if _orqwa.is_paused() or _world.surroundings_ready():
		return
	_orqwa.set_paused(true)
	_waiting = true
	_say.call(_why())


func _why() -> String:
	return tr("Building 3D world") if _world.waiting_for_world() else tr("Preparing the 3D scene")


## Others ride on in a group ride, so its world comes in around the road instead.
func _riding_alone() -> bool:
	return _orqwa.group_status().get("phase", "idle") == "idle"
