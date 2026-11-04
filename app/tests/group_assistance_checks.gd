class_name GroupAssistanceChecks
extends RefCounted
## Assistance must report its direction independently and disappear from excluded views (R209).


static func check(tree: SceneTree, verify: Callable) -> void:
	var scene: PackedScene = load("res://scenes/main.tscn")
	var main: Control = scene.instantiate()
	tree.root.add_child(main)
	var ride: RideScreen = main.get_node("RideScreen")
	var app: OrqwaApp = main.get_node("Orqwa")
	ride._hud.show_layout(app.hud_layout(false))
	ride.show()
	ride.set_process(false)
	var state: Dictionary = {
		"x": 0.0, "paused": false, "assistance": {"drafting": 0.6, "together": 0.4}
	}
	var indicator: GroupAssistance = ride._assistance
	var original_size: Vector2i = tree.root.size
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for window_size: Vector2i in [Vector2i(1600, 900), Vector2i(1024, 640)]:
			tree.root.size = window_size
			for strength: float in [0.4, -0.7]:
				state["assistance"]["together"] = strength
				ride._show_assistance(state)
				await tree.process_frame
				await tree.process_frame
				verify.call(indicator.visible, "active effects show automatically")
				verify.call(
					indicator._draft.visible and indicator._band.visible, "both effects show"
				)
				var key: String = "Support" if strength > 0.0 else "Braking"
				verify.call(
					indicator._band_label.text == TranslationServer.translate(key) + ":",
					"label follows applied direction"
				)
				var bounds: Rect2 = indicator.get_global_rect()
				verify.call(
					(
						is_equal_approx(
							indicator._draft_bar.global_position.x,
							indicator._band_bar.global_position.x
						)
						and indicator._draft.global_position.y < indicator._band.global_position.y
					),
					"active effects form label/bar columns in stacked rows"
				)
				verify.call(
					ride.get_global_rect().encloses(bounds), "indicators fit the riding view"
				)
				var hud_bounds: Rect2 = (ride._hud.get_parent() as Control).get_global_rect()
				verify.call(
					(
						is_equal_approx(bounds.position.x, hud_bounds.position.x)
						and is_equal_approx(bounds.size.x, hud_bounds.size.x)
						and bounds.position.y > hud_bounds.end.y
					),
					"assistance sits below the HUD with the same width"
				)
				verify.call(
					not bounds.intersects(ride._bar.get_global_rect()),
					"indicators clear ride controls"
				)
				var side: Control = ride.get_node("RightColumnScroll")
				verify.call(
					not bounds.intersects(side.get_global_rect()),
					"indicators clear the map and group list"
				)
	ride._hud.show_layout(PackedStringArray(["power", "speed", "grade"]))
	state["assistance"] = {"drafting": 0.6, "together": 0.0}
	ride._show_assistance(state)
	await tree.process_frame
	await tree.process_frame
	var hud: Rect2 = (ride._hud.get_parent() as Control).get_global_rect()
	verify.call(
		(
			is_equal_approx(indicator.size.x, hud.size.x)
			and indicator.get_global_rect().position.y > hud.end.y
		),
		"one active row follows the edited HUD and keeps its width"
	)
	verify.call(
		indicator._draft.visible and not indicator._band.visible, "only drafting occupies space"
	)
	state["assistance"] = {"drafting": 0.0, "together": -0.5}
	ride._show_assistance(state)
	verify.call(
		not indicator._draft.visible and indicator._band.visible, "only braking occupies space"
	)
	state["assistance"] = {}
	ride._show_assistance(state)
	verify.call(not indicator.visible, "neutral effects leave no panel")
	state["assistance"] = {"drafting": 0.8, "together": 0.5}
	state["paused"] = true
	ride._show_assistance(state)
	verify.call(not indicator.visible, "paused rides hide assistance")
	state["paused"] = false
	ride._finished = true
	ride._show_assistance(state)
	verify.call(not indicator.visible, "finished rides hide assistance")
	ride._finished = false
	state.erase("x")
	ride._show_assistance(state)
	verify.call(not indicator.visible, "course-free workouts hide assistance")
	state["x"] = 0.0
	ride._overlay_hud.show()
	ride._show_assistance(state)
	verify.call(not indicator.visible, "overlay hides assistance even with active effects")
	ride._overlay_hud.hide()
	ride._show_assistance(state)
	verify.call(indicator.visible, "main view restores current active effects")
	ride._hud.show_layout(app.hud_layout(false))
	ride._offer.show_offer("Alex", "")
	await tree.process_frame
	await tree.process_frame
	ride._show_assistance(state)
	await tree.process_frame
	verify.call(
		not indicator.get_global_rect().intersects(ride._offer.get_global_rect()),
		"organizer offers do not cover assistance"
	)
	main.free()
	tree.root.size = original_size
	TranslationServer.set_locale("en")
	await tree.process_frame
