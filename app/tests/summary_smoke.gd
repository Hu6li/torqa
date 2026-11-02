extends SceneTree
## Finished rides open their quick summary without another click (R113).

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	root.size = Vector2i(1024, 640)
	var main: Control = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var orqwa: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var ride: RideScreen = main.get_node("RideScreen")
	var summary: RideSummaryDialog = main.get("_quick_summary")
	var detail: HistoryScreen = main.get_node("HistoryScreen")
	var path: String = OS.get_user_data_dir().path_join("summary-smoke.gpx")
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_string(
		(
			"<gpx><trk><name>Finish test</name><trkseg>"
			+ '<trkpt lat="46" lon="7"><ele>500</ele></trkpt>'
			+ '<trkpt lat="46.00005" lon="7"><ele>500</ele></trkpt>'
			+ "</trkseg></trk></gpx>"
		)
	)
	file.close()
	orqwa.load_route(path, true)
	await orqwa.route_loaded
	_check(orqwa.connect_fake_trainer(250.0, 90.0), "fake trainer connected")
	_check(orqwa.start_ride(50.0, false, {"kind": "none"}), "short route started")
	start.ride_started.emit(start.ride_options())
	var until: int = Time.get_ticks_msec() + 15000
	while not summary.visible and Time.get_ticks_msec() < until:
		await process_frame
	_check(summary.visible, "crossing the finish opens the summary automatically")
	_check(ride.visible and not detail.visible, "the dialog stays over the ride")
	_check(not orqwa.ambient_sound_active(), "finishing stops the core audio stream")
	await process_frame
	var sound: RideSound = ride.get("_sound")
	_check(not sound.playing, "the finished ride's playback stops behind the quick summary")
	var bar: RideBar = ride.get("_bar")
	var back: Button = bar.get("_back")
	_check(not back.visible, "a saved ride needs no extra finish button")
	var stats: ReflowGrid = summary.get("_stats")
	var captions: Array[String] = []
	for cell: Node in stats.get_children():
		captions.append((cell.get_child(0) as Label).text)
	_check(
		captions.has("Time") and captions.has("Distance") and captions.has("Avg power"),
		"key figures shown: %s" % [captions]
	)
	var saved: String = summary.get("_path")
	var recap: RideRecap = summary.get("_recap")
	# Earlier runs may have ridden the course already: then it tells of the week, if anything.
	_check(
		not recap.text.contains("{") and not recap.text.contains("enjoy"),
		"at most one line, looking back on the ride (R124): %s" % recap.text
	)
	# A record uses its own duration's watts; other efforts in the curve are not PRs.
	var record_ride: Dictionary = {}
	for entry: Dictionary in orqwa.history():
		if entry["path"] == saved:
			record_ride = entry.duplicate(true)
	record_ride["power_records"] = PackedFloat64Array([5.0, 60.0])
	record_ride["power_curve"] = PackedVector2Array(
		[Vector2(60, 321.4), Vector2(5, 789.6), Vector2(300, 123)]
	)
	summary.call("_show_ride", record_ride)
	var records: PowerRecordBadges = summary.get("_power_records")
	_check(records.visible and records.get_child_count() == 2, "only power PRs get badges")
	record_ride["assisted"] = true
	summary.call("_show_ride", record_ride)
	_check(
		summary._assisted.visible and records.visible,
		"assisted rides are labelled and keep real power records"
	)
	record_ride["assisted"] = false
	var five_seconds: Label = records.get_child(0)
	var one_minute: Label = records.get_child(1)
	_check(
		five_seconds.text == "★ Best 5 s: 790 W" and one_minute.text == "★ Best 1 min: 321 W",
		"power PR durations and watts: %s, %s" % [five_seconds.text, one_minute.text]
	)
	record_ride["power_records"] = PackedFloat64Array()
	summary.call("_show_ride", record_ride)
	_check(not records.visible and records.get_child_count() == 0, "no stale power PR badges")
	# Missing readings are omitted, and distance and climbing follow the rider's units.
	var rider: Dictionary = orqwa.profile()
	var rider_id: String = rider["id"]
	var imperial: Dictionary = rider.duplicate(true)
	imperial["units"] = "imperial"
	orqwa.save_profile(rider_id, imperial)
	var missing_sensors: Dictionary = {
		"elapsed_s": 3661.0,
		"distance_m": 1609.344,
		"elevation_gain_m": 304.8,
		"avg_power": null,
		"avg_heart_rate": null,
		"avg_cadence": null,
	}
	summary.call("_fill_stats", missing_sensors)
	_check(stats.get_child_count() == 3, "missing sensor values have no figure")
	var distance: Label = stats.get_child(1).get_child(1)
	var climbing: Label = stats.get_child(2).get_child(1)
	_check(distance.text == "1.0 mi" and climbing.text == "1000 ft", "imperial figures")
	orqwa.save_profile(rider_id, rider)
	var title: EditableTitle = summary.get("_title")
	title.text = "Finish summary test"
	summary.get_ok_button().pressed.emit()
	await process_frame
	_check(start.visible and not summary.visible, "Done returns home")
	var named: bool = false
	for entry: Dictionary in orqwa.history():
		if entry["path"] == saved:
			named = entry["name"] == "Finish summary test"
	_check(named, "the quick summary keeps the ride's name")
	orqwa.delete_ride(saved)

	# Manual finishes share the dialog, with the full summary still available.
	_check(orqwa.start_workout({"kind": "power", "power_w": 180.0}, false, false), "workout")
	start.ride_started.emit({"workout": {"kind": "power", "power_w": 180.0}})
	await create_timer(1.2).timeout
	ride.call("_on_finish_requested")
	await process_frame
	await process_frame
	_check(summary.visible, "Finish & save opens the same dialog")
	saved = summary.get("_path")
	summary.custom_action.emit(&"details")
	_check(not summary.visible and detail.visible, "View details opens charts and uploads")
	detail.go_back()
	_check(start.visible, "details return home")
	main.call("_show_quick_summary", saved)
	summary.canceled.emit()
	_check(start.visible and FileAccess.file_exists(saved), "closing keeps the saved ride")
	main.call("_show_quick_summary", saved)
	summary.custom_action.emit(&"discard")
	var confirm: ConfirmationDialog = summary.get("_confirm_discard")
	_check(confirm.visible and FileAccess.file_exists(saved), "discard asks before deleting")
	confirm.hide()
	confirm.confirmed.emit()
	_check(not FileAccess.file_exists(saved) and start.visible, "confirmed discard returns home")

	# Aborting has no saved ride and no summary.
	_check(orqwa.start_workout({"kind": "power", "power_w": 180.0}, false, false), "abort")
	start.ride_started.emit({"workout": {"kind": "power", "power_w": 180.0}})
	ride.call("_on_abort_requested")
	await process_frame
	_check(start.visible and not summary.visible, "aborted rides show no summary")
	DirAccess.remove_absolute(path)
	main.free()
	if not _failed:
		print("SUMMARY SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error("SUMMARY SMOKE TEST FAILED: " + what)
