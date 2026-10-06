class_name CloudLayer
extends Node3D
## Low-poly clouds drifting over the world (ADR 0011): the Blender-made puffs of art/clouds
## spread round the camera, high over the land, as many as the weather's cloud cover asks for.
## They drift with the wind; the area they lie in wraps round, so the camera never rides out
## of it.

const DIRECTORY: String = "res://assets/models/clouds/"
const MODELS: PackedStringArray = ["cloud_small", "cloud_puffy", "cloud_long", "cloud_tower"]
## Clouds lie within this many metres of the camera, east-west and north-south.
const SPREAD: float = 9000.0
## How many clouds there are at full cover.
const MOST: int = 60
## Their height above the land (`settle`), and their size: the models are about 10 m long.
const ALTITUDE: Vector2 = Vector2(700.0, 1500.0)
const SCALE: Vector2 = Vector2(25.0, 60.0)
## Wind: metres per second east and north (Godot: x, −z).
const DRIFT: Vector2 = Vector2(5.0, 2.0)

## Share of the sky covered (0 clear, 1 overcast): more clouds, and bigger and lower ones.
var cover: float = 0.3:
	set(value):
		cover = clampf(value, 0.0, 1.0)
		_count = roundi(MOST * cover)
var material: ShaderMaterial = ShaderMaterial.new()
var _count: int = roundi(MOST * 0.3)
var _base: float = 0.0
var _drift: Vector2 = Vector2.ZERO
## Per cloud: model index, position in the area (x, z), height, scale, turn.
var _layout: Array[Array] = []
var _layers: Array[MultiMeshInstance3D] = []


func _ready() -> void:
	material.shader = preload("res://shaders/cloud.gdshader")
	material.set_shader_parameter("cloud_color", Palette.color("sky.cloud"))
	material.set_shader_parameter("shade_color", Palette.color("sky.cloud_shade"))
	var random: RandomNumberGenerator = RandomNumberGenerator.new()
	random.seed = 7
	for k: int in range(MOST):
		(
			_layout
			. append(
				[
					random.randi_range(0, MODELS.size() - 1),
					Vector2(
						random.randf_range(-SPREAD, SPREAD), random.randf_range(-SPREAD, SPREAD)
					),
					random.randf_range(ALTITUDE.x, ALTITUDE.y),
					random.randf_range(SCALE.x, SCALE.y),
					random.randf_range(0.0, TAU),
				]
			)
		)
	for model: String in MODELS:
		var scene: PackedScene = load(DIRECTORY + model + ".glb")
		var root: Node = scene.instantiate()
		var source: MeshInstance3D = root.find_children("*", "MeshInstance3D", true, false)[0]
		var multimesh: MultiMesh = MultiMesh.new()
		multimesh.transform_format = MultiMesh.TRANSFORM_3D
		multimesh.mesh = source.mesh
		root.free()
		var layer: MultiMeshInstance3D = MultiMeshInstance3D.new()
		layer.multimesh = multimesh
		layer.material_override = material
		layer.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		add_child(layer)
		_layers.append(layer)


## Puts the clouds over land at `height` (metres): the route's ground, say.
func settle(height: float) -> void:
	_base = height


## The light: unit vector towards the sun and its colour, the horizon's colour, and how dull
## the weather is (0 fair, 1 overcast).
func light(towards_sun: Vector3, sun: Color, horizon: Color, darkness: float) -> void:
	material.set_shader_parameter("sun_direction", towards_sun.normalized())
	material.set_shader_parameter("sun_color", sun)
	material.set_shader_parameter("horizon_color", horizon)
	material.set_shader_parameter("darkness", darkness)


func _process(delta: float) -> void:
	var camera: Camera3D = get_viewport().get_camera_3d()
	if camera == null or not is_visible_in_tree():
		return
	_drift += DRIFT * delta
	var placed: Array[Array] = placements(camera.global_position)
	for index: int in range(_layers.size()):
		var multimesh: MultiMesh = _layers[index].multimesh
		var transforms: Array = placed[index]
		multimesh.instance_count = transforms.size()
		for k: int in range(transforms.size()):
			var transform_k: Transform3D = transforms[k]
			multimesh.set_instance_transform(k, transform_k)


## Where the clouds are round a camera at `eye`: their transforms by model (`MODELS` order).
func placements(eye: Vector3) -> Array[Array]:
	var around: Vector2 = Vector2(eye.x, eye.z)
	var placed: Array[Array] = []
	for model: String in MODELS:
		placed.append([])
	for k: int in range(_count):
		var cloud: Array = _layout[k]
		var model: int = cloud[0]
		var at: Vector2 = cloud[1]
		# Drifting east and north: Godot's north is −z.
		var moved: Vector2 = at + Vector2(_drift.x, -_drift.y) - around
		var wrapped: Vector2 = Vector2(
			wrapf(moved.x, -SPREAD, SPREAD), wrapf(moved.y, -SPREAD, SPREAD)
		)
		# Bad weather: bigger clouds, hanging lower.
		var height: float = cloud[2] * (1.0 - 0.35 * cover)
		var size: float = cloud[3] * (1.0 + 0.8 * cover)
		var turn: float = cloud[4]
		var shape: Basis = Basis(Vector3.UP, turn).scaled(Vector3.ONE * size)
		var spot: Vector3 = Vector3(around.x + wrapped.x, _base + height, around.y + wrapped.y)
		placed[model].append(Transform3D(shape, spot))
	return placed
