extends SceneTree
## #261: errors translate before formatting; diagnostics survive notification lifetimes.

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	TranslationServer.set_locale("en")
	var course: CourseDetail = CourseDetail.new()
	var settings: RideSettingsDialog = RideSettingsDialog.new()
	# Built before the rider's profile picks the language, as the start page builds it.
	var zones: ZonesEditor = ZonesEditor.new()
	root.add_child(course)
	root.add_child(settings)
	root.add_child(zones)
	await process_frame
	TranslationServer.set_locale("de")
	await process_frame
	var ride_button: Button = course.get("_ride_button")
	var tabs: TabContainer = settings.get("_tabs")
	var ride_options: RideOptions = settings.get("_options")
	_check(ride_button.text == "Fahren", "ride action refreshes after language switch")
	_check(tabs.get_tab_title(ride_options.get_index()) == "Fahrt", "ride tab refreshes")
	_check(TranslationServer.translate("Ride", "ride_action") == "Fahren", "ride action")
	_check(TranslationServer.translate("Ride", "ride_tab") == "Fahrt", "ride tab")
	var heading: Label = zones.get_child(0)
	_check(
		heading.atr(heading.text) == "Leistungszonen: jeweilige Obergrenze in % der FTP",
		"captions built in English follow the switch to German: %s" % heading.atr(heading.text)
	)
	var zone: Label = (zones.get_child(1) as GridContainer).get_child(0)
	_check(zone.text == "Z1 Erholung", "zone names follow the language: %s" % zone.text)
	var path: String = "C:/Räder/100%/%s.fit"
	var failure: Dictionary = {
		"message": "Could not open %s.",
		"args": PackedStringArray([path]),
		"details": "Permission denied: " + path,
	}
	_check(
		FailureDetails.summary(failure) == path + " konnte nicht geöffnet werden.",
		"filenames stay literal after German translation"
	)
	var history: FailureDetails = FailureDetails.new()
	root.add_child(history)
	history.report(failure)
	await process_frame
	_check(history.visible, "details remain available independently of a toast")
	var panel: PanelContainer = history.get("_panel")
	var toggle: Button = history.get("_toggle")
	_check(not panel.visible and toggle.text == "Technische Details", "details start collapsed")
	toggle.button_pressed = true
	_check(panel.visible, "details expand")
	var rows: VBoxContainer = history.get("_rows")
	var original_details: String = failure["details"]
	_check((rows.get_child(1) as Label).text == original_details, "diagnostic stays exact")
	history.clear()
	_check(not history.visible and not panel.visible, "dismissed details hide until the next")
	history.report(failure)
	_check(history.visible and rows.get_child_count() == 2, "dismissing forgets earlier failures")
	var message: Label = history.get("_message")
	var sign: Control = history.get("_sign")
	var earlier: Label = history.get("_earlier")
	var card: PanelContainer = history.get("_card")
	_check(not message.visible and not sign.visible, "beside a status, only the buttons show")
	_check(card.mouse_filter == Control.MOUSE_FILTER_IGNORE, "and the room beside them stays free")
	# Anchored as the main scene anchors it, on its own (#436): it grows upward to fit, the
	# panel opaque, the list above the card rather than behind it.
	history.standalone = true
	_check(sign.visible and message.visible, "on its own, a warning sign and the message")
	_check(message.text == FailureDetails.summary(failure), "the card says what failed")
	_check(not earlier.visible, "with nothing earlier to count")
	var card_box: StyleBoxFlat = card.get_theme_stylebox("panel")
	_check(
		(
			card_box.border_color == UiTheme.DANGER
			and card_box.border_width_left == FailureDetails.STRIPE
			and is_equal_approx(card_box.bg_color.a, 1.0)
		),
		"an opaque card with a red edge"
	)
	history.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT)
	history.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	history.grow_vertical = Control.GROW_DIRECTION_BEGIN
	history.offset_left = -584
	history.offset_right = -24
	history.offset_top = -24
	history.offset_bottom = -24
	history.report({"message": "No trainer connected.", "details": "no trainer connected"})
	toggle.button_pressed = true
	for _frame: int in 3:
		await process_frame
	var repeated: Label = rows.get_child(1)
	_check(not repeated.visible, "a diagnostic only repeating the message is left out")
	_check(message.text == "No trainer connected.", "the card shows the newest failure")
	_check(earlier.text == "+1 frühere", "and counts the ones before it: %s" % earlier.text)
	_check(
		panel.get_global_rect().end.y <= card.get_global_rect().position.y + 0.5,
		"the panel ends above the card: %s, %s" % [panel.get_global_rect(), card.get_global_rect()]
	)
	_check(
		card.get_global_rect().has_point(toggle.get_global_rect().get_center()), "buttons inside"
	)
	var box: StyleBoxFlat = panel.get_theme_stylebox("panel")
	_check(is_equal_approx(box.bg_color.a, 1.0), "the panel is opaque")
	var scroll: ScrollContainer = history.get("_scroll")
	_check(
		(
			scroll.size.y >= rows.get_combined_minimum_size().y - 0.5
			or scroll.size.y >= FailureDetails.MAX_HEIGHT - 0.5
		),
		"the list shows all it holds, or scrolls past the maximum"
	)
	_check(
		scroll.size.y <= rows.get_combined_minimum_size().y + 0.5,
		"and is no taller than it: %s, %s" % [scroll.size.y, rows.get_combined_minimum_size().y]
	)
	TranslationServer.set_locale("en")
	await process_frame
	_check(earlier.text == "+1 earlier", "the count follows the language: %s" % earlier.text)
	_check(message.text == "No trainer connected.", "and so does the message")
	history.clear()
	TranslationServer.set_locale("en")
	await process_frame
	_check(ride_button.text == "Ride", "ride action switches back to English")
	_check(tabs.get_tab_title(ride_options.get_index()) == "Ride", "ride tab switches back")
	_check(FailureDetails.summary(failure) == "Could not open " + path + ".", "English summary")
	var orqwa: OrqwaApp = OrqwaApp.new()
	root.add_child(orqwa)
	await process_frame
	var summaries: Array[String] = []
	orqwa.failed.connect(func(message: String) -> void: summaries.append(message))
	TranslationServer.set_locale("de")
	orqwa.ride_detail("/missing-261.fit", 10)
	var last_summary: String = summaries.back()
	_check(last_summary == "Die Aktivität konnte nicht geladen werden.", "core error translates")
	var payload: Dictionary = orqwa.last_failure()
	var message_key: String = payload["message"]
	var diagnostic: String = payload["details"]
	_check(message_key == "Could not load the activity.", "binding preserves the key")
	_check("/missing-261.fit" in diagnostic, "binding preserves the original cause")
	orqwa.free()
	history.free()
	zones.free()
	settings.free()
	course.free()
	if not _failed:
		print("ERROR SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error(what)
