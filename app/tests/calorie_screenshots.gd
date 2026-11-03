class_name CalorieScreenshots
extends RefCounted
## Reproducible calorie setup and workout views without course generation (#475).


static func capture(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var dialog: ProfileDialog = start.find_children("*", "ProfileDialog", true, false)[0]
	var profile: Dictionary = app.profile()
	var profile_id: String = profile["id"]
	profile["calorie_age"] = 40
	profile["calorie_sex"] = "male"
	profile["calorie_effort"] = "moderate"
	app.save_profile(profile_id, profile)
	var layout: PackedStringArray = PackedStringArray(
		["power", "calories", "work", "heart_rate", "elapsed"]
	)
	app.set_hud_layout(layout)
	for locale: String in ["en", "de"]:
		Input.warp_mouse(Vector2(10, 10))
		TranslationServer.set_locale(locale)
		dialog.edit(app.profile(), layout)
		var tabs: TabContainer = dialog.find_children("*", "TabContainer", true, false)[0]
		var hud: HudEditor = dialog.get("_hud")
		tabs.current_tab = hud.get_index()
		await tree.create_timer(0.4).timeout
		_save(main, out, "hud-" + locale)
		await _full_hud(main, hud, out, locale)
		hud.edit(layout, false)
		tabs.current_tab = 0
		await tree.process_frame
		var scroll: ScrollContainer = tabs.get_child(0)
		var form: ProfileForm = dialog.get("_form")
		var effort: Control = form.get("_calorie_effort")
		scroll.ensure_control_visible(effort)
		await tree.create_timer(0.4).timeout
		_save(main, out, "profile-" + locale)
		var age: SpinBox = form.get("_calorie_age")
		var age_caption: Control = form.get_child(age.get_index() - 1)
		Input.warp_mouse(Vector2(dialog.position) + age_caption.get_global_rect().get_center())
		await tree.create_timer(1.0).timeout
		_save(main, out, "age-" + locale)
		dialog.hide()
	TranslationServer.set_locale("en")
	app.connect_fake_trainer(200.0, 85.0)
	await tree.create_timer(0.3).timeout
	var workout: Dictionary = {"kind": "power", "power_w": 200.0}
	assert(app.start_workout(workout, false, false), "calorie screenshot workout starts")
	start.ride_started.emit({"workout": workout, "on_course": false, "difficulty": 50.0})
	await tree.create_timer(6.0).timeout
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		await tree.create_timer(0.3).timeout
		_save(main, out, "ride-" + locale)
	app.finish_ride()


static func _full_hud(main: Control, hud: HudEditor, out: String, locale: String) -> void:
	(
		hud
		. edit(
			PackedStringArray(
				[
					"power",
					"speed",
					"distance",
					"elevation_gain",
					"grade",
					"elapsed",
					"time_remaining",
					"remaining",
					"ascent_remaining",
					"heart_rate",
					"heart_rate_percent_max",
					"heart_rate_zone",
					"gear",
				]
			),
			false
		)
	)
	await main.get_tree().create_timer(0.4).timeout
	var preview: ScrollContainer = hud.get_child(0).get_child(1)
	var available: ScrollContainer = hud.get_child(1).get_child(1)
	preview.scroll_vertical = ceili(preview.get_v_scroll_bar().max_value)
	available.scroll_vertical = ceili(available.get_v_scroll_bar().max_value)
	await main.get_tree().create_timer(0.4).timeout
	_save(main, out, "hud-full-" + locale)


static func _save(main: Control, out: String, file: String) -> void:
	main.get_viewport().get_texture().get_image().save_png(out.path_join(file + ".png"))
	print("saved calories ", file)
