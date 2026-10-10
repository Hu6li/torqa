extends SceneTree
## Renders the Blender models (app/assets/models) as the world draws them, for review (ADR 0009):
## every building and plant in three variants side by side into $OUT_DIR/<group>/<name>.png,
## buildings also close up (<name>-close.png), and the clouds together (clouds/clouds.png).
## Run it with scripts/render-models.sh; $GROUPS (space-separated: buildings, vegetation,
## clouds) and $MODELS (names) limit it to some. With $GALLERY set, every overview also goes
## there as a small JPEG, <group>/<name>.jpg, for the galleries in art/README.md; a run of a
## whole group replaces its old pictures, so models taken out leave none behind. Light and
## colours are the world's (ADR 0011).

const GROUPS: PackedStringArray = ["buildings", "vegetation", "clouds"]
## Plaster and roof colour (indices into the palette's walls and tiles, or a plant's colours)
## and variant (shutters below 0.25, flowers above 0.65 hidden).
const VARIANTS: Array = [[0, 0, 0.5], [2, 1, 0.3], [1, 2, 0.9]]
## The palette colours of walls and roofs (accents for modern buildings, bands for
## lighthouses) by kind, as the world paints them (core/torqa-world/src/buildings/mod.rs);
## other kinds take plaster and tiles.
const BUILDING_COLOURS: Dictionary[String, Array] = {
	"church": ["buildings.light_walls", "buildings.tiles"],
	"chapel": ["buildings.light_walls", "buildings.tiles"],
	"castle": ["buildings.castle_walls", "buildings.tiles"],
	"lighthouse": ["buildings.frame", "buildings.beacon"],
	"tropical": ["buildings.tropical_walls", "buildings.tiles"],
	"office": ["buildings.modern_walls", "buildings.accents"],
	"hotel": ["buildings.modern_walls", "buildings.accents"],
	"public_flat": ["buildings.modern_walls", "buildings.accents"],
}
## The palette list each kind of plant takes its colour from, as the world picks it
## (core/torqa-world/src/vegetation.rs).
const PLANT_COLOURS: Dictionary[String, String] = {
	"conifer": "plants.conifers",
	"broadleaf": "plants.broadleaves",
	"bush": "plants.bushes",
	"rock": "plants.rocks",
	"palm": "plants.palms",
	"banana": "plants.tropical",
	"tropical_bush": "plants.tropical",
}
const GALLERY_SIZE: Vector2i = Vector2i(640, 360)

var _camera: Camera3D = Camera3D.new()
var _sun: DirectionalLight3D = DirectionalLight3D.new()
var _row: MultiMeshInstance3D = MultiMeshInstance3D.new()
var _out: String
var _gallery: String


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_stage()
	_out = OS.get_environment("OUT_DIR")
	_gallery = OS.get_environment("GALLERY")
	var groups: PackedStringArray = OS.get_environment("GROUPS").split(" ", false)
	var wanted: PackedStringArray = OS.get_environment("MODELS").split(" ", false)
	for group: String in GROUPS:
		if not groups.is_empty() and not groups.has(group):
			continue
		DirAccess.make_dir_recursive_absolute(_out.path_join(group))
		if not _gallery.is_empty():
			var folder: String = _gallery.path_join(group)
			DirAccess.make_dir_recursive_absolute(folder)
			if wanted.is_empty():
				for old: String in DirAccess.get_files_at(folder):
					DirAccess.remove_absolute(folder.path_join(old))
		if group == "clouds":
			await _clouds()
			continue
		var kinds: Dictionary = _kinds(group)
		for model: String in kinds:
			if not wanted.is_empty() and not wanted.has(model):
				continue
			var mesh: Mesh = (
				BuildingModels.mesh(model) if group == "buildings" else VegetationModels.mesh(model)
			)
			var kind: String = kinds[model]
			if group == "buildings":
				_show(mesh, _colours(group, model, kind), Vector3(0.45, 0.42, 1.0), 0.62)
			else:
				_show(mesh, _colours(group, model, kind), Vector3(0.2, 0.3, 1.0), 0.5)
			await _save(group, model, true)
			if group == "buildings":
				_close_up(mesh)
				await _save(group, model + "-close", false)
			print("rendered ", group, "/", model)
	quit(0)


func _stage() -> void:
	var environment: Environment = Environment.new()
	var sky: Sky = Sky.new()
	var sky_material: ProceduralSkyMaterial = ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Palette.color("sky.top")
	sky_material.sky_horizon_color = Palette.color("sky.horizon")
	sky_material.ground_horizon_color = Palette.color("sky.horizon")
	sky_material.ground_bottom_color = Palette.color("ground.meadow")
	sky.sky_material = sky_material
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	# As in the world (world.tscn, RideWorld.TIMES): linear tone mapping, a warm sky light.
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_color = Palette.color("light.ambient")
	environment.ambient_light_sky_contribution = 0.55
	environment.ambient_light_energy = 0.55
	environment.ssao_enabled = true
	var world: WorldEnvironment = WorldEnvironment.new()
	world.environment = environment
	root.add_child(world)

	_sun.rotation_degrees = Vector3(-38.0, -35.0, 0.0)
	_sun.shadow_enabled = true
	_sun.light_color = Palette.color("light.sun")
	_sun.light_energy = 0.8
	_sun.directional_shadow_max_distance = 150.0
	root.add_child(_sun)

	var ground: MeshInstance3D = MeshInstance3D.new()
	var plane: PlaneMesh = PlaneMesh.new()
	plane.size = Vector2(600.0, 600.0)
	var grass: StandardMaterial3D = StandardMaterial3D.new()
	grass.albedo_color = Palette.color("ground.meadow")
	grass.roughness = 0.95
	plane.material = grass
	ground.mesh = plane
	root.add_child(ground)

	# Calm air: trees swaying in the wind would come out differently on every run.
	VegetationModels.set_wind(0.0)
	root.add_child(_row)
	_camera.fov = 40.0
	root.add_child(_camera)
	_camera.make_current()


## The models of `group` by name, with their kind, from the manifest its build wrote.
func _kinds(group: String) -> Dictionary:
	var directory: String = (
		BuildingModels.DIRECTORY if group == "buildings" else VegetationModels.DIRECTORY
	)
	var manifest: Dictionary = JSON.parse_string(
		FileAccess.get_file_as_string(directory + "models.json")
	)
	var models: Dictionary = manifest["models"]
	var kinds: Dictionary = {}
	for model: String in models.keys():
		var entry: Dictionary = models[model]
		kinds[model] = entry["kind"]
	kinds.sort()
	return kinds


## Instance colour and custom data of each variant of `model`, a `kind` of `group`.
func _colours(group: String, model: String, kind: String) -> Array[Array]:
	var colours: Array[Array] = []
	for variant: Array in VARIANTS:
		if group == "vegetation":
			var plants: String = PLANT_COLOURS.get(kind, "plants.bushes")
			var tint: int = variant[0]
			colours.append([_pick(plants, tint), Color(0.0, 0.0, 0.0, 0.0)])
			continue
		var paints: Array = BUILDING_COLOURS.get(
			"public_flat" if model.begins_with("public_flat") else kind,
			["buildings.walls", "buildings.tiles"]
		)
		var walls: String = paints[0]
		var roofs: String = paints[1]
		var wall_index: int = variant[0]
		var roof_index: int = variant[1]
		var plaster: Color = _pick(walls, wall_index)
		var roof: Color = _pick(roofs, roof_index)
		var custom: float = variant[2]
		colours.append([plaster, Color(roof.r, roof.g, roof.b, custom)])
	return colours


## Colour `index` of the palette entry `path`, a list (wrapping round) or a single colour.
func _pick(path: String, index: int) -> Color:
	if not Palette.is_list(path):
		return Palette.color(path)
	var colours: PackedColorArray = Palette.colors(path)
	return colours[index % colours.size()]


## Instances of `mesh` in a row, coloured as `colours` says, the camera looking at them from
## `direction`, `closeness` times as far as would take in the whole row whatever its shape
## (buildings from the front corner; thin plants from nearer the front, and closer).
func _show(mesh: Mesh, colours: Array[Array], direction: Vector3, closeness: float) -> void:
	var size: Vector3 = mesh.get_aabb().size
	var multimesh: MultiMesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_3D
	multimesh.use_colors = true
	multimesh.use_custom_data = true
	multimesh.mesh = mesh
	multimesh.instance_count = colours.size()
	# Buildings stand 6 m apart; plants and rocks closer, by their size.
	var spacing: float = size.x + clampf(size.x, 0.5, 6.0)
	for i: int in colours.size():
		var offset: float = (i - (colours.size() - 1) / 2.0) * spacing
		var colour: Color = colours[i][0]
		var custom: Color = colours[i][1]
		multimesh.set_instance_transform(i, Transform3D(Basis(), Vector3(offset, 0.0, 0.0)))
		multimesh.set_instance_color(i, colour)
		multimesh.set_instance_custom_data(i, custom)
	_row.multimesh = multimesh
	_row.show()
	var extent: float = spacing * colours.size() * 0.5
	var radius: float = Vector2(extent, maxf(size.y, size.z)).length()
	var target: Vector3 = Vector3(0.0, size.y * 0.35, 0.0)
	var distance: float = radius / sin(deg_to_rad(_camera.fov * 0.5)) * closeness
	_camera.look_at_from_position(target + direction.normalized() * distance, target, Vector3.UP)


## The camera at eye level by the middle instance's front corner.
func _close_up(mesh: Mesh) -> void:
	var size: Vector3 = mesh.get_aabb().size
	var corner: Vector3 = Vector3(size.x * 0.5, 0.0, size.z * 0.5)
	var eye: Vector3 = corner + Vector3(size.x * 0.35, 1.7, size.z * 0.9)
	_camera.look_at_from_position(eye, Vector3(0.0, size.y * 0.45, size.z * 0.1), Vector3.UP)


## The clouds side by side in the sky, lit as the cloud layer lights them, seen from below.
func _clouds() -> void:
	_row.hide()
	var material: ShaderMaterial = ShaderMaterial.new()
	material.shader = preload("res://shaders/cloud.gdshader")
	material.set_shader_parameter("cloud_color", Palette.color("sky.cloud"))
	material.set_shader_parameter("shade_color", Palette.color("sky.cloud_shade"))
	material.set_shader_parameter("horizon_color", Palette.color("sky.horizon"))
	material.set_shader_parameter("sun_color", Palette.color("light.sun"))
	material.set_shader_parameter("sun_direction", _sun.global_transform.basis.z)
	var clouds: Array[MeshInstance3D] = []
	var x: float = 0.0
	for model: String in CloudLayer.MODELS:
		var scene: PackedScene = load(CloudLayer.DIRECTORY + model + ".glb")
		var loaded: Node = scene.instantiate()
		var source: MeshInstance3D = loaded.find_children("*", "MeshInstance3D", true, false)[0]
		var cloud: MeshInstance3D = MeshInstance3D.new()
		cloud.mesh = source.mesh
		loaded.free()
		cloud.material_override = material
		cloud.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		var size: Vector3 = cloud.mesh.get_aabb().size
		cloud.position = Vector3(x + size.x * 0.5, 14.0, 0.0)
		x += size.x + 3.0
		root.add_child(cloud)
		clouds.append(cloud)
	var width: float = x - 3.0
	for cloud: MeshInstance3D in clouds:
		cloud.position.x -= width * 0.5
	# Far enough back for the row to fill about four fifths of the picture's width.
	var aspect: float = float(GALLERY_SIZE.x) / GALLERY_SIZE.y
	var distance: float = width * 0.6 / (tan(deg_to_rad(_camera.fov * 0.5)) * aspect)
	var target: Vector3 = Vector3(0.0, 16.0, 0.0)
	var eye: Vector3 = target + Vector3(0.12, -0.1, 1.0).normalized() * distance
	_camera.look_at_from_position(eye, target, Vector3.UP)
	await _save("clouds", "clouds", true)
	for cloud: MeshInstance3D in clouds:
		cloud.queue_free()
	print("rendered clouds")


## Lets the picture settle, then saves it as `group`/`name`.png and, with `gallery`, as a small
## JPEG into the gallery.
func _save(group: String, name: String, gallery: bool) -> void:
	for frame: int in range(20):
		await process_frame
	var image: Image = root.get_texture().get_image()
	image.save_png(_out.path_join(group).path_join(name + ".png"))
	if gallery and not _gallery.is_empty():
		image.resize(GALLERY_SIZE.x, GALLERY_SIZE.y, Image.INTERPOLATE_LANCZOS)
		image.save_jpg(_gallery.path_join(group).path_join(name + ".jpg"), 0.8)
