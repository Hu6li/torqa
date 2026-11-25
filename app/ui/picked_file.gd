class_name PickedFile
extends RefCounted
## A file the rider picked, as ORQWA's core can open or write it. On Android the file pickers
## hand over a content address (`content://…`) that only Android's own file access reads and
## writes (ADR 0024): a picked file is copied into ORQWA's folder first, and one to save is
## written there by the core, then copied to the address. Elsewhere paths stay as they are.

const COPIES: String = "user://picked"
## ORQWA's Android plugin (ADR 0024), there only on Android: it names picked files the address
## itself does not name.
const ANDROID: StringName = &"OrqwaAndroid"
## How much of a picked file is held at once while it is copied.
const CHUNK: int = 8 * 1024 * 1024


## The filters for a file dialog that opens one of `filters`' files: none on Android. Its picker
## turns each extension into a MIME type and greys out files stored under another one, which in
## Recent and Downloads is whatever the download was typed as (a GPX as application/gpx+xml,
## say): the core goes by the picked file's name instead.
static func open_filters(filters: PackedStringArray) -> PackedStringArray:
	return PackedStringArray() if OS.has_feature("android") else filters


## Why ORQWA does not open `picked`, which the rider meant as one of `extensions` (lower case,
## without the dot); "" if it has one of them. Checked by name before the file is copied or
## read, so a file that is something else is never handed to a reader for another format.
static func refusal(picked: String, extensions: PackedStringArray) -> String:
	var file: String = name_of(picked).get_file()
	if file.get_extension().to_lower() in extensions:
		return ""
	var dotted: PackedStringArray = PackedStringArray()
	for extension: String in extensions:
		dotted.append("." + extension)
	return (
		TranslationServer.translate("ORQWA cannot open %s: it opens %s files.")
		% [file, ", ".join(dotted)]
	)


## The path to give the core for `picked`; `picked` itself if an Android file cannot be copied,
## so the core reports it as unreadable as before. Copies on the calling thread: a large file
## goes through [PickedCopy] instead.
static func local_path(picked: String) -> String:
	if not picked.begins_with("content://"):
		return picked
	var copy: String = copy_of(
		picked, name_of(picked), func(_done: int, _total: int) -> bool: return true
	)
	return copy if not copy.is_empty() else picked


## Copies `picked` into ORQWA's folder as `file_name` (from [method name_of], which asks
## Android and so stays on the main thread) and returns the copy's path; "" if it could not be
## read or written whole, or `on_chunk(done, total)` returned false to stop. Safe off the main
## thread.
static func copy_of(picked: String, file_name: String, on_chunk: Callable) -> String:
	var source: FileAccess = FileAccess.open(picked, FileAccess.READ)
	if source == null or source.get_length() == 0:
		return ""
	var total: int = source.get_length()
	DirAccess.make_dir_recursive_absolute(COPIES)
	var copy: String = COPIES.path_join(file_name)
	var file: FileAccess = FileAccess.open(copy, FileAccess.WRITE)
	if file == null:
		return ""
	# In pieces: a world runs to gigabytes, and Android kills an app that holds one whole.
	var done: int = 0
	var whole: bool = true
	while done < total:
		var chunk: PackedByteArray = source.get_buffer(CHUNK)
		# The last read reaching the end sets ERR_FILE_EOF: only a read that brings nothing fails.
		if chunk.is_empty():
			break
		file.store_buffer(chunk)
		done += chunk.size()
		if file.get_error() != OK or not on_chunk.call(done, total):
			whole = false
			break
	file.close()
	# A copy cut short reads as a damaged file to the core: never hand one over.
	if not whole or done != total:
		DirAccess.remove_absolute(copy)
		return ""
	return ProjectSettings.globalize_path(copy)


## Removes `path` if [method local_path] copied it, once the core has its own copy: a world
## would otherwise keep gigabytes of the rider's storage twice.
static func forget(path: String) -> void:
	if path.get_base_dir() == ProjectSettings.globalize_path(COPIES):
		DirAccess.remove_absolute(path)


## Where the core should write a file the rider wants saved at `picked`; then [method save] it.
static func save_path(picked: String) -> String:
	if not picked.begins_with("content://"):
		return picked
	DirAccess.make_dir_recursive_absolute(COPIES)
	return ProjectSettings.globalize_path(COPIES.path_join(name_of(picked)))


## Copies what the core wrote at `written` (from [method save_path]) to `picked`; whether that
## worked. Nothing to copy outside Android.
static func save(written: String, picked: String) -> bool:
	if written == picked:
		return true
	var data: PackedByteArray = FileAccess.get_file_as_bytes(written)
	var file: FileAccess = FileAccess.open(picked, FileAccess.WRITE)
	if data.is_empty() or file == null:
		return false
	file.store_buffer(data)
	file.close()
	DirAccess.remove_absolute(written)
	return true


## How to name `picked` to the rider: the file's own name rather than a content address. One
## ending in the file's path answers itself (…%3ADownload%2FRide.gpx); one ending in a document
## id (…/document/msf:1000003599) names nothing, and the file's extension decides what the core
## makes of it: Android is asked for the display name first.
static func name_of(picked: String) -> String:
	if not picked.begins_with("content://"):
		return picked
	return _content_name(picked, _display_name(picked))


## What a content `address`'s file is called: the `display_name` its provider gives Android
## when it answers one, else the address's own last part, else "picked".
static func _content_name(address: String, display_name: String) -> String:
	var file_name: String = display_name.get_file()
	if file_name.is_empty():
		file_name = address.uri_decode().get_file()
	return file_name if not file_name.is_empty() else "picked"


## The picked file's display name as Android knows it; "" anywhere but on Android, or when its
## provider names nothing.
static func _display_name(address: String) -> String:
	if not Engine.has_singleton(ANDROID):
		return ""
	var named: Variant = Engine.get_singleton(ANDROID).call(&"pickedName", address)
	return named if named is String else ""
