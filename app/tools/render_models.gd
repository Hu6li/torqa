extends SceneTree
## Renders every building model (app/assets/models/buildings) as the world draws it, in three
## variants side by side and close up, into $OUT_DIR/<name>.png and <name>-close.png for review
## (ADR 0009). Run it with scripts/render-models.sh; $MODELS (space-separated names) limits it
## to some. Light and colours are the world's (ADR 0011).

## Plaster and roof colour (indices into the palette's walls and tiles) and variant (shutters
## below 0.25, flowers above 0.65 hidden).
const VARIANTS: Array = [[0, 0, 0.5], [2, 1, 0.3], [1, 2, 0.9]]

var _camera: Camera3D = Camera3D.new()
var _row: MultiMeshInstance3D = MultiMeshInstance3D.new()


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_stage()
	var out: String = OS.get_environment("OUT_DIR")
	DirAccess.make_dir_recursive_absolute(out)
	var wanted: PackedStringArray = OS.get_environment("MODELS").split(" ", false)
	for file: String in DirAccess.get_files_at(BuildingModels.DIRECTORY):
		if not file.ends_with(".glb"):
			continue
		var model: String = file.get_basename()
		if not wanted.is_empty() and not wanted.has(model):
			continue
		var mesh: Mesh = BuildingModels.mesh(model)
		_show(mesh, model)
		for frame: int in range(20):
			await process_frame
		root.get_texture().get_image().save_png(out.path_join(model + ".png"))
		_close_up(mesh)
		for frame: int in range(20):
			await process_frame
		root.get_texture().get_image().save_png(out.path_join(model + "-close.png"))
		print("rendered ", model)
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

	var sun: DirectionalLight3D = DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-38.0, -35.0, 0.0)
	sun.shadow_enabled = true
	sun.light_color = Palette.color("light.sun")
	sun.light_energy = 0.8
	sun.directional_shadow_max_distance = 150.0
	root.add_child(sun)

	var ground: MeshInstance3D = MeshInstance3D.new()
	var plane: PlaneMesh = PlaneMesh.new()
	plane.size = Vector2(600.0, 600.0)
	var grass: StandardMaterial3D = StandardMaterial3D.new()
	grass.albedo_color = Palette.color("ground.meadow")
	grass.roughness = 0.95
	plane.material = grass
	ground.mesh = plane
	root.add_child(ground)

	root.add_child(_row)
	_camera.fov = 40.0
	root.add_child(_camera)
	_camera.make_current()


## Three instances of `mesh` (model `model`) in a row, the camera framing them from the front
## corner.
func _show(mesh: Mesh, model: String) -> void:
	var size: Vector3 = mesh.get_aabb().size
	var multimesh: MultiMesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_3D
	multimesh.use_colors = true
	multimesh.use_custom_data = true
	multimesh.mesh = mesh
	multimesh.instance_count = VARIANTS.size()
	var spacing: float = size.x + 6.0
	for i: int in VARIANTS.size():
		var variant: Array = VARIANTS[i]
		var offset: float = (i - (VARIANTS.size() - 1) / 2.0) * spacing
		multimesh.set_instance_transform(i, Transform3D(Basis(), Vector3(offset, 0.0, 0.0)))
		# Offices, hotels and flat-roofed public buildings: modern walls, an accent colour.
		var modern: bool = (
			model.begins_with("office")
			or model.begins_with("hotel")
			or model.begins_with("public_flat")
		)
		var walls: String = "buildings.modern_walls" if modern else "buildings.walls"
		var plaster: Color = Palette.colors(walls)[variant[0]]
		multimesh.set_instance_color(i, plaster)
		var roofs: String = "buildings.accents" if modern else "buildings.tiles"
		var roof: Color = Palette.colors(roofs)[variant[1]]
		var custom: float = variant[2]
		multimesh.set_instance_custom_data(i, Color(roof.r, roof.g, roof.b, custom))
	_row.multimesh = multimesh
	var extent: float = spacing * VARIANTS.size() * 0.5
	var radius: float = Vector2(extent, maxf(size.y, size.z)).length()
	var direction: Vector3 = Vector3(0.45, 0.42, 1.0).normalized()
	var target: Vector3 = Vector3(0.0, size.y * 0.35, 0.0)
	var distance: float = radius / sin(deg_to_rad(_camera.fov * 0.5)) * 0.62
	_camera.look_at_from_position(target + direction * distance, target, Vector3.UP)


## The camera at eye level by the middle instance's front corner.
func _close_up(mesh: Mesh) -> void:
	var size: Vector3 = mesh.get_aabb().size
	var corner: Vector3 = Vector3(size.x * 0.5, 0.0, size.z * 0.5)
	var eye: Vector3 = corner + Vector3(size.x * 0.35, 1.7, size.z * 0.9)
	_camera.look_at_from_position(eye, Vector3(0.0, size.y * 0.45, size.z * 0.1), Vector3.UP)
