extends SceneTree
## A fixed material review at 700 m: the real ground, foliage, LOD-capable meshes and flowers.
## Run with software Vulkan and SEASON/OUT_DIR; this does not generate or download a course.

var _altitude: float = 700.0


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var altitude: String = OS.get_environment("ALTITUDE")
	if not altitude.is_empty():
		_altitude = float(altitude)
	var orqwa: OrqwaApp = OrqwaApp.new()
	root.add_child(orqwa)
	var world: RideWorld = (load("res://scenes/world.tscn") as PackedScene).instantiate()
	root.add_child(world)
	world.bind(orqwa)
	var season: String = OS.get_environment("SEASON")
	if season.is_empty():
		season = "Summer"
	world.apply_options({"season": season, "weather": "Clear", "time": "Midday", "camera": 0})
	root.msaa_3d = Viewport.MSAA_DISABLED
	root.screen_space_aa = Viewport.SCREEN_SPACE_AA_FXAA
	var environment: Environment = world.get("_environment")
	environment.glow_enabled = false
	_ground(world)
	for index: int in range(8):
		var broadleaf: bool = index % 3 != 0
		var kind: String = "broadleaf_round" if broadleaf else "conifer_broad"
		var colours: PackedColorArray = Palette.colors(
			"plants.broadleaves" if broadleaf else "plants.conifers"
		)
		_model(
			world,
			kind,
			Vector3(float(index % 4) * 10.0 - 15.0, _altitude, float(index / 4) * -16.0),
			colours[index % colours.size()]
		)
	_model(world, "rock_pair", Vector3(16, _altitude, 10), Palette.colors("plants.rocks")[0])
	var flowers: MultiMesh = MultiMesh.new()
	flowers.transform_format = MultiMesh.TRANSFORM_3D
	flowers.mesh = world.get("_flower_mesh")
	flowers.instance_count = 12
	for index: int in range(12):
		flowers.set_instance_transform(
			index,
			Transform3D(
				Basis.IDENTITY,
				Vector3(float(index % 4) * 0.6 - 0.9, _altitude, 12.0 + float(index / 4) * 0.6)
			)
		)
	var small: Dictionary = OrqwaApp.small_plants_for_season(
		PackedFloat32Array(), flowers.buffer, season
	)
	for kind: String in ["grass", "flowers"]:
		var buffer: PackedFloat32Array = small[kind]
		if not buffer.is_empty():
			var clump: MultiMeshInstance3D = world.call(
				"_plants", buffer, kind == "flowers", 1000.0
			)
			world.add_child(clump)
	var out: String = OS.get_environment("OUT_DIR")
	DirAccess.make_dir_recursive_absolute(out)
	var camera: Camera3D = world.get("_camera")
	camera.fov = 45.0
	camera.look_at_from_position(Vector3(20, _altitude + 7, 25), Vector3(0, _altitude + 4, -5))
	await _save(out.path_join(season.to_lower() + "-trees.png"))
	camera.fov = 70.0
	camera.look_at_from_position(
		Vector3(1.4, _altitude + 0.7, 15.4), Vector3(0, _altitude + 0.4, 12.5)
	)
	await _save(out.path_join(season.to_lower() + "-flowers.png"))
	if season == "Winter":
		await _roof(world, camera, out)
		await _blanket(world, camera, out)
	if season == "Spring":
		_model(
			world,
			"broadleaf_round",
			Vector3(0, _altitude, 0),
			Palette.colors("plants.broadleaves")[0]
		)
		camera.fov = 55.0
		camera.look_at_from_position(Vector3(1, _altitude + 3, 10), Vector3(0, _altitude + 3, 0))
		await _save(out.path_join("spring-blossom.png"))
	if season == "Autumn" or OS.get_environment("FOREST_REVIEW") == "1":
		await SeasonAutumnReview.render(world, camera, out, season)
	quit()


func _save(path: String) -> void:
	for frame: int in range(6):
		await process_frame
	await RenderingServer.frame_post_draw
	root.get_texture().get_image().save_png(path)
	print("saved ", path)


func _model(world: RideWorld, name: String, position: Vector3, colour: Color) -> void:
	var instances: MultiMesh = MultiMesh.new()
	instances.transform_format = MultiMesh.TRANSFORM_3D
	instances.use_colors = true
	instances.mesh = VegetationModels.mesh(name)
	instances.instance_count = 1
	instances.set_instance_transform(0, Transform3D(Basis.IDENTITY, position))
	instances.set_instance_color(0, colour)
	var node: MultiMeshInstance3D = MultiMeshInstance3D.new()
	node.multimesh = instances
	world.add_child(node)


func _ground(world: RideWorld) -> void:
	var tool: SurfaceTool = SurfaceTool.new()
	tool.begin(Mesh.PRIMITIVE_TRIANGLES)
	tool.set_normal(Vector3.UP)
	var meadow: Color = Palette.color("ground.meadow")
	meadow.a = 0.0
	tool.set_color(meadow)
	for point: Vector2 in [
		Vector2(-100, -100),
		Vector2(100, -100),
		Vector2(100, 100),
		Vector2(-100, -100),
		Vector2(100, 100),
		Vector2(-100, 100)
	]:
		tool.set_uv(point)
		tool.add_vertex(Vector3(point.x, _altitude, point.y))
	var ground: MeshInstance3D = MeshInstance3D.new()
	ground.mesh = tool.commit()
	ground.material_override = world.get("_terrain_material")
	world.add_child(ground)


func _roof(world: RideWorld, camera: Camera3D, out: String) -> void:
	var house: MultiMesh = MultiMesh.new()
	house.transform_format = MultiMesh.TRANSFORM_3D
	house.use_colors = true
	house.use_custom_data = true
	house.mesh = BuildingModels.mesh("chalet_2_m")
	house.instance_count = 1
	house.set_instance_transform(0, Transform3D(Basis.IDENTITY, Vector3(0, _altitude, 12)))
	house.set_instance_color(0, Palette.colors("buildings.light_walls")[0])
	house.set_instance_custom_data(0, Palette.colors("buildings.tiles")[0])
	var node: MultiMeshInstance3D = MultiMeshInstance3D.new()
	node.multimesh = house
	world.add_child(node)
	camera.fov = 55.0
	camera.look_at_from_position(Vector3(14, _altitude + 12, 26), Vector3(0, _altitude + 3, 12))
	await _save(out.path_join("winter-roof.png"))


func _blanket(world: RideWorld, camera: Camera3D, out: String) -> void:
	# A one-metre stake with ten-centimetre bands makes the blanket's actual depth visible.
	for index: int in range(10):
		var marker: MeshInstance3D = MeshInstance3D.new()
		var box: BoxMesh = BoxMesh.new()
		box.size = Vector3(0.08, 0.1, 0.08)
		marker.mesh = box
		marker.position = Vector3(14.8, _altitude + float(index) * 0.1 + 0.05, 10.0)
		var material: StandardMaterial3D = StandardMaterial3D.new()
		material.albedo_color = Palette.color(
			"props.pole_red" if index % 2 == 0 else "road.marking"
		)
		marker.material_override = material
		world.add_child(marker)
	camera.fov = 50.0
	camera.look_at_from_position(
		Vector3(18.5, _altitude + 1.4, 14), Vector3(15.5, _altitude + 0.4, 10)
	)
	await _save(out.path_join("winter-blanket.png"))
