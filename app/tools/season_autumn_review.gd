class_name SeasonAutumnReview
extends RefCounted
## Real road/terrain materials in a controlled woodland, open-road and distant-canopy scene.

const ALTITUDE: float = 700.0


static func render(world: RideWorld, camera: Camera3D, out: String, season: String) -> void:
	var terrain: ShaderMaterial = world.get("_terrain_material")
	var forest: Color = Palette.color("ground.forest")
	forest.a = 0.0
	_quad(
		world,
		[
			Vector3(-400, 756, -700),
			Vector3(400, 756, -700),
			Vector3(400, 712, -150),
			Vector3(-400, 712, -150)
		],
		terrain,
		forest,
		false
	)
	camera.fov = 55.0
	camera.look_at_from_position(Vector3(150, 850, 50), Vector3(0, 730, -400))
	await _save(world, out.path_join(season.to_lower() + "-canopy.png"))
	var road: ShaderMaterial = world.get("_road_material")
	var pixels: PackedByteArray = PackedByteArray()
	pixels.resize(16)
	for index: int in range(8):
		pixels[index] = 255
	var image: Image = Image.create_from_data(16, 1, false, Image.FORMAT_R8, pixels)
	road.set_shader_parameter("autumn_forest_mask", ImageTexture.create_from_image(image))
	road.set_shader_parameter("autumn_forest_origin", Vector2(-20, 0))
	road.set_shader_parameter("autumn_forest_size", Vector2(40, 40))
	for centre: float in [-8.0, 12.0]:
		_quad(
			world,
			[
				Vector3(centre - 2, ALTITUDE + 0.03, 14),
				Vector3(centre + 2, ALTITUDE + 0.03, 14),
				Vector3(centre + 2, ALTITUDE + 0.03, 30),
				Vector3(centre - 2, ALTITUDE + 0.03, 30)
			],
			road,
			Color(0, 4.0 / 32.0, 0.25, 0.125),
			true
		)
		camera.fov = 65.0
		camera.look_at_from_position(
			Vector3(centre + 0.4, ALTITUDE + 1.6, 30), Vector3(centre, ALTITUDE + 0.05, 22)
		)
		await _save(
			world,
			out.path_join(
				season.to_lower() + ("-forest-road.png" if centre < 0 else "-open-road.png")
			)
		)


static func _quad(
	world: RideWorld, corners: Array[Vector3], material: Material, colour: Color, road: bool
) -> void:
	var tool: SurfaceTool = SurfaceTool.new()
	tool.begin(Mesh.PRIMITIVE_TRIANGLES)
	tool.set_normal(Vector3.UP)
	tool.set_color(colour)
	var uvs: Array[Vector2] = [Vector2(0, 0), Vector2(1, 0), Vector2(1, 16), Vector2(0, 16)]
	for index: int in [0, 1, 2, 0, 2, 3]:
		tool.set_uv(uvs[index] if road else Vector2(corners[index].x, corners[index].z))
		tool.add_vertex(corners[index])
	var node: MeshInstance3D = MeshInstance3D.new()
	node.mesh = tool.commit()
	node.material_override = material
	world.add_child(node)


static func _save(world: RideWorld, path: String) -> void:
	for frame: int in range(6):
		await world.get_tree().process_frame
	await RenderingServer.frame_post_draw
	world.get_viewport().get_texture().get_image().save_png(path)
	print("saved ", path)
