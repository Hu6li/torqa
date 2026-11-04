class_name HudLayoutScreenshots
extends RefCounted
## Separate HUD controls without course generation (R207); run via scripts/screenshots.sh.


static func capture(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var dialog: ProfileDialog = start.find_children("*", "ProfileDialog", true, false)[0]
	var ride: PackedStringArray = PackedStringArray(
		["speed", "distance", "grade", "time_remaining"]
	)
	var workout: PackedStringArray = PackedStringArray(
		["power_3s", "cadence", "heart_rate", "elapsed"]
	)
	app.set_hud_layout(ride, false)
	app.set_hud_layout(workout, true)
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		Input.warp_mouse(Vector2(10, 10))
		dialog.edit(app.profile(), ride, workout)
		var tabs: TabContainer = dialog.find_children("*", "TabContainer", true, false)[0]
		tabs.current_tab = dialog._hud_page.get_index()
		for index: int in [0, 1]:
			dialog._hud_choice.select(index)
			dialog._hud_choice.item_selected.emit(index)
			await tree.create_timer(0.4).timeout
			_save(main, out, ("ride-" if index == 0 else "workout-") + locale)
		dialog.hide()
	var profile_tab: ProfileTab = start.get("_profile")
	TranslationServer.set_locale("en")
	var rider_id: String = app.profile()["id"]
	profile_tab._unfolded[rider_id] = true
	profile_tab.refresh()
	var start_tabs: TabContainer = start.get("_tabs")
	start_tabs.current_tab = StartPage.Tab.PROFILE
	await tree.create_timer(0.4).timeout
	_save(main, out, "profile-en")
	var screen: RideScreen = main.get_node("RideScreen")
	var settings: RideSettingsDialog = screen.get("_settings_dialog")
	settings.edit(start.ride_options(), ride, false)
	settings._tabs.current_tab = settings._hud_page.get_index()
	await tree.create_timer(0.4).timeout
	_save(main, out, "ride-settings-en")
	settings.hide()


static func _save(main: Control, out: String, name: String) -> void:
	main.get_viewport().get_texture().get_image().save_png(out.path_join(name + ".png"))
	print("saved HUD layouts ", name)
