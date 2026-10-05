extends SceneTree
## Renders the riders on their bikes (art/riders) as the world draws them, from a few fixed
## views and at a few crank angles, into $OUT_DIR/<rider>-<view>.png for review (ADR 0009). Run
## it with scripts/render-riders.sh. Light and colours are the world's (ADR 0011).

## Name, camera position and target (rider's local axes: −z forward), crank angle in degrees
## and whether the rider leans into a bend (40 km/h, 40 m radius, to the right).
const VIEWS: Array = [
	["side", Vector3(3.2, 1.0, 0.0), Vector3(0.0, 0.85, 0.0), 0.0],
	["side-down", Vector3(3.2, 1.0, 0.0), Vector3(0.0, 0.85, 0.0), 90.0],
	["front", Vector3(1.6, 1.5, -2.6), Vector3(0.0, 0.9, 0.0), 30.0],
	["chase", Vector3(0.6, 1.9, 4.2), Vector3(0.0, 1.0, -0.4), 200.0],
	["face", Vector3(0.7, 1.4, -1.6), Vector3(0.0, 1.25, -0.35), 120.0],
	["head", Vector3(-0.75, 1.55, 0.45), Vector3(0.0, 1.3, -0.4), 160.0],
	["lean", Vector3(0.0, 1.6, 4.0), Vector3(0.0, 0.9, -0.4), 60.0, true],
]

var _camera: Camera3D = Camera3D.new()


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_stage()
	var out: String = OS.get_environment("OUT_DIR")
	DirAccess.make_dir_recursive_absolute(out)
	for rider: String in RiderAvatar.RIDERS:
		var avatar: RiderAvatar = RiderAvatar.new()
		avatar.rider = rider
		root.add_child(avatar)
		var turned: float = 0.0
		for view: Array in VIEWS:
			var view_name: String = view[0]
			var eye: Vector3 = view[1]
			var target: Vector3 = view[2]
			var crank: float = view[3]
			# At 60 rpm the cranks turn a full turn per second: on to the wanted angle in one step.
			avatar.animate(fposmod(deg_to_rad(crank) - turned, TAU) / TAU, 60.0, 0.0)
			turned = deg_to_rad(crank)
			var leaning: bool = view.size() > 4 and view[4]
			var lean: float = TorqaApp.lean_angle(40.0, 1.0 / 40.0) if leaning else 0.0
			avatar.transform = Transform3D(Basis(Vector3.FORWARD, lean), Vector3.ZERO)
			_camera.look_at_from_position(eye, target, Vector3.UP)
			for frame: int in range(12):
				await process_frame
			root.get_texture().get_image().save_png(out.path_join(rider + "-" + view_name + ".png"))
		print("rendered ", rider)
		avatar.free()
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
	sun.rotation_degrees = Vector3(-42.0, 35.0, 0.0)
	sun.shadow_enabled = true
	sun.light_color = Palette.color("light.sun")
	sun.light_energy = 0.8
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

	_camera.fov = 40.0
	root.add_child(_camera)
	_camera.make_current()
