class_name RiderAvatar
extends Node3D
## A low-poly cyclist on a road bike, built from primitives with road-bike proportions. The
## cranks turn with the cadence, the legs follow the pedals (two-bone inverse kinematics) and
## the wheels turn with the speed.
## Local axes: −z forward, +y up, +x right; the origin is on the ground between the wheels.

const JERSEY: Color = Color(0.04, 0.55, 0.9)
const JERSEY_SIDE: Color = Color(0.95, 0.96, 0.97)
const SHORTS: Color = Color(0.06, 0.06, 0.07)
const SKIN: Color = Color(0.84, 0.65, 0.52)
const HELMET: Color = Color(0.96, 0.96, 0.97)
const SHOE: Color = Color(0.92, 0.93, 0.94)
const FRAME: Color = Color(0.1, 0.12, 0.15)
const FRAME_ACCENT: Color = Color(0.04, 0.55, 0.9)
const TYRE: Color = Color(0.04, 0.04, 0.045)
const RIM: Color = Color(0.55, 0.57, 0.6)

const WHEEL_RADIUS: float = 0.34
const REAR_HUB: Vector3 = Vector3(0, WHEEL_RADIUS, 0.5)
const FRONT_HUB: Vector3 = Vector3(0, WHEEL_RADIUS, -0.5)
const BOTTOM_BRACKET: Vector3 = Vector3(0, 0.27, 0.07)
const SEAT_CLUSTER: Vector3 = Vector3(0, 0.8, 0.22)
const SADDLE: Vector3 = Vector3(0, 0.93, 0.27)
const HEAD_TOP: Vector3 = Vector3(0, 0.84, -0.4)
const HEAD_BOTTOM: Vector3 = Vector3(0, 0.7, -0.44)
const STEM: Vector3 = Vector3(0, 0.88, -0.5)
const HOODS: Vector3 = Vector3(0, 0.9, -0.58)
const CRANK: float = 0.17
const PEDAL_OFFSET: float = 0.12
const THIGH: float = 0.44
const SHIN: float = 0.43
## The ankle sits this far above and behind the pedal axle.
const ANKLE_OFFSET: Vector3 = Vector3(0, 0.07, 0.04)
const HIP: Vector3 = Vector3(0, 0.97, 0.24)
const SHOULDER: Vector3 = Vector3(0, 1.3, -0.24)

var _crank_angle: float = 0.0
var _wheel_angle: float = 0.0
var _wheels: Array[Node3D] = []
var _crankset: Node3D
## Per side (left, right): thigh, shin, shoe.
var _legs: Array[Array] = []
## The rider's body parts (everything that is not the bike).
var _body: Array[Node] = []


func _ready() -> void:
	_build_bike()
	var bike_parts: int = get_child_count()
	_build_rider()
	_body = get_children().slice(bike_parts)
	animate(0.0, 0.0, 0.0)


## Shows or hides the rider (not the bike), e.g. for the first-person view.
func show_rider(shown: bool) -> void:
	for part: Node in _body:
		(part as Node3D).visible = shown


## Advances the animation: cadence in rpm, speed in km/h.
func animate(delta: float, cadence_rpm: float, speed_kmh: float) -> void:
	_crank_angle = fmod(_crank_angle + cadence_rpm / 60.0 * TAU * delta, TAU)
	_wheel_angle = fmod(_wheel_angle + speed_kmh / 3.6 / WHEEL_RADIUS * delta, TAU)
	for wheel: Node3D in _wheels:
		wheel.rotation.x = -_wheel_angle
	_crankset.rotation.x = -_crank_angle
	for side: int in range(2):
		var sign: float = -1.0 if side == 0 else 1.0
		var angle: float = _crank_angle + (PI if side == 0 else 0.0)
		var pedal: Vector3 = (
			BOTTOM_BRACKET + Vector3(sign * PEDAL_OFFSET, CRANK * sin(angle), -CRANK * cos(angle))
		)
		var ankle: Vector3 = pedal + ANKLE_OFFSET
		var hip: Vector3 = HIP + Vector3(sign * 0.09, 0, 0)
		var knee: Vector3 = _knee(hip, ankle)
		var parts: Array = _legs[side]
		var thigh: MeshInstance3D = parts[0]
		var shin: MeshInstance3D = parts[1]
		var shoe: MeshInstance3D = parts[2]
		_place(thigh, hip, knee)
		_place(shin, knee, ankle)
		_place(shoe, ankle + Vector3(0, -0.02, 0.06), pedal + Vector3(0, 0, -0.08))


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
	var frame: StandardMaterial3D = _material(FRAME, 0.35)
	var accent: StandardMaterial3D = _material(FRAME_ACCENT, 0.35)
	var tyre: StandardMaterial3D = _material(TYRE, 0.8)
	var rim: StandardMaterial3D = _material(RIM, 0.3)
	for hub: Vector3 in [REAR_HUB, FRONT_HUB]:
		var wheel: Node3D = Node3D.new()
		wheel.position = hub
		add_child(wheel)
		wheel.add_child(_ring(WHEEL_RADIUS, 0.024, tyre))
		wheel.add_child(_ring(WHEEL_RADIUS - 0.03, 0.012, rim))
		for k: int in range(8):
			var spoke: BoxMesh = BoxMesh.new()
			spoke.size = Vector3(0.004, WHEEL_RADIUS * 2.0 - 0.07, 0.004)
			var spoke_node: MeshInstance3D = _mesh(spoke, rim)
			spoke_node.rotation.x = k * PI / 8.0
			wheel.add_child(spoke_node)
		var hub_mesh: CylinderMesh = CylinderMesh.new()
		hub_mesh.top_radius = 0.025
		hub_mesh.bottom_radius = 0.025
		hub_mesh.height = 0.1
		var hub_node: MeshInstance3D = _mesh(hub_mesh, rim)
		hub_node.rotation.z = PI / 2.0
		wheel.add_child(hub_node)
		_wheels.append(wheel)

	for tube: Array in [
		[BOTTOM_BRACKET, SEAT_CLUSTER, 0.017, frame],
		[BOTTOM_BRACKET, HEAD_BOTTOM, 0.02, accent],
		[SEAT_CLUSTER, HEAD_TOP, 0.015, frame],
		[HEAD_BOTTOM, HEAD_TOP, 0.02, frame],
		[SEAT_CLUSTER, SADDLE, 0.012, frame],
		[HEAD_TOP, STEM, 0.013, frame],
		[HEAD_BOTTOM, FRONT_HUB, 0.012, frame],
	]:
		var from: Vector3 = tube[0]
		var to: Vector3 = tube[1]
		var radius: float = tube[2]
		var material: Material = tube[3]
		_place(_segment(radius, material), from, to)
	for x: float in [-0.05, 0.05]:
		var side: Vector3 = Vector3(x, 0, 0)
		_place(_segment(0.009, frame), BOTTOM_BRACKET + side, REAR_HUB + side)
		_place(_segment(0.008, frame), SEAT_CLUSTER + side * 0.5, REAR_HUB + side)
	# Drop handlebar: bar across, then forward to the hoods and down into the drops.
	_place(_segment(0.011, frame), STEM + Vector3(-0.2, 0, 0), STEM + Vector3(0.2, 0, 0))
	for x: float in [-0.2, 0.2]:
		var bar: Vector3 = STEM + Vector3(x, 0, 0)
		var hood: Vector3 = HOODS + Vector3(x, 0, 0)
		_place(_segment(0.011, frame), bar, hood)
		_place(_segment(0.011, frame), hood, hood + Vector3(0, -0.12, 0.06))
	var saddle: BoxMesh = BoxMesh.new()
	saddle.size = Vector3(0.11, 0.035, 0.27)
	var saddle_node: MeshInstance3D = _mesh(saddle, _material(SHORTS, 0.5))
	saddle_node.position = SADDLE + Vector3(0, 0.02, 0)
	add_child(saddle_node)

	_crankset = Node3D.new()
	_crankset.position = BOTTOM_BRACKET
	add_child(_crankset)
	_crankset.add_child(_ring(0.1, 0.008, rim))
	for side: float in [-1.0, 1.0]:
		var arm: BoxMesh = BoxMesh.new()
		arm.size = Vector3(0.015, 0.03, CRANK)
		var arm_node: MeshInstance3D = _mesh(arm, rim)
		arm_node.position = Vector3(side * PEDAL_OFFSET * 0.85, 0, -side * CRANK / 2.0)
		_crankset.add_child(arm_node)


func _build_rider() -> void:
	var jersey: StandardMaterial3D = _material(JERSEY, 0.55)
	var side_panel: StandardMaterial3D = _material(JERSEY_SIDE, 0.55)
	var shorts: StandardMaterial3D = _material(SHORTS, 0.6)
	var skin: StandardMaterial3D = _material(SKIN, 0.7)
	var shoe: StandardMaterial3D = _material(SHOE, 0.4)
	for side: int in range(2):
		var foot: BoxMesh = BoxMesh.new()
		foot.size = Vector3(0.09, 0.06, 0.22)
		var shoe_node: MeshInstance3D = _mesh(foot, shoe)
		add_child(shoe_node)
		_legs.append([_segment(0.07, shorts), _segment(0.045, skin), shoe_node])

	# Torso: hips to chest, slimmer towards the waist, with white side panels.
	var chest: Vector3 = SHOULDER + Vector3(0, -0.04, 0.04)
	_place(_segment(0.12, jersey), HIP + Vector3(0, 0.02, 0), chest)
	_place(_segment(0.135, jersey), chest + Vector3(0, -0.06, 0.12), chest)
	for sign: float in [-1.0, 1.0]:
		_place(
			_segment(0.03, side_panel),
			HIP + Vector3(sign * 0.12, 0.06, -0.02),
			chest + Vector3(sign * 0.125, -0.02, 0.06)
		)

	var neck: Vector3 = SHOULDER + Vector3(0, 0.08, -0.06)
	var head_center: Vector3 = neck + Vector3(0, 0.12, -0.08)
	_place(_segment(0.045, skin), SHOULDER, neck)
	var head: SphereMesh = SphereMesh.new()
	head.radius = 0.095
	head.height = 0.21
	var head_node: MeshInstance3D = _mesh(head, skin)
	head_node.position = head_center
	add_child(head_node)
	# Streamlined helmet: a flattened, elongated dome.
	var helmet: SphereMesh = SphereMesh.new()
	helmet.radius = 0.12
	helmet.height = 0.18
	helmet.is_hemisphere = true
	var helmet_node: MeshInstance3D = _mesh(helmet, _material(HELMET, 0.25))
	helmet_node.position = head_center + Vector3(0, 0.025, 0.02)
	helmet_node.scale = Vector3(1.0, 0.9, 1.25)
	helmet_node.rotation.x = deg_to_rad(-12.0)
	add_child(helmet_node)

	for sign: float in [-1.0, 1.0]:
		var shoulder: Vector3 = SHOULDER + Vector3(sign * 0.17, -0.02, 0.02)
		var hand: Vector3 = HOODS + Vector3(sign * 0.2, 0.03, 0.02)
		var elbow: Vector3 = (shoulder + hand) / 2.0 + Vector3(sign * 0.04, -0.07, 0.04)
		_place(_segment(0.042, jersey), shoulder, elbow)
		_place(_segment(0.034, skin), elbow, hand)
		var glove: SphereMesh = SphereMesh.new()
		glove.radius = 0.035
		glove.height = 0.07
		var glove_node: MeshInstance3D = _mesh(glove, shorts)
		glove_node.position = hand
		add_child(glove_node)


## A ring in the wheel plane (y–z), for tyres, rims and the chainring.
func _ring(radius: float, thickness: float, material: Material) -> MeshInstance3D:
	var torus: TorusMesh = TorusMesh.new()
	torus.inner_radius = radius - thickness
	torus.outer_radius = radius
	torus.rings = 32
	torus.ring_segments = 6
	var node: MeshInstance3D = _mesh(torus, material)
	node.rotation.z = PI / 2.0
	return node


## A capsule from one point to another, positioned later with `_place`.
func _segment(radius: float, material: Material) -> MeshInstance3D:
	var capsule: CapsuleMesh = CapsuleMesh.new()
	capsule.radius = radius
	capsule.radial_segments = 10
	capsule.rings = 3
	var node: MeshInstance3D = _mesh(capsule, material)
	add_child(node)
	return node


## Stretches a capsule (y axis) between two points; boxes (shoes) are laid along their z axis.
func _place(node: MeshInstance3D, from: Vector3, to: Vector3) -> void:
	var span: Vector3 = to - from
	var length: float = maxf(span.length(), 0.001)
	var capsule: CapsuleMesh = node.mesh as CapsuleMesh
	if capsule != null:
		capsule.height = length + capsule.radius * 2.0
	var up: Vector3 = span / length
	var side: Vector3 = up.cross(Vector3.FORWARD)
	if side.length() < 0.01:
		side = up.cross(Vector3.RIGHT)
	side = side.normalized()
	if capsule == null:
		node.transform = Transform3D(Basis(side, up.cross(side), up), (from + to) / 2.0)
		return
	node.transform = Transform3D(Basis(side, up, side.cross(up)), (from + to) / 2.0)


func _mesh(mesh: Mesh, material: Material) -> MeshInstance3D:
	var node: MeshInstance3D = MeshInstance3D.new()
	node.mesh = mesh
	node.material_override = material
	return node


static func _material(color: Color, roughness: float) -> StandardMaterial3D:
	var material: StandardMaterial3D = StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = roughness
	return material
