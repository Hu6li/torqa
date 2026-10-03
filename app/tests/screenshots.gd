extends SceneTree
## Renders the ride screen in each camera mode to PNG files, for checking visuals without a
## GPU (software Vulkan). Run via scripts/screenshots.sh.

const DEFAULT_ROUTE: String = "res://../core/fixtures/gurtenstrasse.gpx"
const TIMEOUT_S: float = 180.0

var _main: Control


func _initialize() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(_main)
	_run.call_deferred()


func _run() -> void:
	var out_dir: String = OS.get_environment("SCREENSHOT_DIR")
	var torqa: TorqaApp = _main.get_node("Torqa")
	var world: RideWorld = _main.get_node("World")
	var setup: SetupScreen = _main.get_node("SetupScreen")

	var route: String = OS.get_environment("SCREENSHOT_ROUTE")
	if route.is_empty():
		route = ProjectSettings.globalize_path(DEFAULT_ROUTE)
	var ride_s: float = OS.get_environment("SCREENSHOT_RIDE_S").to_float()
	# Another interface language, e.g. SCREENSHOT_LOCALE=de.
	var locale: String = OS.get_environment("SCREENSHOT_LOCALE")
	if not locale.is_empty():
		TranslationServer.set_locale(locale)
	torqa.load_route(route, false)
	# The setup screen while loading, for checking the progress display.
	await create_timer(1.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("loading.png"))
	# The rider settings with the HUD editor.
	for child: Node in setup.get_children():
		if child is ProfileDialog:
			var dialog: ProfileDialog = child
			dialog.edit(torqa.profile(), torqa.hud_layout())
			var tabs: TabContainer = dialog.find_children("*", "TabContainer", true, false)[0]
			tabs.current_tab = 1
			await create_timer(0.5).timeout
			root.get_texture().get_image().save_png(out_dir.path_join("rider-settings.png"))
			dialog.hide()
	await _wait_for(torqa.world_ready)
	# Let the world stream its chunks in.
	for i: int in range(240):
		await process_frame
	# A custom HUD, e.g. SCREENSHOT_HUD=power_3s,speed,normalized_power,upcoming_grade
	var hud: String = OS.get_environment("SCREENSHOT_HUD")
	if not hud.is_empty():
		torqa.set_hud_layout(PackedStringArray(hud.split(",")))
	_check(torqa.connect_fake_trainer(250.0, 90.0), "fake trainer")
	# A pacer to race, e.g. SCREENSHOT_GHOST=300 (watts).
	var ghost: Dictionary = {"kind": "none"}
	var ghost_watts: String = OS.get_environment("SCREENSHOT_GHOST")
	if not ghost_watts.is_empty():
		ghost = {"kind": "power", "watts": ghost_watts.to_float()}
	_check(torqa.start_ride(50.0, false, ghost), "ride started")
	setup.ride_started.emit()
	var time: String = OS.get_environment("SCREENSHOT_TIME")
	var weather: String = OS.get_environment("SCREENSHOT_WEATHER")
	if not time.is_empty() or not weather.is_empty():
		world.apply_conditions(
			time if not time.is_empty() else "Midday",
			weather if not weather.is_empty() else "Clear"
		)
	await create_timer(maxf(ride_s, 4.0)).timeout

	for mode: int in range(3):
		await create_timer(1.5).timeout
		var image: Image = root.get_texture().get_image()
		var path: String = out_dir.path_join("ride-%d.png" % mode)
		image.save_png(path)
		print("saved ", path)
		world.cycle_camera()

	# A close side view of the rider, for checking the avatar.
	var rider: Node3D = world.get_node("Rider")
	var side: Camera3D = Camera3D.new()
	world.add_child(side)
	var target: Vector3 = rider.global_position + Vector3.UP * 0.8
	var right: Vector3 = rider.global_transform.basis.x
	side.global_position = target + right * 3.0 + Vector3.UP * 0.3
	side.look_at(target, Vector3.UP)
	side.current = true
	for frame: int in range(3):
		await create_timer(0.25).timeout
		var image: Image = root.get_texture().get_image()
		image.save_png(out_dir.path_join("side-%d.png" % frame))
		print("saved side-%d" % frame)

	# The HUD editor.
	side.current = false
	var ride_screen: RideScreen = _main.get_node("RideScreen")
	for child: Node in ride_screen.get_children():
		if child is RideSettingsDialog:
			var dialog: RideSettingsDialog = child
			dialog.edit(setup.ride_options(), torqa.hud_layout(), false)
			await create_timer(0.5).timeout
			root.get_texture().get_image().save_png(out_dir.path_join("ride-settings.png"))
			print("saved ride settings")
			var tabs: TabContainer = dialog.find_children("*", "TabContainer", true, false)[0]
			tabs.current_tab = 1
			await process_frame
			# Hover a figure over the HUD's grid, so the drop indicator shows.
			var preview: HudPanel = dialog.find_children("*", "HudPanel", true, false)[0]
			var grid: Node = preview.get_child(preview.get_child_count() - 1)
			grid.get_child(2).call(
				"_can_drop_data", Vector2(80, 10), {HudPanel.DRAG_KEY: "power_3s"}
			)
			await create_timer(0.5).timeout
			root.get_texture().get_image().save_png(out_dir.path_join("hud-dialog.png"))
			print("saved hud dialog")
			dialog.hide()

	# The ride's analysis in the history.
	torqa.finish_ride()
	setup.history_requested.emit()
	await create_timer(1.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("history.png"))
	print("saved history")
	quit(0)


func _wait_for(sig: Signal) -> void:
	var received: Array[bool] = [false]
	sig.connect(func(_arg: Variant) -> void: received[0] = true, CONNECT_ONE_SHOT)
	var waited: float = 0.0
	while not received[0] and waited < TIMEOUT_S:
		await process_frame
		waited += root.get_process_delta_time()
	_check(received[0], "signal %s" % sig.get_name())


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("SCREENSHOTS FAILED: " + what)
		quit(1)
