extends SceneTree
## Offline review of buildings on a hillside (R257), from meshes exported by
## slope_buildings_render_fixture: production shell material, models and terrain.


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var source: String = ProjectSettings.globalize_path("res://../screenshots/slope-buildings.json")
	var data: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(source))
	var out: String = OS.get_environment("OUT_DIR")
	var scene: Node3D = Node3D.new()
	root.add_child(scene)
	var environment: WorldEnvironment = WorldEnvironment.new()
	environment.environment = Environment.new()
	environment.environment.background_mode = Environment.BG_COLOR
	environment.environment.background_color = Color(0.49, 0.62, 0.72)
	environment.environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	environment.environment.ambient_light_color = Color(0.7, 0.77, 0.85)
	environment.environment.ambient_light_energy = 0.6
	environment.environment.tonemap_mode = Environment.TONE_MAPPER_AGX
	scene.add_child(environment)
	var sun: DirectionalLight3D = DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-40.0, -60.0, 0.0)
	sun.light_energy = 2.0
	sun.shadow_enabled = true
	scene.add_child(sun)
	var shell_material: ShaderMaterial = ShaderMaterial.new()
	BuildingModels.configure_shell(shell_material)
	var terrain_material: ShaderMaterial = ShaderMaterial.new()
	_terrain(terrain_material)
	for mesh_data: Dictionary in data["meshes"]:
		var instance: MeshInstance3D = MeshInstance3D.new()
		instance.mesh = WorldMeshes.from_arrays(_arrays(mesh_data))
		var terrain: bool = mesh_data["kind"] == "terrain"
		instance.material_override = terrain_material if terrain else shell_material
		scene.add_child(instance)
	for cell: Dictionary in data["cells"]:
		var models: Dictionary = {}
		var placed: Dictionary = cell["models"]
		for name: String in placed:
			var values: Array = placed[name]
			models[name] = PackedFloat32Array(values)
		var shells: Dictionary = cell["shells"]
		var upload: Dictionary = {"models": models, "shells": _arrays(shells)}
		BuildingModels.add_to(scene, upload, shell_material, 400.0, 1800.0, false)
	var camera: Camera3D = Camera3D.new()
	scene.add_child(camera)
	camera.current = true
	# From the valley, west of the houses and level with their downhill walls.
	for view: Array in [
		["valley", Vector3(-5.0, 4.0, -445.0), Vector3(80.0, 14.0, -488.0)],
		["close", Vector3(55.0, 13.0, -455.0), Vector3(85.0, 16.0, -480.0)],
		["above", Vector3(20.0, 60.0, -400.0), Vector3(80.0, 10.0, -485.0)],
	]:
		var eye: Vector3 = view[1]
		var target: Vector3 = view[2]
		camera.position = eye
		camera.look_at(target)
		var name: String = view[0]
		await _save(out.path_join("slope-%s.png" % name))
	quit()


func _save(path: String) -> void:
	for frame: int in range(8):
		await process_frame
	await RenderingServer.frame_post_draw
	root.get_texture().get_image().save_png(path)
	print("saved slope: " + path)


func _terrain(material: ShaderMaterial) -> void:
	material.shader = preload("res://shaders/terrain.gdshader")
	for surface: String in ["grass", "dirt", "gravel", "rock"]:
		for channel: String in ["albedo", "normal"]:
			material.set_shader_parameter(
				surface + "_" + channel,
				load("res://assets/textures/%s_%s.webp" % [surface, channel])
			)
	UrbanSurfaces.configure_terrain(material)
	for cover: String in ["meadow", "farmland", "orchard", "forest", "town", "industrial", "rock"]:
		var colour: Color = Palette.color("ground." + cover)
		material.set_shader_parameter("cover_" + cover, Vector3(colour.r, colour.g, colour.b))
	material.set_shader_parameter("snow_color", Palette.color("ground.snow"))


func _arrays(data: Dictionary) -> Dictionary:
	var vertices: PackedVector3Array = PackedVector3Array()
	var normals: PackedVector3Array = PackedVector3Array()
	var uvs: PackedVector2Array = PackedVector2Array()
	var colors: PackedColorArray = PackedColorArray()
	var indices: Array = data["indices"]
	for v: Array in data["vertices"]:
		var p: PackedFloat32Array = PackedFloat32Array(v)
		vertices.append(Vector3(p[0], p[1], p[2]))
	for v: Array in data["normals"]:
		var p: PackedFloat32Array = PackedFloat32Array(v)
		normals.append(Vector3(p[0], p[1], p[2]))
	for v: Array in data["uvs"]:
		var p: PackedFloat32Array = PackedFloat32Array(v)
		uvs.append(Vector2(p[0], p[1]))
	for v: Array in data["colors"]:
		var p: PackedFloat32Array = PackedFloat32Array(v)
		colors.append(Color(p[0], p[1], p[2], p[3]))
	return {
		"vertices": vertices,
		"normals": normals,
		"uvs": uvs,
		"colors": colors,
		"indices": PackedInt32Array(indices),
	}
