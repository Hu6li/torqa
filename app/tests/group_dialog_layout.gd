class_name GroupDialogLayout
extends RefCounted
## The live group window keeps its geometry while asynchronous status changes (R212).


static func check(tree: SceneTree, app: OrqwaApp, verify: Callable, tag: String) -> void:
	var ride: RideScreen = (
		(load("res://scenes/ride_screen.tscn") as PackedScene).instantiate() as RideScreen
	)
	tree.root.add_child(ride)
	ride.bind(app, null)
	ride.set_process(false)
	var group: GroupRidesTab = ride._group_controls
	group.set_process(false)
	var panel: AcceptDialog = ride._group_window
	var scroll: ScrollContainer = group.get_parent() as ScrollContainer
	for room: Vector2 in [Vector2(1600, 900), Vector2(1280, 720), UiScale.MIN_WINDOW_SIZE]:
		tree.root.size = Vector2i(room * tree.root.content_scale_factor)
		ride._bar.group_requested.emit()
		await _settle(tree)
		var status: Dictionary = _status()
		group._show_status(status)
		await _settle(tree)
		var opened: Rect2i = Rect2i(panel.position, panel.size)
		if tag == "en-1" and room == Vector2(1600, 900):
			await _capture(tree, "group-status-opened")
		# Discovery can finish after opening, without any interaction with this window.
		status["reachability"] = "strict"
		group._show_status(status)
		await _settle(tree)
		var context: String = "%s %s" % [tag, room]
		verify.call(
			Rect2i(panel.position, panel.size) == opened,
			"a delayed connection warning keeps the group window's size and position: " + context
		)
		verify.call(scroll.get_v_scroll_bar().visible, "long group content scrolls: " + context)
		if tag == "en-1" and room == Vector2(1600, 900):
			await _capture(tree, "group-status-warning")
		var roster: Array = status["roster"]
		for id: int in range(3, 8):
			roster.append({"id": id, "name": "Rider %d" % id, "me": false, "ready": true})
		for tick: int in range(3):
			var friend: Dictionary = roster[1]
			friend["gap_m"] = 14.0 + tick * 0.1
			friend["ping_ms"] = 25 if tick == 0 else 60000
			status["voice"] = tick != 1
			group._show_status(status)
			await _settle(tree)
			verify.call(
				Rect2i(panel.position, panel.size) == opened,
				"roster, telemetry and voice updates keep the group window steady: " + context
			)
		await _settle(tree)
		verify.call(Rect2i(panel.position, panel.size) == opened, "an idle group window stays put")
		scroll.ensure_control_visible(group._fallback_change)
		await _settle(tree)
		verify.call(scroll.scroll_vertical > 0, "the last organizer control is reachable")
		verify.call(
			scroll.get_global_rect().encloses(group._fallback_change.get_global_rect()),
			"scrolling reveals the last organizer control: " + context
		)
		var frame: Rect2 = Rect2(
			Vector2(panel.position) - Vector2(0, panel.get_theme_constant("title_height")),
			Vector2(panel.size) + Vector2(0, panel.get_theme_constant("title_height"))
		)
		verify.call(
			tree.root.get_visible_rect().encloses(frame), "the group frame stays in the app"
		)
		verify.call(
			Rect2(Vector2.ZERO, Vector2(panel.size)).encloses(
				panel.get_ok_button().get_global_rect()
			),
			"Done stays visible below the scrolling group content"
		)
		panel.hide()
	# App resizing still constrains the panel and restores its preferred size when room returns.
	tree.root.size = Vector2i(Vector2(1600, 900) * tree.root.content_scale_factor)
	ride._bar.group_requested.emit()
	await _settle(tree)
	var preferred: Vector2i = panel.size
	tree.root.size = Vector2i(UiScale.MIN_WINDOW_SIZE * tree.root.content_scale_factor)
	await _settle(tree)
	verify.call(panel.size.y < preferred.y, "a smaller app constrains the group window")
	tree.root.size = Vector2i(Vector2(1600, 900) * tree.root.content_scale_factor)
	await _settle(tree)
	verify.call(panel.size == preferred, "a larger app restores the group window's preferred size")
	ride.free()
	await tree.process_frame


static func _status() -> Dictionary:
	return {
		"phase": "riding",
		"host": true,
		"invite": "orqwa:invitation",
		"voice": true,
		"workout_shared": true,
		"can_choose_fallback": true,
		"roster":
		[
			{"id": 0, "name": "Rider", "me": true, "ready": true, "organizer": true},
			{
				"id": 1,
				"name": "Alex",
				"me": false,
				"ready": true,
				"present": true,
				"gap_m": 14.0,
				"power_w": 230.0,
				"watts_per_kg": 3.2,
				"ping_ms": 45,
			},
			{"id": 2, "name": "Mia", "me": false, "ready": true, "fallback": true},
		],
	}


static func _settle(tree: SceneTree) -> void:
	for frame: int in range(8):
		await tree.process_frame


static func _capture(tree: SceneTree, name: String) -> void:
	var shots: String = OS.get_environment("DIALOG_SCREENSHOT_DIR")
	if shots.is_empty() or DisplayServer.get_name() == "headless":
		return
	await tree.process_frame
	await RenderingServer.frame_post_draw
	tree.root.get_texture().get_image().save_png(shots.path_join(name + ".png"))
