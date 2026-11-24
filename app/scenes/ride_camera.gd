class_name RideCamera
extends RefCounted
## Where the ride's camera stands for the rider: their eyes in first person, looking down the
## road or round, up and down; behind them for chase and drone, or from one of eight sides 45°
## apart and nearer or further (R276).

## The sides, from behind round by the rider's right, as an index of `ANGLES`.
# i18n-begin
const ANGLES: Array[String] = [
	"Behind",
	"Behind right",
	"Right",
	"Front right",
	"Front",
	"Front left",
	"Left",
	"Behind left",
]
const DISTANCE_NAMES: Array[String] = ["Near", "Normal", "Far"]
## Where first person looks, turning right in steps as `ANGLES`, and how far up (of `TILTS`).
const LOOKS: Array[String] = [
	"Ahead",
	"Ahead right",
	"Right",
	"Behind right",
	"Behind",
	"Behind left",
	"Left",
	"Ahead left",
]
const TILT_NAMES: Array[String] = ["Down", "Level", "Up"]
# i18n-end
## How far chase and drone stand off, as factors of their usual distance and height.
const DISTANCES: Array[float] = [0.6, 1.0, 1.7]
## How far first person looks up from looking down the road, in radians: down at the road just
## ahead, along it, or up at the hills.
const TILTS: Array[float] = [-0.45, 0.0, 0.35]
## A new side or distance is reached at this rate (1 / seconds), the camera swinging round the
## rider rather than cutting through them.
const SMOOTHING: float = 4.0

## The side chase and drone look from (of `ANGLES`) and how far they stand off (of
## `DISTANCES`).
var angle: int = 0:
	set(value):
		angle = posmod(value, ANGLES.size())
var distance: int = 1:
	set(value):
		distance = clampi(value, 0, DISTANCES.size() - 1)
## Where first person looks (of `LOOKS`) and how far up (of `TILTS`).
var look: int = 0:
	set(value):
		look = posmod(value, LOOKS.size())
var tilt: int = 1:
	set(value):
		tilt = clampi(value, 0, TILTS.size() - 1)
## Where the camera is on its way to `angle` (radians) and `distance` (a factor), and first
## person's turn and tilt (radians).
var _orbit: float = 0.0
var _reach: float = 1.0
var _turn: float = 0.0
var _lift: float = 0.0


## Takes the camera `step` sides round the rider, or `step` distances further if `further`, and
## returns the new side's or distance's name; in first person it looks `step` steps to the right
## instead, or `step` steps down.
func turn(mode: int, step: int, further: bool) -> String:
	if mode == RideWorld.CameraMode.FIRST_PERSON:
		if further:
			tilt -= step
			return TILT_NAMES[tilt]
		look += step
		return LOOKS[look]
	if further:
		distance += step
		return DISTANCE_NAMES[distance]
	angle += step
	return ANGLES[angle]


## First person looks down the road again.
func face_ahead() -> void:
	look = 0
	tilt = 1


## The camera for `mode` (`RideWorld.CameraMode`) and a rider at `rider`, `delta` seconds after
## the last; a long `delta` takes it straight to the side and distance.
func target(mode: int, rider: Transform3D, delta: float) -> Transform3D:
	var forward: Vector3 = -rider.basis.z
	var flat_forward: Vector3 = Vector3(forward.x, 0.0, forward.z).normalized()
	var origin: Vector3 = rider.origin
	var weight: float = 1.0 - exp(-delta * SMOOTHING)
	if mode == RideWorld.CameraMode.FIRST_PERSON:
		# The rider's eyes, slightly ahead of the helmet, looking down the road, or turning the
		# head round, up and down.
		_turn = lerp_angle(_turn, look * TAU / LOOKS.size(), weight)
		_lift = lerpf(_lift, TILTS[tilt], weight)
		var eyes: Vector3 = rider * Vector3(0, 1.56, -0.5)
		var ahead: Vector3 = forward.rotated(Vector3.UP, -_turn) * 20.0
		var up: float = 20.0 * tan(_lift) - 1.5
		return Transform3D(Basis.IDENTITY, eyes).looking_at(
			eyes + ahead + Vector3.UP * up, Vector3.UP
		)
	_orbit = lerp_angle(_orbit, angle * TAU / ANGLES.size(), weight)
	_reach = lerpf(_reach, DISTANCES[distance], weight)
	var right: Vector3 = flat_forward.cross(Vector3.UP)
	var away: Vector3 = -flat_forward * cos(_orbit) + right * sin(_orbit)
	var drone: bool = mode == RideWorld.CameraMode.DRONE
	var stand_off: Vector2 = (Vector2(28.0, 20.0) if drone else Vector2(6.5, 2.6)) * _reach
	var eye: Vector3 = origin + away * stand_off.x + Vector3.UP * stand_off.y
	# From behind the camera looks down the road; from a side or the front, at the rider.
	var look_at: Vector3 = (
		origin + flat_forward * 10.0 * maxf(cos(_orbit), 0.0) + Vector3.UP * (0.0 if drone else 1.0)
	)
	return Transform3D(Basis.IDENTITY, eye).looking_at(look_at, Vector3.UP)
