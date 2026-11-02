class_name GroupRiderScreenshots
extends RefCounted
## Bounded presentation fixtures for reviewing avatars, gaps and map/profile markers.
## Motion/membership and telemetry isolation are tested with real loopback peers in Rust.


static func capture(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var root: Window = tree.root
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var world: RideWorld = main.get_node("World")
	var ride: RideScreen = main.get_node("RideScreen")
	TranslationServer.set_locale("en")
	app.set_graphics_quality("medium")
	var built: Array[bool] = [false]
	app.world_ready.connect(func(_info: Variant = null) -> void: built[0] = true, CONNECT_ONE_SHOT)
	app.load_route(ProjectSettings.globalize_path("res://../core/fixtures/gurtenstrasse.gpx"), true)
	var until: int = Time.get_ticks_msec() + 180000
	while not built[0] and Time.get_ticks_msec() < until:
		await tree.process_frame
	if not built[0]:
		push_error("Group screenshot course did not prepare")
		return
	while not _chunks_ready(world):
		await tree.process_frame
	app.connect_fake_trainer(300.0, 85.0)
	app.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	until = Time.get_ticks_msec() + 10000
	while app.ride_state().get("power") == null and Time.get_ticks_msec() < until:
		await tree.process_frame
	app.set_paused(true)
	var names: Array[String] = ["Alex", "Sam", "Mia", "Noah", "Luca", "Robin", "Charlie"]
	var gaps: Array[float] = [5.0, 10.0, 18.0, 30.0, 45.0, -12.0, -25.0]
	# Each rider's own effort and look (#422, #423).
	var watts: Array[float] = [265.0, 240.0, 210.0, 290.0, 225.0, 185.0, 250.0]
	var weights: Array[float] = [72.0, 80.0, 58.0, 85.0, 64.0, 70.0, 77.0]
	var looks: Array[String] = ["female", "male", "female", "male", "male", "female", "male"]
	var peers: Array[Dictionary] = []
	for index: int in range(names.size()):
		app.jump_to_distance(700.0 + gaps[index])
		var peer: Dictionary = app.ride_state().duplicate(true)
		(
			peer
			. merge(
				{
					"id": index + 1,
					"generation": "1",
					"name": names[index],
					"gap_m": gaps[index],
					"speed_kmh": 18.0,
					"power_w": watts[index],
					"watts_per_kg": watts[index] / weights[index],
					"avatar": looks[index],
					"cadence": 80.0,
					"effort": 4.0,
				},
				true
			)
		)
		peers.append(peer)
	app.jump_to_distance(700.0)
	world.set("_placed", false)
	world.set_camera(0)
	# Rust supplies the same pose fields during a live ride. Hold this fixture during capture.
	var riders: GroupRidersWorld
	for child: Node in world.get_children():
		if child is GroupRidersWorld:
			riders = child as GroupRidersWorld
			riders.set_process(false)
	ride._process(1.0 / 60.0)
	ride.set_process(false)
	ride._toast.hide()
	ride._bar.show_group(true)
	ride._group_riders.show_riders(peers, false)
	ride._minimap.set_group_riders(peers)
	ride._profile.set_group_riders(peers)
	root.msaa_3d = Viewport.MSAA_DISABLED
	root.screen_space_aa = Viewport.SCREEN_SPACE_AA_FXAA
	var environment: Environment = world.get("_environment")
	environment.glow_enabled = false
	for tick: int in range(120):
		riders.show_riders(peers, 1.0 / 60.0)
	for frame: int in range(15):
		await tree.process_frame
	root.get_texture().get_image().save_png(out.path_join("group-riders-chase.png"))
	root.get_texture().get_image().save_jpg(out.path_join("group-riders-chase.jpg"), 0.82)
	print("saved group-riders-chase")
	# The request to organize the ride, over the ride (#424).
	var roster: Array = [{"name": "Alex", "organizer": true}, {"name": "Sam", "me": true}]
	ride._show_group({"phase": "riding", "roster": roster, "handover_offer": true})
	for frame: int in range(5):
		await tree.process_frame
	root.get_texture().get_image().save_png(out.path_join("group-offer.png"))
	root.get_texture().get_image().save_jpg(out.path_join("group-offer.jpg"), 0.82)
	print("saved group-offer")
	ride._show_group({"phase": "riding", "roster": roster})
	if root.size.y < 800:
		var scroll: ScrollContainer = ride.get_node("RightColumnScroll")
		scroll.scroll_vertical = 100000
		await tree.process_frame
		await tree.process_frame
		root.get_texture().get_image().save_png(out.path_join("group-riders-bottom.png"))
		root.get_texture().get_image().save_jpg(out.path_join("group-riders-bottom.jpg"), 0.82)
		print("saved group-riders-bottom")
	ride.hide()
	world.set_camera(2)
	for frame: int in range(15):
		await tree.process_frame
	root.get_texture().get_image().save_png(out.path_join("group-riders-drone.png"))
	root.get_texture().get_image().save_jpg(out.path_join("group-riders-drone.jpg"), 0.82)
	print("saved group-riders-drone")
	var summary: RideSummaryDialog = main.get("_quick_summary")
	(
		summary
		. _show_ride(
			{
				"name": "Morning group ride",
				"route": "Gurtenstrasse",
				"start_unix_s": 1793203200,
				"assisted": true,
				"elapsed_s": 1800.0,
				"distance_m": 12000.0,
				"elevation_gain_m": 350.0,
				"avg_power": 200.0,
				"avg_heart_rate": 145.0,
				"avg_cadence": 85.0,
				"power_records": PackedFloat64Array([300.0]),
				"power_curve": PackedVector2Array([Vector2(300.0, 240.0)]),
			}
		)
	)
	await tree.process_frame
	await tree.process_frame
	root.get_texture().get_image().save_png(out.path_join("group-assisted-summary.png"))
	print("saved group-assisted-summary")


static func _chunks_ready(world: RideWorld) -> bool:
	var next: int = world.get("_next_chunk")
	var count: int = world.get("_chunk_count")
	return next >= count
