class_name GroupReadinessLayout
extends RefCounted
## Readiness and preparation update in place, including translated, narrow lobbies (R234).


static func check(tree: SceneTree, verify: Callable) -> void:
	var main: Control = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	tree.root.add_child(main)
	await tree.process_frame
	var start: StartPage = main.get_node("StartPage")
	var app: OrqwaApp = main.get_node("Orqwa")
	verify.call(app.connect_fake_trainer(180.0, 85.0), "readiness trainer connects")
	var until: int = Time.get_ticks_msec() + 5000
	while not app.trainer_connected() and Time.get_ticks_msec() < until:
		await tree.process_frame
	verify.call(app.trainer_connected(), "readiness has a connected trainer")
	var group: GroupRidesTab = start._group
	start._tabs.current_tab = StartPage.Tab.GROUP_RIDES
	group.set_process(false)
	var course: String = app.courses()[0]["path"]
	var original_size: Vector2i = tree.root.size
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for room: Vector2 in [Vector2(1600, 900), Vector2(1024, 640)]:
			tree.root.size = Vector2i((room * tree.root.content_scale_factor).round())
			for host: bool in [true, false]:
				var reference: Array[Rect2] = []
				for state: String in ["downloading", "preparing", "prompt", "ready", "countdown"]:
					group._show_status(status(app, host, state))
					if host:
						group._picker._select(group._picker._course, course)
						group._picker._select(group._picker._workout, "")
						group._picker._preview()
					await _settle(tree)
					var voice_card: Control = group._voice_controls.get_parent().get_parent()
					var measured: Array[Rect2] = [
						group._roster.get_global_rect(),
						group._next_step.get_global_rect(),
						group._next_step._action_slot.get_global_rect(),
						group._picker._map.get_global_rect(),
						group._picker._elevation.get_global_rect(),
						voice_card.get_global_rect(),
					]
					var context: String = "%s %s host=%s %s" % [locale, room, host, state]
					if reference.is_empty():
						reference = measured
					else:
						for i: int in measured.size():
							verify.call(
								measured[i].is_equal_approx(reference[i]),
								(
									"preparation keeps area %d in place: %s (%s vs %s)"
									% [i, context, measured[i], reference[i]]
								)
							)
					if state == "countdown":
						continue
					verify.call(
						group._ready_pill.size.x == GroupRidesTab.PILL_WIDTH,
						"readiness keeps its width: " + context
					)
					verify.call(
						group._own_state.get_line_count() == 1,
						"the rider's subtitle never wraps: " + context
					)
					if state == "prompt":
						verify.call(
							(
								group._ready_pill.text == TranslationServer.translate("Mark ready")
								and not group._ready_pill.disabled
							),
							"the rider can mark themselves ready: " + context
						)
						verify.call(
							group._own_state.text.begins_with(
								TranslationServer.translate("Please mark ready")
							),
							"the existing subtitle prompts the rider: " + context
						)
					elif state == "ready":
						verify.call(
							group._own_state.text.begins_with(
								TranslationServer.translate("You're ready")
							),
							"the subtitle confirms readiness: " + context
						)
					else:
						verify.call(
							group._ready_pill.disabled,
							"readiness waits for preparation: " + context
						)
	for host: bool in [true, false]:
		var reconnecting: Dictionary = status(app, host, "ready")
		reconnecting.merge({"phase": "riding", "reconnect_s": 35.0}, true)
		group._show_status(reconnecting)
		await _settle(tree)
		var step: GroupNextStep = group._next_step
		verify.call(
			step._line.is_visible_in_tree(), "reconnecting keeps its remaining time visible"
		)
		verify.call(
			not step._progress_slot.get_global_rect().intersects(step._reconnect.get_global_rect()),
			"the reconnect action does not cover its explanation"
		)
	main.free()
	tree.root.size = original_size
	TranslationServer.set_locale("en")
	await tree.process_frame


static func capture(
	tree: SceneTree, start: StartPage, group: GroupRidesTab, app: OrqwaApp, out: String
) -> void:
	var scroll: ScrollContainer = start._group_page
	app.connect_fake_trainer(180.0, 85.0)
	var until: int = Time.get_ticks_msec() + 5000
	while not app.trainer_connected() and Time.get_ticks_msec() < until:
		await tree.process_frame
	var invite: String = app.group_status().get("invite", "")
	var course: String = app.courses()[0]["path"]
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for host: bool in [false, true]:
			for state: String in ["downloading", "preparing", "prompt", "ready", "countdown"]:
				var presentation: Dictionary = status(app, host, state)
				presentation["invite"] = invite
				group._show_status(presentation)
				if host:
					group._picker._select(group._picker._course, course)
					group._picker._select(group._picker._workout, "")
					group._picker._preview()
				scroll.scroll_vertical = 0
				await _settle(tree)
				await tree.create_timer(0.15).timeout
				var role: String = "organizer" if host else "friend"
				var filename: String = "group-readiness-%s-%s-%s.png" % [role, state, locale]
				tree.root.get_texture().get_image().save_png(out.path_join(filename))
				print("saved " + filename)


static func status(app: OrqwaApp, host: bool, state: String) -> Dictionary:
	var prepared: bool = state in ["prompt", "ready", "countdown"]
	var ready: bool = state in ["ready", "countdown"]
	return {
		"phase": "ready" if state in ["prompt", "ready"] else state,
		"host": host,
		"prepared": prepared,
		"ready": ready,
		"can_start": ready and host,
		"countdown_s": 5,
		"name": app.courses()[0]["name"],
		"course_path": app.courses()[0]["path"],
		"received": 500000,
		"total": 2000000,
		"roster":
		[
			{
				"id": 0,
				"name": "Alex",
				"ready": ready if host else true,
				"me": host,
				"organizer": true,
				"stage": "" if prepared or not host else state,
				"percent": 65
			},
			{"id": 1, "name": "Mia", "ready": true, "me": false},
			{
				"id": 2,
				"name": "Rider",
				"ready": ready if not host else true,
				"me": not host,
				"stage": "" if prepared or host else state,
				"percent": 65
			},
		],
	}


static func _settle(tree: SceneTree) -> void:
	for frame: int in range(8):
		await tree.process_frame
