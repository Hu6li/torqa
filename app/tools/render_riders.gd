extends SceneTree
## Renders the riders on their bikes (art/riders) as the world draws them, from a few fixed
## views and at a few crank angles, into $OUT_DIR/<rider>-<view>.png for review (ADR 0009). Run
## it with scripts/render-riders.sh. With $GALLERY set, every view also goes there as a small
## JPEG, riders/<rider>-<view>.jpg, for the gallery in art/README.md. Light and colours are the
## world's (ADR 0020).

## Name, camera position and target (rider's local axes: −z forward), crank angle in degrees,
## whether the rider leans into a bend (40 km/h, 40 m radius, to the right) and whether they
## stand on the pedals (the bike rocked under them as at 60 rpm).
## Helmet views use head-relative offsets and orthographic projection for silhouette review.
## Views prefixed with bike- hide the body; VIEWS selects space-separated view names.
const VIEWS: Array = [
	["side", Vector3(3.2, 1.0, 0.0), Vector3(0.0, 0.85, 0.0), 0.0],
	["side-down", Vector3(3.2, 1.0, 0.0), Vector3(0.0, 0.85, 0.0), 90.0],
	["front", Vector3(1.6, 1.5, -2.6), Vector3(0.0, 0.9, 0.0), 30.0],
	["chase", Vector3(0.6, 1.9, 4.2), Vector3(0.0, 1.0, -0.4), 200.0],
	["face", Vector3(0.7, 1.4, -1.6), Vector3(0.0, 1.25, -0.35), 120.0],
	["head", Vector3(-0.75, 1.55, 0.45), Vector3(0.0, 1.3, -0.4), 160.0],
	["helmet-side", Vector3(0.6, 0.0, 0.0), Vector3.ZERO, 0.0],
	["helmet-rear", Vector3(0.0, 0.0, 0.6), Vector3.ZERO, 0.0],
	["lean", Vector3(0.0, 1.6, 4.0), Vector3(0.0, 0.9, -0.4), 60.0, true],
	["standing", Vector3(3.2, 1.0, 0.0), Vector3(0.0, 0.85, 0.0), 0.0, false, true],
	["standing-chase", Vector3(0.6, 1.9, 4.2), Vector3(0.0, 1.0, -0.4), 70.0, false, true],
	["bike-side", Vector3(2.25, 0.9, 0.0), Vector3(0.0, 0.56, 0.0), 0.0],
	["bike-drive", Vector3(1.05, 0.65, 0.95), Vector3(0.0, 0.35, 0.2), 110.0],
	["bike-front", Vector3(-1.6, 1.25, -1.8), Vector3(0.0, 0.55, -0.1), 0.0],
	["bike-cockpit", Vector3(0.15, 1.5, 0.05), Vector3(0.0, 0.78, -0.6), 0.0],
	["bike-clearance", Vector3(1.05, 0.78, -0.4), Vector3(0.0, 0.7, -0.4), 0.0],
	["bike-saddle", Vector3(0.85, 1.15, 0.75), Vector3(0.0, 0.88, 0.25), 0.0],
]
const GALLERY_SIZE: Vector2i = Vector2i(640, 360)

var _camera: Camera3D = Camera3D.new()


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_stage()
	var out: String = OS.get_environment("OUT_DIR")
	DirAccess.make_dir_recursive_absolute(out)
	var gallery: String = OS.get_environment("GALLERY")
	if not gallery.is_empty():
		gallery = gallery.path_join("riders")
		DirAccess.make_dir_recursive_absolute(gallery)
	for rider: String in RiderAvatar.RIDERS:
		var avatar: RiderAvatar = RiderAvatar.new()
		avatar.rider = rider
		root.add_child(avatar)
		var turned: float = 0.0
		for view: Array in VIEWS:
			var view_name: String = view[0]
			var selected: PackedStringArray = OS.get_environment("VIEWS").split(" ", false)
			if not selected.is_empty() and not selected.has(view_name):
				continue
			avatar.show_rider(not view_name.begins_with("bike-"))
			var eye: Vector3 = view[1]
			var target: Vector3 = view[2]
			var crank: float = view[3]
			var standing: bool = view.size() > 5 and view[5]
			var motion: RiderMotion = RiderMotion.new()
			if standing:
				motion.standing = 1.0
				motion.rock = RiderMotion.ROCK_STANDING * sin(deg_to_rad(crank))
			# At 60 rpm the cranks turn a full turn per second: on to the wanted angle in one step.
			avatar.animate(fposmod(deg_to_rad(crank) - turned, TAU) / TAU, 60.0, 0.0, motion)
			turned = deg_to_rad(crank)
			var leaning: bool = view.size() > 4 and view[4]
			var lean: float = OrqwaApp.lean_angle(40.0, 1.0 / 40.0) if leaning else 0.0
			avatar.transform = Transform3D(Basis(Vector3.FORWARD, lean + motion.rock), Vector3.ZERO)
			var camera_up: Vector3 = Vector3.UP
			if view_name.begins_with("helmet-"):
				var skeletons: Array[Node] = avatar.find_children("*", "Skeleton3D", true, false)
				var skeleton: Skeleton3D = skeletons[0] as Skeleton3D
				var head_pose: Transform3D = (
					skeleton.global_transform
					* skeleton.get_bone_global_pose(skeleton.find_bone("head"))
				)
				var head_basis: Basis = head_pose.basis.orthonormalized()
				target = avatar.head_top() + head_basis * Vector3(0.0, -0.065, -0.035)
				eye = target + head_basis * eye
				camera_up = head_basis.y
				_camera.projection = Camera3D.PROJECTION_ORTHOGONAL
				_camera.size = 0.30
			else:
				_camera.projection = Camera3D.PROJECTION_PERSPECTIVE
			_camera.look_at_from_position(eye, target, camera_up)
			for frame: int in range(12):
				await process_frame
			var image: Image = root.get_texture().get_image()
			image.save_png(out.path_join(rider + "-" + view_name + ".png"))
			print(
				(
					"rendered %s-%s: %d draw calls, %d primitives"
					% [
						rider,
						view_name,
						root.get_render_info(
							Viewport.RENDER_INFO_TYPE_VISIBLE,
							Viewport.RENDER_INFO_DRAW_CALLS_IN_FRAME
						),
						root.get_render_info(
							Viewport.RENDER_INFO_TYPE_VISIBLE,
							Viewport.RENDER_INFO_PRIMITIVES_IN_FRAME
						),
					]
				)
			)
			if not gallery.is_empty():
				image.resize(GALLERY_SIZE.x, GALLERY_SIZE.y, Image.INTERPOLATE_LANCZOS)
				image.save_jpg(gallery.path_join(rider + "-" + view_name + ".jpg"), 0.8)
		print("rendered ", rider)
		avatar.free()
	quit(0)


func _stage() -> void:
	var environment: Environment = Environment.new()
	var sky: Sky = Sky.new()
	var sky_material: ShaderMaterial = ShaderMaterial.new()
	sky_material.shader = preload("res://shaders/sky.gdshader")
	sky_material.set_shader_parameter("ground_color", Palette.color("plants.grass_base"))
	sky_material.set_shader_parameter("cloud_color", Palette.color("sky.cloud"))
	sky_material.set_shader_parameter("cloud_shade_color", Palette.color("sky.cloud_shade"))
	sky_material.set_shader_parameter("cloud_cover", 0.3)
	sky.sky_material = sky_material
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	# Match the riding view's atmosphere and filmic response, so paint is judged in its light.
	environment.tonemap_mode = Environment.TONE_MAPPER_AGX
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_color = Palette.color("light.ambient")
	environment.ambient_light_sky_contribution = 0.9
	environment.ambient_light_energy = RideWorld.TIMES["Midday"]["ambient"]
	environment.ssao_enabled = true
	var world: WorldEnvironment = WorldEnvironment.new()
	world.environment = environment
	root.add_child(world)

	var sun: DirectionalLight3D = DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-42.0, 35.0, 0.0)
	sun.shadow_enabled = true
	sun.light_color = Palette.color("light.sun")
	sun.light_energy = RideWorld.TIMES["Midday"]["energy"]
	sun.directional_shadow_max_distance = 20.0
	root.add_child(sun)

	var ground: MeshInstance3D = MeshInstance3D.new()
	var plane: PlaneMesh = PlaneMesh.new()
	plane.size = Vector2(60.0, 60.0)
	var road: StandardMaterial3D = StandardMaterial3D.new()
	road.albedo_color = Palette.color("road.asphalt")
	road.roughness = 0.95
	plane.material = road
	ground.mesh = plane
	root.add_child(ground)
	root.screen_space_aa = Viewport.SCREEN_SPACE_AA_FXAA

	_camera.fov = 40.0
	root.add_child(_camera)
	_camera.make_current()
