class_name RideJumps
extends RefCounted
## Jumps along a simulated ride, from the minimap or the elevation profile. The scene keeps only
## the rider's surroundings (ADR 0040): after a jump, the clock and the road wait for those of
## the new place, as a pause does, and go on by themselves.

var _orqwa: OrqwaApp
var _world: RideWorld
var _say: Callable
var _waiting: bool = false


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


## Goes on once the new place's surroundings are uploaded; until then, says why it waits.
func update() -> void:
	if not _waiting:
		return
	if _world.surroundings_ready():
		_waiting = false
		_orqwa.set_paused(false)
	else:
		_say.call(tr("Preparing the 3D scene"))


## The rider paused or resumed, or a new ride began: no longer waiting.
func cancel() -> void:
	_waiting = false


func _jumped(jumped: bool) -> void:
	if not jumped or _orqwa.is_paused() or _world.surroundings_ready():
		return
	_orqwa.set_paused(true)
	_waiting = true
	_say.call(tr("Preparing the 3D scene"))
