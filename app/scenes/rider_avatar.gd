class_name RiderAvatar
extends Node3D
## A low-poly cyclist on a road bike, built from primitives. The cranks turn with the cadence,
## the legs follow the pedals (two-bone inverse kinematics) and the wheels with the speed.
## Local axes: −z forward, +y up, +x right; the origin is on the ground between the wheels.

const JERSEY: Color = Color(0.04, 0.61, 0.96)
const SHORTS: Color = Color(0.07, 0.07, 0.08)
const SKIN: Color = Color(0.86, 0.68, 0.55)
const HELMET: Color = Color(0.95, 0.95, 0.96)
const FRAME: Color = Color(0.13, 0.15, 0.18)
const TYRE: Color = Color(0.05, 0.05, 0.05)

const WHEEL_RADIUS: float = 0.34
const REAR_HUB: Vector3 = Vector3(0, WHEEL_RADIUS, 0.5)
const FRONT_HUB: Vector3 = Vector3(0, WHEEL_RADIUS, -0.5)
const BOTTOM_BRACKET: Vector3 = Vector3(0, 0.28, 0.06)
const SADDLE: Vector3 = Vector3(0, 0.92, 0.28)
const HEAD_TUBE: Vector3 = Vector3(0, 0.84, -0.4)
const BARS: Vector3 = Vector3(0, 0.95, -0.46)
const CRANK: float = 0.17
const PEDAL_OFFSET: float = 0.11
const THIGH: float = 0.45
const SHIN: float = 0.47
const HIP: Vector3 = Vector3(0, 0.98, 0.25)
const SHOULDER: Vector3 = Vector3(0, 1.36, -0.12)

var _crank_angle: float = 0.0
var _wheel_angle: float = 0.0
var _wheels: Array[Node3D] = []
## Per side (left, right): thigh, shin, crank arm.
var _legs: Array[Array] = []


func _ready() -> void:
	_build_bike()
	_build_rider()
	animate(0.0, 0.0, 0.0)


## Advances the animation: cadence in rpm, speed in km/h.
func animate(delta: float, cadence_rpm: float, speed_kmh: float) -> void:
	_crank_angle = fmod(_crank_angle + cadence_rpm / 60.0 * TAU * delta, TAU)
	_wheel_angle = fmod(_wheel_angle + speed_kmh / 3.6 / WHEEL_RADIUS * delta, TAU)
	for wheel: Node3D in _wheels:
		wheel.rotation.x = -_wheel_angle
	for side: int in range(2):
		var sign: float = -1.0 if side == 0 else 1.0
		var angle: float = _crank_angle + (PI if side == 0 else 0.0)
		var pedal: Vector3 = (
			BOTTOM_BRACKET + Vector3(sign * PEDAL_OFFSET, CRANK * sin(angle), -CRANK * cos(angle))
		)
		var hip: Vector3 = HIP + Vector3(sign * 0.1, 0, 0)
		var knee: Vector3 = _knee(hip, pedal)
		var parts: Array = _legs[side]
		var thigh: MeshInstance3D = parts[0]
		var shin: MeshInstance3D = parts[1]
		var crank: MeshInstance3D = parts[2]
		_place(thigh, hip, knee)
		_place(shin, knee, pedal)
		_place(crank, BOTTOM_BRACKET + Vector3(sign * PEDAL_OFFSET, 0, 0), pedal)


## Knee position for a leg from `hip` to `foot`, bending forwards.
func _knee(hip: Vector3, foot: Vector3) -> Vector3:
	var to_foot: Vector3 = foot - hip
	var reach: float = clampf(to_foot.length(), 0.05, THIGH + SHIN - 0.001)
	var direction: Vector3 = to_foot.normalized()
	var along: float = (THIGH * THIGH - SHIN * SHIN + reach * reach) / (2.0 * reach)
	var out: float = sqrt(maxf(THIGH * THIGH - along * along, 0.0))
	var forward: Vector3 = Vector3.FORWARD
	var bend: Vector3 = (forward - direction * forward.dot(direction)).normalized()
	return hip + direction * along + bend * out


func _build_bike() -> void:
	var frame: StandardMaterial3D = _material(FRAME)
	var tyre: StandardMaterial3D = _material(TYRE)
	for hub: Vector3 in [REAR_HUB, FRONT_HUB]:
		var wheel: Node3D = Node3D.new()
		wheel.position = hub
		add_child(wheel)
		var rim: TorusMesh = TorusMesh.new()
		rim.inner_radius = WHEEL_RADIUS - 0.035
		rim.outer_radius = WHEEL_RADIUS
		rim.rings = 24
		rim.ring_segments = 6
		var rim_node: MeshInstance3D = _mesh(rim, tyre)
		rim_node.rotation.z = PI / 2.0
		wheel.add_child(rim_node)
		for k: int in range(4):
			var spoke: BoxMesh = BoxMesh.new()
			spoke.size = Vector3(0.008, WHEEL_RADIUS * 2.0 - 0.06, 0.008)
			var spoke_node: MeshInstance3D = _mesh(spoke, frame)
			spoke_node.rotation.x = k * PI / 4.0
			wheel.add_child(spoke_node)
		_wheels.append(wheel)
	for tube: Array in [
		[BOTTOM_BRACKET, SADDLE],
		[BOTTOM_BRACKET, HEAD_TUBE],
		[SADDLE, HEAD_TUBE],
		[BOTTOM_BRACKET, REAR_HUB],
		[SADDLE, REAR_HUB],
		[HEAD_TUBE, FRONT_HUB],
		[HEAD_TUBE, BARS],
	]:
		var from: Vector3 = tube[0]
		var to: Vector3 = tube[1]
		_place(_segment(0.018, frame), from, to)
	_place(_segment(0.012, frame), BARS + Vector3(-0.2, 0, 0), BARS + Vector3(0.2, 0, 0))
	var saddle: BoxMesh = BoxMesh.new()
	saddle.size = Vector3(0.12, 0.03, 0.26)
	var saddle_node: MeshInstance3D = _mesh(saddle, frame)
	saddle_node.position = SADDLE + Vector3(0, 0.02, 0)
	add_child(saddle_node)


func _build_rider() -> void:
	var jersey: StandardMaterial3D = _material(JERSEY)
	var shorts: StandardMaterial3D = _material(SHORTS)
	var skin: StandardMaterial3D = _material(SKIN)
	var frame: StandardMaterial3D = _material(FRAME)
	for side: int in range(2):
		_legs.append([_segment(0.075, shorts), _segment(0.05, skin), _segment(0.015, frame)])
	_place(_segment(0.15, jersey), HIP, SHOULDER)
	var head: SphereMesh = SphereMesh.new()
	head.radius = 0.11
	head.height = 0.22
	var head_node: MeshInstance3D = _mesh(head, skin)
	head_node.position = SHOULDER + Vector3(0, 0.17, -0.1)
	add_child(head_node)
	var helmet: SphereMesh = SphereMesh.new()
	helmet.radius = 0.13
	helmet.height = 0.2
	helmet.is_hemisphere = true
	var helmet_node: MeshInstance3D = _mesh(helmet, _material(HELMET))
	helmet_node.position = head_node.position + Vector3(0, 0.02, 0.01)
	add_child(helmet_node)
	for sign: float in [-1.0, 1.0]:
		var shoulder: Vector3 = SHOULDER + Vector3(sign * 0.17, -0.03, 0)
		var hand: Vector3 = BARS + Vector3(sign * 0.19, 0.03, 0)
		var elbow: Vector3 = (shoulder + hand) / 2.0 + Vector3(sign * 0.05, -0.06, 0)
		_place(_segment(0.05, jersey), shoulder, elbow)
		_place(_segment(0.04, skin), elbow, hand)


## A capsule from one point to another, positioned later with `_place`.
func _segment(radius: float, material: Material) -> MeshInstance3D:
	var capsule: CapsuleMesh = CapsuleMesh.new()
	capsule.radius = radius
	capsule.radial_segments = 8
	capsule.rings = 2
	var node: MeshInstance3D = _mesh(capsule, material)
	add_child(node)
	return node


## Stretches a capsule (y axis) between two points.
func _place(node: MeshInstance3D, from: Vector3, to: Vector3) -> void:
	var capsule: CapsuleMesh = node.mesh as CapsuleMesh
	var span: Vector3 = to - from
	var length: float = maxf(span.length(), 0.001)
	capsule.height = maxf(length + capsule.radius * 2.0, capsule.radius * 2.0)
	var up: Vector3 = span / length
	var side: Vector3 = up.cross(Vector3.FORWARD)
	if side.length() < 0.01:
		side = up.cross(Vector3.RIGHT)
	side = side.normalized()
	node.transform = Transform3D(Basis(side, up, side.cross(up)), (from + to) / 2.0)


func _mesh(mesh: Mesh, material: Material) -> MeshInstance3D:
	var node: MeshInstance3D = MeshInstance3D.new()
	node.mesh = mesh
	node.material_override = material
	return node


static func _material(color: Color) -> StandardMaterial3D:
	var material: StandardMaterial3D = StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.7
	return material
