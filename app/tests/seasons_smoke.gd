extends SceneTree
## Season selection, compatibility with earlier ride options, and independent conditions (R154).

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var options: RideOptions = RideOptions.new()
	root.add_child(options)
	options.set_options({})
	var selected: String = options.options()["season"]
	_check(selected == "Summer", "old options default to Summer")
	for season: String in OrqwaApp.seasons():
		options.set_options({"season": season, "weather": "Rain", "time": "Evening"})
		selected = options.options()["season"]
		var weather: String = options.options()["weather"]
		var time: String = options.options()["time"]
		_check(selected == season, "each available season can be selected")
		_check(weather == "Rain", "season preserves weather")
		_check(time == "Evening", "season preserves time of day")
		options.show_option_groups(false, true)
		var control: OptionButton = options.get("_season")
		_check(not control.visible, "season is hidden for video rides")
		selected = options.options()["season"]
		_check(selected == season, "hidden season retains selection")
	options.free()
	_small_plants()

	_winter()
	_spring()
	_autumn()
	_clothing()
	if not _failed:
		print("SEASONS SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _check(ok: bool, message: String) -> void:
	if not ok:
		_failed = true
		push_error(message)


func _new_world() -> RideWorld:
	return (load("res://scenes/world.tscn") as PackedScene).instantiate() as RideWorld


func _small_plants() -> void:
	var original: PackedFloat32Array = PackedFloat32Array()
	for index: int in range(100):
		original.append_array(PackedFloat32Array([1, 0, 0, float(index), 0, 1, 0, 700, 0, 0, 1, 5]))
	for season: String in OrqwaApp.seasons():
		var plants: Dictionary = OrqwaApp.small_plants_for_season(
			PackedFloat32Array(), original, season
		)
		var grass: PackedFloat32Array = plants["grass"]
		var flowers: PackedFloat32Array = plants["flowers"]
		_check(
			grass.size() + flowers.size() == original.size(), "season preserves all plant positions"
		)
		if season == "Summer":
			_check(grass.size() > original.size() * 0.75, "summer is predominantly grass")
		elif season == "Autumn":
			_check(flowers.is_empty(), "autumn has no meadow flowers")
		elif season == "Spring":
			_check(flowers == original, "spring restores all meadow flowers")


func _winter() -> void:
	var world: RideWorld = _new_world()
	root.add_child(world)
	world.apply_options({"season": "Winter", "weather": "Clear", "time": "Midday", "camera": 0})
	var needles: ShaderMaterial = VegetationModels.material("needles")
	var strength: float = needles.get_shader_parameter("winter_strength")
	_check(strength == 1.0, "winter reaches existing evergreen materials")
	var broadleaf: bool = needles.get_shader_parameter("winter_broadleaf")
	_check(not broadleaf, "winter keeps evergreen needles dense")
	var road: ShaderMaterial = world.get("_road_material")
	strength = road.get_shader_parameter("winter_strength")
	_check(strength == 1.0, "winter reaches street snow and roadside grass")
	var banks: WinterSnowbanks = world.get_node("WinterSnowbanks") as WinterSnowbanks
	_check(banks.visible, "winter enables the disposable snowbank layer")
	var terrain: ShaderMaterial = world.get("_terrain_material")
	strength = terrain.get_shader_parameter("winter_strength")
	_check(strength == 1.0, "winter enables the raised natural-ground blanket")
	world.apply_options({"season": "Summer", "weather": "Rain", "time": "Evening", "camera": 0})
	strength = needles.get_shader_parameter("winter_strength")
	_check(strength == 0.0, "summer clears accumulated snow")
	_check(not banks.visible, "summer hides roadside snow banks")
	strength = road.get_shader_parameter("winter_strength")
	_check(strength == 0.0, "summer restores road verges too")
	strength = terrain.get_shader_parameter("winter_strength")
	_check(strength == 0.0, "summer restores the original ground height")
	var snow_line: float = terrain.get_shader_parameter("snow_line")
	_check(snow_line == 2400.0, "summer restores the high snow line")
	world.free()


func _autumn() -> void:
	var seasons: SeasonMaterials = SeasonMaterials.new()
	var terrain: ShaderMaterial = ShaderMaterial.new()
	terrain.shader = load("res://shaders/terrain.gdshader")
	seasons.apply("Autumn", null, terrain)
	var leaves: ShaderMaterial = VegetationModels.material("leaves")
	var strength: float = leaves.get_shader_parameter("autumn_strength")
	_check(strength == 1.0, "autumn reaches existing broadleaf materials")
	var needles: ShaderMaterial = VegetationModels.material("needles")
	strength = needles.get_shader_parameter("autumn_strength")
	_check(strength == 0.0, "evergreen needles stay green")
	var flowers: ShaderMaterial = BuildingModels.material("flowers") as ShaderMaterial
	var hidden: bool = flowers.get_shader_parameter("autumn_flowers_hidden")
	_check(hidden, "autumn hides flower-box blooms")
	seasons.apply("Summer", null, terrain)
	strength = leaves.get_shader_parameter("autumn_strength")
	_check(strength == 0.0, "summer restores existing broadleaves")
	hidden = flowers.get_shader_parameter("autumn_flowers_hidden")
	_check(not hidden, "summer restores flower-box blooms")
	var world: RideWorld = _new_world()
	root.add_child(world)
	world.apply_options({"season": "Autumn", "weather": "Rain", "time": "Evening", "camera": 0})
	var road: ShaderMaterial = world.get("_road_material")
	strength = road.get_shader_parameter("autumn_strength")
	_check(strength == 1.0, "autumn enables woodland road litter")
	world.apply_options({"season": "Summer", "weather": "Rain", "time": "Evening", "camera": 0})
	strength = road.get_shader_parameter("autumn_strength")
	_check(strength == 0.0, "summer clears fallen road leaves")
	world.free()


func _spring() -> void:
	var world: RideWorld = _new_world()
	root.add_child(world)
	world.apply_options({"season": "Spring", "weather": "Rain", "time": "Evening", "camera": 0})
	var plants: ShaderMaterial = world.get("_plant_material")
	var bloom: float = plants.get_shader_parameter("spring_bloom")
	_check(
		bloom == 1.0,
		"spring opens existing flower clumps (bloom %s, available %s)" % [bloom, OrqwaApp.seasons()]
	)
	var needles: ShaderMaterial = VegetationModels.material("needles")
	var strength: float = needles.get_shader_parameter("spring_strength")
	_check(strength == 0.0, "spring preserves evergreen needles")
	var leaves: ShaderMaterial = VegetationModels.material("leaves")
	var atlas: Texture2D = leaves.get_shader_parameter("spring_blossom_texture")
	_check(
		atlas != null and atlas.get_width() == 1024, "spring binds the petal-shaped blossom atlas"
	)
	world.apply_options({"season": "Summer", "weather": "Clear", "time": "Midday", "camera": 0})
	bloom = plants.get_shader_parameter("spring_bloom")
	_check(bloom == 0.0, "summer restores smaller flower heads")
	strength = leaves.get_shader_parameter("spring_strength")
	_check(strength == 0.0, "summer restores leaf silhouettes on flowering trees")
	world.free()


func _clothing() -> void:
	var world: RideWorld = _new_world()
	root.add_child(world)
	var local: RiderAvatar = world.get("_avatar")
	var ghost: RiderAvatar = world.get("_ghost")
	var group: GroupRidersWorld = world.get("_group_riders")
	group.set_process(false)
	var peer: Dictionary = {
		"id": 1,
		"generation": "1",
		"avatar": "male",
		"name": "Alex",
		"rider_x": 0.0,
		"rider_y": 5.0,
		"elevation_m": 0.0,
		"heading": 0.0,
		"grade": 0.0,
		"speed_kmh": 20.0,
		"cadence": 80.0,
		"effort": 2.0,
		"curvature": 0.0,
	}
	group.show_riders([peer], 0.0)
	local.animate(0.25, 60.0, 20.0)
	var crank: float = local.crank_angle()
	var wheel: Node3D = local.find_child("wheel_front", true, false) as Node3D
	var wheel_rotation: Vector3 = wheel.rotation
	for season: String in ["Autumn", "Winter", "Spring", "Winter", "Summer", "unknown"]:
		world.apply_options({"season": season, "weather": "Clear", "time": "Midday", "camera": 1})
		var winter: bool = season in ["Autumn", "Winter"]
		var friend: GroupRiderAvatar = group.get_child(0)
		for avatar: RiderAvatar in [local, ghost, friend._avatar]:
			_check(avatar.winter_clothing == winter, "%s dresses local, ghost and friends" % season)
			var body: MeshInstance3D = avatar.find_child("body", true, false) as MeshInstance3D
			_check(
				is_equal_approx(body.transparency, 0.45 if avatar.ghostly else 0.0),
				"changing kit preserves ghost transparency"
			)
			var worn: PackedStringArray = []
			for surface: int in range(body.mesh.get_surface_count()):
				var kind: String = body.mesh.surface_get_material(surface).resource_name
				worn.append(kind)
				if kind == "gilet":
					var fabric: StandardMaterial3D = body.get_surface_override_material(surface)
					var color: Color = avatar.accent
					if color.a == 0.0:
						color = Palette.color("rider_" + avatar.rider + ".jersey")
					_check(fabric.albedo_color == color, "gilets retain rider and ghost colours")
			for item: String in ["cap", "gloves", "tights", "gilet"]:
				_check(worn.has(item) == winter, "%s uses the expected %s" % [season, item])
		_check(is_equal_approx(local.crank_angle(), crank), "changing kit preserves pedal phase")
		wheel = local.find_child("wheel_front", true, false) as Node3D
		_check(
			wheel.rotation.is_equal_approx(wheel_rotation), "changing kit preserves wheel rotation"
		)
		var local_body: Node3D = local.find_child("body", true, false) as Node3D
		_check(not local_body.visible, "changing kit preserves first-person body hiding")
		var id: int = peer["id"]
		peer["id"] = id + 1
		group.show_riders([peer], 0.0)
		var joined: GroupRiderAvatar = group.get_child(0)
		_check(joined._avatar.winter_clothing == winter, "new friends inherit the selected season")
	world.free()
