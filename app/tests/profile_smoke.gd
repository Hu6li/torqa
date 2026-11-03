extends SceneTree
## Headless checks of the rider's profile (R9, R98): the drivetrain in the rider settings, the
## ride settings' Profile tab that changes the profile during a ride, and the pause that opens
## the ride settings.
## Run: godot --headless --path app -s res://tests/profile_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_rider_drivetrain()
	_calorie_profile()
	_ride_profile()
	await _pause_opens_settings()
	if not _failed:
		print("PROFILE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## The rider's drivetrain (R9, R98): chainring and cog only for a single cog, the default.
func _rider_drivetrain() -> void:
	var dialog: ProfileDialog = ProfileDialog.new()
	root.add_child(dialog)
	var confirmed: Array[Dictionary] = []
	dialog.profile_confirmed.connect(
		func(_id: String, profile: Dictionary, _hud: PackedStringArray) -> void:
			confirmed.append(profile)
	)
	dialog.edit(
		{"id": "r", "name": "R", "drivetrain": "single_cog", "chainring": 46, "cog": 14},
		PackedStringArray(["power"])
	)
	# The cog is the last of the teeth fields; the zones tab has spin boxes of its own.
	var teeth: Array[Node] = dialog.find_children("*", "SpinBox", true, false).filter(
		func(node: Node) -> bool: return (node as SpinBox).suffix == " T"
	)
	var cog: SpinBox = teeth[teeth.size() - 1]
	_check(cog.visible, "chainring and cog for a single cog")
	dialog.confirmed.emit()
	var saved: Dictionary = confirmed[0]
	var drivetrain: String = saved["drivetrain"]
	var chainring: float = saved["chainring"]
	_check(drivetrain == "single_cog" and chainring == 46.0, "saved: %s" % saved)
	var voice: String = saved["voice"]
	_check(voice == "casual", "a rider hears the casual voice first (R124)")
	dialog.edit({"id": "r", "name": "R", "voice": "off"}, PackedStringArray(["power"]))
	dialog.confirmed.emit()
	voice = confirmed[1]["voice"]
	_check(voice == "off", "the voice is kept: %s" % confirmed[1])
	dialog.edit({"id": "r", "name": "R", "drivetrain": "cassette"}, PackedStringArray(["power"]))
	_check(not cog.visible, "a cassette needs no teeth")
	dialog.edit({}, PackedStringArray(["power"]))
	_check(cog.visible, "a new rider starts on virtual gears (R98)")
	dialog.free()


## Optional calorie inputs round-trip through the form, Rust boundary and profile storage.
func _calorie_profile() -> void:
	var form: ProfileForm = ProfileForm.new()
	root.add_child(form)
	form.edit({})
	var initial: Dictionary = form.profile()
	var initial_age: int = initial["calorie_age"]
	_check(
		initial_age == 0 and str(initial["calorie_sex"]) == "", "optional demographics start unset"
	)
	_check(str(initial["calorie_effort"]) == "", "no assumed calories by default")
	form.edit(
		{"avatar": "male", "calorie_age": 40, "calorie_sex": "female", "calorie_effort": "hard"}
	)
	var edited: Dictionary = form.profile()
	var edited_age: int = edited["calorie_age"]
	_check(
		str(edited["calorie_sex"]) == "female" and str(edited["avatar"]) == "male",
		"sex independent of avatar"
	)
	_check(
		edited_age == 40 and str(edited["calorie_effort"]) == "hard",
		"calorie choices kept by the form"
	)
	var app: OrqwaApp = OrqwaApp.new()
	root.add_child(app)
	edited["name"] = "Calorie profile check"
	var previous: Dictionary = app.profile()
	var previous_id: String = previous["id"]
	var id: String = app.save_profile("", edited)
	var saved: Dictionary = app.profile()
	var saved_age: int = saved["calorie_age"]
	_check(not id.is_empty() and saved_age == 40, "age through Rust and storage")
	_check(
		str(saved["calorie_sex"]) == "female" and str(saved["calorie_effort"]) == "hard",
		"fallbacks through Rust and storage"
	)
	app.select_profile(previous_id)
	app.delete_profile(id)
	app.free()
	form.free()


## The ride settings' Profile tab (R98) forwards the whole profile and its zones.
func _ride_profile() -> void:
	var dialog: RideSettingsDialog = RideSettingsDialog.new()
	root.add_child(dialog)
	var changes: Array[Dictionary] = []
	dialog.profile_changed.connect(func(profile: Dictionary) -> void: changes.append(profile))
	var tab: RideProfile = dialog.find_children("*", "RideProfile", true, false)[0]
	var rider: Dictionary = {
		"id": "r",
		"name": "R",
		"ftp_w": 250.0,
		"drivetrain": "cassette",
		"default_difficulty_pct": 30.0,
	}
	tab.edit(rider)
	var form: ProfileForm = tab.get("_form")
	var difficulty: SpinBox = form.get("_difficulty")
	var cog: SpinBox = form.get("_cog")
	_check(not difficulty.visible, "the ride's own difficulty is on the Ride tab")
	_check(not cog.visible and changes.is_empty(), "a cassette, nothing changed yet")
	var drivetrain: OptionButton = form.get("_drivetrain")
	drivetrain.select(1)
	drivetrain.item_selected.emit(1)
	_check(cog.visible, "a single cog's teeth")
	var zones: ZonesEditor = tab.get("_zones")
	var first_bound: SpinBox = zones.find_children("*", "SpinBox", true, false)[0]
	first_bound.value = 60.0
	_check(changes.size() == 2, "each change reaches the ride: %s" % [changes])
	var last: Dictionary = changes[changes.size() - 1] if not changes.is_empty() else {}
	var power: PackedFloat64Array = last.get("power_zones_pct", PackedFloat64Array())
	var single_cog: String = last.get("drivetrain", "")
	var rider_name: String = last.get("name", "")
	var start_difficulty: float = last.get("default_difficulty_pct", 0.0)
	_check(
		single_cog == "single_cog" and rider_name == "R" and start_difficulty == 30.0,
		"the whole profile: %s" % last
	)
	_check(power.size() == 6 and power[0] == 60.0, "with its zones: %s" % power)
	dialog.free()


## Paused, the ride settings open (R98) and going on closes them; settings the rider opened
## themselves stay.
func _pause_opens_settings() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	(main.get_node("StartPage") as Control).hide()
	var ride: RideScreen = main.get_node("RideScreen")
	ride.begin({"workout": {"kind": "zone", "zone": 3}, "on_course": false})
	ride.show()
	var settings: RideSettingsDialog = ride.find_children("*", "RideSettingsDialog", true, false)[0]
	ride.call("_show_paused", true)
	_check(settings.visible, "paused: the ride settings open")
	ride.call("_show_paused", false)
	_check(not settings.visible, "going on closes them")
	ride.call("_open_settings")
	ride.call("_show_paused", true)
	ride.call("_show_paused", false)
	_check(settings.visible, "settings opened before the pause stay")
	main.free()


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("PROFILE SMOKE TEST FAILED: " + what)
		_failed = true
