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
	_check_forget()
	_check_copy()
	await _check_threaded_copy()
	if not _failed:
		print("PICKED FILE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## A copy goes once the core has its own; the rider's own files never do.
func _check_forget() -> void:
	var copy: String = PickedFile.save_path("content://downloads/World.oqx")
	var own: String = ProjectSettings.globalize_path("user://World.oqx")
	for path: String in [copy, own]:
		FileAccess.open(path, FileAccess.WRITE).close()
	PickedFile.forget(copy)
	PickedFile.forget(own)
	_check(not FileAccess.file_exists(copy), "an imported copy is removed")
	_check(FileAccess.file_exists(own), "a file outside ORQWA's copies is kept")
	DirAccess.remove_absolute(own)


## A file of several chunks, its last one short, so the copy reads up to the end of the file.
func _source() -> String:
	var path: String = ProjectSettings.globalize_path("user://source.oqx")
	var data: PackedByteArray = PackedByteArray()
	data.resize(PickedFile.CHUNK * 2 + 12345)
	for i: int in range(0, data.size(), 4096):
		data[i] = i % 251
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_buffer(data)
	file.close()
	return path


## Copied whole and as it is, however large; stopped, it leaves nothing for the core to read.
func _check_copy() -> void:
	var source: String = _source()
	var steps: Array[int] = []
	var copy: String = PickedFile.copy_of(
		source,
		"World.oqx",
		func(done: int, _total: int) -> bool:
			steps.append(done)
			return true
	)
	_check(not copy.is_empty(), "a file is copied")
	_check(
		FileAccess.get_md5(copy) == FileAccess.get_md5(source), "byte for byte, past its last chunk"
	)
	_check(steps.size() == 3 and steps[-1] == FileAccess.get_size(source), "in chunks")
	PickedFile.forget(copy)
	var stopped: String = PickedFile.copy_of(
		source, "World.oqx", func(_done: int, _total: int) -> bool: return false
	)
	_check(stopped.is_empty(), "a stopped copy is no copy")
	_check(
		not FileAccess.file_exists(PickedFile.save_path("content://downloads/World.oqx")),
		"and leaves no part behind"
	)
	_check(
		(
			PickedFile
			. copy_of("/no/such/file.oqx", "x.oqx", func(_d: int, _t: int) -> bool: return true)
			. is_empty()
		),
		"an unreadable file is not copied"
	)
	DirAccess.remove_absolute(source)


## Off the main thread, with its progress and its end reported on it.
func _check_threaded_copy() -> void:
	var source: String = _source()
	var copy: PickedCopy = PickedCopy.new()
	var reported: Array[int] = []
	copy.progressed.connect(func(done: int, _total: int) -> void: reported.append(done))
	copy.start(source)
	var path: String = await copy.finished
	_check(
		not path.is_empty() and FileAccess.get_md5(path) == FileAccess.get_md5(source),
		"copied on a thread"
	)
	_check(not reported.is_empty(), "reporting how far it is")
	PickedFile.forget(path)
	DirAccess.remove_absolute(source)


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error(what)
