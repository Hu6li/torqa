extends SceneTree
## R218: native white UI text survives world post-processing, while scenery hides nameplates.

var _failed: bool = false
var _camera: Camera3D
var _world: Node3D
var _wall: MeshInstance3D
var _visibility: GroupNameplateVisibility
var _cards: Array[GroupRiderNameplate] = []


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_world = Node3D.new()
	root.add_child(_world)
	var environment: WorldEnvironment = WorldEnvironment.new()
	environment.environment = Environment.new()
	environment.environment.background_mode = Environment.BG_COLOR
	environment.environment.background_color = Color(0.35, 0.42, 0.44)
	environment.environment.tonemap_mode = Environment.TONE_MAPPER_AGX
	_world.add_child(environment)
	_camera = Camera3D.new()
	_camera.position.y = 1.0
	_world.add_child(_camera)
	_camera.make_current()
	_visibility = GroupNameplateVisibility.new()
	_camera.compositor = Compositor.new()
	_camera.compositor.compositor_effects = [_visibility]
	_wall = MeshInstance3D.new()
	var box: BoxMesh = BoxMesh.new()
	box.size = Vector3(1.5, 2, 0.2)
	_wall.mesh = box
	_wall.position = Vector3(0, 1, -4)
	var material: StandardMaterial3D = StandardMaterial3D.new()
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.albedo_color = Color(0.4, 0.4, 0.4)
	_wall.material_override = material
	_world.add_child(_wall)
	for name_text: String in ["Alex", "Hidden", "Sam"]:
		var card: GroupRiderNameplate = GroupRiderNameplate.new(UiTheme.ACCENT)
		_world.add_child(card)
		card.show_name(name_text)
		_cards.append(card)
	root.screen_space_aa = Viewport.SCREEN_SPACE_AA_FXAA
	# Exercise MSAA with Lavapipe's supported 4x sample count.
	var cases: Array[Vector3] = [
		Vector3(0, 1, 1), Vector3(2, 1, 1), Vector3(0, 0.5, 1), Vector3(2, 0.5, 1), Vector3(2, 1, 2)
	]
	for mode: Vector3 in cases:
		var msaa: Viewport.MSAA = int(mode.x) as Viewport.MSAA
		root.msaa_3d = msaa
		root.scaling_3d_scale = mode.y
		root.content_scale_factor = mode.z
		root.size = Vector2i(800, 450) if msaa == Viewport.MSAA_DISABLED else Vector2i(1024, 640)
		_wall.show()
		await _settle()
		var image: Image = root.get_texture().get_image()
		var out: String = OS.get_environment("NAMEPLATE_RENDER_DIR")
		if not out.is_empty():
			image.save_png(out.path_join("nameplate-%d-%.1f-%.1f.png" % [msaa, mode.y, mode.z]))
		var hidden_pixels: int = _white_pixels(image, _cards[1])
		_check(_white_pixels(image, _cards[0]) > 20, "near-white text survives AgX and FXAA")
		_check(
			hidden_pixels == 0,
			"scenery hides the card behind it (MSAA %s, white pixels %s)" % [msaa, hidden_pixels]
		)
		_check(_white_pixels(image, _cards[2]) > 20, "scenery leaves the other visible card alone")
		_wall.hide()
		await _settle()
		image = root.get_texture().get_image()
		_check(_white_pixels(image, _cards[1]) > 20, "the name returns when the obstacle leaves")
	_camera.compositor = null
	_visibility = null
	# Existing canvas materials retain their last result until the badges leave the tree.
	await process_frame
	await process_frame
	_check(
		_white_pixels(root.get_texture().get_image(), _cards[0]) > 20,
		"camera detach retains a valid mask"
	)
	_world.free()
	await process_frame
	if not _failed:
		print("NAMEPLATE RENDER SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _settle() -> void:
	for frame: int in range(12):
		var points: PackedFloat32Array = PackedFloat32Array()
		points.resize(GroupNameplateVisibility.SLOTS * 4)
		for i: int in range(_cards.size()):
			var anchor: Vector3 = Vector3(float(i - 1) * 2.0, 1.0, -6.0)
			var center: Vector2 = _camera.unproject_position(anchor)
			_cards[i].position = center - _cards[i].get_combined_minimum_size() * 0.5
			_cards[i].set_occlusion(_visibility.visibility_texture(), i)
			var point: Vector4 = GroupNameplateVisibility.sample(_camera, center, anchor)
			for component: int in range(4):
				points[i * 4 + component] = point[component]
		_visibility.update_samples(points)
		await process_frame


func _white_pixels(image: Image, card: Control) -> int:
	var rect: Rect2i = Rect2i(root.get_stretch_transform() * card.get_global_rect()).intersection(
		Rect2i(Vector2i.ZERO, image.get_size())
	)
	var count: int = 0
	for y: int in range(rect.position.y, rect.end.y):
		for x: int in range(rect.position.x, rect.end.x):
			var color: Color = image.get_pixel(x, y)
			if color.r > 0.88 and color.g > 0.88 and color.b > 0.88:
				count += 1
	return count


func _check(ok: bool, message: String) -> void:
	if not ok:
		push_error(message)
		_failed = true
