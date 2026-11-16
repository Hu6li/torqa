extends SceneTree
## Picked files (ADR 0024): paths pass through as they are; Android's content addresses are
## named by the file they end in, or by the display name their provider gives; a file whose
## name says it is something else is refused before anything reads it.

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var path: String = "/home/rider/Rides/Ride.gpx"
	_check(PickedFile.local_path(path) == path, "a picked path is opened as it is")
	_check(PickedFile.save_path(path) == path, "and saved as it is")
	_check(PickedFile.save(path, path), "with nothing to copy")
	var address: String = (
		"content://com.android.externalstorage.documents/document/"
		+ "primary%3ADownload%2FRide.gpx"
	)
	_check(PickedFile.name_of(address) == "Ride.gpx", "a content address shows its file name")
	_check(PickedFile.save_path(address).ends_with("/picked/Ride.gpx"), "saved in ORQWA's folder")
	# Off Android a document id keeps naming the copy; on Android its display name does.
	var document: String = "content://com.android.providers.media.documents/document/msf%3A1000003599"
	_check(PickedFile.name_of(document) == "msf:1000003599", "a document id answers itself")
	# The Android plugin is not there headless, so the provider's answer is handed in directly;
	# the plugin's query itself is covered only on a device.
	_check(
		PickedFile._content_name(document, "Ride.gpx") == "Ride.gpx",
		"a provider's display name names the copy"
	)
	var routes: PackedStringArray = PackedStringArray(["gpx", "fit"])
	_check(PickedFile.refusal("/rides/Ride.GPX", routes).is_empty(), "a route is opened")
	_check(PickedFile.refusal(address, routes).is_empty(), "by its name behind an address")
	var refused: String = PickedFile.refusal("/rides/Ride.pdf", routes)
	_check(
		refused.contains("Ride.pdf") and refused.contains(".gpx, .fit"),
		"another file is refused, naming it and what opens instead: " + refused
	)
	_check(not PickedFile.refusal("/rides/Ride", routes).is_empty(), "so is one without extension")
	_check(not PickedFile.refusal(document, routes).is_empty(), "and an unnamed document")
	if not _failed:
		print("PICKED FILE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error(what)
